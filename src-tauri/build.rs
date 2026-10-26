fn main() {
    let worker = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/vendor/chart-worker.js");
    assert!(worker.is_file(), "Missing local chart renderer. Run npm ci and npm run build:ui before building Rust targets.");
    tauri_build::build()
}
