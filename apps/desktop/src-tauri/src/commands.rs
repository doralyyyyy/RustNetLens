use chrono::Utc;
use rustnetlens_core::{
    CapturedSession, ProxyStatus, Rule, RuleEngine, SessionFilter, SessionStore, SessionSummary,
    TrafficOverview, export_sessions_har_like, export_sessions_json, replay_session as replay_core,
    write_export_file,
};
use tauri::State;
use uuid::Uuid;

use crate::state::AppState;

#[tauri::command]
pub async fn proxy_status(state: State<'_, AppState>) -> Result<ProxyStatus, String> {
    Ok(state.proxy_status().await)
}

#[tauri::command]
pub async fn start_proxy(state: State<'_, AppState>, port: u16) -> Result<String, String> {
    state.start_proxy(port).await
}

#[tauri::command]
pub async fn stop_proxy(state: State<'_, AppState>) -> Result<(), String> {
    state.stop_proxy().await
}

#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    filter: SessionFilter,
    limit: u32,
    offset: u32,
) -> Result<Vec<SessionSummary>, String> {
    state
        .store
        .list_sessions(filter, limit, offset)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn traffic_overview(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<TrafficOverview, String> {
    state
        .store
        .traffic_overview(limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_session_detail(
    state: State<'_, AppState>,
    id: String,
) -> Result<CapturedSession, String> {
    let id = Uuid::parse_str(&id).map_err(|e| format!("invalid session id: {e}"))?;
    state
        .store
        .get_session(id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "session not found".to_string())
}

#[tauri::command]
pub async fn clear_sessions(state: State<'_, AppState>) -> Result<(), String> {
    state
        .store
        .clear_sessions()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_rules(state: State<'_, AppState>) -> Result<Vec<Rule>, String> {
    state.store.list_rules().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_rule(state: State<'_, AppState>, mut rule: Rule) -> Result<(), String> {
    RuleEngine::validate_rule(&rule)
        .await
        .map_err(|e| e.to_string())?;
    rule.updated_at = Utc::now();
    state
        .store
        .save_rule(&rule)
        .await
        .map_err(|e| e.to_string())?;
    let rules = state.store.list_rules().await.map_err(|e| e.to_string())?;
    state.rules.set_rules(rules).await;
    Ok(())
}

#[tauri::command]
pub async fn delete_rule(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .store
        .delete_rule(&id)
        .await
        .map_err(|e| e.to_string())?;
    let rules = state.store.list_rules().await.map_err(|e| e.to_string())?;
    state.rules.set_rules(rules).await;
    Ok(())
}

#[tauri::command]
pub async fn replay_session(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let id = Uuid::parse_str(&id).map_err(|e| format!("invalid session id: {e}"))?;
    let mut replayed = replay_core(state.store.as_ref(), id)
        .await
        .map_err(|e| e.to_string())?;
    replayed.finish(rustnetlens_core::SessionState::Completed);
    let new_id = replayed.id.to_string();
    state
        .store
        .insert_session(&replayed)
        .await
        .map_err(|e| e.to_string())?;
    let _ = state
        .event_tx
        .send(rustnetlens_core::CaptureEvent { session: replayed });
    Ok(new_id)
}

#[tauri::command]
pub async fn export_sessions(
    state: State<'_, AppState>,
    ids: Vec<String>,
    format: Option<String>,
) -> Result<String, String> {
    let parsed = ids
        .iter()
        .map(|id| Uuid::parse_str(id).map_err(|e| format!("invalid session id {id}: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let sessions = state
        .store
        .get_sessions_by_ids(&parsed)
        .await
        .map_err(|e| e.to_string())?;
    let format = format.unwrap_or_else(|| "json".into());
    let (content, suffix) = match format.as_str() {
        "har" => (
            export_sessions_har_like(sessions).map_err(|e| e.to_string())?,
            "har",
        ),
        _ => (
            export_sessions_json(sessions).map_err(|e| e.to_string())?,
            "json",
        ),
    };
    let path = write_export_file(&state.export_dir, &content, suffix).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}
