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
fn invalid_notify_fails_without_starting_ui() {
    let fixture = project_root().join("protocol/examples/invalid/notify-with-form.json");
    let output = Command::new(env!("CARGO_BIN_EXE_popu"))
        .args(["notify", "--request"])
        .arg(fixture)
        .output()
        .expect("run popu");

    assert_eq!(output.status.code(), Some(12));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("notify cannot contain form or actions")
    );
}
