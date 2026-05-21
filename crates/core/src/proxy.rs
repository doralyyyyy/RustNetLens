use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use http::{Method, Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use reqwest::Client;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, oneshot};
use tracing::{error, warn};

use crate::capture::{preview_body, redact_headers};
use crate::error::ProxyError;
use crate::model::{
    CaptureEvent, CapturedSession, HeaderPair, RequestContext, ResponseContext, SessionKind,
    SessionState,
};
use crate::rules::{RuleEngine, merge_headers};
use crate::store::SessionStore;

#[derive(Clone)]
pub struct ProxyConfig {
    pub listen_addr: SocketAddr,
    pub max_request_body_bytes: usize,
    pub max_response_body_bytes: usize,
}

pub struct ProxyServer<S: SessionStore> {
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
}

impl<S: SessionStore> ProxyServer<S> {
    pub fn new(
        config: ProxyConfig,
        store: Arc<S>,
        rules: Arc<RuleEngine>,
        event_tx: broadcast::Sender<CaptureEvent>,
    ) -> Self {
        Self {
            config,
            store,
            rules,
            event_tx,
        }
    }

    pub async fn run(self, mut shutdown: oneshot::Receiver<()>) -> Result<(), ProxyError> {
        let listener = TcpListener::bind(self.config.listen_addr)
            .await
            .map_err(|source| ProxyError::BindFailed {
                addr: self.config.listen_addr,
                source,
            })?;
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                accept = listener.accept() => {
                    let (stream, peer) = match accept {
                        Ok(value) => value,
                        Err(err) => {
                            error!(error = %err, "accept failed");
                            continue;
                        }
                    };
                    let config = self.config.clone();
                    let store = self.store.clone();
                    let rules = self.rules.clone();
                    let event_tx = self.event_tx.clone();
                    tokio::spawn(async move {
                        if let Err(err) = handle_client(stream, peer, config, store, rules, event_tx).await {
                            error!(error = %err, "proxy connection failed");
                        }
                    });
                }
            }
        }
        Ok(())
    }
}

async fn handle_client<S: SessionStore>(
    stream: TcpStream,
    peer: SocketAddr,
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
) -> Result<(), ProxyError> {
    let io = TokioIo::new(stream);
    let service = service_fn(move |req: Request<Incoming>| {
        let store = store.clone();
        let rules = rules.clone();
        let event_tx = event_tx.clone();
        let config = config.clone();
        async move {
            Ok::<_, std::convert::Infallible>(
                handle_request(req, peer, config, store, rules, event_tx).await,
            )
        }
    });
    http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(io, service)
        .with_upgrades()
        .await
        .map_err(|e| ProxyError::Http(e.to_string()))?;
    Ok(())
}

async fn handle_request<S: SessionStore>(
    req: Request<Incoming>,
    _peer: SocketAddr,
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
) -> Response<Full<Bytes>> {
    if req.method() == Method::CONNECT {
        return match handle_connect(req, config, store, event_tx).await {
            Ok(resp) => resp,
            Err(err) => internal_error(err),
        };
    }
    match handle_http(req, config, store, rules, event_tx).await {
        Ok(resp) => resp,
        Err(err) => internal_error(err),
    }
}

async fn handle_http<S: SessionStore>(
    mut req: Request<Incoming>,
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
) -> Result<Response<Full<Bytes>>, ProxyError> {
    let mut session = CapturedSession::new(SessionKind::Http);
    session.started_at = Utc::now();
    session.method = Some(req.method().to_string());
    session.url = Some(req.uri().to_string());
    session.host = req.uri().host().map(|s| s.to_string()).or_else(|| {
        req.headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    });
    session.scheme = req
        .uri()
        .scheme_str()
        .map(|s| s.to_string())
        .or_else(|| Some("http".into()));
    session.port = req
        .uri()
        .port_u16()
        .or_else(|| parse_host_port(req.headers().get(header::HOST)));
    session.path = Some(
        req.uri()
            .path_and_query()
            .map(|p| p.as_str().to_string())
            .unwrap_or_else(|| "/".to_string()),
    );
    session.request_headers = headers_to_pairs(req.headers());
    let body_bytes = req
        .body_mut()
        .collect()
        .await
        .map_err(|e| ProxyError::Http(e.to_string()))?
        .to_bytes();
    session.request_headers = redact_headers(&session.request_headers);
    session.request_body = preview_body(
        req.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        &body_bytes,
        config.max_request_body_bytes,
    );
    session.bytes_up = body_bytes.len() as u64;

    let mut request_ctx = RequestContext {
        session: session.clone(),
        request_headers: session.request_headers.clone(),
        request_body: body_bytes.to_vec(),
        should_mock: false,
        delay_ms: None,
        rewrite_request_headers: Vec::new(),
        matched_rule_ids: Vec::new(),
    };
    let rule_result = rules.apply_request(&mut request_ctx).await;
    session.matched_rule_ids = request_ctx.matched_rule_ids.clone();
    merge_headers(
        &mut session.request_headers,
        &rule_result.rewrite_request_headers,
    );
    if let Some(delay_ms) = rule_result.delay_ms {
        RuleEngine::maybe_delay(Some(delay_ms)).await;
    }
    if let Some((status, headers, body)) = rule_result.mock_response {
        session.state = SessionState::Mocked;
        session.status = Some(status);
        session.response_headers = headers.clone();
        session.response_body = preview_body(
            headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case("content-type"))
                .map(|h| h.value.as_str()),
            body.as_bytes(),
            config.max_response_body_bytes,
        );
        session.bytes_down = body.len() as u64;
        session.finish(SessionState::Mocked);
        store
            .insert_session(&session)
            .await
            .map_err(|e| ProxyError::Store(e.to_string()))?;
        let _ = event_tx.send(CaptureEvent {
            session: session.clone(),
        });
        return Ok(response_from_parts(
            status,
            &headers,
            Full::from(Bytes::from(body)),
        ));
    }

    let target = target_uri(&req, &session)?;
    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| ProxyError::Http(e.to_string()))?;
    let mut builder = client.request(req.method().clone(), &target);
    for header in &session.request_headers {
        builder = builder.header(&header.name, &header.value);
    }
    let upstream_response = builder.body(body_bytes.clone()).send().await.map_err(|e| {
        ProxyError::UpstreamConnectFailed {
            target: target.clone(),
            source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        }
    })?;

    let status = upstream_response.status().as_u16();
    let mut response_headers = headers_to_pairs(upstream_response.headers());
    let mut response_ctx = ResponseContext {
        session: session.clone(),
        response_headers: response_headers.clone(),
        response_body: Vec::new(),
        delay_ms: None,
        rewrite_response_headers: Vec::new(),
        matched_rule_ids: Vec::new(),
    };
    let response_rules = rules.apply_response(&mut response_ctx).await;
    session
        .matched_rule_ids
        .extend(response_rules.matched_rule_ids);
    merge_headers(
        &mut response_headers,
        &response_rules.rewrite_response_headers,
    );
    if let Some(delay_ms) = response_rules.delay_ms {
        RuleEngine::maybe_delay(Some(delay_ms)).await;
    }
    let body = upstream_response
        .bytes()
        .await
        .map_err(|e| ProxyError::Http(e.to_string()))?;
    session.status = Some(status);
    session.response_headers = response_headers.clone();
    session.response_body = preview_body(
        response_headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .map(|h| h.value.as_str()),
        &body,
        config.max_response_body_bytes,
    );
    session.bytes_down = body.len() as u64;
    session.finish(SessionState::Completed);
    store
        .insert_session(&session)
        .await
        .map_err(|e| ProxyError::Store(e.to_string()))?;
    let _ = event_tx.send(CaptureEvent {
        session: session.clone(),
    });
    Ok(response_from_parts(
        status,
        &response_headers,
        Full::from(body),
    ))
}

async fn handle_connect<S: SessionStore>(
    req: Request<Incoming>,
    _config: ProxyConfig,
    store: Arc<S>,
    event_tx: broadcast::Sender<CaptureEvent>,
) -> Result<Response<Full<Bytes>>, ProxyError> {
    let authority = req
        .uri()
        .authority()
        .map(|a| a.as_str().to_string())
        .or_else(|| {
            req.headers()
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .ok_or_else(|| ProxyError::InvalidRequest("CONNECT target missing".into()))?;
    let (host, port) = parse_connect_target(&authority)?;
    let mut session = CapturedSession::new(SessionKind::ConnectTunnel);
    session.started_at = Utc::now();
    session.method = Some("CONNECT".into());
    session.host = Some(host.clone());
    session.port = Some(port);
    session.url = Some(format!("{host}:{port}"));
    session.state = SessionState::Tunneling;

    let upgrade = hyper::upgrade::on(req);
    let target = format!("{host}:{port}");
    tokio::spawn(async move {
        match upgrade.await {
            Ok(upgraded) => {
                if let Err(err) = tunnel(upgraded, &target).await {
                    warn!(error = %err, "connect tunnel failed");
                }
            }
            Err(err) => warn!(error = %err, "upgrade failed"),
        }
    });

    session.finish(SessionState::Completed);
    store
        .insert_session(&session)
        .await
        .map_err(|e| ProxyError::Store(e.to_string()))?;
    let _ = event_tx.send(CaptureEvent { session });
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Full::from(Bytes::new()))
        .map_err(|e| ProxyError::Http(e.to_string()))?)
}

async fn tunnel(upgraded: Upgraded, target: &str) -> Result<(), ProxyError> {
    let mut upstream =
        TcpStream::connect(target)
            .await
            .map_err(|e| ProxyError::UpstreamConnectFailed {
                target: target.to_string(),
                source: e,
            })?;
    let mut upgraded = TokioIo::new(upgraded);
    copy_bidirectional(&mut upgraded, &mut upstream)
        .await
        .map_err(|e| ProxyError::Http(e.to_string()))?;
    Ok(())
}

fn parse_connect_target(target: &str) -> Result<(String, u16), ProxyError> {
    let mut parts = target.splitn(2, ':');
    let host = parts
        .next()
        .ok_or_else(|| ProxyError::InvalidRequest("missing connect host".into()))?
        .to_string();
    let port = parts
        .next()
        .map(|p| p.parse::<u16>().unwrap_or(443))
        .unwrap_or(443);
    Ok((host, port))
}

fn target_uri(req: &Request<Incoming>, session: &CapturedSession) -> Result<String, ProxyError> {
    if req.uri().scheme().is_some() && req.uri().authority().is_some() {
        return Ok(req.uri().to_string());
    }
    let host = session
        .host
        .clone()
        .ok_or_else(|| ProxyError::InvalidRequest("missing host".into()))?;
    let scheme = session.scheme.clone().unwrap_or_else(|| "http".into());
    let path = session.path.clone().unwrap_or_else(|| "/".into());
    let uri = format!("{scheme}://{host}{path}");
    Ok(uri)
}

fn headers_to_pairs(headers: &http::HeaderMap) -> Vec<HeaderPair> {
    headers
        .iter()
        .map(|(name, value)| HeaderPair {
            name: name.to_string(),
            value: value.to_str().unwrap_or_default().to_string(),
        })
        .collect()
}

fn response_from_parts(
    status: u16,
    headers: &[HeaderPair],
    body: Full<Bytes>,
) -> Response<Full<Bytes>> {
    let mut builder =
        Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK));
    for header in headers {
        builder = builder.header(&header.name, &header.value);
    }
    builder
        .body(body)
        .unwrap_or_else(|_| Response::new(Full::from(Bytes::new())))
}

fn internal_error(err: ProxyError) -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Full::from(Bytes::from(format!("proxy error: {err}"))))
        .unwrap_or_else(|_| Response::new(Full::from(Bytes::from_static(b"proxy error"))))
}

fn parse_host_port(host: Option<&http::HeaderValue>) -> Option<u16> {
    host.and_then(|value| value.to_str().ok())
        .and_then(|host| host.rsplit(':').next()?.parse::<u16>().ok())
}
