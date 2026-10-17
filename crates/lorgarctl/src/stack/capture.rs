use crate::Cli;
use lorgar_core::capture::{self, CaptureWriter, Header};
use lorgar_core::ipc::Action;
use lorgar_core::monitor::{self, Event, HidProvider, Monitor, TimedEvent};
use lorgar_core::protocol::Command;
use lorgar_core::{LorgarError, Result};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub fn event_output(event: &TimedEvent, as_json: bool) -> Result<()> {
    if as_json {
        return super::common::output(event, true);
    }
    let text = match &event.event {
        Event::Connected { device } => format!(
            "connected HID {} {:04x}:{:04x}",
            device.path, device.vendor_id, device.product_id
        ),
        Event::Disconnected { reason } => format!("disconnected: {reason}"),
        Event::BatteryChanged { percentage } => format!("battery {percentage}%"),
        Event::DpiStageChanged { stage, confidence } => {
            format!("DPI stage {stage} (confidence: {confidence})")
        }
        Event::Warning { message } => format!("warning: {message}"),
        Event::ProfileApplied { name } => format!("profile applied: {name}"),
        Event::Packet {
            device,
            direction,
            raw,
            parsed,
        } => format!(
            "{} {direction:?} {} {:?}",
            device.path,
            crate::hex::format_hex_bytes(raw),
            parsed.as_ref().map(|p| &p.event)
        ),
    };
    writeln!(std::io::stdout().lock(), "{} {text}", event.timestamp_ms)?;
    Ok(())
}

pub fn watch(cli: &Cli, capture: Option<(&Path, u64, bool, &str, u64)>) -> Result<()> {
    let stopped = crate::shutdown_flag()?;
    let start = Instant::now();
    let duration = capture.map(|(_, duration, _, _, _)| duration).unwrap_or(0);
    let mut writer = if let Some((path, _, _, label, _)) = capture {
        Some(CaptureWriter::create(
            path,
            &Header {
                version: 1,
                created_ms: monitor::timestamp_ms(),
                source: format!(
                    "{}; {label}",
                    if super::common::is_daemon_mode(cli) {
                        "lorgard event stream"
                    } else {
                        "direct HID client; only own feature output is observed"
                    }
                ),
                descriptors: capture::descriptors()?,
            },
        )?)
    } else {
        None
    };
    let query = capture.is_none() || capture.is_some_and(|(_, _, query, _, _)| query);
    let mut monitor = Monitor::new(
        HidProvider {
            path: cli.device.clone(),
        },
        query.then_some(
            capture
                .map(|(_, _, _, _, interval)| interval)
                .unwrap_or(30000),
        ),
    );
    let mut cursor = if super::common::is_daemon_mode(cli) {
        super::common::response(cli, Action::Events { after: u64::MAX })?.cursor
    } else {
        0
    };
    let mut next_daemon_query_ms = 0;
    while !stopped.load(Ordering::Relaxed)
        && (duration == 0 || start.elapsed().as_secs() < duration)
    {
        if super::common::is_daemon_mode(cli)
            && capture.is_some_and(|(_, _, query, _, _)| query)
            && start.elapsed().as_millis() as u64 >= next_daemon_query_ms
        {
            super::common::response(
                cli,
                Action::Execute {
                    command: Command::QueryState,
                },
            )?;
            next_daemon_query_ms = start.elapsed().as_millis() as u64
                + capture
                    .map(|(_, _, _, _, interval)| interval)
                    .unwrap_or(30000);
        }
        let events = if super::common::is_daemon_mode(cli) {
            let response = super::common::response(cli, Action::Events { after: cursor })?;
            cursor = response.cursor;
            if response.gap {
                let warning = TimedEvent::new(Event::Warning {
                    message: "daemon event gap or restart; some events are unavailable".into(),
                });
                if let Some(writer) = &mut writer {
                    writer.push(&warning)?;
                    return Err(LorgarError::Transport(
                        "capture incomplete: daemon event gap".into(),
                    ));
                }
                event_output(&warning, cli.json)?;
            }
            response.events.into_iter().map(|e| e.event).collect()
        } else {
            monitor.tick(start.elapsed().as_millis() as u64)
        };
        for event in events {
            if let Some(writer) = &mut writer {
                writer.push(&event)?;
            } else {
                event_output(&event, cli.json)?;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if let Some((path, _, _, _, _)) = capture {
        super::common::output(&json!({"ok": true, "capture": path}), cli.json)?;
    }
    Ok(())
}
