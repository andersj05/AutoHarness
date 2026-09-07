fn main() {
    // Keep `ah` attached to a terminal, but prevent a console flashing behind
    // installer-launched GUI windows. Rust's CRT entry remains authoritative.
    #[cfg(feature = "gui-package")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg-bin=autoharness=/SUBSYSTEM:WINDOWS");
        println!("cargo:rustc-link-arg-bin=autoharness=/ENTRY:mainCRTStartup");
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
