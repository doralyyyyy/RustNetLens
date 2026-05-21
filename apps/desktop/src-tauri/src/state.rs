use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rustnetlens_core::{
    CaptureEvent, HttpsMitmState, HttpsMitmStatus, ProxyConfig, ProxyServer, ProxyStatus,
    RequestCollection, RootCaInfo, RuleEngine, SessionId, SessionStore, SessionSummary,
    SqliteStore,
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
    pub app_dir: PathBuf,
    pub https_mitm: Arc<Mutex<HttpsMitmState>>,
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
            app_dir,
            https_mitm: Arc::new(Mutex::new(HttpsMitmState::default())),
        })
    }

    pub async fn proxy_status(&self) -> ProxyStatus {
        let guard = self.proxy.lock().await;
        let https_mitm = self.https_mitm.lock().await.status();
        if let Some(handle) = guard.as_ref() {
            ProxyStatus {
                running: true,
                listen_addr: Some(format!("http://{}", handle.listen_addr)),
                started_at: Some(handle.started_at),
                active_sessions: 0,
                https_mitm,
            }
        } else {
            let mut stopped = ProxyStatus::stopped();
            stopped.https_mitm = https_mitm;
            stopped
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
            self.https_mitm.clone(),
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

    pub async fn https_mitm_status(&self) -> HttpsMitmStatus {
        self.https_mitm.lock().await.status()
    }

    pub async fn ensure_root_ca(&self) -> Result<RootCaInfo, String> {
        let mut guard = self.https_mitm.lock().await;
        let root_dir = self.app_dir.join("certs");
        guard.ensure_root_ca(&root_dir).map_err(|e| e.to_string())
    }

    pub async fn set_https_mitm_enabled(&self, enabled: bool) -> Result<HttpsMitmStatus, String> {
        let mut guard = self.https_mitm.lock().await;
        if enabled && !guard.is_ready() {
            let root_dir = self.app_dir.join("certs");
            guard.ensure_root_ca(&root_dir).map_err(|e| e.to_string())?;
        }
        guard.set_enabled(enabled);
        Ok(guard.status())
    }

    pub async fn list_collections(&self) -> Result<Vec<RequestCollection>, String> {
        self.store
            .list_collections()
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn save_collection(&self, collection: &RequestCollection) -> Result<(), String> {
        self.store
            .save_collection(collection)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn delete_collection(&self, id: &str) -> Result<(), String> {
        self.store
            .delete_collection(id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn add_session_to_collection(
        &self,
        collection_id: &str,
        session_id: SessionId,
    ) -> Result<(), String> {
        let session = self
            .store
            .get_session(session_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "session not found".to_string())?;
        self.store
            .add_session_to_collection(collection_id, &session)
            .await
            .map_err(|e| e.to_string())
    }
}
