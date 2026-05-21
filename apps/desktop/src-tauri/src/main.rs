mod commands;
mod state;

use std::path::PathBuf;

use state::AppState;
use tauri::Manager;

fn app_data_dir(app: &tauri::App) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&base).map_err(|e| format!("failed to create app data dir: {e}"))?;
    Ok(base)
}

fn main() {
    tracing_subscriber::fmt::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_dir = app_data_dir(app)?;
            let state =
                tauri::async_runtime::block_on(AppState::new(app.handle().clone(), app_dir))
                    .map_err(|e| format!("failed to initialize state: {e}"))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::proxy_status,
            commands::start_proxy,
            commands::stop_proxy,
            commands::list_sessions,
            commands::traffic_overview,
            commands::get_session_detail,
            commands::clear_sessions,
            commands::list_rules,
            commands::save_rule,
            commands::delete_rule,
            commands::replay_session,
            commands::export_sessions
        ])
        .run(tauri::generate_context!())
        .expect("failed to run RustNetLens");
}
