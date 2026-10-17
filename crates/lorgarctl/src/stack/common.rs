use crate::Cli;
use colored::*;
use lorgar_core::ipc::{self, Action};
use lorgar_core::protocol::Command;
use lorgar_core::{LorgarError, Result};
use serde_json::json;
use std::io::Write;

#[derive(Debug, Clone)]
pub struct DpiStageDisplay {
    pub stage: u8,
    pub dpi: u16,
    pub hex: String,
    pub color_name: String,
}

pub fn output(value: &impl serde::Serialize, as_json: bool) -> Result<()> {
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

pub fn battery_bar(percentage: u8) -> String {
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

pub fn get_active_stage(cli: &Cli) -> Option<u8> {
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

pub fn color_name(r: u8, g: u8, b: u8) -> String {
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

pub fn get_hardware_or_default_dpi_info(cli: &Cli) -> (Option<u8>, Vec<DpiStageDisplay>) {
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

pub fn response(cli: &Cli, action: Action) -> Result<ipc::Response> {
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

pub fn execute(cli: &Cli, command: Command) -> Result<()> {
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

pub fn pause_prompt() {
    print!("{}", "\nPress [Enter] to continue...".dimmed());
    std::io::stdout().flush().ok();
    let mut dummy = String::new();
    let _ = std::io::stdin().read_line(&mut dummy);
}
