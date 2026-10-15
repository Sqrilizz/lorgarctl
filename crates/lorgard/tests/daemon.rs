use lorgar_core::ipc::{self, Action, Request, Response};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn disconnected_daemon_serves_ipc_rejects_bad_frames_and_shuts_down() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = Process(
        Command::new(env!("CARGO_BIN_EXE_lorgard"))
            .env("XDG_RUNTIME_DIR", dir.path())
            .env("XDG_CONFIG_HOME", dir.path())
            .args([
                "--foreground",
                "none",
                "--device",
                "/dev/lorgar-test-not-present",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let socket = dir.path().join("lorgar/lorgard.sock");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !socket.exists() && Instant::now() < deadline {
        assert!(child.0.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(10));
    }
    let response = ipc::request(&socket, Action::Status).unwrap();
    assert!(response.ok);
    assert!(response.state.unwrap().device.is_none());
    assert_eq!(
        response.config_path.unwrap(),
        dir.path().join("lorgar/config.json")
    );
    let response = ipc::request(
        &socket,
        Action::ApplyDraft {
            profile: Default::default(),
        },
    )
    .unwrap();
    assert!(!response.ok);
    assert_eq!(response.error_code, Some(2));
    let response = ipc::request(
        &socket,
        Action::ApplyDraft {
            profile: lorgar_core::profiles::Profile {
                polling: Some(1000),
                ..Default::default()
            },
        },
    )
    .unwrap();
    assert_eq!(response.error_code, Some(3));

    let response = ipc::request(&socket, Action::GetDpiTable).unwrap();
    assert!(!response.ok);
    assert_eq!(response.error_code, Some(3));

    let response = ipc::request(&socket, Action::GetButtonMappings { secondary: false }).unwrap();
    assert!(!response.ok);
    assert_eq!(response.error_code, Some(3));

    let response = ipc::request(&socket, Action::DumpMemory).unwrap();
    assert!(!response.ok);
    assert_eq!(response.error_code, Some(3));

    let response = ipc::request(&socket, Action::SetDpiStages { stages: vec![50] }).unwrap();
    assert!(!response.ok);
    assert_eq!(response.error_code, Some(2));

    let response = ipc::request(
        &socket,
        Action::SetDpiStages {
            stages: vec![800, 1600],
        },
    )
    .unwrap();
    assert!(!response.ok);
    assert_eq!(response.error_code, Some(3));
    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    ipc::write_frame(
        &mut stream,
        &Request {
            version: 99,
            action: Action::Status,
        },
    )
    .unwrap();
    let response: Response = ipc::read_frame(&mut stream).unwrap();
    assert!(!response.ok);
    assert_eq!(response.error_code, Some(2));
    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(&u32::MAX.to_le_bytes()).unwrap();
    assert!(!ipc::read_frame::<Response>(&mut stream).unwrap().ok);
    let _stalled = UnixStream::connect(&socket).unwrap();
    let start = Instant::now();
    assert!(ipc::request(&socket, Action::Status).unwrap().ok);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .unwrap()
        .success());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "daemon did not terminate cleanly"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!socket.exists());
}
