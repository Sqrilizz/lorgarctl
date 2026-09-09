use std::process::{Command, Output};

fn run(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lorgarctl"))
        .env("XDG_CONFIG_HOME", dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn profile_workflow_and_failed_updates() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["profile", "create", "gaming"],
        vec![
            "profile",
            "set",
            "gaming",
            "--dpi-stage",
            "0=800",
            "--dpi-stage",
            "1=1600",
            "--polling",
            "1000",
            "--rgb",
            "static",
            "--color",
            "ff5500",
        ],
    ] {
        let output = run(dir.path(), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let shown = run(dir.path(), &["profile", "show", "gaming", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(value["dpi_stages"]["0"], 800);
    assert_eq!(value["dpi_stages"]["1"], 1600);
    assert_eq!(value["color"], "ff5500");
    let path = dir.path().join("lorgar/config.json");
    let before = std::fs::read(&path).unwrap();
    let invalid = run(
        dir.path(),
        &["profile", "set", "gaming", "--polling", "7", "--json"],
    );
    assert_eq!(invalid.status.code(), Some(2));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&invalid.stdout).unwrap()["ok"],
        false
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(run(dir.path(), &["profile", "list"]).status.success());
    assert!(run(dir.path(), &["profile", "delete", "gaming"])
        .status
        .success());
    assert_eq!(
        run(dir.path(), &["profile", "show", "gaming"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn invalid_hardware_arguments_are_validated_before_opening() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["dpi", "set", "8", "800"],
        vec!["rgb", "color", "€€"],
        vec!["debug", "send-feature", "€€"],
    ] {
        let output = run(dir.path(), &args);
        assert_eq!(output.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
    let output = run(dir.path(), &["completions", "bash"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("lorgarctl"));
}

#[test]
fn future_config_is_readable_but_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.json");
    let bytes = br#"{"version":99,"profiles":{},"unknown":[1,2,3]}"#;
    std::fs::write(&path, bytes).unwrap();
    let path_str = path.to_str().unwrap();
    assert!(run(dir.path(), &["--config", path_str, "profile", "list"])
        .status
        .success());
    assert!(!run(
        dir.path(),
        &["--config", path_str, "profile", "create", "bad"]
    )
    .status
    .success());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn doctor_returns_structured_diagnostics_without_hardware_commands() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(dir.path(), &["doctor", "--json"]);
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["interfaces"].is_array());
    assert!(value["config_valid"].is_boolean());
    assert!(value["daemon_reachable"].is_boolean());
    assert!(value["foreground_backend"].is_string());
}
