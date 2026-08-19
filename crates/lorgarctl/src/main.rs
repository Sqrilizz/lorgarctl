use clap::{Parser, Subcommand};
use colored::*;
use lorgar_core::{LightingMode, LorgarError, Msa10w, PollingRate, Result};

#[derive(Parser, Debug)]
#[command(
    name = "lorgarctl",
    author,
    version,
    about = "Linux CLI controller & diagnostic tool for LORGAR MSA10W gaming mouse",
    long_about = None
)]
struct Cli {
    /// Enable detailed debug output and packet inspection
    #[arg(global = true, short = 'd', long)]
    debug: bool,

    /// Override automatic discovery with specific hidraw path (e.g. /dev/hidraw2)
    #[arg(global = true, short = 'p', long)]
    device: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show mouse dashboard summary (battery, device info, connection)
    #[command(alias = "info", alias = "hub")]
    Status,

    /// Run native System Tray icon daemon for KDE Plasma / GNOME / XFCE
    #[command(alias = "daemon")]
    Tray,

    /// Enumerate and display diagnostic info for connected LORGAR devices
    Devices,

    /// Display battery status
    Battery {
        /// Print short string for KDE panel / status bars (e.g. "83%" or "🖱️ 83%")
        #[arg(short, long)]
        short: bool,

        /// Output JSON payload for KDE widgets, Waybar, or Polybar
        #[arg(short, long)]
        json: bool,

        /// Send desktop notification if battery level is below threshold
        #[arg(short, long)]
        notify: bool,

        /// Low battery notification threshold percentage (default: 20)
        #[arg(short, long, default_value_t = 20)]
        threshold: u8,
    },

    /// Manage DPI settings and active stage (e.g. lorgarctl dpi 1 or lorgarctl dpi 1 1400)
    Dpi {
        #[command(subcommand)]
        sub: Option<DpiCommands>,

        /// DPI stage index to activate or configure (0..7)
        stage: Option<u8>,

        /// DPI value in decimal (e.g. 800, 1400, 1600, 3000)
        dpi: Option<u16>,
    },

    /// Manage USB polling rate (e.g. lorgarctl polling 1000 or lorgarctl polling set 1000)
    Polling {
        #[command(subcommand)]
        sub: Option<PollingSubcommands>,

        /// Frequency in Hz: 125 | 250 | 500 | 1000
        #[arg(value_parser = parse_polling_rate)]
        hz: Option<PollingRate>,
    },

    /// Manage RGB lighting settings and modes (e.g. lorgarctl rgb wave or lorgarctl rgb ff0000)
    Rgb {
        #[command(subcommand)]
        sub: Option<RgbCommands>,

        /// Direct mode name (static, breathing, color-cycle, tapping, wave) or hex color (#RRGGBB)
        target: Option<String>,
    },

    /// Developer and reverse-engineering utilities
    Debug {
        #[command(subcommand)]
        sub: DebugCommands,
    },
}

#[derive(Subcommand, Debug)]
enum PollingSubcommands {
    /// Set USB polling rate in Hz (125, 250, 500, 1000)
    Set {
        /// Frequency in Hz: 125 | 250 | 500 | 1000
        #[arg(value_parser = parse_polling_rate)]
        hz: PollingRate,
    },
}

fn parse_polling_rate(s: &str) -> std::result::Result<PollingRate, String> {
    let hz: u16 = s.parse().map_err(|_| {
        format!(
            "unsupported polling rate: {} Hz\nsupported values: 125, 250, 500, 1000",
            s
        )
    })?;
    PollingRate::from_hz(hz).map_err(|_| {
        format!(
            "unsupported polling rate: {} Hz\nsupported values: 125, 250, 500, 1000",
            hz
        )
    })
}

#[derive(Subcommand, Debug)]
enum RgbCommands {
    /// Set RGB lighting mode (static, breathing, color-cycle, tapping, wave)
    Mode {
        /// Lighting mode name: static | breathing | color-cycle | tapping | wave
        #[arg(value_parser = parse_lighting_mode)]
        mode: LightingMode,
    },

    /// Set static RGB lighting color (format: RRGGBB or #RRGGBB, e.g. ff0000, #a28466)
    Color {
        /// Hex color string
        hex: String,
    },
}

fn parse_lighting_mode(s: &str) -> std::result::Result<LightingMode, String> {
    match s.to_lowercase().replace('_', "-").as_str() {
        "static" => Ok(LightingMode::Static),
        "breathing" => Ok(LightingMode::Breathing),
        "color-cycle" | "colorcycle" => Ok(LightingMode::ColorCycle),
        "tapping" => Ok(LightingMode::Tapping),
        "wave" => Ok(LightingMode::Wave),
        _ => Err(format!(
            "Invalid lighting mode '{}'. Valid options: static, breathing, color-cycle, tapping, wave",
            s
        )),
    }
}

#[derive(Subcommand, Debug)]
enum DpiCommands {
    /// Set DPI value for a specific stage (stage: 0..7, dpi: 100..12000)
    Set {
        /// DPI stage index (0..7)
        stage: u8,
        /// DPI value in decimal (e.g. 800, 1400, 1600, 3000)
        dpi: u16,
    },

    /// Switch active DPI stage index (stage: 0..7)
    Active {
        /// DPI stage index to activate (0..7)
        stage: u8,
    },
}

#[derive(Subcommand, Debug)]
enum DebugCommands {
    /// Send raw 8-byte feature report to configuration interface (DANGER: for development)
    SendFeature {
        /// 8 hex bytes (e.g. "26 00 01 78 05 00 00 00" or single string "2600017805000000")
        bytes: Vec<String>,
    },

    /// Send 8-byte query feature report and read 64-byte response packet
    Query {
        /// 8 hex bytes (defaults to 82 00 00 00 00 00 00 00)
        bytes: Vec<String>,
    },
}

fn render_battery_bar(pct: u8) -> String {
    let filled = ((pct as usize) + 5) / 10;
    let empty = 10 - filled.min(10);
    format!("[{}{}]", "█".repeat(filled.min(10)), "░".repeat(empty))
}

fn format_hex_bytes(slice: &[u8]) -> String {
    slice
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_hex_payload(inputs: &[String]) -> Result<[u8; 8]> {
    let joined = inputs.join(" ").replace("0x", "").replace(',', "");
    let hex_chars: String = joined.chars().filter(|c| !c.is_whitespace()).collect();

    if hex_chars.len() % 2 != 0 {
        return Err(LorgarError::HexParseError(
            "Odd number of hex characters".into(),
        ));
    }

    let mut bytes = Vec::new();
    for i in (0..hex_chars.len()).step_by(2) {
        let byte_str = &hex_chars[i..i + 2];
        let byte = u8::from_str_radix(byte_str, 16)
            .map_err(|e| LorgarError::HexParseError(format!("Invalid hex byte '{}': {}", byte_str, e)))?;
        bytes.push(byte);
    }

    if bytes.len() != 8 {
        return Err(LorgarError::InvalidFeatureLength { len: bytes.len() });
    }

    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn open_device(cli_device: Option<&str>, debug: bool) -> Result<Msa10w> {
    if let Some(path) = cli_device {
        if debug {
            println!("{}", format!("Opening device explicitly at path: {}", path).dimmed());
        }
        Msa10w::open_path(path)
    } else {
        if debug {
            println!("{}", "Auto-discovering LORGAR MSA10W configuration interface...".dimmed());
        }
        Msa10w::open()
    }
}

fn send_desktop_notification(title: &str, body: &str, critical: bool) {
    let mut cmd = std::process::Command::new("notify-send");
    cmd.arg("-a").arg("lorgarctl");
    cmd.arg("-i").arg("input-mouse");
    if critical {
        cmd.arg("-u").arg("critical");
    } else {
        cmd.arg("-u").arg("normal");
    }
    cmd.arg(title).arg(body);
    let _ = cmd.status();
}

struct LorgarTray {
    battery_percentage: u8,
    active_stage: u8,
    polling_rate_str: String,
    rgb_mode_str: String,
}

impl ksni::Tray for LorgarTray {
    fn icon_name(&self) -> String {
        "input-mouse".to_string()
    }

    fn title(&self) -> String {
        format!("LORGAR MSA10W ({}%)", self.battery_percentage)
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: format!("LORGAR MSA10W: {}%", self.battery_percentage),
            description: format!(
                "Stage: {} | Polling: {} | RGB: {}",
                self.active_stage, self.polling_rate_str, self.rgb_mode_str
            ),
            icon_name: "input-mouse".to_string(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: format!("🔋 Battery: {}%", self.battery_percentage),
                enabled: false,
                ..Default::default()
            }.into(),
            StandardItem {
                label: format!("🎯 Active Stage: {}", self.active_stage),
                enabled: false,
                ..Default::default()
            }.into(),
            StandardItem {
                label: format!("⚡ Polling Rate: {}", self.polling_rate_str),
                enabled: false,
                ..Default::default()
            }.into(),
            StandardItem {
                label: format!("🎨 RGB Mode: {}", self.rgb_mode_str),
                enabled: false,
                ..Default::default()
            }.into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }.into(),
        ]
    }
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    if cli.debug {
        std::env::set_var("RUST_LOG", "debug");
        let _ = env_logger::builder().is_test(false).try_init();
    }

    match run_app(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{} {}", "Error:".red().bold(), err);
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_app(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Tray => {
            println!("{}", "Starting LORGAR MSA10W System Tray Daemon...".bold().green());
            println!("Icon registered in System Tray. Low battery notifications active.");

            let dev = open_device(cli.device.as_deref(), cli.debug)?;
            let initial_status = dev.get_status().ok();

            let initial_bat = initial_status.as_ref().map(|s| s.battery_percentage).unwrap_or(0);
            let initial_stage = initial_status.as_ref().map(|s| s.active_stage).unwrap_or(0);
            let initial_poll = initial_status.as_ref().and_then(|s| s.polling_rate).map(|r| format!("{} Hz", r.hz())).unwrap_or_else(|| "N/A".into());
            let initial_rgb = initial_status.as_ref().and_then(|s| s.lighting_mode).map(|m| format!("{:?}", m)).unwrap_or_else(|| "N/A".into());

            let tray = LorgarTray {
                battery_percentage: initial_bat,
                active_stage: initial_stage,
                polling_rate_str: initial_poll,
                rgb_mode_str: initial_rgb,
            };

            let service = ksni::TrayService::new(tray);
            let handle = service.handle();
            service.spawn();

            loop {
                std::thread::sleep(std::time::Duration::from_secs(120));

                if let Ok(dev) = open_device(cli.device.as_deref(), false) {
                    if let Ok(st) = dev.get_status() {
                        let bat = st.battery_percentage;
                        let stage = st.active_stage;
                        let poll = st.polling_rate.map(|r| format!("{} Hz", r.hz())).unwrap_or_else(|| "N/A".into());
                        let rgb = st.lighting_mode.map(|m| format!("{:?}", m)).unwrap_or_else(|| "N/A".into());

                        if bat <= 20 {
                            let is_critical = bat <= 10;
                            let title = format!("🔋 LORGAR MSA10W Battery Low ({}%)", bat);
                            let body = format!("Mouse battery level is at {}%. Please connect charger.", bat);
                            send_desktop_notification(&title, &body, is_critical);
                        }

                        handle.update(move |t: &mut LorgarTray| {
                            t.battery_percentage = bat;
                            t.active_stage = stage;
                            t.polling_rate_str = poll;
                            t.rgb_mode_str = rgb;
                        });
                    }
                }
            }
        }
        Commands::Status => {
            let dev = open_device(cli.device.as_deref(), cli.debug)?;
            let info = dev.info();
            let status = dev.get_status().ok();

            println!("{}", "┌──────────────────────────────────────────────────────────┐".cyan());
            println!("│              {}             │", "LORGAR MSA10W CONTROL DASHBOARD".bold().yellow());
            println!("{}", "└──────────────────────────────────────────────────────────┘".cyan());

            println!("  Device Model: {}", info.product_string.as_deref().unwrap_or("LORGAR MSA10W").bold());
            println!("  VID:PID:      {:04x}:{:04x}", info.vendor_id, info.product_id);
            println!("  Path:         {}", info.path.cyan());
            println!("  Usage Page:   0x{:04x} (Config Interface)", info.usage_page);

            if let Some(st) = &status {
                let bar = render_battery_bar(st.battery_percentage);
                let color_bat = if st.battery_percentage > 50 {
                    format!("{} {}%", bar, st.battery_percentage).green().bold()
                } else if st.battery_percentage > 20 {
                    format!("{} {}%", bar, st.battery_percentage).yellow().bold()
                } else {
                    format!("{} {}%", bar, st.battery_percentage).red().bold()
                };
                println!("  Battery:      {}", color_bat);

                println!("\n{}", "Current Device Settings:".bold());
                println!("  • Active Stage: {}", format!("Stage {}", st.active_stage).cyan().bold());

                if let Some(rate) = st.polling_rate {
                    println!("  • Polling Rate: {}", format!("{} Hz ({} ms)", rate.hz(), rate.interval_ms()).cyan().bold());
                }

                if let Some(mode) = st.lighting_mode {
                    println!("  • RGB Mode:     {}", format!("{:?}", mode).magenta().bold());
                }
            } else {
                println!("  Battery:      {}", "N/A".dimmed());
            }

            println!("\n{}", "Available Controls & Short Commands:".bold());
            println!("  • Active DPI:  {}", "lorgarctl dpi <0..7>".cyan());
            println!("  • Set DPI:     {}", "lorgarctl dpi <0..7> <dpi>".dimmed());
            println!("  • Polling:     {}", "lorgarctl polling <125|250|500|1000>".cyan());
            println!("  • RGB Mode:    {}", "lorgarctl rgb <static|breathing|color-cycle|tapping|wave>".magenta());
            println!("  • RGB Color:   {}", "lorgarctl rgb <RRGGBB>".magenta());

            Ok(())
        }
        Commands::Devices => {
            println!("{}", "Scanning for LORGAR MSA10W devices...".bold());
            let devices = Msa10w::list_devices()?;

            if devices.is_empty() {
                println!("{}", "No LORGAR devices found.".yellow());
                println!("Make sure the mouse or receiver is connected (VID: 0x093a, PID: 0xc600 / 0xc601).");
                return Ok(());
            }

            println!("Found {} interface(s):", devices.len());
            println!("{}", "--------------------------------------------------".dimmed());

            for (idx, dev) in devices.iter().enumerate() {
                let tag = if dev.is_config_interface {
                    " [CONFIG INTERFACE]".green().bold().to_string()
                } else {
                    "".to_string()
                };

                println!("Interface #{}{}", idx + 1, tag);
                println!("  Product:      {}", dev.product_string.as_deref().unwrap_or("LORGAR MSA10W"));
                println!("  VID:PID:      {:04x}:{:04x}", dev.vendor_id, dev.product_id);
                println!("  Path:         {}", dev.path.cyan());
                println!("  Usage Page:   0x{:04x}", dev.usage_page);
                println!("  Usage:        0x{:04x}", dev.usage);
                println!("  Interface #:  {}", dev.interface_number);
                println!("{}", "--------------------------------------------------".dimmed());
            }

            Ok(())
        }

        Commands::Battery {
            short,
            json,
            notify,
            threshold,
        } => {
            let dev = open_device(cli.device.as_deref(), cli.debug)?;
            let battery = dev.get_battery()?;

            if notify && battery.percentage <= threshold {
                let is_critical = battery.percentage <= 10;
                let title = format!("🔋 LORGAR MSA10W Battery Low ({}%)", battery.percentage);
                let body = format!(
                    "Mouse battery level is at {}%. Please connect charger.",
                    battery.percentage
                );
                send_desktop_notification(&title, &body, is_critical);
            }

            if json {
                let class_str = if battery.percentage > 50 {
                    "good"
                } else if battery.percentage > 20 {
                    "warning"
                } else {
                    "critical"
                };
                println!(
                    "{{\"percentage\":{},\"icon\":\"🖱️\",\"text\":\"{}%\",\"tooltip\":\"LORGAR MSA10W: {}%\",\"class\":\"{}\"}}",
                    battery.percentage, battery.percentage, battery.percentage, class_str
                );
            } else if short {
                println!("🖱️ {}%", battery.percentage);
            } else {
                if cli.debug {
                    println!("\n{}", "Raw 64-byte response packet:".bold());
                    for chunk in battery.raw_response.chunks(16) {
                        println!("  {}", format_hex_bytes(chunk).yellow());
                    }
                    println!();
                }

                println!(
                    "🔋 Battery Level: {}",
                    format!("{}%", battery.percentage).cyan().bold()
                );
            }

            Ok(())
        }

        Commands::Dpi { sub, stage, dpi } => {
            let (target_stage, target_dpi) = if let Some(s) = sub {
                match s {
                    DpiCommands::Set { stage, dpi } => (Some(stage), Some(dpi)),
                    DpiCommands::Active { stage } => (Some(stage), None),
                }
            } else if let Some(st) = stage {
                (Some(st), dpi)
            } else {
                return Err(LorgarError::Transport(
                    "Please specify DPI stage index (e.g. 'lorgarctl dpi 1' or 'lorgarctl dpi set 1 1400')".into(),
                ));
            };

            let dev = open_device(cli.device.as_deref(), cli.debug)?;
            let info = dev.info();

            if let (Some(st), Some(val)) = (target_stage, target_dpi) {
                let payload = dev.set_dpi(st, val)?;
                if cli.debug {
                    let mut report = [0u8; 9];
                    report[0] = 0x00;
                    report[1..9].copy_from_slice(&payload);
                    println!("\n{}", "Feature payload (8 bytes):".bold());
                    println!("  {}", format_hex_bytes(&payload).yellow());
                    println!("\n{}", "Feature report (9 bytes sent to transport):".bold());
                    println!("  {}", format_hex_bytes(&report).yellow());
                    println!("\n{}", "Result:".bold());
                    println!("  {}", "OK".green().bold());
                } else {
                    println!(
                        "{} Set DPI stage {} to {} DPI on {}",
                        "✓".green().bold(),
                        st.to_string().cyan(),
                        val.to_string().cyan(),
                        info.path.dimmed()
                    );
                }
            } else if let Some(st) = target_stage {
                let payload = dev.set_active_dpi_stage(st)?;
                if cli.debug {
                    let mut report = [0u8; 9];
                    report[0] = 0x00;
                    report[1..9].copy_from_slice(&payload);
                    println!("\n{}", "Feature payload (8 bytes):".bold());
                    println!("  {}", format_hex_bytes(&payload).yellow());
                    println!("\n{}", "Feature report (9 bytes sent to transport):".bold());
                    println!("  {}", format_hex_bytes(&report).yellow());
                    println!("\n{}", "Result:".bold());
                    println!("  {}", "OK".green().bold());
                } else {
                    println!(
                        "{} Activated DPI stage {} on {}",
                        "✓".green().bold(),
                        st.to_string().cyan(),
                        info.path.dimmed()
                    );
                }
            }

            Ok(())
        }

        Commands::Polling { sub, hz } => {
            let target_hz = if let Some(s) = sub {
                match s {
                    PollingSubcommands::Set { hz } => hz,
                }
            } else if let Some(rate) = hz {
                rate
            } else {
                return Err(LorgarError::Transport(
                    "Please specify polling rate in Hz (e.g. 'lorgarctl polling 1000' or 'lorgarctl polling set 1000')".into(),
                ));
            };

            let dev = open_device(cli.device.as_deref(), cli.debug)?;
            let info = dev.info();

            let payload = dev.set_polling_rate(target_hz)?;

            if cli.debug {
                let mut report = [0u8; 9];
                report[0] = 0x00;
                report[1..9].copy_from_slice(&payload);

                println!("Polling rate: {} Hz", target_hz.hz());
                println!("Interval: {} ms", target_hz.interval_ms());

                println!("\n{}", "Feature payload:".bold());
                println!("{}", format_hex_bytes(&payload).yellow());

                println!("\n{}", "Feature report:".bold());
                println!("{}", format_hex_bytes(&report).yellow());

                println!("\n{}", "Result:".bold());
                println!("  {}", "OK".green().bold());
            } else {
                println!(
                    "{} Set polling rate to {} Hz ({} ms) on {}",
                    "✓".green().bold(),
                    target_hz.hz().to_string().cyan(),
                    target_hz.interval_ms().to_string().cyan(),
                    info.path.dimmed()
                );
            }

            Ok(())
        }

        Commands::Rgb { sub, target } => {
            if let Some(s) = sub {
                match s {
                    RgbCommands::Mode { mode } => {
                        let dev = open_device(cli.device.as_deref(), cli.debug)?;
                        let info = dev.info();

                        let payload = dev.set_lighting_mode(mode)?;

                        if cli.debug {
                            let mut report = [0u8; 9];
                            report[0] = 0x00;
                            report[1..9].copy_from_slice(&payload);

                            println!("\n{}", "Feature payload (8 bytes):".bold());
                            println!("  {}", format_hex_bytes(&payload).yellow());

                            println!("\n{}", "Feature report (9 bytes sent to transport):".bold());
                            println!("  {}", format_hex_bytes(&report).yellow());

                            println!("\n{}", "Result:".bold());
                            println!("  {}", "OK".green().bold());
                        } else {
                            println!(
                                "{} Set RGB mode to {:?} on {}",
                                "✓".green().bold(),
                                mode,
                                info.path.dimmed()
                            );
                        }

                        Ok(())
                    }

                    RgbCommands::Color { hex } => {
                        let (r, g, b) = lorgar_core::parse_hex_color(&hex)?;
                        let dev = open_device(cli.device.as_deref(), cli.debug)?;
                        let info = dev.info();

                        let payload = dev.set_lighting_color(r, g, b)?;

                        if cli.debug {
                            let mut report = [0u8; 9];
                            report[0] = 0x00;
                            report[1..9].copy_from_slice(&payload);

                            println!("\n{}", "Feature payload (8 bytes):".bold());
                            println!("  {}", format_hex_bytes(&payload).yellow());

                            println!("\n{}", "Feature report (9 bytes sent to transport):".bold());
                            println!("  {}", format_hex_bytes(&report).yellow());

                            println!("\n{}", "Result:".bold());
                            println!("  {}", "OK".green().bold());
                        } else {
                            println!(
                                "{} Set RGB color to #{:02x}{:02x}{:02x} (R: {}, G: {}, B: {}) on {}",
                                "✓".green().bold(),
                                r, g, b, r, g, b,
                                info.path.dimmed()
                            );
                        }

                        Ok(())
                    }
                }
            } else if let Some(arg) = target {
                if let Ok(mode) = parse_lighting_mode(&arg) {
                    let dev = open_device(cli.device.as_deref(), cli.debug)?;
                    let info = dev.info();

                    let payload = dev.set_lighting_mode(mode)?;

                    if cli.debug {
                        let mut report = [0u8; 9];
                        report[0] = 0x00;
                        report[1..9].copy_from_slice(&payload);

                        println!("\n{}", "Feature payload (8 bytes):".bold());
                        println!("  {}", format_hex_bytes(&payload).yellow());

                        println!("\n{}", "Feature report (9 bytes sent to transport):".bold());
                        println!("  {}", format_hex_bytes(&report).yellow());

                        println!("\n{}", "Result:".bold());
                        println!("  {}", "OK".green().bold());
                    } else {
                        println!(
                            "{} Set RGB mode to {:?} on {}",
                            "✓".green().bold(),
                            mode,
                            info.path.dimmed()
                        );
                    }

                    Ok(())
                } else if let Ok((r, g, b)) = lorgar_core::parse_hex_color(&arg) {
                    let dev = open_device(cli.device.as_deref(), cli.debug)?;
                    let info = dev.info();

                    let payload = dev.set_lighting_color(r, g, b)?;

                    if cli.debug {
                        let mut report = [0u8; 9];
                        report[0] = 0x00;
                        report[1..9].copy_from_slice(&payload);

                        println!("\n{}", "Feature payload (8 bytes):".bold());
                        println!("  {}", format_hex_bytes(&payload).yellow());

                        println!("\n{}", "Feature report (9 bytes sent to transport):".bold());
                        println!("  {}", format_hex_bytes(&report).yellow());

                        println!("\n{}", "Result:".bold());
                        println!("  {}", "OK".green().bold());
                    } else {
                        println!(
                            "{} Set RGB color to #{:02x}{:02x}{:02x} (R: {}, G: {}, B: {}) on {}",
                            "✓".green().bold(),
                            r, g, b, r, g, b,
                            info.path.dimmed()
                        );
                    }

                    Ok(())
                } else {
                    Err(LorgarError::Transport(format!(
                        "Invalid RGB argument '{}'. Expected mode (static, breathing, color-cycle, tapping, wave) or hex color (#RRGGBB)",
                        arg
                    )))
                }
            } else {
                Err(LorgarError::Transport(
                    "Please specify RGB mode or color (e.g. 'lorgarctl rgb static' or 'lorgarctl rgb ff0000')".into(),
                ))
            }
        }

        Commands::Debug { sub } => match sub {
            DebugCommands::SendFeature { bytes } => {
                let payload = parse_hex_payload(&bytes)?;
                let dev = open_device(cli.device.as_deref(), cli.debug)?;
                let info = dev.info();

                println!(
                    "{} Sending raw feature payload to {}",
                    "⚠️  WARNING:".yellow().bold(),
                    info.path.cyan()
                );

                let mut report = [0u8; 9];
                report[0] = 0x00;
                report[1..9].copy_from_slice(&payload);

                println!("Payload: {}", format_hex_bytes(&payload).yellow());
                println!("Report:  {}", format_hex_bytes(&report).yellow());

                dev.send_raw_feature(&payload)?;
                println!("Result:  {}", "OK".green().bold());

                Ok(())
            }

            DebugCommands::Query { bytes } => {
                let payload = if bytes.is_empty() {
                    [0x82, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
                } else {
                    parse_hex_payload(&bytes)?
                };

                let dev = open_device(cli.device.as_deref(), cli.debug)?;
                let info = dev.info();

                println!(
                    "Sending feature query to {} ({})",
                    info.path.cyan(),
                    format_hex_bytes(&payload).yellow()
                );

                let response = dev.query_feature(&payload, 1000)?;

                println!("Received {} byte(s) response:", response.len());
                for chunk in response.chunks(16) {
                    println!("  {}", format_hex_bytes(chunk).cyan());
                }

                Ok(())
            }
        },
    }
}
