use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rustnetlens_core::{
    CaptureEvent, ProxyConfig, ProxyServer, ProxyStatus, RuleEngine, SessionSummary, SqliteStore,
};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, broadcast, oneshot};

pub struct ProxyHandle {
    pub listen_addr: SocketAddr,
    pub started_at: DateTime<Utc>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: tauri::async_runtime::JoinHandle<()>,
}

impl ProxyHandle {
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }
}

pub struct AppState {
    pub store: Arc<SqliteStore>,
    pub rules: Arc<RuleEngine>,
    pub event_tx: broadcast::Sender<CaptureEvent>,
    pub proxy: Mutex<Option<ProxyHandle>>,
    pub export_dir: PathBuf,
}

impl AppState {
    pub async fn new(app_handle: AppHandle, app_dir: PathBuf) -> Result<Self, String> {
        let db_path = app_dir.join("rustnetlens.sqlite3");
        let export_dir = app_dir.join("exports");
        std::fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;
        let store = Arc::new(SqliteStore::new(db_path).await.map_err(|e| e.to_string())?);
        let rules = Arc::new(RuleEngine::default());
        let existing_rules = store.list_rules().await.map_err(|e| e.to_string())?;
        rules.set_rules(existing_rules).await;
        let (event_tx, mut event_rx) = broadcast::channel::<CaptureEvent>(1024);
        let emitter = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                let summary = SessionSummary::from_session(&event.session);
                let _ = emitter.emit("session://captured", summary);
            }
        });
        Ok(Self {
            store,
            rules,
            event_tx,
            proxy: Mutex::new(None),
            export_dir,
        })
    }

    pub async fn proxy_status(&self) -> ProxyStatus {
        let guard = self.proxy.lock().await;
        if let Some(handle) = guard.as_ref() {
            ProxyStatus {
                running: true,
                listen_addr: Some(format!("http://{}", handle.listen_addr)),
                started_at: Some(handle.started_at),
                active_sessions: 0,
            }
        } else {
            ProxyStatus::stopped()
        }
    }

    pub async fn start_proxy(&self, port: u16) -> Result<String, String> {
        let mut guard = self.proxy.lock().await;
        if guard.is_some() {
            return Err("proxy is already running".into());
        }
        let listen_addr: SocketAddr = format!("127.0.0.1:{port}")
            .parse()
            .map_err(|e| format!("invalid port: {e}"))?;
        let config = ProxyConfig {
            listen_addr,
            max_request_body_bytes: 1024 * 1024,
            max_response_body_bytes: 2 * 1024 * 1024,
            max_websocket_frame_bytes: 64 * 1024,
        };
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server = ProxyServer::new(
            config,
            self.store.clone(),
            self.rules.clone(),
            self.event_tx.clone(),
        );
        let task = tauri::async_runtime::spawn(async move {
            if let Err(err) = server.run(shutdown_rx).await {
                tracing::error!(error = %err, "proxy server stopped with error");
            }
        });
        *guard = Some(ProxyHandle {
            listen_addr,
            started_at: Utc::now(),
            shutdown_tx: Some(shutdown_tx),
            task,
        });
        Ok(format!("http://{listen_addr}"))
    }

    pub async fn stop_proxy(&self) -> Result<(), String> {
        let mut guard = self.proxy.lock().await;
        let Some(handle) = guard.take() else {
            return Ok(());
        };
        drop(guard);
        handle.stop().await;
        Ok(())
    }
}
