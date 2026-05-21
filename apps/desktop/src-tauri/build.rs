fn main() {
    let attrs =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "proxy_status",
            "start_proxy",
            "stop_proxy",
            "list_sessions",
            "get_session_detail",
            "clear_sessions",
            "list_rules",
            "save_rule",
            "delete_rule",
            "replay_session",
            "export_sessions",
        ]));
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
