//! Diagnose the WebView prerequisite before starting a detached GUI process.
use std::io;

#[cfg(any(windows, test))]
const INSTALL_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

#[cfg(windows)]
pub(crate) fn check() -> io::Result<()> {
    require_runtime(tauri::webview_version().map_err(|error| error.to_string())).map(|_| ())
}

#[cfg(not(windows))]
pub(crate) fn check() -> io::Result<()> {
    Ok(())
}

#[cfg(any(windows, test))]
fn require_runtime(result: Result<String, String>) -> io::Result<String> {
    match result {
        Ok(version) if !version.trim().is_empty() => Ok(version),
        result => {
            let detail = result
                .err()
                .unwrap_or_else(|| "the loader returned an empty version".into());
            Err(io::Error::new(io::ErrorKind::NotFound, format!("Microsoft Edge WebView2 Runtime is unavailable for this user ({detail}). Install or repair the Evergreen Runtime manually from {INSTALL_URL}, then retry Popu.")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_runtime_passes_without_side_effects() {
        assert_eq!(
            require_runtime(Ok("130.0.1.2".into())).unwrap(),
            "130.0.1.2"
        );
    }

    #[test]
    fn missing_runtime_has_a_manual_install_link_and_loader_detail() {
        for value in [Err("loader failure 0x80070002".into()), Ok(String::new())] {
            let error = require_runtime(value).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::NotFound);
            assert!(error.to_string().contains(INSTALL_URL));
            assert!(error.to_string().contains("manually"));
            assert!(error.to_string().contains("unavailable for this user"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn real_loader_reports_missing_runtime_without_installing_or_opening_ui() {
        let empty = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "preflight::tests::runtime_probe",
                "--ignored",
                "--nocapture",
            ])
            .env("POPU_RUNTIME_PROBE", "1")
            .env("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", empty.path())
            .output()
            .unwrap();
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(12), "{diagnostic}");
        assert!(diagnostic.contains(INSTALL_URL), "{diagnostic}");
        assert!(diagnostic.contains("manually"), "{diagnostic}");
        assert_eq!(std::fs::read_dir(empty.path()).unwrap().count(), 0);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "subprocess helper for WebView2 detection; never starts a Host"]
    fn runtime_probe() {
        if std::env::var_os("POPU_RUNTIME_PROBE").is_none() {
            return;
        }
        if let Err(error) = check() {
            eprintln!("Popu error: {error}");
            std::process::exit(12);
        }
    }
}
