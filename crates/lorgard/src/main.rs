use clap::Parser;
use lorgar_core::foreground::ForegroundBackend;
use lorgar_core::ipc::{self, Action, EventLog, Request, Response};
use lorgar_core::monitor::{Event, HidProvider, Monitor, TimedEvent};
use lorgar_core::profiles::{self, Config};
use lorgar_core::{LorgarError, Result};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    version,
    about = "LORGAR MSA10W user daemon; no root required with udev access"
)]
struct Args {
    #[arg(long)]
    device: Option<String>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, default_value = "auto", value_parser = ["auto", "none", "x11", "kwin"])]
    foreground: String,
    #[arg(long)]
    restore_last_profile: bool,
}

struct SocketGuard {
    path: PathBuf,
    inode: u64,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|meta| meta.ino() == self.inode && meta.file_type().is_socket())
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn bind(path: &Path) -> Result<(UnixListener, SocketGuard)> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.file_type().is_socket() {
                return Err(LorgarError::Config(
                    "refusing to replace non-socket at daemon socket path".into(),
                ));
            }
            match UnixStream::connect(path) {
                Ok(_) => return Err(LorgarError::DeviceBusy),
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                    std::fs::remove_file(path)?
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let guard = SocketGuard {
        path: path.to_path_buf(),
        inode: std::fs::symlink_metadata(path)?.ino(),
    };
    listener.set_nonblocking(true)?;
    Ok((listener, guard))
}

struct Daemon {
    monitor: Monitor<HidProvider>,
    log: EventLog,
    config: Config,
    config_path: PathBuf,
    last_config_modified: Option<std::time::SystemTime>,
    backend: Box<dyn ForegroundBackend>,
    last_selection: Option<String>,
    last_warning: Option<String>,
    restore: bool,
    notified_low: bool,
    notified_critical: bool,
}

impl Daemon {
    fn check_battery_notification(&mut self, percentage: u8) {
        if percentage <= 10 {
            if !self.notified_critical {
                lorgar_core::notify::send_desktop_notification(
                    "LORGAR MSA10W: Battery Critical",
                    &format!("Battery is at {percentage}%! Mouse may power off soon. Connect charging cable."),
                    lorgar_core::notify::Urgency::Critical,
                    Some("battery-caution"),
                );
                self.notified_critical = true;
                self.notified_low = true;
            }
        } else if percentage <= 20 {
            if !self.notified_low {
                lorgar_core::notify::send_desktop_notification(
                    "LORGAR MSA10W: Low Battery",
                    &format!(
                        "Battery is at {percentage}%. Consider plugging in the charging cable."
                    ),
                    lorgar_core::notify::Urgency::Normal,
                    Some("battery-low"),
                );
                self.notified_low = true;
            }
        } else if percentage > 25 {
            self.notified_low = false;
            self.notified_critical = false;
        }
    }

    fn warn(&mut self, message: String) {
        if self.last_warning.as_ref() != Some(&message) {
            eprintln!("lorgard: {message}");
            self.log.push(TimedEvent::new(Event::Warning {
                message: message.clone(),
            }));
            self.last_warning = Some(message);
        }
    }

    fn apply(&mut self, name: &str, persist: bool) -> Result<()> {
        let profile = self.config.profile(name)?.clone();
        if persist {
            self.config.writable()?;
        }
        for event in self.monitor.apply(name, &profile)? {
            self.log.push(event);
        }
        if persist {
            profiles::update(&self.config_path, |config| {
                config.last_profile = Some(name.into());
                Ok(())
            })
            .map_err(|e| {
                LorgarError::Config(format!(
                    "profile applied, but saving last profile failed: {e}"
                ))
            })?;
            self.config.last_profile = Some(name.into());
        }
        Ok(())
    }

    fn reload(&mut self) -> Result<()> {
        if let Ok(meta) = std::fs::metadata(&self.config_path) {
            if let Ok(modified) = meta.modified() {
                if self.last_config_modified == Some(modified) {
                    return Ok(());
                }
                self.last_config_modified = Some(modified);
            }
        }
        let next = profiles::load(&self.config_path)?;
        next.validate()?;
        if next.profiles != self.config.profiles || next.applications != self.config.applications {
            self.last_selection = None;
        }
        self.config = next;
        Ok(())
    }

    fn request(&mut self, request: Request) -> Result<Response> {
        if request.version != ipc::VERSION {
            return Err(LorgarError::Config(
                "unsupported daemon protocol version".into(),
            ));
        }
        match request.action {
            Action::Status => Ok(Response {
                state: Some(self.monitor.state.clone()),
                config_path: Some(self.config_path.clone()),
                ..Response::default()
            }),
            Action::Devices => Ok(Response {
                devices: lorgar_core::Msa10w::list_devices()?,
                ..Response::default()
            }),
            Action::Execute { command } => {
                command.packet()?;
                let event = self.monitor.execute(&command)?;
                if !matches!(command, lorgar_core::protocol::Command::QueryState) {
                    self.monitor.state.applied_profile = None;
                }
                self.log.push(event);
                Ok(Response::default())
            }
            Action::ApplyProfile { name } => {
                self.reload()?;
                self.apply(&name, true)?;
                Ok(Response::default())
            }
            Action::ApplyDraft { profile } => {
                if profile.commands()?.is_empty() {
                    return Err(LorgarError::Config(
                        "choose at least one setting to apply".into(),
                    ));
                }
                for event in self.monitor.apply("Unsaved draft", &profile)? {
                    self.log.push(event);
                }
                Ok(Response::default())
            }
            Action::Events { after } => Ok(self.log.since(after)),
            Action::SetSleepTimeout { minutes } => {
                self.monitor.set_sleep_timeout(minutes)?;
                Ok(Response::default())
            }
            Action::Reload => {
                self.reload()?;
                Ok(Response::default())
            }
            Action::GetDpiTable => {
                let (stages, colors) = self.monitor.get_dpi_table()?;
                Ok(Response {
                    dpi_stages: Some(stages.to_vec()),
                    stage_colors: Some(colors.to_vec()),
                    ..Response::default()
                })
            }
            Action::SetDpiStages { stages } => {
                if stages.is_empty() || stages.len() > 8 {
                    return Err(LorgarError::Config(
                        "Specify between 1 and 8 DPI values".into(),
                    ));
                }
                for &dpi in &stages {
                    if !(lorgar_core::MIN_DPI..=lorgar_core::MAX_DPI).contains(&dpi) {
                        return Err(LorgarError::InvalidDpi {
                            dpi,
                            min: lorgar_core::MIN_DPI,
                            max: lorgar_core::MAX_DPI,
                        });
                    }
                }
                let mut stages_arr = [0u16; 8];
                for (i, &s) in stages.iter().enumerate() {
                    stages_arr[i] = s;
                }
                self.monitor.set_dpi_stages(&stages_arr)?;
                Ok(Response::default())
            }
            Action::GetButtonMappings { secondary } => {
                let mappings = self.monitor.get_button_mappings(secondary)?;
                Ok(Response {
                    button_mappings: Some(mappings),
                    ..Response::default()
                })
            }
            Action::DumpMemory => {
                let dump = self.monitor.dump_onboard_memory()?;
                Ok(Response {
                    memory_dump: Some(dump),
                    ..Response::default()
                })
            }
            Action::DumpFlash { addr, len } => {
                let bytes = self.monitor.dump_flash(addr, len)?;
                Ok(Response {
                    flash_dump: Some(bytes),
                    ..Response::default()
                })
            }
            Action::SetLod { mm } => {
                self.monitor.set_lod(mm)?;
                Ok(Response::default())
            }
            Action::SetDebounce { ms, rapid_fire } => {
                self.monitor.set_debounce(ms, rapid_fire)?;
                Ok(Response::default())
            }
            Action::SetAngleSnapping { enabled } => {
                self.monitor.set_angle_snapping(enabled)?;
                Ok(Response::default())
            }
            Action::SetMotionSync { enabled } => {
                self.monitor.set_motion_sync(enabled)?;
                Ok(Response::default())
            }
        }
    }

    fn tick(&mut self, now: u64) {
        let events = self.monitor.tick(now);
        let connected = events
            .iter()
            .any(|e| matches!(e.event, Event::Connected { .. }));
        for event in &events {
            if let Event::BatteryChanged { percentage } = event.event {
                self.check_battery_notification(percentage);
            }
        }
        for event in events {
            self.log.push(event);
        }
        if connected {
            self.last_selection = None;
            if self.restore || self.config.restore_on_reconnect {
                if let Some(name) = self.config.last_profile.clone() {
                    if let Err(e) = self.apply(&name, false) {
                        self.warn(e.to_string());
                    }
                }
            }
        }
    }

    fn foreground(&mut self) {
        if self.monitor.state.device.is_none() || self.backend.name() == "disabled" {
            return;
        }
        match self.backend.active_application() {
            Ok(app) => {
                let selected = self
                    .config
                    .applications
                    .select(app.as_ref().map(|a| a.process.as_str()))
                    .map(String::from);
                if selected != self.last_selection {
                    if let Some(name) = &selected {
                        if let Err(e) = self.apply(name, false) {
                            self.warn(e.to_string());
                            return;
                        }
                    }
                    self.last_selection = selected;
                }
            }
            Err(e) => self.warn(e.to_string()),
        }
    }
}

fn run(args: Args) -> Result<()> {
    let stopped = lorgar_core::monitor::shutdown_flag()?;
    let path = ipc::socket_path()?;
    let _lock = lorgar_core::storage::FileLock::acquire(&path.with_extension("lock"))?;
    let config_path =
        std::path::absolute(args.config.map(Ok).unwrap_or_else(profiles::config_path)?)?;
    let config = profiles::load(&config_path)?;
    config.validate()?;
    let backend = lorgar_core::foreground::backend(&args.foreground)?;
    let (listener, _guard) = bind(&path)?;
    eprintln!(
        "lorgard: socket {}; foreground backend {}",
        path.display(),
        backend.name()
    );
    if backend.name() == "disabled" {
        eprintln!("lorgard: application switching disabled; no supported window manager detected (supports X11 and KDE Wayland)");
    }
    let last_config_modified = std::fs::metadata(&config_path)
        .ok()
        .and_then(|m| m.modified().ok());
    let mut daemon = Daemon {
        monitor: Monitor::new(HidProvider { path: args.device }, Some(30000)),
        log: EventLog::new(2048),
        config,
        config_path,
        last_config_modified,
        backend,
        last_selection: None,
        last_warning: None,
        restore: args.restore_last_profile,
        notified_low: false,
        notified_critical: false,
    };
    let start = Instant::now();
    let mut next_config = 0;
    while !stopped.load(Ordering::Relaxed) {
        let now = start.elapsed().as_millis() as u64;
        daemon.tick(now);
        if now >= next_config {
            next_config = now + 1000;
            if let Err(e) = daemon.reload() {
                daemon.warn(e.to_string());
            }
            daemon.foreground();
        }
        for _ in 0..4 {
            let mut stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            };
            stream.set_read_timeout(Some(Duration::from_millis(500)))?;
            stream.set_write_timeout(Some(Duration::from_secs(5)))?;
            let response = match ipc::read_timed_frame(&mut stream, Duration::from_millis(500))
                .and_then(|request| daemon.request(request))
            {
                Ok(response) => response,
                Err(error) => Response {
                    ok: false,
                    error: Some(error.to_string()),
                    error_code: Some(ipc::exit_code(&error)),
                    ..Response::default()
                },
            };
            if let Err(e) = ipc::write_frame(&mut stream, &response) {
                daemon.warn(format!("client response failed: {e}"));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run(Args::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lorgard: {e}");
            std::process::ExitCode::from(ipc::exit_code(&e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_guard_and_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.sock");
        std::fs::write(&path, b"user data").unwrap();
        assert!(bind(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"user data");
        std::fs::remove_file(&path).unwrap();
        let (listener, guard) = bind(&path).unwrap();
        assert!(bind(&path).is_err());
        drop(listener);
        drop(guard);
        assert!(!path.exists());
    }
}
