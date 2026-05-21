use std::net::SocketAddr;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use reqwest::{Client, Proxy};
use rustnetlens_core::{
    HeaderPair, HttpsMitmState, ProxyConfig, ProxyServer, Rule, RuleAction, RuleEngine,
    SessionKind, SessionState, SessionStore, SqliteStore,
};
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, broadcast, oneshot};

fn init_test_crypto() {
    let _ = rustnetlens_core::init_crypto_provider();
}

static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

async fn start_http_server() -> (SocketAddr, Arc<AtomicUsize>, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let counter = Arc::new(AtomicUsize::new(0));
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
    let counter_clone = counter.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                accept = listener.accept() => {
                    let (stream, _) = accept.unwrap();
                    let counter = counter_clone.clone();
                    tokio::spawn(async move {
                        let service = service_fn(move |req: Request<Incoming>| {
                            let counter = counter.clone();
                            async move {
                                counter.fetch_add(1, Ordering::SeqCst);
                                let path = req.uri().path().to_string();
                                let body = req.into_body().collect().await.unwrap().to_bytes();
                                let response_body = if path == "/api/user" {
                                    Bytes::from_static(b"{\"id\":1,\"name\":\"upstream\"}")
                                } else if body.is_empty() {
                                    Bytes::from_static(b"upstream-ok")
                                } else {
                                    body
                                };
                                let body: Full<Bytes> = Full::new(response_body);
                                Ok::<_, std::convert::Infallible>(
                                    Response::builder()
                                        .status(StatusCode::OK)
                                        .header("content-type", "application/json")
                                        .body(body)
                                        .unwrap(),
                                )
                            }
                        });
                        let io = TokioIo::new(stream);
                        let _ = http1::Builder::new()
                            .serve_connection(io, service)
                            .await;
                    });
                }
            }
        }
    });
    (addr, counter, shutdown_tx)
}

async fn start_echo_server() -> (SocketAddr, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                accept = listener.accept() => {
                    let (mut stream, _) = accept.unwrap();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        loop {
                            let n = match stream.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => n,
                            };
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    });
                }
            }
        }
    });
    (addr, shutdown_tx)
}

async fn start_proxy_server(
    store: Arc<SqliteStore>,
    rules: Arc<RuleEngine>,
) -> (SocketAddr, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let config = ProxyConfig {
        listen_addr: addr,
        max_request_body_bytes: 1024 * 1024,
        max_response_body_bytes: 2 * 1024 * 1024,
        max_websocket_frame_bytes: 64 * 1024,
    };
    let (event_tx, _) = broadcast::channel(32);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = ProxyServer::new(
        config,
        store,
        rules,
        event_tx,
        Arc::new(Mutex::new(HttpsMitmState::default())),
    );
    tokio::spawn(async move {
        let _ = server.run(shutdown_rx).await;
    });
    (addr, shutdown_tx)
}

fn proxy_client(proxy_addr: SocketAddr) -> Client {
    Client::builder()
        .proxy(Proxy::http(format!("http://{proxy_addr}")).unwrap())
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proxy_http_roundtrip_and_persists_session() {
    let _guard = SERIAL.get_or_init(|| Mutex::new(())).lock().await;
    init_test_crypto();
    let dir = tempdir().unwrap();
    let store = Arc::new(
        SqliteStore::new(dir.path().join("roundtrip.sqlite3"))
            .await
            .unwrap(),
    );
    let rules = Arc::new(RuleEngine::default());
    let (upstream_addr, counter, upstream_shutdown) = start_http_server().await;
    let (proxy_addr, proxy_shutdown) = start_proxy_server(store.clone(), rules).await;

    let client = proxy_client(proxy_addr);
    let response = client
        .get(format!("http://{upstream_addr}/hello"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "upstream-ok");
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    let sessions = SessionStore::list_sessions(&*store, Default::default(), 10, 0)
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].kind, SessionKind::Http);

    let _ = proxy_shutdown.send(());
    let _ = upstream_shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_rule_short_circuits_upstream() {
    let _guard = SERIAL.get_or_init(|| Mutex::new(())).lock().await;
    init_test_crypto();
    let dir = tempdir().unwrap();
    let store = Arc::new(
        SqliteStore::new(dir.path().join("mock.sqlite3"))
            .await
            .unwrap(),
    );
    let rules = Arc::new(RuleEngine::default());
    rules
        .set_rules(vec![Rule {
            id: "mock-1".into(),
            name: "Mock user".into(),
            enabled: true,
            priority: 1,
            match_: rustnetlens_core::RuleMatch {
                url_contains: Some("/api/user".into()),
                method: Some("GET".into()),
                host: None,
                status: None,
            },
            action: RuleAction::MockResponse {
                status: 200,
                headers: vec![HeaderPair {
                    name: "content-type".into(),
                    value: "application/json".into(),
                }],
                body: "{\"id\":1,\"name\":\"demo\"}".into(),
            },
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }])
        .await;
    let (upstream_addr, counter, upstream_shutdown) = start_http_server().await;
    let (proxy_addr, proxy_shutdown) = start_proxy_server(store.clone(), rules).await;

    let client = proxy_client(proxy_addr);
    let response = client
        .get(format!("http://{upstream_addr}/api/user"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.text().await.unwrap(),
        "{\"id\":1,\"name\":\"demo\"}"
    );
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    let sessions = SessionStore::list_sessions(&*store, Default::default(), 10, 0)
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].state, SessionState::Mocked);
    assert_eq!(sessions[0].matched_rule_ids, vec!["mock-1"]);

    let _ = proxy_shutdown.send(());
    let _ = upstream_shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_tunnel_records_session() {
    let _guard = SERIAL.get_or_init(|| Mutex::new(())).lock().await;
    init_test_crypto();
    let dir = tempdir().unwrap();
    let store = Arc::new(
        SqliteStore::new(dir.path().join("connect.sqlite3"))
            .await
            .unwrap(),
    );
    let rules = Arc::new(RuleEngine::default());
    let (upstream_addr, upstream_shutdown) = start_echo_server().await;
    let (proxy_addr, proxy_shutdown) = start_proxy_server(store.clone(), rules).await;

    let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
    let connect_request = format!(
        "CONNECT {} HTTP/1.1\r\nHost: {}\r\n\r\n",
        upstream_addr, upstream_addr
    );
    stream.write_all(connect_request.as_bytes()).await.unwrap();

    let mut response = Vec::new();
    let mut buffer = [0u8; 1];
    while !response.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buffer).await.unwrap();
        assert!(n > 0);
        response.push(buffer[0]);
    }
    let response_text = String::from_utf8_lossy(&response);
    assert!(response_text.contains("200"));

    stream.write_all(b"ping").await.unwrap();
    let mut echoed = [0u8; 4];
    stream.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"ping");

    let sessions = SessionStore::list_sessions(&*store, Default::default(), 10, 0)
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].kind, SessionKind::ConnectTunnel);

    let _ = proxy_shutdown.send(());
    let _ = upstream_shutdown.send(());
}
