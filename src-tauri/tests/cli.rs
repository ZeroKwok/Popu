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
    let validate = |flags: &[&str]| Command::new(env!("CARGO_BIN_EXE_popu"))
        .args(["config", "validate"])
        .args(flags)
        .arg(&path)
        .output()
        .expect("run popu");

    assert_eq!(validate(&[]).status.code(), Some(12));
    assert!(!path.exists());

    std::fs::write(&path, "[notification]\nmax_visible = 2").unwrap();
    for flags in [&[][..], &["--path"][..]] {
        let valid = validate(flags);
        assert!(valid.status.success(), "{}", String::from_utf8_lossy(&valid.stderr));
        let result: serde_json::Value = serde_json::from_slice(&valid.stdout).unwrap();
        assert_eq!(result["status"], "valid");
    }

    std::fs::write(&path, "[notification]\nmax_visible = 0").unwrap();
    let invalid = validate(&[]);
    assert_eq!(invalid.status.code(), Some(12));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("/notification/max_visible"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[notification]\nmax_visible = 0");
}

fn config_command(directory: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_popu"));
    let config_home = if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_CONFIG_HOME" };
    command.env(config_home, directory).arg("config");
    command
}

#[test]
fn bare_config_initializes_only_a_missing_default_file() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| config_command(dir.path()).args(args).output().unwrap();
    let folder = if cfg!(windows) { "Popu" } else { "popu" };
    let path = dir.path().join(folder).join("config.toml");

    let output = run(&["path"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim_end(), path.to_str().unwrap());
    assert_eq!(run(&["validate"]).status.code(), Some(12));
    assert!(!path.parent().unwrap().exists());

    for initialized in [true, false] {
        let output = run(&[]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["status"], "valid");
        assert_eq!(result["initialized"], initialized);
    }

    let invalid = "[notification]\nmax_visible = 0";
    std::fs::write(&path, invalid).unwrap();
    assert_eq!(run(&[]).status.code(), Some(12));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
}

#[test]
fn config_init_requires_force_to_replace_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| config_command(dir.path()).args(args).output().unwrap();
    let created = run(&["init"]);
    assert!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));
    let result: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(result["status"], "initialized");
    let path = PathBuf::from(result["path"].as_str().unwrap());

    let customized = "[notification]\nmax_visible = 2";
    std::fs::write(&path, customized).unwrap();
    let existing = run(&["init"]);
    assert_eq!(existing.status.code(), Some(12));
    assert!(String::from_utf8_lossy(&existing.stderr).contains("init --force"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), customized);

    let replaced = run(&["init", "--force"]);
    assert!(replaced.status.success(), "{}", String::from_utf8_lossy(&replaced.stderr));
    let template = std::fs::read_to_string(&path).unwrap();
    assert!(template.contains("# max_visible = 3"));
    let overrides: toml::Table = toml::from_str(&template).unwrap();
    assert!(overrides.values().all(|value| value.as_table().unwrap().is_empty()));
    assert!(run(&["validate"]).status.success());
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
