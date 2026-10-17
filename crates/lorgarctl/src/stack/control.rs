use crate::{Cli, Commands, DpiCommands, FirmwareCommands, PollingSubcommands, RgbCommands};
use colored::*;
use lorgar_core::ipc::Action;
use lorgar_core::monitor;
use lorgar_core::protocol::Command;
use lorgar_core::{LorgarError, Result};
use serde_json::json;
use std::io::Write;

pub fn show_dpi_list(cli: &Cli) -> Result<()> {
    let (active_stage, stages) = super::common::get_hardware_or_default_dpi_info(cli);

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
        return super::common::output(
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

pub fn set_all_dpi_stages(cli: &Cli, stages: &[u16]) -> Result<()> {
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
    if super::common::is_daemon_mode(cli) {
        super::common::response(
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
        return super::common::output(
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

pub fn show_button_mappings(cli: &Cli, secondary: bool) -> Result<()> {
    let mode_name = if secondary {
        "Secondary Profile"
    } else {
        "Primary Profile"
    };
    let bank_id = if secondary { "0xa5" } else { "0xa4" };

    let buttons = if super::common::is_daemon_mode(cli) {
        let res = super::common::response(cli, Action::GetButtonMappings { secondary })?;
        res.button_mappings.ok_or_else(|| {
            LorgarError::Transport("missing button mappings in daemon response".into())
        })?
    } else {
        let dev = crate::open_device(cli.device.as_deref(), false)?;
        dev.get_hardware_button_mappings(secondary)?
    };

    if cli.json {
        return super::common::output(
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

pub fn show_polling_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return super::common::output(
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

pub fn show_rgb_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return super::common::output(
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

pub fn status(cli: &Cli, battery_only: bool) -> Result<()> {
    let daemon_mode = super::common::is_daemon_mode(cli);
    let (info, battery, stage, observed, raw, error, applied_profile) = if daemon_mode {
        let state = super::common::response(cli, Action::Status)?
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
                super::common::battery_bar(percentage),
                health
            )?;
            return Ok(());
        }
        super::common::output(
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
                let bar = super::common::battery_bar(p);
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
        super::common::output(
            &json!({"device": info, "battery_percentage": battery, "battery_observed_ms": observed, "applied_profile":applied_profile,
            "active_stage": stage, "charging": null, "polling_rate": null, "lighting_mode": null, "raw":raw, "last_error":error}),
            cli.json,
        )
    }
}

pub fn handle_sleep(cli: &Cli, minutes: Option<u8>) -> Result<()> {
    if let Some(mins) = minutes {
        if !(1..=10).contains(&mins) {
            return Err(LorgarError::Config(format!(
                "sleep timeout must be between 1 and 10 minutes (got {mins})"
            )));
        }
        if super::common::is_daemon_mode(cli) {
            super::common::response(cli, Action::SetSleepTimeout { minutes: mins })?;
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
            super::common::output(
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
            super::common::output(
                &json!({ "min_minutes": 1, "max_minutes": 10, "default_minutes": 5 }),
                cli.json,
            )?;
        }
    }
    Ok(())
}

pub fn color_command(value: &str) -> Result<Command> {
    let (red, green, blue) = lorgar_core::parse_hex_color(value)?;
    Ok(Command::SetLightingColor { red, green, blue })
}

pub fn handle_firmware(cli: &Cli, sub: &FirmwareCommands) -> Result<()> {
    match sub {
        FirmwareCommands::Info { path } => {
            let data = std::fs::read(path)?;
            let info = lorgar_core::firmware::verify_header(&data)?;
            let code_bytes = data
                .len()
                .saturating_sub(lorgar_core::firmware::FIRMWARE_HEADER_SIZE);
            if cli.json {
                super::common::output(
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
                super::common::output(
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
                super::common::output(
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
                let dump = if super::common::is_daemon_mode(cli) {
                    let res = super::common::response(cli, Action::DumpMemory)?;
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
                        super::common::output(
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
                    super::common::output(&dump, cli.json)?;
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
                let dumped = if super::common::is_daemon_mode(cli) {
                    let res = super::common::response(
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
                        super::common::output(
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
                    let hex_str: String = dumped.iter().map(|b| format!("{b:02x}")).collect();
                    if cli.json {
                        super::common::output(
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

pub fn show_lod_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return super::common::output(&json!({"supported_lod_mm": [1, 2]}), cli.json);
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

pub fn show_debounce_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return super::common::output(&json!({"supported_debounce_range_ms": [1, 20]}), cli.json);
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

pub fn show_angle_snap_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return super::common::output(&json!({"options": ["on", "off"]}), cli.json);
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

pub fn show_motion_sync_help(cli: &Cli) -> Result<()> {
    if cli.json {
        return super::common::output(&json!({"options": ["on", "off"]}), cli.json);
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

pub fn control(cli: &Cli) -> Result<()> {
    match &cli.command {
        Commands::Devices => {
            let devices = if super::common::is_daemon_mode(cli) {
                super::common::response(cli, Action::Devices)?.devices
            } else {
                lorgar_core::Msa10w::list_devices()?
            };
            super::common::output(&devices, cli.json)
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
                return super::profile::apply_dpi_preset(cli, name);
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
            super::common::execute(cli, command)
        }
        Commands::Polling { sub, hz } => {
            if sub.is_none() && hz.is_none() {
                return show_polling_help(cli);
            }
            let rate = match sub {
                Some(PollingSubcommands::Set { hz }) => *hz,
                None => hz.ok_or_else(|| LorgarError::Config("specify polling set HZ".into()))?,
            };
            super::common::execute(cli, Command::SetPolling { rate })
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
            super::common::execute(cli, command)
        }
        Commands::Lod { mm } => {
            let Some(val) = mm else {
                return show_lod_help(cli);
            };
            if super::common::is_daemon_mode(cli) {
                super::common::response(cli, Action::SetLod { mm: *val })?;
            } else {
                let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
                dev.set_lod(*val)?;
            }
            if cli.json {
                super::common::output(&json!({"ok": true, "lod_mm": val}), cli.json)?;
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
            if super::common::is_daemon_mode(cli) {
                super::common::response(
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
                super::common::output(
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
            if super::common::is_daemon_mode(cli) {
                super::common::response(cli, Action::SetAngleSnapping { enabled: *val })?;
            } else {
                let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
                dev.set_angle_snapping(*val)?;
            }
            if cli.json {
                super::common::output(&json!({"ok": true, "angle_snapping": val}), cli.json)?;
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
            if super::common::is_daemon_mode(cli) {
                super::common::response(cli, Action::SetMotionSync { enabled: *val })?;
            } else {
                let dev = crate::open_device(cli.device.as_deref(), cli.debug)?;
                dev.set_motion_sync(*val)?;
            }
            if cli.json {
                super::common::output(&json!({"ok": true, "motion_sync": val}), cli.json)?;
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
