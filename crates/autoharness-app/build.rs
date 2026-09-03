fn main() {
    #[cfg(feature = "gui")]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "gui_connect",
            "gui_dispatch",
            "gui_submit_credential",
            "gui_acknowledge_frame",
        ]),
    ))
    .expect("Tauri build integration failed");
}
