use std::net::SocketAddr;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use reqwest::{Client, Proxy};
use rustnetlens_core::{ProxyConfig, ProxyServer, RuleEngine, SqliteStore};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, broadcast, oneshot};

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
                                let response_body = if body.is_empty() && path == "/ping" {
                                    Bytes::from_static(b"pong")
                                } else {
                                    Bytes::from_static(b"ok")
                                };
                                Ok::<_, std::convert::Infallible>(
                                    Response::builder()
                                        .status(StatusCode::OK)
                                        .body(Full::<Bytes>::from(response_body))
                                        .unwrap(),
                                )
                            }
                        });
                        let io = TokioIo::new(stream);
                        let _ = http1::Builder::new().serve_connection(io, service).await;
                    });
                }
            }
        }
    });
    (addr, counter, shutdown_tx)
}

fn proxy_client(proxy_addr: SocketAddr) -> Client {
    Client::builder()
        .proxy(Proxy::http(format!("http://{proxy_addr}")).unwrap())
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn proxy_smoke_perf() {
    let _guard = SERIAL.get_or_init(|| Mutex::new(())).lock().await;
    let dir = tempdir().unwrap();
    let store = Arc::new(
        SqliteStore::new(dir.path().join("perf.sqlite3"))
            .await
            .unwrap(),
    );
    let rules = Arc::new(RuleEngine::default());
    let (upstream_addr, _, upstream_shutdown) = start_http_server().await;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    drop(listener);
    let config = ProxyConfig {
        listen_addr: proxy_addr,
        max_request_body_bytes: 1024 * 1024,
        max_response_body_bytes: 2 * 1024 * 1024,
    };
    let (event_tx, _) = broadcast::channel(32);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = ProxyServer::new(config, store.clone(), rules, event_tx);
    tokio::spawn(async move {
        let _ = server.run(shutdown_rx).await;
    });

    let client = proxy_client(proxy_addr);
    let started = Instant::now();
    for _ in 0..100 {
        let response = client
            .get(format!("http://{upstream_addr}/ping"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.unwrap();
        assert_eq!(body, "pong");
    }
    let elapsed = started.elapsed();
    let avg_ms = elapsed.as_secs_f64() * 1000.0 / 100.0;
    eprintln!(
        "proxy_smoke_perf: total={:.2}ms avg={:.2}ms req/s={:.2}",
        elapsed.as_secs_f64() * 1000.0,
        avg_ms,
        100.0 / elapsed.as_secs_f64()
    );

    let _ = shutdown_tx.send(());
    let _ = upstream_shutdown.send(());
}
