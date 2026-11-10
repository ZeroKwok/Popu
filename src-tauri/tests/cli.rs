use std::path::PathBuf;
use std::process::Command;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent")
        .to_owned()
}

#[test]
fn lists_builtin_templates_as_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_popu"))
        .args(["templates", "list"])
        .output()
        .expect("run popu");

    assert!(output.status.success());
    let names: Vec<String> = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(names, ["confirm", "form", "notification"]);
}

#[test]
fn validates_host_configuration_without_starting_host() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let validate = || Command::new(env!("CARGO_BIN_EXE_popu"))
        .args(["config", "validate", "--path"])
        .arg(&path)
        .output()
        .expect("run popu");

    std::fs::write(&path, "[notification]\nmax_visible = 2").unwrap();
    let valid = validate();
    assert!(valid.status.success(), "{}", String::from_utf8_lossy(&valid.stderr));
    let result: serde_json::Value = serde_json::from_slice(&valid.stdout).unwrap();
    assert_eq!(result["status"], "valid");

    std::fs::write(&path, "[notification]\nmax_visible = 0").unwrap();
    let invalid = validate();
    assert_eq!(invalid.status.code(), Some(12));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("/notification/max_visible"));
}

#[test]
fn invalid_notify_request_fails_without_starting_ui() {
    let fixture = project_root().join("protocol/examples/invalid/notify-with-form.json");
    let output = Command::new(env!("CARGO_BIN_EXE_popu"))
        .arg("request")
        .arg(fixture)
        .output()
        .expect("run popu");

    assert_eq!(output.status.code(), Some(12));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("notify cannot contain form blocks or actions"));
}

#[test]
fn legacy_mode_commands_and_flags_are_rejected() {
    let executable = env!("CARGO_BIN_EXE_popu");
    let dialog = Command::new(executable)
        .arg("dialog")
        .output()
        .expect("run popu");
    assert!(!dialog.status.success());

    let fixture = project_root().join("protocol/examples/invalid/notify-with-form.json");
    let legacy_flag = Command::new(executable)
        .arg("request")
        .arg(fixture)
        .args(["--template-dir", "unused"])
        .output()
        .expect("run popu");
    assert!(!legacy_flag.status.success());

    let legacy_wait = Command::new(executable)
        .arg("request")
        .arg(project_root().join("protocol/examples/invalid/notify-with-form.json"))
        .arg("--wait")
        .output()
        .expect("run popu");
    assert!(!legacy_wait.status.success());

    let legacy_request_flag = Command::new(executable)
        .arg("request")
        .arg("--request")
        .arg(project_root().join("protocol/examples/invalid/notify-with-form.json"))
        .output()
        .expect("run popu");
    assert!(!legacy_request_flag.status.success());
}

#[test]
fn template_flag_is_accepted_by_request() {
    let fixture = project_root().join("protocol/examples/invalid/notify-with-form.json");
    let output = Command::new(env!("CARGO_BIN_EXE_popu"))
        .arg("request")
        .arg(fixture)
        .args(["--template", "unused"])
        .output()
        .expect("run popu");

    // Schema validation runs after CLI parsing; this proves the new flag was
    // accepted without needing to create a real WebView window in the test.
    assert_eq!(output.status.code(), Some(12));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("notify cannot contain form blocks or actions"));
}

#[cfg(windows)]
#[test]
fn missing_webview_runtime_fails_before_host_startup_with_a_manual_install_link() {
    let empty = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_popu"))
        .args(["host", "run"])
        .env("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", empty.path())
        .output()
        .expect("run popu");
    assert_eq!(output.status.code(), Some(12));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("WebView2 Runtime is unavailable"), "{diagnostic}");
    assert!(diagnostic.contains("https://developer.microsoft.com/microsoft-edge/webview2/"));
    assert!(diagnostic.contains("manually"));
    assert_eq!(std::fs::read_dir(empty.path()).unwrap().count(), 0);
}
