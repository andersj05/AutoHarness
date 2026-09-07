fn main() {
    // Both desktop entry points use the native window subsystem when packaged.
    #[cfg(feature = "gui-package")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        for binary in ["autoharness", "ah"] {
            println!("cargo:rustc-link-arg-bin={binary}=/SUBSYSTEM:WINDOWS");
            println!("cargo:rustc-link-arg-bin={binary}=/ENTRY:mainCRTStartup");
        }
    }
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
