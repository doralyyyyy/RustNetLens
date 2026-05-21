use base64::Engine;
use http::{HeaderName, HeaderValue, Method};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::capture::preview_response_body;
use crate::error::ReplayError;
use crate::model::{CapturedSession, HeaderPair, SessionKind, SessionState};
use crate::store::SessionStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayResult {
    pub source_session_id: String,
    pub replayed_session: CapturedSession,
}

pub async fn replay_session<S: SessionStore>(
    store: &S,
    session_id: uuid::Uuid,
) -> Result<CapturedSession, ReplayError> {
    let session = store
        .get_session(session_id)
        .await
        .map_err(|e| ReplayError::Store(e.to_string()))?;
    let session = session.ok_or(ReplayError::NotReplayable)?;
    if session.kind != SessionKind::Http {
        return Err(ReplayError::NotReplayable);
    }

    let url = session.url.clone().ok_or(ReplayError::NotReplayable)?;
    let method = session.method.clone().unwrap_or_else(|| "GET".to_string());
    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| ReplayError::Request(e.to_string()))?;
    let mut req_builder = client.request(
        Method::from_bytes(method.as_bytes()).map_err(|e| ReplayError::Request(e.to_string()))?,
        &url,
    );

    for header in &session.request_headers {
        if let Ok(name) = HeaderName::from_bytes(header.name.as_bytes()) {
            if let Ok(value) = HeaderValue::from_str(&header.value) {
                req_builder = req_builder.header(name, value);
            }
        }
    }

    let body = if let Some(text) = &session.request_body.text {
        text.clone().into_bytes()
    } else if let Some(base64) = &session.request_body.base64 {
        base64::engine::general_purpose::STANDARD
            .decode(base64)
            .map_err(|e| ReplayError::Request(e.to_string()))?
    } else {
        Vec::new()
    };
    let response = req_builder
        .body(body.clone())
        .send()
        .await
        .map_err(|e| ReplayError::Request(e.to_string()))?;

    let status = response.status().as_u16();
    let response_content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let response_headers = response
        .headers()
        .iter()
        .map(|(name, value)| HeaderPair {
            name: name.to_string(),
            value: value.to_str().unwrap_or_default().to_string(),
        })
        .collect::<Vec<_>>();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| ReplayError::Request(e.to_string()))?;
    let replayed = CapturedSession {
        id: uuid::Uuid::new_v4(),
        kind: SessionKind::Http,
        state: SessionState::Completed,
        started_at: chrono::Utc::now(),
        ended_at: Some(chrono::Utc::now()),
        duration_ms: Some(0),
        method: Some(method),
        url: Some(url),
        scheme: session.scheme.clone(),
        host: session.host.clone(),
        port: session.port,
        path: session.path.clone(),
        status: Some(status),
        request_headers: session.request_headers.clone(),
        response_headers,
        request_body: session.request_body.clone(),
        response_body: preview_response_body(response_content_type.as_deref(), None, &bytes),
        grpc_request_metadata: Vec::new(),
        grpc_response_metadata: Vec::new(),
        grpc_request_trailers: Default::default(),
        grpc_response_trailers: Default::default(),
        timeline: Vec::new(),
        websocket_frames: Vec::new(),
        bytes_up: body.len() as u64,
        bytes_down: bytes.len() as u64,
        error: None,
        matched_rule_ids: Vec::new(),
    };
    Ok(replayed)
}
