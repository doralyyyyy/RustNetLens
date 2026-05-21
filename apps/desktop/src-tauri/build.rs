fn main() {
    let attrs =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "proxy_status",
            "start_proxy",
            "stop_proxy",
            "list_sessions",
            "traffic_overview",
            "get_session_detail",
            "clear_sessions",
            "list_rules",
            "save_rule",
            "delete_rule",
            "replay_session",
            "export_sessions",
            "https_mitm_status",
            "generate_root_ca",
            "set_https_mitm_enabled",
            "list_collections",
            "save_collection",
            "delete_collection",
            "add_session_to_collection",
        ]));
    tauri_build::try_build(attrs).expect("failed to run tauri-build");
}
