fn main() {
    for name in ["chart-worker.js", "chart-runtime.js"] {
        let asset = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/vendor").join(name);
        assert!(asset.is_file(), "Missing local chart renderer. Run npm ci and npm run build:ui before building Rust targets.");
    }
    // Tauri embeds Common Controls v6 in the application executable. Native
    // WebView tests need the same activation context in their test executables.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/windows.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::build()
}
