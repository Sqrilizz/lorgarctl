use std::process::Command;

#[test]
fn model_exports_without_a_display_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_lorgar-app"))
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .args(["--export-model", path.to_str().unwrap()])
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let obj = std::fs::read(path.join("msa10w.obj")).unwrap();
    let mtl = std::fs::read(path.join("msa10w.mtl")).unwrap();
    assert!(obj.len() > 100_000);
    assert!(mtl.starts_with(b"newmtl graphite_shell"));
    assert!(!run().status.success());
    assert_eq!(std::fs::read(path.join("msa10w.obj")).unwrap(), obj);
    assert_eq!(std::fs::read(path.join("msa10w.mtl")).unwrap(), mtl);
}
