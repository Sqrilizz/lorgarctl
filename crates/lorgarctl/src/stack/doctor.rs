use crate::Cli;
use lorgar_core::ipc::{self, Action};
use lorgar_core::monitor;
use lorgar_core::profiles;
use lorgar_core::Result;
use serde_json::json;

pub fn doctor(cli: &Cli) -> Result<()> {
    let devices = lorgar_core::Msa10w::list_devices()?;
    let mut seen = std::collections::BTreeSet::new();
    let interfaces: Vec<_> = devices
        .iter()
        .filter(|d| seen.insert(d.path.clone()))
        .map(|device| {
            let access = lorgar_core::device::check_access(&device.path);
            json!({
                "device": device,
                "read_write_access": access.is_ok(),
                "error": access.err().map(|e| e.to_string()),
                "physical_usb_parent": monitor::usb_parent(&device.path)
            })
        })
        .collect();
    let path = cli
        .config
        .clone()
        .map(Ok)
        .unwrap_or_else(profiles::config_path)?;
    let config_result = profiles::load(&path).and_then(|c| c.validate());
    let socket = cli
        .socket
        .clone()
        .map(Ok)
        .unwrap_or_else(ipc::socket_path)?;
    let daemon = ipc::request(&socket, Action::Status);
    let mut backend = lorgar_core::foreground::backend("auto")?;
    let active = backend.active_application();
    super::common::output(
        &json!({
            "interfaces": interfaces,
            "config_path": path,
            "config_valid": config_result.is_ok(),
            "config_error": config_result.err().map(|e| e.to_string()),
            "daemon_socket": socket,
            "daemon_reachable": daemon.is_ok(),
            "daemon_error": daemon.err().map(|e| e.to_string()),
            "session_type": std::env::var("XDG_SESSION_TYPE").ok(),
            "foreground_backend": backend.name(),
            "active_process": active.as_ref().ok().and_then(|app| app.as_ref()).map(|app| &app.process),
            "foreground_error": active.err().map(|e| e.to_string()),
            "notes": [
                "HID presence is not proof of radio connectivity or charging",
                "legacy tray/upower clients should be stopped when using lorgard",
                "udev rules belong before 73-seat-late.rules for uaccess ACL assignment"
            ]
        }),
        cli.json,
    )
}
