use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::error::StoreError;
use crate::model::{CapturedSession, ExportBundle};

pub fn export_bundle_json(sessions: Vec<CapturedSession>) -> Result<String, StoreError> {
    let bundle = ExportBundle {
        format: "rustnetlens-export-v1".into(),
        exported_at: Utc::now(),
        sessions,
    };
    serde_json::to_string_pretty(&bundle).map_err(|e| StoreError::Serialization(e.to_string()))
}

pub fn export_sessions_json(sessions: Vec<CapturedSession>) -> Result<String, StoreError> {
    serde_json::to_string_pretty(&sessions).map_err(|e| StoreError::Serialization(e.to_string()))
}

pub fn export_sessions_har_like(sessions: Vec<CapturedSession>) -> Result<String, StoreError> {
    let entries = sessions
        .into_iter()
        .map(|session| {
            serde_json::json!({
                "id": session.id,
                "startedAt": session.started_at,
                "endedAt": session.ended_at,
                "kind": session.kind,
                "state": session.state,
                "method": session.method,
                "url": session.url,
                "host": session.host,
                "status": session.status,
                "durationMs": session.duration_ms,
                "bytesUp": session.bytes_up,
                "bytesDown": session.bytes_down,
                "requestHeaders": session.request_headers,
                "responseHeaders": session.response_headers,
                "requestBody": session.request_body,
                "responseBody": session.response_body,
                "error": session.error,
                "matchedRuleIds": session.matched_rule_ids,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&serde_json::json!({
        "log": {
            "version": "1.2",
            "creator": {"name": "RustNetLens", "version": env!("CARGO_PKG_VERSION")},
            "entries": entries
        }
    }))
    .map_err(|e| StoreError::Serialization(e.to_string()))
}

pub fn write_export_file(
    base_dir: impl AsRef<Path>,
    content: &str,
    suffix: &str,
) -> Result<PathBuf, StoreError> {
    let base_dir = base_dir.as_ref();
    fs::create_dir_all(base_dir).map_err(|e| StoreError::Database(e.to_string()))?;
    let file_name = format!(
        "rustnetlens-export-{}.{}",
        Utc::now().format("%Y%m%d-%H%M%S"),
        suffix
    );
    let path = base_dir.join(file_name);
    fs::write(&path, content).map_err(|e| StoreError::Database(e.to_string()))?;
    Ok(path)
}
