use clap::{Parser, Subcommand};
use colored::*;
use lorgar_core::{LightingMode, LorgarError, Msa10w, PollingRate, Result};
mod stack;

#[derive(Parser, Debug)]
#[command(
    name = "lorgarctl",
    author,
    version,
    about = "Linux CLI controller & diagnostic tool for LORGAR MSA10W gaming mouse",
    long_about = None
)]
struct Cli {
    #[arg(
        global = true,
        long,
        short = 'j',
        help = "Emit JSON; monitor emits one object per event"
    )]
    json: bool,

    #[arg(
        global = true,
        long,
        conflicts_with = "device",
        conflicts_with = "direct",
        help = "Use lorgard instead of opening HID directly"
    )]
    daemon: bool,

    #[arg(
        global = true,
        long,
        conflicts_with = "daemon",
        help = "Force direct HID access, bypassing running lorgard daemon"
    )]
    direct: bool,

    #[arg(
        global = true,
        long,
        requires = "daemon",
        help = "Override the private lorgard socket path"
    )]
    socket: Option<std::path::PathBuf>,

    #[arg(
        global = true,
        long,
        help = "Override the XDG profile configuration path"
    )]
    config: Option<std::path::PathBuf>,
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
    #[command(
        about = "Diagnose permissions, interfaces, configuration and daemon availability without sending hardware commands"
    )]
    Doctor,
    #[command(about = "Inspect capture metadata, descriptors and every raw packet")]
    InspectCapture { path: std::path::PathBuf },
    #[command(
        about = "Monitor HID presence, battery, vendor events and raw unknown reports; reconnect automatically"
    )]
    Monitor,
    #[command(
        about = "Save timestamped raw HID packets and descriptors; passive unless --query-state is given"
    )]
    Capture {
        output: std::path::PathBuf,
        #[arg(long, default_value_t = 0)]
        duration: u64,
        #[arg(long)]
        query_state: bool,
        #[arg(long, default_value_t = 30000, requires = "query_state", value_parser = clap::value_parser!(u64).range(1000..=600000))]
        query_interval_ms: u64,
        #[arg(long, default_value = "")]
        label: String,
    },
    #[command(
        about = "Compare lorgar captures; offset changes, repetitions and explicitly unconfirmed hypotheses"
    )]
    DiffCaptures {
        before: std::path::PathBuf,
        after: std::path::PathBuf,
    },
    #[command(about = "Create, edit, inspect and apply persistent XDG profiles")]
    Profile {
        #[command(subcommand)]
        sub: stack::ProfileCommand,
    },
    #[command(about = "Generate shell completions to stdout")]
    Completions { shell: clap_complete::Shell },
    /// Interactive terminal menu for quick mouse configuration
    #[command(alias = "tui", alias = "interactive")]
    Menu,

    /// Show mouse dashboard summary (battery, device info, connection)
    #[command(alias = "info", alias = "hub")]
    Status,

    /// Run native System Tray icon daemon for KDE Plasma / GNOME / XFCE
    #[command(alias = "daemon")]
    Tray,

    /// Run UPower system battery daemon (integrates mouse battery into KDE Plasma 'Battery & Power' menu)
    #[command(alias = "upower-daemon")]
    Upower,

    /// Enumerate and display diagnostic info for connected LORGAR devices
    Devices,

    /// Display battery status
    Battery {
        /// Print short string for KDE panel / status bars (e.g. "83%" or "🖱️ 83%")
        #[arg(short, long)]
        short: bool,

        /// Send desktop notification if battery level is below threshold
        #[arg(short, long)]
        notify: bool,

        /// Low battery notification threshold percentage (default: 20)
        #[arg(short, long, default_value_t = 20)]
        threshold: u8,

        /// Send a test notification to verify desktop popups
        #[arg(long)]
        test_notify: bool,
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

    /// Manage mouse sleep / hibernation timeout (1..10 minutes)
    Sleep {
        /// Timeout in minutes (1..10, default: 5)
        minutes: Option<u8>,
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
        "off" | "none" | "disabled" => Ok(LightingMode::Off),
        "static" => Ok(LightingMode::Static),
        "breathing" => Ok(LightingMode::Breathing),
        "color-cycle" | "colorcycle" => Ok(LightingMode::ColorCycle),
        "tapping" => Ok(LightingMode::Tapping),
        "wave" => Ok(LightingMode::Wave),
        _ => Err(format!(
            "Invalid lighting mode '{}'. Valid options: off, static, breathing, color-cycle, tapping, wave",
            s
        )),
    }
}

#[derive(Subcommand, Debug)]
enum DpiCommands {
    /// Show all DPI stages and active stage
    List,

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

    /// Set top DPI indicator LED color for a stage (stage: 0..7, hex: #RRGGBB or RRGGBB)
    Color {
        /// DPI stage index (0..7)
        stage: u8,
        /// Hex color string (e.g. #ff0000, 32cd32)
        hex: String,
    },

    /// Apply standard DPI preset (fps, balanced, high)
    Preset {
        /// Preset name: fps (400/800/1600/3200) | balanced (800/1200/1600/2400) | high (1600/2400/3200/6400)
        name: String,
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

    if hex_chars.len() & 1 != 0 || !hex_chars.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(LorgarError::HexParseError(
            "Odd number of hex characters".into(),
        ));
    }

    let mut bytes = Vec::new();
    for i in (0..hex_chars.len()).step_by(2) {
        let byte_str = &hex_chars[i..i + 2];
        let byte = u8::from_str_radix(byte_str, 16).map_err(|e| {
            LorgarError::HexParseError(format!("Invalid hex byte '{}': {}", byte_str, e))
        })?;
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
            println!(
                "{}",
                format!("Opening device explicitly at path: {}", path).dimmed()
            );
        }
        Msa10w::open_path(path)
    } else {
        if debug {
            println!(
                "{}",
                "Auto-discovering LORGAR MSA10W configuration interface...".dimmed()
            );
        }
        Msa10w::open()
    }
}

fn send_desktop_notification(title: &str, body: &str, critical: bool) {
    lorgar_core::notify::send_desktop_notification(
        title,
        body,
        if critical {
            lorgar_core::notify::Urgency::Critical
        } else {
            lorgar_core::notify::Urgency::Normal
        },
        Some(if critical {
            "battery-caution"
        } else {
            "battery-low"
        }),
    );
}

#[cfg(feature = "tray")]
struct LorgarTray {
    battery_percentage: Option<u8>,
    active_stage: Option<u8>,
    polling_rate_str: String,
    rgb_mode_str: String,
    stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(feature = "tray")]
impl ksni::Tray for LorgarTray {
    fn icon_name(&self) -> String {
        "input-mouse".to_string()
    }

    fn title(&self) -> String {
        match self.battery_percentage {
            Some(percentage) => format!("LORGAR MSA10W ({percentage}%)"),
            None => "LORGAR MSA10W (unknown)".into(),
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.title(),
            description: format!(
                "Stage: {:?} | Polling: {} | RGB: {}",
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
                label: match self.battery_percentage {
                    Some(percentage) => format!("Battery: {percentage}%"),
                    None => "Battery: unknown".into(),
                },
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("Active stage: {:?}", self.active_stage),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("Polling rate: {}", self.polling_rate_str),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("RGB mode: {}", self.rgb_mode_str),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|tray: &mut LorgarTray| {
                    tray.stopped
                        .store(true, std::sync::atomic::Ordering::Relaxed)
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let as_json = cli.json;

    if cli.debug {
        std::env::set_var("RUST_LOG", "debug");
        let _ = env_logger::builder().is_test(false).try_init();
    }

    match run_app(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            if matches!(&err, LorgarError::Io(e) if e.kind() == std::io::ErrorKind::BrokenPipe)
                || matches!(&err, LorgarError::Json(e) if e.io_error_kind() == Some(std::io::ErrorKind::BrokenPipe))
            {
                return std::process::ExitCode::SUCCESS;
            }
            if as_json {
                let _ = serde_json::to_writer(
                    std::io::stdout(),
                    &serde_json::json!({"ok":false,"error":err.to_string(),"exit_code":lorgar_core::ipc::exit_code(&err)}),
                );
                use std::io::Write;
                let _ = writeln!(std::io::stdout());
            } else {
                eprintln!("{} {}", "Error:".red().bold(), err);
            }
            std::process::ExitCode::from(lorgar_core::ipc::exit_code(&err))
        }
    }
}

fn run_app(cli: Cli) -> Result<()> {
    if let Some(result) = stack::run(&cli) {
        return result;
    }
    match cli.command {
        Commands::Status
        | Commands::Menu
        | Commands::Sleep { .. }
        | Commands::Devices
        | Commands::Battery { .. }
        | Commands::Dpi { .. }
        | Commands::Polling { .. }
        | Commands::Rgb { .. }
        | Commands::Doctor
        | Commands::Monitor
        | Commands::Capture { .. }
        | Commands::DiffCaptures { .. }
        | Commands::Profile { .. }
        | Commands::Completions { .. }
        | Commands::InspectCapture { .. } => Err(LorgarError::Config("unhandled command".into())),
        Commands::Upower => {
            println!(
                "{}",
                "Starting LORGAR MSA10W UPower Integration Daemon..."
                    .bold()
                    .green()
            );
            unsafe extern "C" {
                fn lorgar_uhid_open() -> std::ffi::c_int;
                fn lorgar_uhid_update_battery(
                    fd: std::ffi::c_int,
                    percentage: u8,
                ) -> std::ffi::c_int;
            }

            let fd = unsafe { lorgar_uhid_open() };
            if fd < 0 {
                return Err(LorgarError::Transport(
                    "Failed to open /dev/uhid. The legacy bridge requires the optional UHID udev rule.".into(),
                ));
            }

            use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
            let fd = unsafe { OwnedFd::from_raw_fd(fd) };
            let stopped = lorgar_core::monitor::shutdown_flag()?;
            println!("Virtual HID battery created; desktop integration depends on UPower support.");

            while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(dev) = open_device(cli.device.as_deref(), false) {
                    if let Ok(battery) = dev.get_battery() {
                        if unsafe {
                            lorgar_uhid_update_battery(fd.as_raw_fd(), battery.percentage.min(100))
                        } < 0
                        {
                            return Err(LorgarError::Transport(
                                "UHID battery update failed".into(),
                            ));
                        }
                    }
                }
                wait_or_stop(&stopped, 30);
            }
            Ok(())
        }

        Commands::Tray => {
            #[cfg(not(feature = "tray"))]
            {
                eprintln!("lorgarctl: tray support was not enabled at compile time");
                Err(lorgar_core::LorgarError::Config(
                    "tray feature disabled".into(),
                ))
            }
            #[cfg(feature = "tray")]
            {
                println!(
                    "{}",
                    "Starting LORGAR MSA10W System Tray Daemon..."
                        .bold()
                        .green()
                );
                println!("Icon registered in System Tray. Low battery notifications active.");

                let stopped = lorgar_core::monitor::shutdown_flag()?;
                let initial_status = open_device(cli.device.as_deref(), cli.debug)
                    .and_then(|dev| dev.get_status())
                    .ok();

                let initial_bat = initial_status.as_ref().map(|s| s.battery_percentage);
                let initial_stage = initial_status.as_ref().and_then(|s| s.active_stage);
                let initial_poll = initial_status
                    .as_ref()
                    .and_then(|s| s.polling_rate)
                    .map(|r| format!("{} Hz", r.hz()))
                    .unwrap_or_else(|| "N/A".into());
                let initial_rgb = initial_status
                    .as_ref()
                    .and_then(|s| s.lighting_mode)
                    .map(|m| format!("{:?}", m))
                    .unwrap_or_else(|| "N/A".into());

                let tray = LorgarTray {
                    battery_percentage: initial_bat,
                    active_stage: initial_stage,
                    polling_rate_str: initial_poll,
                    rgb_mode_str: initial_rgb,
                    stopped: stopped.clone(),
                };

                let service = ksni::TrayService::new(tray);
                let handle = service.handle();
                service.spawn();
                let mut low_notified = false;

                while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                    wait_or_stop(&stopped, 120);
                    if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }

                    match open_device(cli.device.as_deref(), false).and_then(|dev| dev.get_status())
                    {
                        Ok(st) => {
                            let bat = st.battery_percentage;
                            let stage = st.active_stage;
                            let poll = st
                                .polling_rate
                                .map(|r| format!("{} Hz", r.hz()))
                                .unwrap_or_else(|| "N/A".into());
                            let rgb = st
                                .lighting_mode
                                .map(|m| format!("{:?}", m))
                                .unwrap_or_else(|| "N/A".into());

                            if bat <= 20 && !low_notified {
                                let is_critical = bat <= 10;
                                let title = format!("LORGAR MSA10W battery low ({bat}%)");
                                let body = format!(
                                    "Mouse battery level is at {}%. Please connect charger.",
                                    bat
                                );
                                send_desktop_notification(&title, &body, is_critical);
                                low_notified = true;
                            } else if bat > 20 {
                                low_notified = false;
                            }

                            handle.update(move |t: &mut LorgarTray| {
                                t.battery_percentage = Some(bat);
                                t.active_stage = stage;
                                t.polling_rate_str = poll;
                                t.rgb_mode_str = rgb;
                            });
                        }
                        Err(_) => {
                            handle.update(|t: &mut LorgarTray| {
                                t.battery_percentage = None;
                                t.active_stage = None;
                            });
                        }
                    }
                }
                handle.shutdown();
                Ok(())
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

fn wait_or_stop(stopped: &std::sync::atomic::AtomicBool, seconds: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < deadline
        && !stopped.load(std::sync::atomic::Ordering::Relaxed)
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "tray")]
    use super::*;

    #[cfg(feature = "tray")]
    #[test]
    fn tray_never_fabricates_zero_battery() {
        let mut tray = LorgarTray {
            battery_percentage: None,
            active_stage: None,
            polling_rate_str: "N/A".into(),
            rgb_mode_str: "N/A".into(),
            stopped: Default::default(),
        };
        assert_eq!(ksni::Tray::title(&tray), "LORGAR MSA10W (unknown)");
        tray.battery_percentage = Some(57);
        assert_eq!(ksni::Tray::title(&tray), "LORGAR MSA10W (57%)");
    }
}
