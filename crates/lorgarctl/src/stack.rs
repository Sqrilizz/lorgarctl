use crate::{Cli, Commands, DpiCommands, PollingSubcommands, RgbCommands};
use clap::{CommandFactory, Subcommand};
use lorgar_core::capture::{self, CaptureWriter, Header};
use lorgar_core::ipc::{self, Action};
use lorgar_core::monitor::{self, Event, HidProvider, Monitor, TimedEvent};
use lorgar_core::profiles::{self, Profile};
use lorgar_core::protocol::Command;
use lorgar_core::{LorgarError, Result};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    #[command(about = "Create an empty named profile")]
    Create { name: String },
    #[command(about = "Update selected settings, preserving all others")]
    Set {
        name: String,
        #[arg(long, value_name = "STAGE=DPI")]
        dpi_stage: Vec<String>,
        #[arg(long)]
        active_stage: Option<u8>,
        #[arg(long)]
        polling: Option<u16>,
        #[arg(long, value_parser = crate::parse_lighting_mode)]
        rgb: Option<lorgar_core::LightingMode>,
        #[arg(long, value_name = "RRGGBB")]
        color: Option<String>,
    },
    #[command(about = "Send confirmed commands and remember the profile name")]
    Apply { name: String },
    #[command(about = "List saved profile names")]
    List,
    #[command(about = "Show settings and preserved extension fields")]
    Show { name: String },
    #[command(about = "Delete a profile unless application rules reference it")]
    Delete { name: String },
    #[command(about = "Print the XDG configuration file path")]
    Path,
}

fn output(value: &impl serde::Serialize, as_json: bool) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if as_json {
        serde_json::to_writer(&mut out, value)?;
    } else {
        serde_json::to_writer_pretty(&mut out, value)?;
    }
    writeln!(out)?;
    Ok(())
}

fn response(cli: &Cli, action: Action) -> Result<ipc::Response> {
    let path = cli
        .socket
        .clone()
        .map(Ok)
        .unwrap_or_else(ipc::socket_path)?;
    let response = ipc::request(&path, action)?;
    if !response.ok {
        return Err(LorgarError::Remote {
            code: response.error_code.unwrap_or(1),
            message: response
                .error
                .unwrap_or_else(|| "daemon request failed".into()),
        });
    }
    Ok(response)
}

fn execute(cli: &Cli, command: Command) -> Result<()> {
    let packet = command.packet()?;
    let description = format!("{command:?}");
    if cli.daemon {
        response(cli, Action::Execute { command })?;
    } else {
        crate::open_device(cli.device.as_deref(), false)?.execute(&command)?;
    }
    if !cli.json {
        writeln!(
            std::io::stdout().lock(),
            "Applied {description} (sent; readback not verified)"
        )?;
        if cli.debug {
            writeln!(
                std::io::stdout().lock(),
                "Report: {}",
                crate::format_hex_bytes(&packet.report())
            )?;
        }
        return Ok(());
    }
    output(
        &json!({"ok": true, "packet": packet, "hardware_readback_verified": false}),
        cli.json,
    )
}

fn profile(cli: &Cli, sub: &ProfileCommand) -> Result<()> {
    let path = cli
        .config
        .clone()
        .map(Ok)
        .unwrap_or_else(profiles::config_path)?;
    match sub {
        ProfileCommand::Path => output(&json!({"path": path}), cli.json),
        ProfileCommand::List => output(
            &profiles::load(&path)?.profiles.keys().collect::<Vec<_>>(),
            cli.json,
        ),
        ProfileCommand::Show { name } => output(profiles::load(&path)?.profile(name)?, cli.json),
        ProfileCommand::Apply { name } => {
            if cli.daemon {
                response(cli, Action::ApplyProfile { name: name.clone() })?;
            } else {
                let config = profiles::load(&path)?;
                config.writable()?;
                let commands = config.profile(name)?.commands()?;
                let device = crate::open_device(cli.device.as_deref(), false)?;
                for (index, command) in commands.iter().enumerate() {
                    device
                        .execute(command)
                        .map_err(|source| LorgarError::ProfileApply {
                            name: name.clone(),
                            index,
                            source: Box::new(source),
                        })?;
                }
                profiles::update(&path, |config| {
                    config.last_profile = Some(name.clone());
                    Ok(())
                })
                .map_err(|e| {
                    LorgarError::Config(format!(
                        "profile applied, but saving last profile failed: {e}"
                    ))
                })?;
            }
            output(
                &json!({"ok": true, "profile": name, "hardware_readback_verified": false}),
                cli.json,
            )
        }
        _ => {
            profiles::update(&path, |config| {
                match sub {
                    ProfileCommand::Create { name } => {
                        profiles::validate_name(name)?;
                        if config.profiles.contains_key(name) {
                            return Err(LorgarError::Config(format!(
                                "profile '{name}' already exists"
                            )));
                        }
                        config.profiles.insert(name.clone(), Profile::default());
                    }
                    ProfileCommand::Delete { name } => {
                        config.profile(name)?;
                        if config.applications.default.as_ref() == Some(name)
                            || config.applications.rules.iter().any(|r| &r.profile == name)
                        {
                            return Err(LorgarError::Config("profile is referenced by application rules; update those rules first".into()));
                        }
                        config.profiles.remove(name);
                        if config.last_profile.as_ref() == Some(name) {
                            config.last_profile = None;
                        }
                    }
                    ProfileCommand::Set {
                        name,
                        dpi_stage,
                        active_stage,
                        polling,
                        rgb,
                        color,
                    } => {
                        let profile = config.profiles.get_mut(name).ok_or_else(|| {
                            LorgarError::Config(format!("profile '{name}' does not exist"))
                        })?;
                        for value in dpi_stage {
                            let (stage, dpi) = value.split_once('=').ok_or_else(|| {
                                LorgarError::Config(
                                    "--dpi-stage requires STAGE=DPI, e.g. 0=800".into(),
                                )
                            })?;
                            let stage = stage
                                .parse::<u8>()
                                .map_err(|_| LorgarError::Config("invalid stage index".into()))?;
                            let dpi = dpi
                                .parse::<u16>()
                                .map_err(|_| LorgarError::Config("invalid DPI value".into()))?;
                            profile.dpi_stages.insert(stage, dpi);
                        }
                        if let Some(stage) = active_stage {
                            profile.active_stage = Some(*stage);
                        }
                        if let Some(polling) = polling {
                            profile.polling = Some(*polling);
                        }
                        if let Some(rgb) = rgb {
                            profile.rgb = Some(*rgb);
                        }
                        if let Some(color) = color {
                            profile.color = Some(color.clone());
                        }
                    }
                    _ => (),
                }
                Ok(())
            })?;
            output(&json!({"ok": true, "config": path}), cli.json)
        }
    }
}

fn event_output(event: &TimedEvent, as_json: bool) -> Result<()> {
    if as_json {
        return output(event, true);
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
            crate::format_hex_bytes(raw),
            parsed.as_ref().map(|p| &p.event)
        ),
    };
    writeln!(std::io::stdout().lock(), "{} {text}", event.timestamp_ms)?;
    Ok(())
}

fn watch(cli: &Cli, capture: Option<(&Path, u64, bool, &str, u64)>) -> Result<()> {
    let stopped = monitor::shutdown_flag()?;
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
                    if cli.daemon {
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
    let mut cursor = if cli.daemon {
        response(cli, Action::Events { after: u64::MAX })?.cursor
    } else {
        0
    };
    let mut next_daemon_query_ms = 0;
    while !stopped.load(Ordering::Relaxed)
        && (duration == 0 || start.elapsed().as_secs() < duration)
    {
        if cli.daemon
            && capture.is_some_and(|(_, _, query, _, _)| query)
            && start.elapsed().as_millis() as u64 >= next_daemon_query_ms
        {
            response(
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
        let events = if cli.daemon {
            let response = response(cli, Action::Events { after: cursor })?;
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
        output(&json!({"ok": true, "capture": path}), cli.json)?;
    }
    Ok(())
}

fn status(cli: &Cli, battery_only: bool) -> Result<()> {
    let (info, battery, stage, observed, raw, error, applied_profile) = if cli.daemon {
        let state = response(cli, Action::Status)?
            .state
            .ok_or_else(|| LorgarError::Transport("daemon omitted state".into()))?;
        (
            state.device,
            state.battery_percentage,
            state.active_stage,
            state.battery_observed_ms,
            None,
            state.last_error,
            state.applied_profile,
        )
    } else {
        let device = crate::open_device(cli.device.as_deref(), false)?;
        let raw = device.query_device_state()?;
        let battery = lorgar_core::parse_battery_percentage(&raw)?;
        (
            Some(device.info().clone()),
            Some(battery),
            None,
            Some(monitor::timestamp_ms()),
            Some(raw),
            None,
            None,
        )
    };
    if battery_only {
        let percentage = battery.ok_or_else(|| {
            if info.is_none() {
                LorgarError::DeviceNotFound {
                    vid: lorgar_core::VENDOR_ID,
                    pid: lorgar_core::PRODUCT_IDS[1],
                }
            } else {
                LorgarError::Timeout
            }
        })?;
        if let Commands::Battery {
            short,
            notify,
            threshold,
        } = &cli.command
        {
            if *notify && percentage <= *threshold {
                crate::send_desktop_notification(
                    "LORGAR battery low",
                    &format!("Battery: {percentage}%"),
                    percentage <= 10,
                );
            }
            if *short && !cli.json {
                writeln!(std::io::stdout().lock(), "🖱️ {percentage}%")?;
                return Ok(());
            }
        }
        if !cli.json {
            writeln!(
                std::io::stdout().lock(),
                "Battery: {percentage}% (charging status unknown)"
            )?;
            return Ok(());
        }
        output(
            &json!({"percentage": percentage, "icon":"🖱️", "text":format!("{percentage}%"),
            "tooltip":format!("LORGAR MSA10W: {percentage}%"), "class":if percentage > 50 { "good" } else if percentage > 20 { "warning" } else { "critical" },
            "observed_ms":observed, "charging":null}),
            cli.json,
        )
    } else {
        if !cli.json {
            let mut out = std::io::stdout().lock();
            writeln!(out, "LORGAR MSA10W")?;
            if let Some(info) = &info {
                writeln!(
                    out,
                    "Device: {} ({:04x}:{:04x})",
                    info.path, info.vendor_id, info.product_id
                )?;
            } else {
                writeln!(out, "Device: disconnected")?;
            }
            writeln!(
                out,
                "Battery: {}",
                battery
                    .map(|p| format!("{p}%"))
                    .unwrap_or_else(|| "unknown".into())
            )?;
            writeln!(
                out,
                "Active DPI stage: {}",
                stage
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".into())
            )?;
            writeln!(
                out,
                "Last applied profile: {}",
                applied_profile.as_deref().unwrap_or("none")
            )?;
            writeln!(out, "Charging, polling and lighting readback: unknown")?;
            if let Some(error) = &error {
                writeln!(out, "Last error: {error}")?;
            }
            if cli.debug {
                if let Some(raw) = &raw {
                    writeln!(out, "Raw: {}", crate::format_hex_bytes(raw))?;
                }
            }
            return Ok(());
        }
        output(
            &json!({"device": info, "battery_percentage": battery, "battery_observed_ms": observed, "applied_profile":applied_profile,
            "active_stage": stage, "charging": null, "polling_rate": null, "lighting_mode": null, "raw":raw, "last_error":error}),
            cli.json,
        )
    }
}

fn doctor(cli: &Cli) -> Result<()> {
    let devices = lorgar_core::Msa10w::list_devices()?;
    let mut seen = std::collections::BTreeSet::new();
    let interfaces: Vec<_> = devices.iter().filter(|d| seen.insert(d.path.clone())).map(|device| {
        let access = lorgar_core::device::check_access(&device.path);
        json!({"device":device,"read_write_access":access.is_ok(),"error":access.err().map(|e| e.to_string()),
            "physical_usb_parent":monitor::usb_parent(&device.path)})
    }).collect();
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
    output(
        &json!({"interfaces":interfaces,"config_path":path,"config_valid":config_result.is_ok(),
        "config_error":config_result.err().map(|e| e.to_string()),"daemon_socket":socket,"daemon_reachable":daemon.is_ok(),
        "daemon_error":daemon.err().map(|e| e.to_string()),"session_type":std::env::var("XDG_SESSION_TYPE").ok(),
        "foreground_backend":backend.name(),
        "active_process":active.as_ref().ok().and_then(|app| app.as_ref()).map(|app| &app.process),
        "foreground_error":active.err().map(|e| e.to_string()),
        "notes":["HID presence is not proof of radio connectivity or charging", "legacy tray/upower clients should be stopped when using lorgard", "udev rules belong before 73-seat-late.rules for uaccess ACL assignment"]}),
        cli.json,
    )
}

fn control(cli: &Cli) -> Result<()> {
    match &cli.command {
        Commands::Devices => {
            let devices = if cli.daemon {
                response(cli, Action::Devices)?.devices
            } else {
                lorgar_core::Msa10w::list_devices()?
            };
            output(&devices, cli.json)
        }
        Commands::Status => status(cli, false),
        Commands::Battery { .. } => status(cli, true),
        Commands::Dpi { sub, stage, dpi } => {
            let command = match sub {
                Some(DpiCommands::Set { stage, dpi }) => Command::SetDpi {
                    stage: *stage,
                    dpi: *dpi,
                },
                Some(DpiCommands::Active { stage }) => Command::SetActiveStage { stage: *stage },
                None => match (stage, dpi) {
                    (Some(stage), Some(dpi)) => Command::SetDpi {
                        stage: *stage,
                        dpi: *dpi,
                    },
                    (Some(stage), None) => Command::SetActiveStage { stage: *stage },
                    _ => {
                        return Err(LorgarError::Config(
                            "specify dpi set STAGE DPI or dpi active STAGE".into(),
                        ))
                    }
                },
            };
            execute(cli, command)
        }
        Commands::Polling { sub, hz } => {
            let rate = match sub {
                Some(PollingSubcommands::Set { hz }) => *hz,
                None => hz.ok_or_else(|| LorgarError::Config("specify polling set HZ".into()))?,
            };
            execute(cli, Command::SetPolling { rate })
        }
        Commands::Rgb { sub, target } => {
            let command = match sub {
                Some(RgbCommands::Mode { mode }) => Command::SetLightingMode { mode: *mode },
                Some(RgbCommands::Color { hex }) => color_command(hex)?,
                None => {
                    let value = target.as_deref().ok_or_else(|| {
                        LorgarError::Config("specify rgb mode MODE or rgb color RRGGBB".into())
                    })?;
                    match crate::parse_lighting_mode(value) {
                        Ok(mode) => Command::SetLightingMode { mode },
                        Err(_) => color_command(value)?,
                    }
                }
            };
            execute(cli, command)
        }
        _ => Err(LorgarError::Config(
            "--daemon/--json is not supported for this legacy integration or raw debug command"
                .into(),
        )),
    }
}

fn color_command(value: &str) -> Result<Command> {
    let (red, green, blue) = lorgar_core::parse_hex_color(value)?;
    Ok(Command::SetLightingColor { red, green, blue })
}

pub fn run(cli: &Cli) -> Option<Result<()>> {
    Some(match &cli.command {
        Commands::Monitor => watch(cli, None),
        Commands::Capture {
            output,
            duration,
            query_state,
            query_interval_ms,
            label,
        } => watch(
            cli,
            Some((output, *duration, *query_state, label, *query_interval_ms)),
        ),
        Commands::DiffCaptures { before, after } => (|| {
            let (_, before) = capture::read_capture(before)?;
            let (_, after) = capture::read_capture(after)?;
            output(
                &json!({"alignment": "ordinal within VID/PID/interface/direction/report/length streams; insertion can shift alignment",
                "offsets_include_report_id": true, "streams": capture::diff(&before, &after)}),
                cli.json,
            )
        })(),
        Commands::Profile { sub } => profile(cli, sub),
        Commands::Completions { shell } => {
            clap_complete::generate(
                *shell,
                &mut Cli::command(),
                "lorgarctl",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Commands::Doctor => doctor(cli),
        Commands::InspectCapture { path } => (|| {
            let (header, events) = capture::read_capture(path)?;
            output(
                &json!({"type":"capture_header", "header": header}),
                cli.json,
            )?;
            for event in events {
                event_output(&event, cli.json)?;
            }
            Ok(())
        })(),
        Commands::Tray | Commands::Upower | Commands::Debug { .. } if !cli.daemon && !cli.json => {
            return None
        }
        _ => control(cli),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn command_tree_and_compatibility() {
        Cli::command().debug_assert();
        for args in [
            vec!["lorgarctl", "battery", "--json"],
            vec!["lorgarctl", "monitor", "--json"],
            vec!["lorgarctl", "--daemon", "dpi", "set", "1", "1600"],
            vec!["lorgarctl", "dpi", "1", "1400"],
            vec![
                "lorgarctl",
                "profile",
                "set",
                "gaming",
                "--dpi-stage",
                "0=800",
                "--rgb",
                "static",
                "--color",
                "ff5500",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }
    #[test]
    fn unicode_hex_never_panics() {
        assert!(crate::parse_hex_payload(&["€€".into()]).is_err());
        assert!(lorgar_core::parse_hex_color("€€").is_err());
        assert!(lorgar_core::protocol::InputPacket::parse(&[]).is_err());
    }
}
