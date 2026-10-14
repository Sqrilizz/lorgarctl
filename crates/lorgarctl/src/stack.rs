use crate::{Cli, Commands, DpiCommands, PollingSubcommands, RgbCommands};
use clap::{CommandFactory, Subcommand};
use colored::*;
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

pub fn is_daemon_mode(cli: &Cli) -> bool {
    if cli.daemon {
        return true;
    }
    if cli.direct || cli.device.is_some() {
        return false;
    }
    let socket = cli.socket.clone().map(Ok).unwrap_or_else(ipc::socket_path);
    if let Ok(path) = socket {
        if path.exists() {
            if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&path) {
                stream
                    .set_read_timeout(Some(std::time::Duration::from_millis(150)))
                    .ok();
                stream
                    .set_write_timeout(Some(std::time::Duration::from_millis(150)))
                    .ok();
                if ipc::write_frame(
                    &mut stream,
                    &ipc::Request {
                        version: ipc::VERSION,
                        action: ipc::Action::Status,
                    },
                )
                .is_ok()
                {
                    if let Ok(resp) = ipc::read_frame::<ipc::Response>(&mut stream) {
                        return resp.ok;
                    }
                }
            }
        }
    }
    false
}

fn battery_bar(percentage: u8) -> String {
    let total_bars = 20;
    let filled = ((percentage as usize * total_bars) + 50) / 100;
    let filled = filled.min(total_bars);
    let empty = total_bars - filled;
    let bar_str: String = "█".repeat(filled) + &"░".repeat(empty);
    if percentage > 50 {
        bar_str.green().to_string()
    } else if percentage > 20 {
        bar_str.yellow().to_string()
    } else {
        bar_str.red().bold().to_string()
    }
}

fn get_active_stage(cli: &Cli) -> Option<u8> {
    if is_daemon_mode(cli) {
        response(cli, Action::Status)
            .ok()
            .and_then(|r| r.state)
            .and_then(|s| s.active_stage)
    } else {
        crate::open_device(cli.device.as_deref(), false)
            .and_then(|dev| dev.query_device_state())
            .ok()
            .and_then(|raw| lorgar_core::protocol::parse_active_dpi_stage(&raw).ok())
    }
}

fn show_dpi_list(cli: &Cli) -> Result<()> {
    let active_stage = get_active_stage(cli);
    let stages = [
        (0u8, 800u16, "#32cd32", "Lime"),
        (1, 1200, "#9900ff", "Purple"),
        (2, 1600, "#1e90ff", "Blue"),
        (3, 2400, "#ff7f27", "Orange"),
        (4, 3200, "#dc143c", "Crimson"),
        (5, 6400, "#fff300", "Yellow"),
        (6, 12000, "#fb569d", "Pink"),
        (7, 16000, "#4c1130", "Dark Magenta"),
    ];

    if cli.json {
        let items: Vec<_> = stages
            .iter()
            .map(|(idx, dpi, hex, name)| {
                json!({
                    "stage": idx,
                    "default_dpi": dpi,
                    "color": hex,
                    "color_name": name,
                    "active": active_stage == Some(*idx),
                })
            })
            .collect();
        return output(
            &json!({
                "ok": true,
                "active_stage": active_stage,
                "stages": items,
            }),
            cli.json,
        );
    }

    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{} {}",
        "LORGAR MSA10W DPI Stages".bold().white(),
        "(PixArt PAW3395 100-26000 DPI)".dimmed()
    )?;

    for (idx, dpi, hex, name) in &stages {
        let is_active = active_stage == Some(*idx);
        let marker = if is_active {
            " ◄ active".green().bold()
        } else {
            "".normal()
        };
        let stage_str = format!("[{idx}]");
        writeln!(
            out,
            "  {} {:>5} DPI   {:<9} ({}){}",
            if is_active {
                stage_str.green().bold()
            } else {
                stage_str.cyan()
            },
            dpi.to_string().yellow().bold(),
            hex.dimmed(),
            name,
            marker
        )?;
    }

    writeln!(out)?;
    writeln!(out, "{}", "Quick usage:".dimmed())?;
    writeln!(
        out,
        "  lorgarctl dpi <stage>             Switch stage (0..7)"
    )?;
    writeln!(
        out,
        "  lorgarctl dpi <stage> <dpi>       Set DPI (e.g. lorgarctl dpi 1 1600)"
    )?;
    writeln!(
        out,
        "  lorgarctl dpi color <stage> <hex> Set indicator LED color"
    )?;
    writeln!(
        out,
        "  lorgarctl dpi preset fps          Apply FPS gaming preset (400/800/1600/3200)"
    )?;
    Ok(())
}

fn apply_dpi_preset(cli: &Cli, name: &str) -> Result<()> {
    let (stages, active): (Vec<(u8, u16)>, u8) = match name.to_ascii_lowercase().as_str() {
        "fps" | "cs" | "shooter" => (vec![(0, 400), (1, 800), (2, 1600), (3, 3200)], 1),
        "balanced" | "standard" | "default" => (vec![(0, 800), (1, 1200), (2, 1600), (3, 2400)], 1),
        "high" | "speed" | "fast" => (vec![(0, 1600), (1, 2400), (2, 3200), (3, 6400)], 2),
        _ => {
            return Err(LorgarError::Config(format!(
                "unknown preset '{name}'. Supported presets: fps, balanced, high"
            )))
        }
    };

    for (stage, dpi) in &stages {
        execute(
            cli,
            Command::SetDpi {
                stage: *stage,
                dpi: *dpi,
            },
        )?;
    }
    execute(cli, Command::SetActiveStage { stage: active })?;

    if !cli.json {
        let stages_str = stages
            .iter()
            .map(|(s, d)| {
                if *s == active {
                    format!("{s}: {d} DPI (active)")
                } else {
                    format!("{s}: {d} DPI")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            std::io::stdout().lock(),
            "{}",
            format!("Applied DPI preset '{name}': [{stages_str}]")
                .green()
                .bold()
        )?;
        return Ok(());
    }

    output(
        &json!({
            "ok": true,
            "preset": name,
            "active_stage": active,
            "stages": stages.iter().map(|(s, d)| json!({"stage": s, "dpi": d})).collect::<Vec<_>>(),
        }),
        cli.json,
    )
}

fn show_polling_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return output(
            &json!({"supported_rates_hz": [125, 250, 500, 1000]}),
            cli.json,
        );
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", "LORGAR MSA10W USB Polling Rate".bold().white())?;
    writeln!(out, "  Supported: 125 Hz, 250 Hz, 500 Hz, 1000 Hz")?;
    writeln!(out)?;
    writeln!(out, "{}", "Usage:".dimmed())?;
    writeln!(
        out,
        "  lorgarctl polling <hz>            Set rate (e.g. lorgarctl polling 1000)"
    )?;
    Ok(())
}

fn show_rgb_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return output(
            &json!({
                "supported_modes": ["off", "static", "breathing", "color-cycle", "tapping", "wave"]
            }),
            cli.json,
        );
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", "LORGAR MSA10W RGB Lighting".bold().white())?;
    writeln!(
        out,
        "  Supported modes: off, static, breathing, color-cycle, tapping, wave"
    )?;
    writeln!(out)?;
    writeln!(out, "{}", "Usage:".dimmed())?;
    writeln!(
        out,
        "  lorgarctl rgb <mode>              Set lighting mode (e.g. lorgarctl rgb wave)"
    )?;
    writeln!(
        out,
        "  lorgarctl rgb <#RRGGBB>           Set static color (e.g. lorgarctl rgb #ff5500)"
    )?;
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
    if is_daemon_mode(cli) {
        response(
            cli,
            Action::Execute {
                command: command.clone(),
            },
        )?;
    } else {
        crate::open_device(cli.device.as_deref(), false)?.execute(&command)?;
    }
    if !cli.json {
        let msg = match &command {
            Command::SetActiveStage { stage } => format!("Activated DPI stage {stage}"),
            Command::SetDpi { stage, dpi } => format!("Set DPI stage {stage} to {dpi} DPI"),
            Command::SetStageColor {
                stage,
                red,
                green,
                blue,
            } => {
                format!("Set DPI stage {stage} color to #{red:02x}{green:02x}{blue:02x}")
            }
            Command::SetPolling { rate } => format!("Set polling rate to {} Hz", rate.hz()),
            Command::SetLightingMode { mode } => format!("Set lighting mode to {mode:?}"),
            Command::SetLightingColor { red, green, blue } => {
                format!("Set lighting color to #{red:02x}{green:02x}{blue:02x}")
            }
            _ => format!("Applied {description} (sent; readback not verified)"),
        };
        writeln!(std::io::stdout().lock(), "{}", msg.green().bold())?;
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
            if is_daemon_mode(cli) {
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
                    if is_daemon_mode(cli) {
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
    let mut cursor = if is_daemon_mode(cli) {
        response(cli, Action::Events { after: u64::MAX })?.cursor
    } else {
        0
    };
    let mut next_daemon_query_ms = 0;
    while !stopped.load(Ordering::Relaxed)
        && (duration == 0 || start.elapsed().as_secs() < duration)
    {
        if is_daemon_mode(cli)
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
        let events = if is_daemon_mode(cli) {
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
    let daemon_mode = is_daemon_mode(cli);
    let (info, battery, stage, observed, raw, error, applied_profile) = if daemon_mode {
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
        let stage = lorgar_core::protocol::parse_active_dpi_stage(&raw).ok();
        (
            Some(device.info().clone()),
            Some(battery),
            stage,
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
            let health = if percentage > 50 {
                "good".green()
            } else if percentage > 20 {
                "moderate".yellow()
            } else {
                "low (charge soon)".red().bold()
            };
            writeln!(
                std::io::stdout().lock(),
                "Battery: {:>3}% [{}] ({})",
                percentage,
                battery_bar(percentage),
                health
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
            let mode_tag = if daemon_mode {
                "[daemon]".cyan()
            } else {
                "[direct]".purple()
            };

            if let Some(info) = &info {
                writeln!(
                    out,
                    "{} {} {}",
                    "LORGAR MSA10W".bold().white(),
                    "● Connected".green().bold(),
                    mode_tag
                )?;
                let conn_type = if info.product_id == 0xc601 {
                    "2.4 GHz Wireless"
                } else {
                    "USB Wired"
                };
                writeln!(
                    out,
                    "  {:<14} {} ({})",
                    "Device:".dimmed(),
                    info.path.cyan(),
                    conn_type
                )?;
            } else {
                writeln!(
                    out,
                    "{} {} {}",
                    "LORGAR MSA10W".bold().white(),
                    "○ Disconnected".red().bold(),
                    mode_tag
                )?;
            }

            let is_sleeping = battery.is_none()
                && error
                    .as_deref()
                    .map(|e| e.contains("timed out"))
                    .unwrap_or(false);

            if let Some(p) = battery {
                let bar = battery_bar(p);
                let health = if p > 50 {
                    "good".green()
                } else if p > 20 {
                    "moderate".yellow()
                } else {
                    "low (charge soon)".red().bold()
                };
                writeln!(
                    out,
                    "  {:<14} [{}] {:>3}% ({})",
                    "Battery:".dimmed(),
                    bar,
                    p,
                    health
                )?;
            } else if is_sleeping {
                writeln!(
                    out,
                    "  {:<14} {}",
                    "Battery:".dimmed(),
                    "asleep (standby)".yellow()
                )?;
            } else {
                writeln!(out, "  {:<14} unknown", "Battery:".dimmed())?;
            }

            if let Some(s) = stage {
                let stages = [
                    (800, "#32cd32 (Lime)"),
                    (1200, "#9900ff (Purple)"),
                    (1600, "#1e90ff (Blue)"),
                    (2400, "#ff7f27 (Orange)"),
                    (3200, "#dc143c (Crimson)"),
                    (6400, "#fff300 (Yellow)"),
                    (12000, "#fb569d (Pink)"),
                    (16000, "#4c1130 (Dark Magenta)"),
                ];
                let (dpi_val, color_tag) = stages.get(s as usize).copied().unwrap_or((1200, ""));
                writeln!(
                    out,
                    "  {:<14} Stage {} ({} DPI) {}",
                    "DPI Stage:".dimmed(),
                    s.to_string().bold().cyan(),
                    dpi_val.to_string().bold().yellow(),
                    color_tag.dimmed()
                )?;
            } else {
                writeln!(out, "  {:<14} unknown", "DPI Stage:".dimmed())?;
            }

            let lighting_readback = raw.as_ref().and_then(|r| {
                lorgar_core::protocol::parse_lighting_mode_readback(r)
                    .ok()
                    .flatten()
            });
            if let Some(mode) = lighting_readback {
                writeln!(out, "  {:<14} {:?}", "Lighting:".dimmed(), mode)?;
            }

            writeln!(
                out,
                "  {:<14} {}",
                "Profile:".dimmed(),
                applied_profile.as_deref().unwrap_or("none")
            )?;

            if is_sleeping {
                writeln!(
                    out,
                    "  {:<14} {}",
                    "State:".dimmed(),
                    "Standby / Sleep (hibernation)".yellow().bold()
                )?;
                writeln!(
                    out,
                    "  {:<14} {}",
                    "Notice:".dimmed(),
                    "Mouse is sleeping to save power. Move mouse or click to wake.".cyan()
                )?;
            } else if let Some(error) = &error {
                writeln!(out, "  {:<14} {}", "Notice:".yellow(), error)?;
            }
            if cli.debug {
                if let Some(raw) = &raw {
                    writeln!(
                        out,
                        "  {:<14} {}",
                        "Raw:".dimmed(),
                        crate::format_hex_bytes(raw)
                    )?;
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
            let devices = if is_daemon_mode(cli) {
                response(cli, Action::Devices)?.devices
            } else {
                lorgar_core::Msa10w::list_devices()?
            };
            output(&devices, cli.json)
        }
        Commands::Status => status(cli, false),
        Commands::Battery { .. } => status(cli, true),
        Commands::Dpi { sub, stage, dpi } => {
            if matches!(sub, Some(DpiCommands::List))
                || (sub.is_none() && stage.is_none() && dpi.is_none())
            {
                return show_dpi_list(cli);
            }
            if let Some(DpiCommands::Preset { name }) = sub {
                return apply_dpi_preset(cli, name);
            }
            let command = match sub {
                Some(DpiCommands::Set { stage, dpi }) => Command::SetDpi {
                    stage: *stage,
                    dpi: *dpi,
                },
                Some(DpiCommands::Active { stage }) => Command::SetActiveStage { stage: *stage },
                Some(DpiCommands::Color { stage, hex }) => {
                    let (red, green, blue) = lorgar_core::parse_hex_color(hex)?;
                    Command::SetStageColor {
                        stage: *stage,
                        red,
                        green,
                        blue,
                    }
                }
                Some(DpiCommands::List) | Some(DpiCommands::Preset { .. }) => unreachable!(),
                None => {
                    match (stage, dpi) {
                        (Some(stage), Some(dpi)) => Command::SetDpi {
                            stage: *stage,
                            dpi: *dpi,
                        },
                        (Some(stage), None) => Command::SetActiveStage { stage: *stage },
                        _ => return Err(LorgarError::Config(
                            "specify dpi set STAGE DPI, dpi active STAGE, or dpi color STAGE HEX"
                                .into(),
                        )),
                    }
                }
            };
            execute(cli, command)
        }
        Commands::Polling { sub, hz } => {
            if sub.is_none() && hz.is_none() {
                return show_polling_help(cli);
            }
            let rate = match sub {
                Some(PollingSubcommands::Set { hz }) => *hz,
                None => hz.ok_or_else(|| LorgarError::Config("specify polling set HZ".into()))?,
            };
            execute(cli, Command::SetPolling { rate })
        }
        Commands::Rgb { sub, target } => {
            if sub.is_none() && target.is_none() {
                return show_rgb_help(cli);
            }
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

fn run_menu(cli: &Cli) -> Result<()> {
    use std::io::{stdin, stdout, Write};

    loop {
        print!("{}[2J{}[1;1H", 27 as char, 27 as char);
        let mut out = stdout().lock();
        writeln!(
            out,
            "{}",
            "═══════════════════════════════════════════════════════════".cyan()
        )?;
        writeln!(
            out,
            "{}",
            "         LORGAR MSA10W Control Menu (PAW3395)"
                .bold()
                .white()
        )?;
        writeln!(
            out,
            "{}",
            "═══════════════════════════════════════════════════════════".cyan()
        )?;

        let daemon_mode = is_daemon_mode(cli);
        let (info, battery, stage) = if daemon_mode {
            let state = response(cli, Action::Status).ok().and_then(|r| r.state);
            (
                state.as_ref().and_then(|s| s.device.clone()),
                state.as_ref().and_then(|s| s.battery_percentage),
                state.as_ref().and_then(|s| s.active_stage),
            )
        } else {
            let dev = crate::open_device(cli.device.as_deref(), false).ok();
            let raw = dev.as_ref().and_then(|d| d.query_device_state().ok());
            (
                dev.as_ref().map(|d| d.info().clone()),
                raw.as_ref()
                    .and_then(|r| lorgar_core::parse_battery_percentage(r).ok()),
                raw.as_ref()
                    .and_then(|r| lorgar_core::protocol::parse_active_dpi_stage(r).ok()),
            )
        };

        if let Some(info) = &info {
            let conn = if info.product_id == 0xc601 {
                "2.4 GHz Wireless"
            } else {
                "USB Wired"
            };
            writeln!(out, "  Device:  {} ({})", info.path.cyan(), conn)?;
        } else {
            writeln!(
                out,
                "  Device:  {}",
                "Disconnected / Receiver unplugged".red()
            )?;
        }

        if let Some(p) = battery {
            writeln!(out, "  Battery: {:>3}% [{}]", p, battery_bar(p))?;
        } else {
            writeln!(
                out,
                "  Battery: {}",
                "Asleep / Standby (move mouse to wake)".yellow()
            )?;
        }

        if let Some(s) = stage {
            let dpis = [800, 1200, 1600, 2400, 3200, 6400, 12000, 16000];
            let dpi_val = dpis.get(s as usize).copied().unwrap_or(1200);
            writeln!(
                out,
                "  DPI:     Stage {} ({} DPI)",
                s.to_string().cyan().bold(),
                dpi_val.to_string().yellow().bold()
            )?;
        } else {
            writeln!(
                out,
                "  DPI:     {}",
                "unknown (move mouse to query)".dimmed()
            )?;
        }

        writeln!(
            out,
            "{}",
            "───────────────────────────────────────────────────────────".dimmed()
        )?;
        writeln!(out, "  [1] Switch active DPI stage (0..7)")?;
        writeln!(out, "  [2] Set custom DPI value for a stage")?;
        writeln!(out, "  [3] Apply DPI Preset (FPS / Balanced / High)")?;
        writeln!(out, "  [4] Change Polling Rate (125 / 250 / 500 / 1000 Hz)")?;
        writeln!(out, "  [5] Change RGB Lighting Mode")?;
        writeln!(out, "  [6] Set RGB Static Color")?;
        writeln!(out, "  [7] Power-Saving & Sleep (Hibernation) Info")?;
        writeln!(out, "  [8] Refresh status")?;
        writeln!(out, "  [0] Exit")?;
        writeln!(
            out,
            "{}",
            "───────────────────────────────────────────────────────────".dimmed()
        )?;
        write!(out, "Select option: ")?;
        out.flush()?;
        drop(out);

        let mut input = String::new();
        if stdin().read_line(&mut input).is_err() {
            break;
        }
        let choice = input.trim();

        match choice {
            "0" | "q" | "exit" => {
                println!("Exiting menu.");
                break;
            }
            "1" => {
                print!("Enter stage index to activate (0..7): ");
                stdout().flush()?;
                let mut buf = String::new();
                if stdin().read_line(&mut buf).is_ok() {
                    if let Ok(st) = buf.trim().parse::<u8>() {
                        if st < 8 {
                            let _ = execute(cli, Command::SetActiveStage { stage: st });
                        } else {
                            println!("{}", "Stage index must be 0..7".red());
                        }
                    }
                }
                pause_prompt();
            }
            "2" => {
                print!("Enter stage index (0..7): ");
                stdout().flush()?;
                let mut buf = String::new();
                let _ = stdin().read_line(&mut buf);
                if let Ok(st) = buf.trim().parse::<u8>() {
                    if st < 8 {
                        print!("Enter DPI value (100..26000): ");
                        stdout().flush()?;
                        let mut dpibuf = String::new();
                        let _ = stdin().read_line(&mut dpibuf);
                        if let Ok(dpi) = dpibuf.trim().parse::<u16>() {
                            let _ = execute(cli, Command::SetDpi { stage: st, dpi });
                        }
                    }
                }
                pause_prompt();
            }
            "3" => {
                println!("\nAvailable presets:");
                println!("  [1] FPS Gaming    (400/800/1600/3200, active: 800)");
                println!("  [2] Balanced      (800/1200/1600/2400, active: 1200)");
                println!("  [3] High Speed    (1600/2400/3200/6400, active: 3200)");
                print!("Choose preset [1-3]: ");
                stdout().flush()?;
                let mut pbuf = String::new();
                let _ = stdin().read_line(&mut pbuf);
                match pbuf.trim() {
                    "1" | "fps" => {
                        let _ = apply_dpi_preset(cli, "fps");
                    }
                    "2" | "balanced" => {
                        let _ = apply_dpi_preset(cli, "balanced");
                    }
                    "3" | "high" => {
                        let _ = apply_dpi_preset(cli, "high");
                    }
                    _ => println!("{}", "Invalid choice".red()),
                }
                pause_prompt();
            }
            "4" => {
                println!("\nSelect Polling Rate:");
                println!("  [1] 125 Hz   (8 ms latency, maximum battery saving)");
                println!("  [2] 250 Hz   (4 ms latency)");
                println!("  [3] 500 Hz   (2 ms latency, balanced)");
                println!("  [4] 1000 Hz  (1 ms latency, competitive gaming)");
                print!("Choose rate [1-4]: ");
                stdout().flush()?;
                let mut pbuf = String::new();
                let _ = stdin().read_line(&mut pbuf);
                let rate = match pbuf.trim() {
                    "1" | "125" => Some(lorgar_core::PollingRate::Hz125),
                    "2" | "250" => Some(lorgar_core::PollingRate::Hz250),
                    "3" | "500" => Some(lorgar_core::PollingRate::Hz500),
                    "4" | "1000" => Some(lorgar_core::PollingRate::Hz1000),
                    _ => None,
                };
                if let Some(r) = rate {
                    let _ = execute(cli, Command::SetPolling { rate: r });
                } else {
                    println!("{}", "Invalid polling rate choice".red());
                }
                pause_prompt();
            }
            "5" => {
                println!("\nSelect Lighting Mode:");
                println!("  [1] Wave (dynamic rainbow wave)");
                println!("  [2] Breathing");
                println!("  [3] Color Cycle");
                println!("  [4] Tapping (react on click)");
                println!("  [5] Static (solid color)");
                println!("  [6] Off (turn off RGB for max battery)");
                print!("Choose mode [1-6]: ");
                stdout().flush()?;
                let mut mbuf = String::new();
                let _ = stdin().read_line(&mut mbuf);
                let mode = match mbuf.trim() {
                    "1" | "wave" => Some(lorgar_core::LightingMode::Wave),
                    "2" | "breathing" => Some(lorgar_core::LightingMode::Breathing),
                    "3" | "color-cycle" => Some(lorgar_core::LightingMode::ColorCycle),
                    "4" | "tapping" => Some(lorgar_core::LightingMode::Tapping),
                    "5" | "static" => Some(lorgar_core::LightingMode::Static),
                    "6" | "off" => Some(lorgar_core::LightingMode::Off),
                    _ => None,
                };
                if let Some(m) = mode {
                    let _ = execute(cli, Command::SetLightingMode { mode: m });
                } else {
                    println!("{}", "Invalid lighting mode".red());
                }
                pause_prompt();
            }
            "6" => {
                print!("Enter hex color (e.g. #ff5500 or ff0000): ");
                stdout().flush()?;
                let mut cbuf = String::new();
                let _ = stdin().read_line(&mut cbuf);
                if let Ok(cmd) = color_command(cbuf.trim()) {
                    let _ = execute(cli, cmd);
                } else {
                    println!("{}", "Invalid hex color format".red());
                }
                pause_prompt();
            }
            "7" => {
                println!(
                    "\n{}",
                    "LORGAR MSA10W Power Saving & Hibernation".bold().white()
                );
                println!("  • Sensor: PixArt PAW3395 with multi-stage dynamic power management.");
                println!(
                    "  • Light Standby: After ~30s of inactivity, sensor tracking frequency dims."
                );
                println!(
                    "  • Deep Sleep (Hibernation): Shuts off RF transmitter to preserve battery."
                );
                println!("    The 2.4 GHz USB receiver remains connected to your PC, but the mouse sleeps.");
                println!(
                    "  • Wake-up: Move the mouse or click any button to resume tracking in <50ms."
                );
                println!("  • Tip: Set RGB to 'off' or Polling to 500 Hz to extend battery life up to 100+ hours.");
                pause_prompt();
            }
            "8" => {}
            _ => {
                println!("{}", "Unknown option".yellow());
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
    Ok(())
}

fn pause_prompt() {
    use std::io::{stdin, stdout, Write};
    print!("\nPress Enter to continue...");
    let _ = stdout().flush();
    let mut dummy = String::new();
    let _ = stdin().read_line(&mut dummy);
}

pub fn run(cli: &Cli) -> Option<Result<()>> {
    Some(match &cli.command {
        Commands::Menu => run_menu(cli),
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
        Commands::Tray | Commands::Upower | Commands::Debug { .. }
            if !is_daemon_mode(cli) && !cli.json =>
        {
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
            vec!["lorgarctl", "status"],
            vec!["lorgarctl", "dpi"],
            vec!["lorgarctl", "dpi", "list"],
            vec!["lorgarctl", "dpi", "preset", "fps"],
            vec!["lorgarctl", "polling"],
            vec!["lorgarctl", "rgb"],
            vec!["lorgarctl", "--direct", "status"],
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
