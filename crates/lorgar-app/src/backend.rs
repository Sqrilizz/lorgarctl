use lorgar_core::ipc::{self, Action, Response};
use lorgar_core::profiles::{self, Applications, Config, Profile};
use lorgar_core::{LorgarError, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

pub enum Work {
    Connect,
    Save {
        name: String,
        profile: Profile,
        expected: Option<Profile>,
    },
    Create {
        name: String,
        profile: Profile,
    },
    Delete {
        name: String,
        expected: Profile,
    },
    Apply {
        name: Option<String>,
        profile: Profile,
    },
    Preferences {
        restore: bool,
        applications: Applications,
        expected: (bool, Applications),
    },
    Stop,
}

pub enum Update {
    Status(std::result::Result<Response, String>),
    Config(std::result::Result<Config, String>),
    Done {
        result: std::result::Result<String, String>,
        selected: Option<String>,
    },
}

struct OwnedDaemon(Child);
impl Drop for OwnedDaemon {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        unsafe {
            libc::kill(self.0.id() as i32, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(socket: &Path, action: Action) -> Result<Response> {
    let response = ipc::request(socket, action)?;
    if !response.ok {
        return Err(LorgarError::Remote {
            code: response.error_code.unwrap_or(1),
            message: response
                .error
                .unwrap_or_else(|| "The device service rejected this request".into()),
        });
    }
    Ok(response)
}

pub fn save_profile(
    path: &Path,
    name: &str,
    profile: &Profile,
    expected: Option<&Profile>,
) -> Result<()> {
    profiles::validate_name(name)?;
    profile.commands()?;
    profiles::update(path, |config| {
        if config.profiles.get(name) != expected {
            return Err(LorgarError::Config(
                "This profile changed in another app. Reload it before saving.".into(),
            ));
        }
        config.profiles.insert(name.into(), profile.clone());
        Ok(())
    })
}

fn operation(work: Work, socket: &Path, path: &Path) -> Result<(String, Option<String>)> {
    match work {
        Work::Save {
            name,
            profile,
            expected,
        } => {
            save_profile(path, &name, &profile, expected.as_ref())?;
            Ok((
                format!("Saved {name}. Your mouse settings have not changed."),
                Some(name),
            ))
        }
        Work::Create { name, profile } => {
            save_profile(path, &name, &profile, None)?;
            Ok((
                format!("Created {name}. Choose the settings to include."),
                Some(name),
            ))
        }
        Work::Delete { name, expected } => {
            profiles::update(path, |config| {
                if config.profiles.get(&name) != Some(&expected) {
                    return Err(LorgarError::Config(
                        "Profile changed; reload before deleting.".into(),
                    ));
                }
                if config.applications.default.as_ref() == Some(&name)
                    || config.applications.rules.iter().any(|r| r.profile == name)
                {
                    return Err(LorgarError::Config(
                        "Remove this profile from application rules before deleting it.".into(),
                    ));
                }
                config.profiles.remove(&name);
                if config.last_profile.as_ref() == Some(&name) {
                    config.last_profile = None;
                }
                Ok(())
            })?;
            Ok((format!("Deleted {name}."), Some(String::new())))
        }
        Work::Apply { name, profile } => {
            if profile.commands()?.is_empty() {
                return Err(LorgarError::Config(
                    "Choose at least one setting before applying.".into(),
                ));
            }
            if let Some(name) = name {
                let current = profiles::load(path)?;
                if current.profile(&name)? != &profile {
                    return Err(LorgarError::Config(
                        "The saved profile changed. Reload it before applying.".into(),
                    ));
                }
                request(socket, Action::ApplyProfile { name: name.clone() })?;
                Ok((format!("Applied {name} to your mouse."), None))
            } else {
                request(socket, Action::ApplyDraft { profile })?;
                Ok((
                    "Applied your draft. Save a profile to keep these settings.".into(),
                    None,
                ))
            }
        }
        Work::Preferences {
            restore,
            applications,
            expected,
        } => {
            profiles::update(path, |config| {
                if (config.restore_on_reconnect, &config.applications) != (expected.0, &expected.1)
                {
                    return Err(LorgarError::Config(
                        "Application preferences changed elsewhere. Reopen the app before saving."
                            .into(),
                    ));
                }
                config.restore_on_reconnect = restore;
                config.applications = applications;
                Ok(())
            })?;
            Ok(("Saved reconnect and application preferences.".into(), None))
        }
        _ => Ok(("Refreshed.".into(), None)),
    }
}

pub fn start(
    options: super::Options,
) -> (Sender<Work>, Receiver<Update>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    let (updates, output) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let setup = (|| -> Result<_> {
            let socket = options
                .socket
                .clone()
                .map(Ok)
                .unwrap_or_else(ipc::socket_path)?;
            let path = options
                .config
                .clone()
                .map(Ok)
                .unwrap_or_else(profiles::config_path)?;
            Ok((socket, std::path::absolute(path)?))
        })();
        let (socket, mut path) = match setup {
            Ok(value) => value,
            Err(error) => {
                let _ = updates.send(Update::Status(Err(error.to_string())));
                return;
            }
        };
        let mut child: Option<OwnedDaemon> = None;
        let mut may_start = !options.no_start_daemon && options.socket.is_none();
        let mut next_poll = Instant::now();
        let _ = updates.send(Update::Config(
            profiles::load(&path).map_err(|e| e.to_string()),
        ));
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Work::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Work::Connect) => {
                    may_start = !options.no_start_daemon && options.socket.is_none();
                    let _ = updates.send(Update::Config(
                        profiles::load(&path).map_err(|e| e.to_string()),
                    ));
                    next_poll = Instant::now();
                }
                Ok(work) => {
                    let result = operation(work, &socket, &path);
                    let selected = result.as_ref().ok().and_then(|(_, name)| name.clone());
                    let _ = updates.send(Update::Config(
                        profiles::load(&path).map_err(|e| e.to_string()),
                    ));
                    let _ = updates.send(Update::Done {
                        result: result
                            .map(|(message, _)| message)
                            .map_err(|e| e.to_string()),
                        selected,
                    });
                    next_poll = Instant::now();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
            if Instant::now() < next_poll {
                continue;
            }
            next_poll = Instant::now() + Duration::from_secs(2);
            let mut result = request(&socket, Action::Status);
            if result.is_err() && may_start {
                may_start = false;
                if child
                    .as_mut()
                    .is_some_and(|c| c.0.try_wait().ok().flatten().is_some())
                {
                    child = None;
                }
                if child.is_none() {
                    let executable = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|parent| parent.join("lorgard")))
                        .filter(|p| p.is_file())
                        .unwrap_or_else(|| PathBuf::from("lorgard"));
                    match Command::new(executable).arg("--config").arg(&path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn() {
                        Ok(process) => {
                            child = Some(OwnedDaemon(process));
                            for _ in 0..20 {
                                std::thread::sleep(Duration::from_millis(50));
                                result = request(&socket, Action::Status);
                                if result.is_ok() { break; }
                            }
                        }
                        Err(e) => result = Err(LorgarError::Transport(format!("Could not start lorgard: {e}. Install lorgard beside lorgar-app, then reconnect."))),
                    }
                }
            }
            if let Ok(response) = &result {
                if let Some(daemon_path) = &response.config_path {
                    if options.config.is_none() && path != *daemon_path {
                        path = daemon_path.clone();
                        let _ = updates.send(Update::Config(
                            profiles::load(&path).map_err(|e| e.to_string()),
                        ));
                    } else if options.config.is_some() && path != *daemon_path {
                        result = Err(LorgarError::Config(format!(
                            "lorgard uses {}. Restart it with the same --config as this app.",
                            daemon_path.display()
                        )));
                    }
                }
            }
            let _ = updates.send(Update::Status(result.map_err(|e| e.to_string())));
        }
        drop(child);
    });
    (tx, output, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_preserves_extensions_and_detects_concurrent_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut profile = Profile::default();
        profile.extra.insert("future".into(), 42.into());
        save_profile(&path, "gaming", &profile, None).unwrap();
        let mut changed = profile.clone();
        changed.polling = Some(1000);
        save_profile(&path, "gaming", &changed, Some(&profile)).unwrap();
        assert!(save_profile(&path, "gaming", &profile, Some(&profile)).is_err());
        assert_eq!(profiles::load(&path).unwrap().profiles["gaming"], changed);
    }

    #[test]
    fn preferences_detect_conflicts_and_protect_referenced_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let socket = dir.path().join("absent.sock");
        operation(
            Work::Create {
                name: "gaming".into(),
                profile: Profile::default(),
            },
            &socket,
            &path,
        )
        .unwrap();
        let applications = Applications {
            default: Some("gaming".into()),
            ..Default::default()
        };
        operation(
            Work::Preferences {
                restore: true,
                applications: applications.clone(),
                expected: (false, Applications::default()),
            },
            &socket,
            &path,
        )
        .unwrap();
        assert!(operation(
            Work::Preferences {
                restore: false,
                applications: Applications::default(),
                expected: (false, Applications::default())
            },
            &socket,
            &path
        )
        .is_err());
        assert!(operation(
            Work::Delete {
                name: "gaming".into(),
                expected: Profile::default()
            },
            &socket,
            &path
        )
        .is_err());
        let config = profiles::load(&path).unwrap();
        assert!(config.restore_on_reconnect);
        assert_eq!(config.applications, applications);
        assert!(config.profiles.contains_key("gaming"));
    }
}
