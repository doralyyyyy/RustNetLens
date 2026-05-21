use std::convert::Infallible;
use std::future::ready;
use std::net::SocketAddr;
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use chrono::Utc;
use http::{HeaderMap, Method, Request, Response, StatusCode, Uri, header};
use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::TokioIo;
use hyper_util::{rt::TokioExecutor, server::conn::auto::Builder as AutoBuilder};
use ring::digest::{SHA1_FOR_LEGACY_USE_ONLY, digest};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, copy_bidirectional, split};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, broadcast, oneshot};
use tokio_rustls::TlsAcceptor;
use tracing::{error, warn};

use crate::capture::{
    preview_body_with_encoding, preview_request_body, preview_response_body, redact_headers,
};
use crate::error::ProxyError;
use crate::model::{
    CaptureEvent, CapturedSession, HeaderPair, RequestContext, ResponseContext, SessionKind,
    SessionState, TimelineEntry, TrailersPreview,
};
use crate::rules::{RuleEngine, merge_headers};
use crate::security::{HttpsMitmState, build_mitm_server_config};
use crate::store::SessionStore;

type ProxyBody = BoxBody<Bytes, Infallible>;

fn empty_body() -> ProxyBody {
    Empty::<Bytes>::new().boxed()
}

fn boxed_body(bytes: Bytes) -> ProxyBody {
    Full::new(bytes).boxed()
}

fn boxed_body_with_trailers(bytes: Bytes, trailers: Vec<HeaderPair>) -> ProxyBody {
    if trailers.is_empty() {
        return boxed_body(bytes);
    }
    let trailer_map = pairs_to_header_map(&trailers);
    Full::new(bytes)
        .with_trailers(ready(Some(Ok::<_, Infallible>(trailer_map))))
        .boxed()
}

fn build_upstream_client() -> Client<hyper_rustls::HttpsConnector<HttpConnector>, ProxyBody> {
    let connector = HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();
    Client::builder(TokioExecutor::new()).build(connector)
}

fn pairs_to_header_map(headers: &[HeaderPair]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for header in headers {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::from_bytes(header.name.as_bytes()),
            header::HeaderValue::from_str(&header.value),
        ) {
            map.append(name, value);
        }
    }
    map
}

fn is_grpc_content_type(headers: &[HeaderPair]) -> bool {
    headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("content-type")
            && header
                .value
                .to_ascii_lowercase()
                .contains("application/grpc")
    })
}

fn extract_grpc_metadata(headers: &[HeaderPair]) -> Vec<HeaderPair> {
    headers
        .iter()
        .filter(|header| {
            !header.name.starts_with(':') && !header.name.eq_ignore_ascii_case("content-length")
        })
        .cloned()
        .collect()
}

async fn mitm_connection<S: SessionStore>(
    upgraded: Upgraded,
    _peer: SocketAddr,
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
    https_mitm: Arc<Mutex<HttpsMitmState>>,
) -> Result<(), ProxyError> {
    let server_config =
        build_mitm_server_config(https_mitm).map_err(|err| ProxyError::Http(err.to_string()))?;
    let acceptor = TlsAcceptor::from(server_config);
    let tls_stream = acceptor
        .accept(TokioIo::new(upgraded))
        .await
        .map_err(|e| ProxyError::Http(e.to_string()))?;
    let io = TokioIo::new(tls_stream);
    let service = service_fn(move |req: Request<Incoming>| {
        let store = store.clone();
        let rules = rules.clone();
        let event_tx = event_tx.clone();
        let config = config.clone();
        async move {
            let response =
                match handle_routed_request(req, config, store, rules, event_tx, "https").await {
                    Ok(response) => response,
                    Err(err) => internal_error(err),
                };
            Ok::<_, Infallible>(response)
        }
    });
    AutoBuilder::new(TokioExecutor::new())
        .serve_connection_with_upgrades(io, service)
        .await
        .map_err(|e| ProxyError::Http(e.to_string()))?;
    Ok(())
}

#[derive(Clone)]
pub struct ProxyConfig {
    pub listen_addr: SocketAddr,
    pub max_request_body_bytes: usize,
    pub max_response_body_bytes: usize,
    pub max_websocket_frame_bytes: usize,
}

pub struct ProxyServer<S: SessionStore> {
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
    #[allow(dead_code)]
    https_mitm: Arc<Mutex<HttpsMitmState>>,
}

impl<S: SessionStore> ProxyServer<S> {
    pub fn new(
        config: ProxyConfig,
        store: Arc<S>,
        rules: Arc<RuleEngine>,
        event_tx: broadcast::Sender<CaptureEvent>,
        https_mitm: Arc<Mutex<HttpsMitmState>>,
    ) -> Self {
        Self {
            config,
            store,
            rules,
            event_tx,
            https_mitm,
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
                    let https_mitm = self.https_mitm.clone();
                    tokio::spawn(async move {
                        if let Err(err) = handle_client(
                            stream,
                            peer,
                            config,
                            store,
                            rules,
                            event_tx,
                            https_mitm,
                        )
                        .await
                        {
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
    https_mitm: Arc<Mutex<HttpsMitmState>>,
) -> Result<(), ProxyError> {
    let io = TokioIo::new(stream);
    let service = service_fn(move |req: Request<Incoming>| {
        let store = store.clone();
        let rules = rules.clone();
        let event_tx = event_tx.clone();
        let config = config.clone();
        let https_mitm = https_mitm.clone();
        async move {
            let response = match handle_request(
                req, peer, config, store, rules, event_tx, https_mitm, "http",
            )
            .await
            {
                Ok(response) => response,
                Err(err) => internal_error(err),
            };
            Ok::<_, Infallible>(response)
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
    peer: SocketAddr,
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
    https_mitm: Arc<Mutex<HttpsMitmState>>,
    default_scheme: &'static str,
) -> Result<Response<ProxyBody>, ProxyError> {
    if req.method() == Method::CONNECT {
        return handle_connect(req, peer, config, store, rules, event_tx, https_mitm).await;
    }
    handle_routed_request(req, config, store, rules, event_tx, default_scheme).await
}

async fn handle_routed_request<S: SessionStore>(
    req: Request<Incoming>,
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
    default_scheme: &'static str,
) -> Result<Response<ProxyBody>, ProxyError> {
    if default_scheme == "http" && is_websocket_upgrade(&req) {
        return handle_websocket(req, config, store, event_tx).await;
    }
    handle_http(req, config, store, rules, event_tx, default_scheme).await
}

async fn handle_http<S: SessionStore>(
    mut req: Request<Incoming>,
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
    default_scheme: &'static str,
) -> Result<Response<ProxyBody>, ProxyError> {
    let mut session = CapturedSession::new(SessionKind::Http);
    session.started_at = Utc::now();
    let request_headers_raw = headers_to_pairs(req.headers());
    let request_headers_redacted = redact_headers(&request_headers_raw);
    session.request_headers = request_headers_redacted.clone();
    session.host = req
        .uri()
        .authority()
        .map(|authority| authority.as_str().to_string())
        .or_else(|| {
            req.headers()
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        });
    session.port = req
        .uri()
        .port_u16()
        .or_else(|| parse_host_port(req.headers().get(header::HOST)));
    let target = build_target_uri(&req, &session, default_scheme)?;
    session.method = Some(req.method().to_string());
    session.url = Some(target.to_string());
    session.scheme = target.scheme_str().map(|scheme| scheme.to_string());
    session.path = Some(
        target
            .path_and_query()
            .map(|p| p.as_str().to_string())
            .unwrap_or_else(|| "/".to_string()),
    );
    session.timeline.push(TimelineEntry {
        name: "request_received".into(),
        started_at: session.started_at,
        ended_at: Some(session.started_at),
        duration_ms: Some(0),
    });
    let request_content_type = request_headers_raw
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.clone());
    let request_content_encoding = request_headers_raw
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-encoding"))
        .map(|header| header.value.clone());
    let collected_request = req
        .body_mut()
        .collect()
        .await
        .map_err(|e| ProxyError::Http(e.to_string()))?;
    let request_trailer_map = collected_request.trailers().cloned().unwrap_or_default();
    let body_bytes = collected_request.to_bytes();
    let request_trailers_raw = headers_to_pairs(&request_trailer_map);
    session.grpc_request_metadata = if is_grpc_content_type(&request_headers_raw) {
        extract_grpc_metadata(&request_headers_redacted)
    } else {
        Vec::new()
    };
    session.grpc_request_trailers = TrailersPreview {
        headers: redact_headers(&request_trailers_raw),
    };
    session.request_body = preview_request_body(
        request_content_type.as_deref(),
        request_content_encoding.as_deref(),
        &body_bytes,
    );
    session.bytes_up = body_bytes.len() as u64;

    let mut request_ctx = RequestContext {
        session: session.clone(),
        request_headers: request_headers_redacted.clone(),
        request_body: body_bytes.to_vec(),
        request_trailers: redact_headers(&request_trailers_raw),
        should_mock: false,
        delay_ms: None,
        rewrite_request_headers: Vec::new(),
        rewrite_request_trailers: Vec::new(),
        matched_rule_ids: Vec::new(),
    };
    let request_rules = rules.apply_request(&mut request_ctx).await;
    session.matched_rule_ids = request_ctx.matched_rule_ids.clone();

    let mut outgoing_request_headers = request_headers_raw.clone();
    merge_headers(
        &mut outgoing_request_headers,
        &request_rules.rewrite_request_headers,
    );
    let mut outgoing_request_trailers = request_trailers_raw.clone();
    merge_headers(
        &mut outgoing_request_trailers,
        &request_rules.rewrite_request_trailers,
    );
    let outgoing_request_headers_redacted = redact_headers(&outgoing_request_headers);
    session.request_headers = outgoing_request_headers_redacted.clone();
    session.grpc_request_metadata = if is_grpc_content_type(&outgoing_request_headers) {
        extract_grpc_metadata(&outgoing_request_headers_redacted)
    } else {
        Vec::new()
    };
    session.grpc_request_trailers = TrailersPreview {
        headers: redact_headers(&outgoing_request_trailers),
    };

    if let Some(delay_ms) = request_rules.delay_ms {
        RuleEngine::maybe_delay(Some(delay_ms)).await;
    }
    if let Some((status, headers, body)) = request_rules.mock_response {
        let response_headers_redacted = redact_headers(&headers);
        session.state = SessionState::Mocked;
        session.status = Some(status);
        session.response_headers = response_headers_redacted.clone();
        session.response_body = preview_body_with_headers(
            &response_headers_redacted,
            body.as_bytes(),
            config.max_response_body_bytes,
        );
        session.bytes_down = body.len() as u64;
        session.grpc_response_metadata = if is_grpc_content_type(&response_headers_redacted) {
            extract_grpc_metadata(&response_headers_redacted)
        } else {
            Vec::new()
        };
        session.grpc_response_trailers = TrailersPreview::default();
        session.timeline.push(TimelineEntry {
            name: "completed".into(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            duration_ms: Some(0),
        });
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
            boxed_body(Bytes::from(body)),
        ));
    }

    let client = build_upstream_client();
    let mut request_builder = Request::builder()
        .method(req.method().clone())
        .uri(target.clone());
    for header in &outgoing_request_headers {
        request_builder = request_builder.header(&header.name, &header.value);
    }
    let request_body = if outgoing_request_trailers.is_empty() {
        boxed_body(Bytes::from(body_bytes.clone()))
    } else {
        boxed_body_with_trailers(
            Bytes::from(body_bytes.clone()),
            outgoing_request_trailers.clone(),
        )
    };
    let upstream_request = request_builder
        .body(request_body)
        .map_err(|e| ProxyError::Http(e.to_string()))?;
    let upstream_response =
        client
            .request(upstream_request)
            .await
            .map_err(|e| ProxyError::UpstreamConnectFailed {
                target: target.to_string(),
                source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            })?;

    let status = upstream_response.status().as_u16();
    let response_headers_raw = headers_to_pairs(upstream_response.headers());
    let response_headers_redacted = redact_headers(&response_headers_raw);
    let response_content_type = response_headers_raw
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.clone());
    let response_content_encoding = response_headers_raw
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-encoding"))
        .map(|header| header.value.clone());
    let collected_response = upstream_response
        .into_body()
        .collect()
        .await
        .map_err(|e| ProxyError::Http(e.to_string()))?;
    let response_trailer_map = collected_response.trailers().cloned().unwrap_or_default();
    let response_body = collected_response.to_bytes();
    let response_trailers_raw = headers_to_pairs(&response_trailer_map);

    let mut response_ctx = ResponseContext {
        session: session.clone(),
        response_headers: response_headers_redacted.clone(),
        response_body: response_body.to_vec(),
        response_trailers: redact_headers(&response_trailers_raw),
        delay_ms: None,
        rewrite_response_headers: Vec::new(),
        rewrite_response_trailers: Vec::new(),
        matched_rule_ids: Vec::new(),
    };
    let response_rules = rules.apply_response(&mut response_ctx).await;
    session
        .matched_rule_ids
        .extend(response_rules.matched_rule_ids);

    let mut outgoing_response_headers = response_headers_raw.clone();
    merge_headers(
        &mut outgoing_response_headers,
        &response_rules.rewrite_response_headers,
    );
    let mut outgoing_response_trailers = response_trailers_raw.clone();
    merge_headers(
        &mut outgoing_response_trailers,
        &response_rules.rewrite_response_trailers,
    );
    let outgoing_response_headers_redacted = redact_headers(&outgoing_response_headers);
    let response_body_preview = if let Some((status, headers, body)) = response_rules.mock_response
    {
        let response_headers_redacted = redact_headers(&headers);
        session.status = Some(status);
        session.response_headers = response_headers_redacted.clone();
        session.response_body = preview_body_with_headers(
            &response_headers_redacted,
            body.as_bytes(),
            config.max_response_body_bytes,
        );
        session.bytes_down = body.len() as u64;
        session.grpc_response_metadata = if is_grpc_content_type(&response_headers_redacted) {
            extract_grpc_metadata(&response_headers_redacted)
        } else {
            Vec::new()
        };
        session.grpc_response_trailers = TrailersPreview::default();
        session.timeline.push(TimelineEntry {
            name: "completed".into(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            duration_ms: Some(0),
        });
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
            boxed_body(Bytes::from(body)),
        ));
    } else {
        session.status = Some(status);
        session.response_headers = outgoing_response_headers_redacted.clone();
        session.response_body = preview_response_body(
            response_content_type.as_deref(),
            response_content_encoding.as_deref(),
            &response_body,
        );
        session.bytes_down = response_body.len() as u64;
        session.grpc_response_metadata = if is_grpc_content_type(&outgoing_response_headers) {
            extract_grpc_metadata(&outgoing_response_headers_redacted)
        } else {
            Vec::new()
        };
        session.grpc_response_trailers = TrailersPreview {
            headers: redact_headers(&outgoing_response_trailers),
        };
        session.timeline.push(TimelineEntry {
            name: "first_byte".into(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            duration_ms: Some(0),
        });
        session.timeline.push(TimelineEntry {
            name: "completed".into(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            duration_ms: Some(0),
        });
        session.finish(SessionState::Completed);
        store
            .insert_session(&session)
            .await
            .map_err(|e| ProxyError::Store(e.to_string()))?;
        let _ = event_tx.send(CaptureEvent {
            session: session.clone(),
        });
        response_body
    };

    Ok(response_from_parts(
        status,
        &outgoing_response_headers,
        boxed_body_with_trailers(response_body_preview, outgoing_response_trailers),
    ))
}

async fn handle_connect<S: SessionStore>(
    req: Request<Incoming>,
    peer: SocketAddr,
    config: ProxyConfig,
    store: Arc<S>,
    rules: Arc<RuleEngine>,
    event_tx: broadcast::Sender<CaptureEvent>,
    https_mitm: Arc<Mutex<HttpsMitmState>>,
) -> Result<Response<ProxyBody>, ProxyError> {
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
    session.timeline.push(TimelineEntry {
        name: "connect".into(),
        started_at: session.started_at,
        ended_at: Some(session.started_at),
        duration_ms: Some(0),
    });

    let use_mitm = {
        let guard = https_mitm.lock().await;
        guard.enabled() && guard.is_ready()
    };
    let upgrade = hyper::upgrade::on(req);
    let target = format!("{host}:{port}");
    if use_mitm {
        let store = store.clone();
        let rules = rules.clone();
        let event_tx = event_tx.clone();
        let config = config.clone();
        let https_mitm = https_mitm.clone();
        tokio::spawn(async move {
            match upgrade.await {
                Ok(upgraded) => {
                    if let Err(err) =
                        mitm_connection(upgraded, peer, config, store, rules, event_tx, https_mitm)
                            .await
                    {
                        warn!(error = %err, "mitm tunnel failed");
                    }
                }
                Err(err) => warn!(error = %err, "upgrade failed"),
            }
        });
    } else {
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
    }

    session.timeline.push(TimelineEntry {
        name: "completed".into(),
        started_at: Utc::now(),
        ended_at: Some(Utc::now()),
        duration_ms: Some(0),
    });
    session.finish(SessionState::Completed);
    store
        .insert_session(&session)
        .await
        .map_err(|e| ProxyError::Store(e.to_string()))?;
    let _ = event_tx.send(CaptureEvent { session });
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(empty_body())
        .map_err(|e| ProxyError::Http(e.to_string()))?)
}

fn build_target_uri(
    req: &Request<Incoming>,
    session: &CapturedSession,
    default_scheme: &str,
) -> Result<Uri, ProxyError> {
    if req.uri().scheme().is_some() && req.uri().authority().is_some() {
        return Ok(req.uri().clone());
    }
    let host = session
        .host
        .clone()
        .or_else(|| {
            req.headers()
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string())
        })
        .ok_or_else(|| ProxyError::InvalidRequest("missing host".into()))?;
    let scheme = req.uri().scheme_str().unwrap_or(default_scheme);
    let path = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    format!("{scheme}://{host}{path}")
        .parse()
        .map_err(|e| ProxyError::InvalidRequest(format!("invalid target uri: {e}")))
}
async fn handle_websocket<S: SessionStore>(
    req: Request<Incoming>,
    config: ProxyConfig,
    store: Arc<S>,
    event_tx: broadcast::Sender<CaptureEvent>,
) -> Result<Response<ProxyBody>, ProxyError> {
    let target_uri = websocket_target_uri(&req)?;
    if target_uri.scheme_str() != Some("ws") {
        return Err(ProxyError::InvalidRequest(
            "only cleartext ws:// capture is supported; wss:// remains a CONNECT tunnel".into(),
        ));
    }
    let host = target_uri
        .host()
        .ok_or_else(|| ProxyError::InvalidRequest("missing websocket host".into()))?
        .to_string();
    let port = target_uri.port_u16().unwrap_or(80);

    let mut session = CapturedSession::new(SessionKind::WebSocket);
    session.started_at = Utc::now();
    session.method = Some("GET".into());
    session.url = Some(target_uri.to_string());
    session.scheme = Some("ws".into());
    session.host = Some(host.clone());
    session.port = Some(port);
    session.path = target_uri
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .or_else(|| Some("/".into()));
    session.request_headers = redact_headers(&headers_to_pairs(req.headers()));
    session.status = Some(StatusCode::SWITCHING_PROTOCOLS.as_u16());
    session.timeline.push(TimelineEntry {
        name: "upgrade".into(),
        started_at: session.started_at,
        ended_at: Some(session.started_at),
        duration_ms: Some(0),
    });

    let response = websocket_upgrade_response(&req)?;
    let upgrade = hyper::upgrade::on(req);
    let target = format!("{host}:{port}");
    let session = Arc::new(Mutex::new(session));
    let session_task = session.clone();
    let store_task = store.clone();
    let event_task = event_tx.clone();
    tokio::spawn(async move {
        match upgrade.await {
            Ok(upgraded) => {
                if let Err(err) = relay_websocket(
                    upgraded,
                    target,
                    session_task,
                    config.max_websocket_frame_bytes,
                    store_task,
                    event_task,
                )
                .await
                {
                    warn!(error = %err, "websocket tunnel failed");
                }
            }
            Err(err) => warn!(error = %err, "websocket upgrade failed"),
        }
    });
    Ok(response)
}

async fn relay_websocket<S: SessionStore>(
    upgraded: Upgraded,
    target: String,
    session: Arc<Mutex<CapturedSession>>,
    max_frame_bytes: usize,
    store: Arc<S>,
    event_tx: broadcast::Sender<CaptureEvent>,
) -> Result<(), ProxyError> {
    let upstream =
        TcpStream::connect(&target)
            .await
            .map_err(|e| ProxyError::UpstreamConnectFailed {
                target: target.clone(),
                source: e,
            })?;
    let client_io = TokioIo::new(upgraded);
    let (mut client_read, mut client_write) = split(client_io);
    let (mut upstream_read, mut upstream_write) = split(upstream);

    let client_task = relay_websocket_frames(
        &mut client_read,
        &mut upstream_write,
        "client",
        max_frame_bytes,
        session.clone(),
    );
    let upstream_task = relay_websocket_frames(
        &mut upstream_read,
        &mut client_write,
        "upstream",
        max_frame_bytes,
        session.clone(),
    );
    let (client_result, upstream_result) = tokio::join!(client_task, upstream_task);

    let session_snapshot = {
        let mut session = session.lock().await;
        if let Err(err) = &client_result {
            session.error = Some(err.to_string());
            session.finish(SessionState::Failed);
        } else if let Err(err) = &upstream_result {
            session.error = Some(err.to_string());
            session.finish(SessionState::Failed);
        } else {
            session.finish(SessionState::Completed);
        }
        session.timeline.push(TimelineEntry {
            name: "completed".into(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            duration_ms: Some(0),
        });
        session.clone()
    };
    store
        .insert_session(&session_snapshot)
        .await
        .map_err(|e| ProxyError::Store(e.to_string()))?;
    let _ = event_tx.send(CaptureEvent {
        session: session_snapshot.clone(),
    });
    if let Err(err) = client_result {
        return Err(err);
    }
    if let Err(err) = upstream_result {
        return Err(err);
    }
    Ok(())
}

async fn relay_websocket_frames<R, W>(
    reader: &mut R,
    writer: &mut W,
    direction: &'static str,
    max_frame_bytes: usize,
    session: Arc<Mutex<CapturedSession>>,
) -> Result<(), ProxyError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        let Some(frame) = read_websocket_frame(reader).await? else {
            break;
        };
        let preview = crate::capture::preview_websocket_frame(
            direction,
            frame.opcode_name(),
            &frame.payload,
            max_frame_bytes,
        );
        {
            let mut session = session.lock().await;
            if session.timeline.len() == 1 {
                session.timeline.push(TimelineEntry {
                    name: "first_frame".into(),
                    started_at: Utc::now(),
                    ended_at: Some(Utc::now()),
                    duration_ms: Some(0),
                });
            }
            session.websocket_frames.push(preview);
        }
        writer
            .write_all(&frame.raw)
            .await
            .map_err(|e| ProxyError::Http(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| ProxyError::Http(e.to_string()))?;
        if frame.opcode == 0x8 {
            break;
        }
    }
    Ok(())
}

struct WebSocketFrame {
    opcode: u8,
    payload: Vec<u8>,
    raw: Vec<u8>,
}

impl WebSocketFrame {
    fn opcode_name(&self) -> &'static str {
        match self.opcode {
            0x1 => "text",
            0x2 => "binary",
            0x8 => "close",
            0x9 => "ping",
            0xA => "pong",
            _ => "frame",
        }
    }
}

async fn read_websocket_frame<R>(reader: &mut R) -> Result<Option<WebSocketFrame>, ProxyError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; 2];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(ProxyError::Http(err.to_string())),
    }

    let fin = header[0] & 0x80 != 0;
    let opcode = header[0] & 0x0F;
    let masked = header[1] & 0x80 != 0;
    let mut len = (header[1] & 0x7F) as u64;
    let mut raw = header.to_vec();
    if len == 126 {
        let mut extended = [0u8; 2];
        reader
            .read_exact(&mut extended)
            .await
            .map_err(|e| ProxyError::Http(e.to_string()))?;
        raw.extend_from_slice(&extended);
        len = u16::from_be_bytes(extended) as u64;
    } else if len == 127 {
        let mut extended = [0u8; 8];
        reader
            .read_exact(&mut extended)
            .await
            .map_err(|e| ProxyError::Http(e.to_string()))?;
        raw.extend_from_slice(&extended);
        len = u64::from_be_bytes(extended);
    }

    let mut mask_key = [0u8; 4];
    if masked {
        reader
            .read_exact(&mut mask_key)
            .await
            .map_err(|e| ProxyError::Http(e.to_string()))?;
        raw.extend_from_slice(&mask_key);
    }

    let mut payload = vec![0u8; len as usize];
    if !payload.is_empty() {
        reader
            .read_exact(&mut payload)
            .await
            .map_err(|e| ProxyError::Http(e.to_string()))?;
    }
    raw.extend_from_slice(&payload);

    let payload = if masked {
        let mut decoded = payload;
        for (index, byte) in decoded.iter_mut().enumerate() {
            *byte ^= mask_key[index % 4];
        }
        decoded
    } else {
        payload
    };

    let _ = fin;
    Ok(Some(WebSocketFrame {
        opcode,
        payload,
        raw,
    }))
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

fn is_websocket_upgrade(req: &Request<Incoming>) -> bool {
    req.method() == Method::GET
        && req
            .headers()
            .get(header::UPGRADE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false)
        && req
            .headers()
            .get(header::CONNECTION)
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("upgrade"))
            })
            .unwrap_or(false)
}

fn websocket_target_uri(req: &Request<Incoming>) -> Result<http::Uri, ProxyError> {
    if req.uri().scheme().is_some() && req.uri().authority().is_some() {
        return req
            .uri()
            .to_string()
            .parse()
            .map_err(|e| ProxyError::InvalidRequest(format!("invalid websocket uri: {e}")));
    }
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ProxyError::InvalidRequest("missing websocket host".into()))?;
    let path = req
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    format!("ws://{host}{path}")
        .parse()
        .map_err(|e| ProxyError::InvalidRequest(format!("invalid websocket uri: {e}")))
}

fn websocket_upgrade_response(req: &Request<Incoming>) -> Result<Response<ProxyBody>, ProxyError> {
    let key = req
        .headers()
        .get(header::SEC_WEBSOCKET_KEY)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ProxyError::InvalidRequest("missing Sec-WebSocket-Key".into()))?;
    let mut accept_seed = Vec::with_capacity(key.len() + 36);
    accept_seed.extend_from_slice(key.as_bytes());
    accept_seed.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let accept = STANDARD.encode(digest(&SHA1_FOR_LEGACY_USE_ONLY, &accept_seed).as_ref());
    let mut builder = Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(header::CONNECTION, "Upgrade")
        .header(header::UPGRADE, "websocket")
        .header(header::SEC_WEBSOCKET_ACCEPT, accept);
    if let Some(protocol) = req.headers().get(header::SEC_WEBSOCKET_PROTOCOL) {
        builder = builder.header(header::SEC_WEBSOCKET_PROTOCOL, protocol);
    }
    builder
        .body(empty_body())
        .map_err(|e| ProxyError::Http(e.to_string()))
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
    body: ProxyBody,
) -> Response<ProxyBody> {
    let mut builder =
        Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK));
    for header in headers {
        builder = builder.header(&header.name, &header.value);
    }
    builder
        .body(body)
        .unwrap_or_else(|_| Response::new(empty_body()))
}

fn internal_error(err: ProxyError) -> Response<ProxyBody> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(boxed_body(Bytes::from(format!("proxy error: {err}"))))
        .unwrap_or_else(|_| Response::new(empty_body()))
}

fn parse_host_port(host: Option<&http::HeaderValue>) -> Option<u16> {
    host.and_then(|value| value.to_str().ok())
        .and_then(|host| host.rsplit(':').next()?.parse::<u16>().ok())
}

fn preview_body_with_headers(
    headers: &[HeaderPair],
    bytes: &[u8],
    max_bytes: usize,
) -> crate::model::BodyPreview {
    let content_type = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.as_str());
    let content_encoding = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-encoding"))
        .map(|header| header.value.as_str());
    preview_body_with_encoding(content_type, content_encoding, bytes, max_bytes)
}
