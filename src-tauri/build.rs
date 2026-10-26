fn main() {
    for name in ["chart-worker.js", "chart-runtime.js"] {
        let asset = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/vendor").join(name);
        assert!(asset.is_file(), "Missing local chart renderer. Run npm ci and npm run build:ui before building Rust targets.");
    }
    tauri_build::build()
}
