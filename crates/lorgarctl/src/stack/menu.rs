use crate::Cli;
use colored::*;
use lorgar_core::ipc::Action;
use lorgar_core::protocol::Command;
use lorgar_core::Result;
use std::io::{stdin, stdout, Write};

pub fn run_menu(cli: &Cli) -> Result<()> {
    loop {
        print!("{}[2J{}[1;1H", 27 as char, 27 as char);
        let mut out = stdout().lock();
        writeln!(
            out,
            "{}",
            "┌─── LORGAR // TERMINAL ──────────────────────────────────────────────────┐"
                .cyan()
                .bold()
        )?;
        writeln!(
            out,
            "{}",
            r#"│  _     ___  ____   ____    _    ____    __  __ ____    _    _  _____    │"#.cyan()
        )?;
        writeln!(
            out,
            "{}",
            r#"│ | |   / _ \|  _ \ / ___|  / \  |  _ \  |  \/  / ___|  / \  | |/ _ \ \   │"#.cyan()
        )?;
        writeln!(
            out,
            "{}",
            r#"│ | |  | | | | |_) | |  _  / _ \ | |_) | | |\/| \___ \ / _ \ | | | | | |  │"#
                .white()
                .bold()
        )?;
        writeln!(
            out,
            "{}",
            r#"│ | |__| |_| |  _ <| |_| |/ ___ \|  _ <  | |  | |___) / ___ \| | |_| | |  │"#
                .white()
                .bold()
        )?;
        writeln!(
            out,
            "{}",
            r#"│ |_____\___/|_| \_\____/_/   \_\_| \_\ |_|  |_|____/_/   \_\_|\___/|_|  │"#.white()
        )?;
        writeln!(
            out,
            "│  {}    │",
            "ENGINE v2.3.4 // SENSOR: PAW3395 // PROTOCOL: TOGRAN TM271".dimmed()
        )?;
        writeln!(
            out,
            "{}",
            "├─── HARDWARE TELEMETRY ──────────────────────────────────────────────────┤"
                .cyan()
                .bold()
        )?;

        let daemon_mode = super::common::is_daemon_mode(cli);
        let (info, battery, stage) = if daemon_mode {
            let state = super::common::response(cli, Action::Status)
                .ok()
                .and_then(|r| r.state);
            (
                state.as_ref().and_then(|s| s.device.clone()),
                state.as_ref().and_then(|s| s.battery_percentage),
                state.as_ref().and_then(|s| s.active_stage),
            )
        } else {
            let msa = crate::open_device(cli.device.as_deref(), false).ok();
            let raw = msa.as_ref().and_then(|m| m.query_device_state().ok());
            let bat = raw
                .as_ref()
                .and_then(|r| lorgar_core::parse_battery_percentage(r).ok());
            let st = raw
                .as_ref()
                .and_then(|r| lorgar_core::protocol::parse_active_dpi_stage(r).ok());
            (msa.map(|m| m.info().clone()), bat, st)
        };

        if let Some(info) = &info {
            let conn = if info.product_id == 0xc601 {
                "2.4G WIRELESS".green().bold()
            } else {
                "USB WIRED".cyan().bold()
            };
            writeln!(
                out,
                "│  DEVICE:    ONLINE ({})   NODE: {:<27} │",
                conn,
                info.path.dimmed()
            )?;
        } else {
            writeln!(
                out,
                "│  DEVICE:    OFFLINE / RECEIVER UNPLUGGED                                │"
            )?;
        }

        if let Some(p) = battery {
            writeln!(
                out,
                "│  POWER:     {:>3}% [{}]       HEALTH: OPTIMAL       │",
                p,
                super::common::battery_bar(p)
            )?;
        } else {
            writeln!(
                out,
                "│  POWER:     STANDBY / SLEEP (move mouse to refresh)                     │"
            )?;
        }

        if let Some(s) = stage {
            let dpis = [800, 1200, 1600, 2400, 3200, 6400, 12000, 16000];
            let dpi_val = dpis.get(s as usize).copied().unwrap_or(1200);
            writeln!(
                out,
                "│  TRACKING:  STAGE {} // {:<5} DPI   1000 Hz // 1ms // 1mm LOD             │",
                s.to_string().cyan().bold(),
                dpi_val.to_string().yellow().bold()
            )?;
        } else {
            writeln!(
                out,
                "│  TRACKING:  STAGE ? // UNKNOWN                                          │"
            )?;
        }

        writeln!(
            out,
            "{}",
            "├─── CONTROL REGISTERS ───────────────────────────────────────────────────┤"
                .cyan()
                .bold()
        )?;
        writeln!(
            out,
            "│  [1] > Switch active DPI stage (0..7)                                  │"
        )?;
        writeln!(
            out,
            "│  [2] > Tune custom resolution for a stage (100..26000 DPI)             │"
        )?;
        writeln!(
            out,
            "│  [3] > Apply esports presets (FPS / Balanced / Speed)                  │"
        )?;
        writeln!(
            out,
            "│  [4] > Set USB polling rate (125 / 250 / 500 / 1000 Hz)                │"
        )?;
        writeln!(
            out,
            "│  [5] > Configure RGB lighting mode (Static, Breathing, Wave, Off)      │"
        )?;
        writeln!(
            out,
            "│  [6] > Set custom RGB hex color                                        │"
        )?;
        writeln!(
            out,
            "│  [7] > Set sleep timeout / hibernation timer (1..10 min)               │"
        )?;
        writeln!(
            out,
            "│  [8] > Test low-battery notification                                   │"
        )?;
        writeln!(
            out,
            "│  [9] > Resync hardware telemetry                                       │"
        )?;
        writeln!(
            out,
            "│  [0] > Exit terminal                                                   │"
        )?;
        writeln!(
            out,
            "{}",
            "└────────────────────────────────────────────────────────────────────────┘"
                .cyan()
                .bold()
        )?;
        write!(out, "wallhack@lorgar:~$ ")?;
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
                            let _ =
                                super::common::execute(cli, Command::SetActiveStage { stage: st });
                        } else {
                            println!("{}", "Stage index must be 0..7".red());
                        }
                    }
                }
                super::common::pause_prompt();
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
                            let _ = super::common::execute(cli, Command::SetDpi { stage: st, dpi });
                        }
                    }
                }
                super::common::pause_prompt();
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
                        let _ = super::profile::apply_dpi_preset(cli, "fps");
                    }
                    "2" | "balanced" => {
                        let _ = super::profile::apply_dpi_preset(cli, "balanced");
                    }
                    "3" | "high" => {
                        let _ = super::profile::apply_dpi_preset(cli, "high");
                    }
                    _ => println!("{}", "Invalid choice".red()),
                }
                super::common::pause_prompt();
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
                    let _ = super::common::execute(cli, Command::SetPolling { rate: r });
                } else {
                    println!("{}", "Invalid polling rate choice".red());
                }
                super::common::pause_prompt();
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
                    let _ = super::common::execute(cli, Command::SetLightingMode { mode: m });
                } else {
                    println!("{}", "Invalid lighting mode".red());
                }
                super::common::pause_prompt();
            }
            "6" => {
                print!("Enter hex color (e.g. #ff5500 or ff0000): ");
                stdout().flush()?;
                let mut cbuf = String::new();
                let _ = stdin().read_line(&mut cbuf);
                if let Ok(cmd) = super::control::color_command(cbuf.trim()) {
                    let _ = super::common::execute(cli, cmd);
                } else {
                    println!("{}", "Invalid hex color format".red());
                }
                super::common::pause_prompt();
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
                                let _ = super::control::handle_sleep(cli, Some(m));
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
                super::common::pause_prompt();
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
                super::common::pause_prompt();
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
