use crate::{LorgarError, Result};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveApplication {
    pub pid: u32,
    pub process: String,
}

pub trait ForegroundBackend {
    fn active_application(&mut self) -> Result<Option<ActiveApplication>>;
    fn name(&self) -> &'static str;
}

pub struct Disabled;
impl ForegroundBackend for Disabled {
    fn active_application(&mut self) -> Result<Option<ActiveApplication>> {
        Ok(None)
    }
    fn name(&self) -> &'static str {
        "disabled"
    }
}

pub struct X11;

fn xprop(args: &[&str]) -> Result<String> {
    let output = tempfile::tempfile()?;
    let mut child = Command::new("xprop")
        .env("LC_ALL", "C")
        .args(args)
        .stdout(Stdio::from(output.try_clone()?))
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(LorgarError::Transport(
                        "xprop could not query foreground window".into(),
                    ));
                }
                break;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(result
                    .err()
                    .map(LorgarError::Io)
                    .unwrap_or(LorgarError::Timeout));
            }
        }
    }
    use std::io::{Seek, SeekFrom};
    let mut output = output;
    output.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    output.take(65536).read_to_string(&mut text)?;
    Ok(text)
}

fn process_name(pid: u32) -> Result<String> {
    let path = std::fs::read_link(format!("/proc/{pid}/exe"))?;
    path.file_name()
        .and_then(|name| name.to_str())
        .map(String::from)
        .ok_or_else(|| {
            LorgarError::Config("foreground process has no UTF-8 executable name".into())
        })
}

pub fn parse_window(text: &str) -> Result<Option<u32>> {
    let value = text
        .split_once("# 0x")
        .map(|(_, value)| value.trim())
        .ok_or_else(|| {
            LorgarError::Transport("window manager does not expose _NET_ACTIVE_WINDOW".into())
        })?;
    let window = u32::from_str_radix(value, 16)
        .map_err(|_| LorgarError::Transport("invalid X11 active window".into()))?;
    Ok((window != 0).then_some(window))
}

impl ForegroundBackend for X11 {
    fn active_application(&mut self) -> Result<Option<ActiveApplication>> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
        {
            return Err(LorgarError::Config("X11 foreground detection is disabled on Wayland; XWayland cannot see native windows".into()));
        }
        let Some(window) = parse_window(&xprop(&["-root", "_NET_ACTIVE_WINDOW"])?)? else {
            return Ok(None);
        };
        let text = xprop(&[
            "-id",
            &format!("0x{window:x}"),
            "_NET_WM_PID",
            "WM_CLIENT_MACHINE",
        ])?;
        let machine = text
            .lines()
            .find(|line| line.starts_with("WM_CLIENT_MACHINE("))
            .and_then(|s| s.split_once(" = "))
            .map(|(_, s)| s.trim_matches('"'));
        let local = std::fs::read_to_string("/proc/sys/kernel/hostname")?;
        if machine != Some(local.trim()) {
            return Err(LorgarError::Transport(
                "foreground window has no matching local WM_CLIENT_MACHINE".into(),
            ));
        }
        let pid = text
            .lines()
            .find(|line| line.starts_with("_NET_WM_PID(CARDINAL) = "))
            .and_then(|line| line.split_once(" = "))
            .and_then(|(_, value)| value.parse::<u32>().ok())
            .ok_or_else(|| {
                LorgarError::Transport("foreground window has no usable _NET_WM_PID".into())
            })?;
        Ok(Some(ActiveApplication {
            pid,
            process: process_name(pid)?,
        }))
    }
    fn name(&self) -> &'static str {
        "x11-ewmh"
    }
}

pub fn backend(name: &str) -> Result<Box<dyn ForegroundBackend>> {
    match name {
        "none" => Ok(Box::new(Disabled)),
        "x11" => {
            if std::env::var_os("WAYLAND_DISPLAY").is_some()
                || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
            {
                return Err(LorgarError::Config(
                    "native Wayland focus detection is unavailable; use --foreground none".into(),
                ));
            }
            Ok(Box::new(X11))
        }
        "auto"
            if std::env::var_os("WAYLAND_DISPLAY").is_none()
                && std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("x11") =>
        {
            Ok(Box::new(X11))
        }
        "auto" => Ok(Box::new(Disabled)),
        _ => Err(LorgarError::Config(
            "foreground backend must be auto, none or x11".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn x11_window_parsing() {
        assert_eq!(
            parse_window("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x240003\n").unwrap(),
            Some(0x240003)
        );
        assert_eq!(
            parse_window("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0\n").unwrap(),
            None
        );
        assert!(parse_window("_NET_ACTIVE_WINDOW: not found").is_err());
    }
}
