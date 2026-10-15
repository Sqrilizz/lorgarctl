use crate::{Cli, Commands, DpiCommands, FirmwareCommands, PollingSubcommands, RgbCommands};
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

fn color_name(r: u8, g: u8, b: u8) -> String {
    match (r, g, b) {
        (255, 0, 0) => "Red".to_string(),
        (255, 127..=128, 0) => "Orange".to_string(),
        (255, 255, 0) => "Yellow".to_string(),
        (0, 255, 0) => "Green".to_string(),
        (0, 0, 255) => "Blue".to_string(),
        (0, 255, 255) => "Cyan".to_string(),
        (139, 0, 255) => "Violet".to_string(),
        (255, 255, 255) => "White".to_string(),
        (50, 205, 50) => "Lime".to_string(),
        (153, 0, 255) => "Purple".to_string(),
        (30, 144, 255) => "Dodger Blue".to_string(),
        (220, 20, 60) => "Crimson".to_string(),
        (251, 86, 157) => "Pink".to_string(),
        (76, 17, 48) => "Dark Magenta".to_string(),
        _ => format!("RGB({r},{g},{b})"),
    }
}

#[derive(Debug, Clone)]
struct DpiStageDisplay {
    stage: u8,
    dpi: u16,
    hex: String,
    color_name: String,
}

fn get_hardware_or_default_dpi_info(cli: &Cli) -> (Option<u8>, Vec<DpiStageDisplay>) {
    let default_stages = [
        (0u8, 800u16, "#ff0000", "Red"),
        (1, 1600, "#ff7f00", "Orange"),
        (2, 2400, "#ffff00", "Yellow"),
        (3, 3200, "#00ff00", "Green"),
        (4, 6400, "#0000ff", "Blue"),
        (5, 12000, "#00ffff", "Cyan"),
        (6, 8200, "#8b00ff", "Violet"),
        (7, 6400, "#ffffff", "White"),
    ];

    let (hw_dpis, hw_colors, hw_active) = if is_daemon_mode(cli) {
        if let Ok(res) = response(cli, Action::GetDpiTable) {
            let active = get_active_stage(cli);
            (res.dpi_stages, res.stage_colors, active)
        } else {
            (None, None, get_active_stage(cli))
        }
    } else if let Ok(dev) = crate::open_device(cli.device.as_deref(), false) {
        let active = dev
            .query_device_state()
            .ok()
            .and_then(|raw| lorgar_core::protocol::parse_active_dpi_stage(&raw).ok());
        let dpis = dev.get_hardware_dpi_stages().ok().map(|d| d.to_vec());
        let colors = dev.get_hardware_stage_colors().ok().map(|c| c.to_vec());
        (dpis, colors, active)
    } else {
        (None, None, get_active_stage(cli))
    };

    if let Some(dpis) = hw_dpis {
        if dpis.len() >= 8 {
            let mut stages = Vec::with_capacity(8);
            for i in 0..8 {
                let dpi = dpis[i];
                let (hex, name) = if let Some(colors) = &hw_colors {
                    if let Some(&[r, g, b]) = colors.get(i) {
                        let hex = format!("#{r:02x}{g:02x}{b:02x}");
                        let name = color_name(r, g, b);
                        (hex, name)
                    } else {
                        (
                            default_stages[i].2.to_string(),
                            default_stages[i].3.to_string(),
                        )
                    }
                } else {
                    (
                        default_stages[i].2.to_string(),
                        default_stages[i].3.to_string(),
                    )
                };
                stages.push(DpiStageDisplay {
                    stage: i as u8,
                    dpi,
                    hex,
                    color_name: name,
                });
            }
            return (hw_active, stages);
        }
    }

    let active_stage = get_active_stage(cli);
    let stages = default_stages
        .iter()
        .map(|&(stage, dpi, hex, name)| DpiStageDisplay {
            stage,
            dpi,
            hex: hex.to_string(),
            color_name: name.to_string(),
        })
        .collect();
    (active_stage, stages)
}

fn show_dpi_list(cli: &Cli) -> Result<()> {
    let (active_stage, stages) = get_hardware_or_default_dpi_info(cli);

    if cli.json {
        let items: Vec<_> = stages
            .iter()
            .map(|s| {
                json!({
                    "stage": s.stage,
                    "default_dpi": s.dpi,
                    "color": s.hex,
                    "color_name": s.color_name,
                    "active": active_stage == Some(s.stage),
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

    for s in &stages {
        let is_active = active_stage == Some(s.stage);
        let marker = if is_active {
            " ◄ active".green().bold()
        } else {
            "".normal()
        };
        let stage_str = format!("[{}]", s.stage);
        writeln!(
            out,
            "  {} {:>5} DPI   {:<9} ({}){}",
            if is_active {
                stage_str.green().bold()
            } else {
                stage_str.cyan()
            },
            s.dpi.to_string().yellow().bold(),
            s.hex.dimmed(),
            s.color_name,
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
        "  lorgarctl dpi set-all <d0> <d1>.. Set multiple stages in hardware"
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

fn set_all_dpi_stages(cli: &Cli, stages: &[u16]) -> Result<()> {
    if stages.is_empty() || stages.len() > 8 {
        return Err(LorgarError::Config(
            "Specify between 1 and 8 DPI values".into(),
        ));
    }
    for &dpi in stages {
        if !(lorgar_core::MIN_DPI..=lorgar_core::MAX_DPI).contains(&dpi) {
            return Err(LorgarError::InvalidDpi {
                dpi,
                min: lorgar_core::MIN_DPI,
                max: lorgar_core::MAX_DPI,
            });
        }
    }
    if is_daemon_mode(cli) {
        response(
            cli,
            Action::SetDpiStages {
                stages: stages.to_vec(),
            },
        )?;
    } else {
        let dev = crate::open_device(cli.device.as_deref(), false)?;
        for (stage, &dpi) in stages.iter().enumerate() {
            dev.set_dpi(stage as u8, dpi)?;
        }
    }

    if cli.json {
        return output(
            &json!({
                "ok": true,
                "stages_updated": stages.len(),
                "values": stages,
            }),
            cli.json,
        );
    }

    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{} {}",
        "Updated hardware DPI stages:".green().bold(),
        stages
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )?;
    Ok(())
}

fn show_button_mappings(cli: &Cli, secondary: bool) -> Result<()> {
    let mode_name = if secondary {
        "Secondary Profile"
    } else {
        "Primary Profile"
    };
    let bank_id = if secondary { "0xa5" } else { "0xa4" };

    let buttons = if is_daemon_mode(cli) {
        let res = response(cli, Action::GetButtonMappings { secondary })?;
        res.button_mappings.ok_or_else(|| {
            LorgarError::Transport("missing button mappings in daemon response".into())
        })?
    } else {
        let dev = crate::open_device(cli.device.as_deref(), false)?;
        dev.get_hardware_button_mappings(secondary)?
    };

    if cli.json {
        return output(
            &json!({
                "ok": true,
                "secondary": secondary,
                "bank": bank_id,
                "profile": mode_name,
                "buttons": buttons
                    .iter()
                    .map(|b| {
                        json!({
                            "button": b.button_index + 1,
                            "name": b.name,
                            "raw_hex": format!(
                                "{:02x}{:02x}{:02x}{:02x}",
                                b.raw[0], b.raw[1], b.raw[2], b.raw[3]
                            ),
                            "action": b.action.display_name(),
                        })
                    })
                    .collect::<Vec<_>>(),
            }),
            cli.json,
        );
    }

    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{} {} {}",
        "LORGAR MSA10W Button Mappings".bold().white(),
        format!("({mode_name} / Bank {bank_id})").cyan(),
        "[Hardware EEPROM]".green().bold()
    )?;

    for b in &buttons {
        let btn_str = format!("[#{}]", b.button_index + 1);
        let hex_str = format!(
            "{:02x} {:02x} {:02x} {:02x}",
            b.raw[0], b.raw[1], b.raw[2], b.raw[3]
        );
        writeln!(
            out,
            "  {} {:<28} {:<14} {}",
            btn_str.cyan().bold(),
            b.name.white(),
            hex_str.dimmed(),
            b.action.display_name().green().bold()
        )?;
    }

    writeln!(out)?;
    writeln!(
        out,
        "{}",
        "Hardware physical button mapping stored in non-volatile mouse memory.".dimmed()
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
                crate::hex::format_hex_bytes(&packet.report())
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
            crate::hex::format_hex_bytes(raw),
            parsed.as_ref().map(|p| &p.event)
        ),
    };
    writeln!(std::io::stdout().lock(), "{} {text}", event.timestamp_ms)?;
    Ok(())
}

fn watch(cli: &Cli, capture: Option<(&Path, u64, bool, &str, u64)>) -> Result<()> {
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
            test_notify,
        } = &cli.command
        {
            if *test_notify {
                crate::send_desktop_notification(
                    "LORGAR MSA10W: Low Battery (Test)",
                    &format!("Test notification: current battery is {percentage}%."),
                    percentage <= 10,
                );
                if !cli.json {
                    println!("{}", "✓ Sent test battery notification to desktop.".green());
                }
            }
            if *notify && percentage <= *threshold {
                crate::send_desktop_notification(
                    "LORGAR MSA10W: Low Battery",
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
                        crate::hex::format_hex_bytes(raw)
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

fn handle_sleep(cli: &Cli, minutes: Option<u8>) -> Result<()> {
    if let Some(mins) = minutes {
        if !(1..=10).contains(&mins) {
            return Err(LorgarError::Config(format!(
                "sleep timeout must be between 1 and 10 minutes (got {mins})"
            )));
        }
        if is_daemon_mode(cli) {
            response(cli, Action::SetSleepTimeout { minutes: mins })?;
        } else {
            let msa = crate::open_device(cli.device.as_deref(), cli.debug)?;
            msa.set_sleep_timeout(mins, 1)?;
        }
        if !cli.json {
            println!(
                "{} Sleep timeout set to {} minutes.",
                "✓".green().bold(),
                mins.to_string().cyan().bold()
            );
        } else {
            output(
                &json!({ "sleep_timeout_minutes": mins, "ok": true }),
                cli.json,
            )?;
        }
    } else {
        if !cli.json {
            println!(
                "{}",
                "LORGAR MSA10W Sleep / Hibernation Configuration".bold()
            );
            println!("  Allowed range: 1..10 minutes (hardware default: 5 min)");
            println!("  Usage:         lorgarctl sleep <1..10>");
            println!("  Example:       lorgarctl sleep 5");
            println!("  Interactive:   lorgarctl menu (option 7)");
        } else {
            output(
                &json!({ "min_minutes": 1, "max_minutes": 10, "default_minutes": 5 }),
                cli.json,
            )?;
        }
    }
    Ok(())
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
        Commands::Sleep { minutes } => handle_sleep(cli, *minutes),
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
            if let Some(DpiCommands::SetAll { stages }) = sub {
                return set_all_dpi_stages(cli, stages);
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
                Some(DpiCommands::List)
                | Some(DpiCommands::Preset { .. })
                | Some(DpiCommands::SetAll { .. }) => unreachable!(),
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
        Commands::Lod { mm } => {
            let Some(val) = mm else {
                return show_lod_help(cli);
            };
            if is_daemon_mode(cli) {
                response(cli, Action::SetLod { mm: *val })?;
            } else {
                let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
                dev.set_lod(*val)?;
            }
            if cli.json {
                output(&json!({"ok": true, "lod_mm": val}), cli.json)?;
            } else {
                writeln!(
                    std::io::stdout().lock(),
                    "{}",
                    format!("Lift-Off Distance set to {} mm", val)
                        .green()
                        .bold()
                )?;
            }
            Ok(())
        }
        Commands::Debounce { ms, rapid_fire } => {
            let Some(val) = ms else {
                return show_debounce_help(cli);
            };
            if is_daemon_mode(cli) {
                response(
                    cli,
                    Action::SetDebounce {
                        ms: *val,
                        rapid_fire: *rapid_fire,
                    },
                )?;
            } else {
                let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
                dev.set_debounce(*val, *rapid_fire)?;
            }
            if cli.json {
                output(
                    &json!({"ok": true, "debounce_ms": val, "rapid_fire": rapid_fire}),
                    cli.json,
                )?;
            } else {
                writeln!(
                    std::io::stdout().lock(),
                    "{}",
                    format!(
                        "Button debounce set to {} ms (rapid fire: {})",
                        val, rapid_fire
                    )
                    .green()
                    .bold()
                )?;
            }
            Ok(())
        }
        Commands::AngleSnap { enabled } => {
            let Some(val) = enabled else {
                return show_angle_snap_help(cli);
            };
            if is_daemon_mode(cli) {
                response(cli, Action::SetAngleSnapping { enabled: *val })?;
            } else {
                let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
                dev.set_angle_snapping(*val)?;
            }
            if cli.json {
                output(&json!({"ok": true, "angle_snapping": val}), cli.json)?;
            } else {
                writeln!(
                    std::io::stdout().lock(),
                    "{}",
                    format!(
                        "Angle snapping {}",
                        if *val { "enabled" } else { "disabled" }
                    )
                    .green()
                    .bold()
                )?;
            }
            Ok(())
        }
        Commands::MotionSync { enabled } => {
            let Some(val) = enabled else {
                return show_motion_sync_help(cli);
            };
            if is_daemon_mode(cli) {
                response(cli, Action::SetMotionSync { enabled: *val })?;
            } else {
                let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
                dev.set_motion_sync(*val)?;
            }
            if cli.json {
                output(&json!({"ok": true, "motion_sync": val}), cli.json)?;
            } else {
                writeln!(
                    std::io::stdout().lock(),
                    "{}",
                    format!("Motion sync {}", if *val { "enabled" } else { "disabled" })
                        .green()
                        .bold()
                )?;
            }
            Ok(())
        }
        Commands::Buttons { secondary } => show_button_mappings(cli, *secondary),
        Commands::Firmware { sub } => handle_firmware(cli, sub),
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

fn handle_firmware(cli: &Cli, sub: &FirmwareCommands) -> Result<()> {
    match sub {
        FirmwareCommands::Info { path } => {
            let data = std::fs::read(path)?;
            let info = lorgar_core::firmware::verify_header(&data)?;
            let code_bytes = data
                .len()
                .saturating_sub(lorgar_core::firmware::FIRMWARE_HEADER_SIZE);
            if cli.json {
                output(
                    &json!({
                        "valid": true,
                        "header_len": lorgar_core::firmware::FIRMWARE_HEADER_SIZE,
                        "product_name": info.product_name,
                        "ic_name": info.ic_name,
                        "sensor_name": info.sensor_name,
                        "flash_base_addr": format!("0x{:08x}", info.flash_base_addr),
                        "code_payload_bytes": code_bytes,
                        "total_file_bytes": data.len(),
                    }),
                    cli.json,
                )
            } else {
                let mut out = std::io::stdout().lock();
                writeln!(out, "{}", "LORGAR Firmware Container Info".bold().white())?;
                writeln!(
                    out,
                    "  Status:        {}",
                    "Valid (Checksum verified)".green()
                )?;
                writeln!(out, "  Product Name:  {}", info.product_name.cyan())?;
                writeln!(out, "  MCU IC:        {}", info.ic_name.yellow())?;
                writeln!(out, "  Sensor:        {}", info.sensor_name.magenta())?;
                writeln!(out, "  Flash Base:    0x{:08X}", info.flash_base_addr)?;
                writeln!(
                    out,
                    "  Header Size:   0x{:X} ({} bytes)",
                    lorgar_core::firmware::FIRMWARE_HEADER_SIZE,
                    lorgar_core::firmware::FIRMWARE_HEADER_SIZE
                )?;
                writeln!(out, "  Code Size:     {} bytes", code_bytes)?;
                Ok(())
            }
        }
        FirmwareCommands::Create {
            product,
            ic,
            sensor,
            flash_base,
            input,
            output: out_path,
        } => {
            let payload = std::fs::read(input)?;
            let container =
                lorgar_core::firmware::create_firmware(product, ic, sensor, *flash_base, &payload);
            let verified = lorgar_core::firmware::verify_header(&container)?;
            std::fs::write(out_path, &container)?;
            if cli.json {
                output(
                    &json!({
                        "ok": true,
                        "created": out_path.display().to_string(),
                        "product": verified.product_name,
                        "ic": verified.ic_name,
                        "sensor": verified.sensor_name,
                        "payload_size": payload.len(),
                        "container_size": container.len(),
                    }),
                    cli.json,
                )
            } else {
                let mut out = std::io::stdout().lock();
                writeln!(
                    out,
                    "{}",
                    "Firmware Container Created Successfully".green().bold()
                )?;
                writeln!(out, "  Target Output:  {}", out_path.display())?;
                writeln!(out, "  Product:        {}", verified.product_name.cyan())?;
                writeln!(out, "  MCU IC:         {}", verified.ic_name.yellow())?;
                writeln!(out, "  Sensor:         {}", verified.sensor_name.magenta())?;
                writeln!(out, "  Payload Size:   {} bytes", payload.len())?;
                writeln!(out, "  Total Size:     {} bytes", container.len())?;
                Ok(())
            }
        }
        FirmwareCommands::Bootloader => {
            let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
            dev.enter_bootloader()?;
            if cli.json {
                output(
                    &json!({
                        "ok": true,
                        "message": "Device entered USB update / bootloader mode"
                    }),
                    cli.json,
                )
            } else {
                writeln!(
                    std::io::stdout().lock(),
                    "{}",
                    "Device successfully switched to USB Update / Bootloader mode."
                        .green()
                        .bold()
                )?;
                Ok(())
            }
        }
        FirmwareCommands::Dump {
            addr,
            len,
            output: out_path,
        } => {
            if *addr == 0 && *len == 256 {
                let dump = if is_daemon_mode(cli) {
                    let res = response(cli, Action::DumpMemory)?;
                    res.memory_dump.ok_or_else(|| {
                        LorgarError::Transport("missing memory dump in daemon response".into())
                    })?
                } else {
                    let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
                    dev.dump_onboard_memory()?
                };
                let raw_bytes = dump.raw_bytes();

                if let Some(path) = out_path {
                    if path.extension().is_some_and(|ext| ext == "json") {
                        let json_str = serde_json::to_string_pretty(&dump)?;
                        std::fs::write(path, json_str)?;
                    } else {
                        std::fs::write(path, &raw_bytes)?;
                    }

                    if cli.json {
                        output(
                            &json!({
                                "ok": true,
                                "banks": dump.banks.len(),
                                "bytes": raw_bytes.len(),
                                "path": path.display().to_string(),
                                "dpi_stages": dump.parse_dpi_stages(),
                                "rgb_palette": dump.parse_rgb_palette(),
                            }),
                            cli.json,
                        )?;
                    } else {
                        let mut out = std::io::stdout().lock();
                        writeln!(
                            out,
                            "{}",
                            format!(
                                "Successfully dumped {} bytes ({} hardware banks) to {}",
                                raw_bytes.len(),
                                dump.banks.len(),
                                path.display()
                            )
                            .green()
                            .bold()
                        )?;
                        if let Some(stages) = dump.parse_dpi_stages() {
                            writeln!(out, "  Parsed Hardware DPI Stages: {:?}", stages)?;
                        }
                    }
                } else if cli.json {
                    output(&dump, cli.json)?;
                } else {
                    let mut out = std::io::stdout().lock();
                    writeln!(
                        out,
                        "{}",
                        "LORGAR MSA10W Onboard Memory Dump (8 Hardware Banks)"
                            .white()
                            .bold()
                    )?;
                    for bank in &dump.banks {
                        writeln!(
                            out,
                            "  Bank 0x{:02X} ({}) - {} bytes:",
                            bank.opcode,
                            bank.name.cyan(),
                            bank.data.len()
                        )?;
                        for (i, chunk) in bank.data.chunks(16).enumerate() {
                            let mut hex_s = String::new();
                            let mut asc_s = String::new();
                            for &b in chunk {
                                use std::fmt::Write;
                                let _ = write!(hex_s, "{:02x} ", b);
                                asc_s.push(if (32..127).contains(&b) {
                                    b as char
                                } else {
                                    '.'
                                });
                            }
                            writeln!(out, "    {:02x}: {:48} |{}|", i * 16, hex_s, asc_s)?;
                        }
                    }
                    if let Some(stages) = dump.parse_dpi_stages() {
                        writeln!(
                            out,
                            "  {}",
                            format!("Parsed DPI Stages: {:?}", stages).yellow().bold()
                        )?;
                    }
                }
            } else {
                let dumped = if is_daemon_mode(cli) {
                    let res = response(
                        cli,
                        Action::DumpFlash {
                            addr: *addr,
                            len: *len,
                        },
                    )?;
                    res.flash_dump.ok_or_else(|| {
                        LorgarError::Transport("missing flash dump in daemon response".into())
                    })?
                } else {
                    let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
                    dev.dump_flash(*addr, *len)?
                };
                if let Some(path) = out_path {
                    std::fs::write(path, &dumped)?;
                    if cli.json {
                        output(
                            &json!({
                                "ok": true,
                                "addr": addr,
                                "bytes": dumped.len(),
                                "path": path.display().to_string(),
                            }),
                            cli.json,
                        )?;
                    } else {
                        writeln!(
                            std::io::stdout().lock(),
                            "{}",
                            format!(
                                "Dumped {} bytes from flash addr 0x{:04X} to {}",
                                dumped.len(),
                                addr,
                                path.display()
                            )
                            .green()
                            .bold()
                        )?;
                    }
                } else {
                    let hex_str: String = dumped.iter().map(|b| format!("{:02x}", b)).collect();
                    if cli.json {
                        output(
                            &json!({
                                "ok": true,
                                "addr": addr,
                                "bytes": dumped.len(),
                                "hex": hex_str,
                            }),
                            cli.json,
                        )?;
                    } else {
                        let mut out = std::io::stdout().lock();
                        writeln!(
                            out,
                            "{}",
                            format!("Flash dump: 0x{:04X} ({} bytes)", addr, dumped.len())
                                .cyan()
                                .bold()
                        )?;
                        for (i, chunk) in dumped.chunks(16).enumerate() {
                            let offset = addr + (i * 16) as u16;
                            write!(out, "{:04x}: ", offset)?;
                            for b in chunk {
                                write!(out, "{:02x} ", b)?;
                            }
                            writeln!(out)?;
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

fn show_lod_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return output(&json!({"supported_lod_mm": [1, 2]}), cli.json);
    }
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{}",
        "LORGAR MSA10W Lift-Off Distance (LOD)".bold().white()
    )?;
    writeln!(out, "  Supported: 1 mm, 2 mm")?;
    writeln!(out)?;
    writeln!(out, "{}", "Usage:".dimmed())?;
    writeln!(
        out,
        "  lorgarctl lod <1|2>             Set Lift-Off Distance (1mm or 2mm)"
    )?;
    Ok(())
}

fn show_debounce_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return output(&json!({"supported_debounce_range_ms": [1, 20]}), cli.json);
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", "LORGAR MSA10W Button Debounce".bold().white())?;
    writeln!(out, "  Supported: 1..20 ms")?;
    writeln!(out)?;
    writeln!(out, "{}", "Usage:".dimmed())?;
    writeln!(
        out,
        "  lorgarctl debounce <ms> [--rapid-fire]   Set button debounce delay"
    )?;
    Ok(())
}

fn show_angle_snap_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return output(&json!({"options": ["on", "off"]}), cli.json);
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", "LORGAR MSA10W Angle Snapping".bold().white())?;
    writeln!(out, "  Supported: on, off")?;
    writeln!(out)?;
    writeln!(out, "{}", "Usage:".dimmed())?;
    writeln!(
        out,
        "  lorgarctl angle-snap <on|off>   Enable or disable angle snapping"
    )?;
    Ok(())
}

fn show_motion_sync_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return output(&json!({"options": ["on", "off"]}), cli.json);
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", "LORGAR MSA10W Motion Sync".bold().white())?;
    writeln!(out, "  Supported: on, off")?;
    writeln!(out)?;
    writeln!(out, "{}", "Usage:".dimmed())?;
    writeln!(
        out,
        "  lorgarctl motion-sync <on|off>  Enable or disable motion sync"
    )?;
    Ok(())
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
        writeln!(out, "  [7] Set Sleep Timeout (1..10 minutes)")?;
        writeln!(out, "  [8] Test Low-Battery Notification")?;
        writeln!(out, "  [9] Refresh status")?;
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
                    "Sleep / Hibernation Timeout (1..10 minutes)".bold().white()
                );
                println!(
                    "  • Sensor enters deep sleep and shuts off RF transmitter when inactive."
                );
                println!("  • Current allowed range: 1 to 10 minutes.");
                print!("Enter sleep timeout in minutes (1..10, [Enter] to cancel): ");
                stdout().flush()?;
                let mut sbuf = String::new();
                if stdin().read_line(&mut sbuf).is_ok() {
                    let trimmed = sbuf.trim();
                    if !trimmed.is_empty() {
                        if let Ok(m) = trimmed.parse::<u8>() {
                            if (1..=10).contains(&m) {
                                let _ = handle_sleep(cli, Some(m));
                            } else {
                                println!(
                                    "{}",
                                    "Timeout must be between 1 and 10 minutes.".yellow()
                                );
                            }
                        } else {
                            println!("{}", "Invalid number.".red());
                        }
                    }
                }
                pause_prompt();
            }
            "8" => {
                println!("\nSending test battery notification to desktop...");
                let p = battery.unwrap_or(15);
                lorgar_core::notify::send_desktop_notification(
                    "LORGAR MSA10W: Low Battery (Test)",
                    &format!(
                        "Test notification: battery is at {p}%. Consider plugging in charger soon."
                    ),
                    lorgar_core::notify::Urgency::Normal,
                    Some("battery-low"),
                );
                println!("{}", "✓ Sent desktop notification (battery-low)!".green());
                pause_prompt();
            }
            "9" => {}
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
            vec!["lorgarctl", "sleep"],
            vec!["lorgarctl", "sleep", "5"],
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
            vec!["lorgarctl", "lod", "1"],
            vec!["lorgarctl", "debounce", "8"],
            vec!["lorgarctl", "angle-snap", "on"],
            vec!["lorgarctl", "motion-sync", "off"],
            vec![
                "lorgarctl",
                "firmware",
                "info",
                "crates/lorgarctl/src/main.rs",
            ],
            vec!["lorgarctl", "firmware", "bootloader"],
            vec!["lorgarctl", "firmware", "dump", "--len", "64"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }
    #[test]
    fn unicode_hex_never_panics() {
        assert!(crate::hex::parse_hex_payload(&["€€".into()]).is_err());
        assert!(lorgar_core::parse_hex_color("€€").is_err());
        assert!(lorgar_core::protocol::InputPacket::parse(&[]).is_err());
    }
}
