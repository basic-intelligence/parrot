fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_app_snapshot",
            "save_settings",
            "set_launch_at_login",
            "download_model",
            "delete_model",
            "warm_models",
            "start_test_dictation",
            "stop_test_dictation",
            "set_hotkey_monitor_enabled",
            "capture_shortcut",
            "permission_statuses",
            "request_permission",
            "save_recording_result",
            "delete_history_item",
            "clear_history",
        ]),
    ))
    .expect("failed to run tauri build script");
}
