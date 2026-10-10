fn main() {
    // Tauri embeds its Windows manifest in the app binary only, and test
    // binaries without it fail to start (STATUS_ENTRYPOINT_NOT_FOUND): embed
    // ours in every binary instead.
    let mut windows = tauri_build::WindowsAttributes::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        let manifest =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(windows)
            // Listing the commands makes each one need a permission, which
            // only the main window's capability grants (capabilities/main.json).
            .app_manifest(tauri_build::AppManifest::new().commands(&[
                "open_app",
                "switch_profile",
                "press_key",
                "reload",
                "screenshot",
                "profiles",
            ])),
    )
    .expect("failed to run the Tauri build script");
}
