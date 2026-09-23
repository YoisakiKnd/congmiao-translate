fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "translate",
            "translate_compare",
            "test_engine",
            "lookup_dict",
            "history_action",
            "vocabulary_action",
            "clear_cache",
            "speak",
            "app_action",
            "job_action",
            "get_settings",
            "save_settings",
            "daemon_status",
        ]),
    ))
    .expect("failed to run tauri build script");
}
