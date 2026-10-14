use clap::{Parser, Subcommand};
use colored::*;
use lorgar_core::{LightingMode, LorgarError, Msa10w, PollingRate, Result};
mod hex;
mod stack;
mod tray;

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
pub enum Commands {
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

    /// Manage sensor Lift-Off Distance (1 mm or 2 mm)
    Lod {
        /// LOD in millimeters: 1 | 2
        #[arg(value_parser = clap::value_parser!(u8).range(1..=2))]
        mm: Option<u8>,
    },

    /// Manage button debounce delay in milliseconds (1..20 ms)
    Debounce {
        /// Debounce delay in milliseconds (1..20)
        #[arg(value_parser = clap::value_parser!(u16).range(1..=20))]
        ms: Option<u16>,

        /// Enable or disable rapid fire mode
        #[arg(long)]
        rapid_fire: bool,
    },

    /// Manage sensor angle snapping / prediction (on | off)
    AngleSnap {
        /// Enable or disable angle snapping: on / true / 1 / off / false / 0
        #[arg(value_parser = parse_bool)]
        enabled: Option<bool>,
    },

    /// Manage sensor motion sync (on | off)
    MotionSync {
        /// Enable or disable motion sync: on / true / 1 / off / false / 0
        #[arg(value_parser = parse_bool)]
        enabled: Option<bool>,
    },

    /// Firmware analysis, verification, dumping, container creation, and bootloader entry
    Firmware {
        #[command(subcommand)]
        sub: FirmwareCommands,
    },

    /// Developer and reverse-engineering utilities
    Debug {
        #[command(subcommand)]
        sub: DebugCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum FirmwareCommands {
    /// Inspect and verify firmware binary container, headers, and checksums
    Info {
        /// Path to firmware binary file
        path: std::path::PathBuf,
    },
    /// Create a signed/checksummed firmware container from raw code payload
    Create {
        /// Target product name (default: "LORGAR MSA10W")
        #[arg(long, default_value = "LORGAR MSA10W")]
        product: String,
        /// Target MCU IC model (default: "571R")
        #[arg(long, default_value = "571R")]
        ic: String,
        /// Target optical sensor (default: "PAW3395")
        #[arg(long, default_value = "PAW3395")]
        sensor: String,
        /// Flash base destination address in hex or decimal (default: 0)
        #[arg(long, default_value_t = 0)]
        flash_base: u32,
        /// Input raw binary firmware code payload
        #[arg(long)]
        input: std::path::PathBuf,
        /// Output container file
        #[arg(long)]
        output: std::path::PathBuf,
    },
    /// Switch mouse MCU into USB Bootloader / Update mode
    Bootloader,
    /// Read back raw flash memory chunks from device
    Dump {
        /// Flash memory starting address (default: 0)
        #[arg(long, default_value_t = 0)]
        addr: u16,
        /// Total number of bytes to dump (default: 256)
        #[arg(long, default_value_t = 256)]
        len: usize,
        /// Optional output file path to write dumped bytes
        #[arg(long)]
        output: Option<std::path::PathBuf>,
    },
}

pub fn parse_bool(s: &str) -> std::result::Result<bool, String> {
    match s.to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "enable" | "enabled" | "yes" => Ok(true),
        "off" | "false" | "0" | "disable" | "disabled" | "no" => Ok(false),
        _ => Err(format!("expected 'on' or 'off', got '{s}'")),
    }
}

#[derive(Subcommand, Debug)]
pub enum PollingSubcommands {
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
pub enum RgbCommands {
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

pub fn parse_lighting_mode(s: &str) -> std::result::Result<LightingMode, String> {
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
pub enum DpiCommands {
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
pub enum DebugCommands {
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
                "Opening first discovered LORGAR MSA10W device...".dimmed()
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

fn wait_or_stop(stopped: &std::sync::atomic::AtomicBool, seconds: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < deadline
        && !stopped.load(std::sync::atomic::Ordering::Relaxed)
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
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
        | Commands::Lod { .. }
        | Commands::Debounce { .. }
        | Commands::AngleSnap { .. }
        | Commands::MotionSync { .. }
        | Commands::Firmware { .. }
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

        Commands::Tray => crate::tray::run(
            cli.device.as_deref(),
            cli.debug,
            &open_device,
            &send_desktop_notification,
            &wait_or_stop,
        ),
        Commands::Debug { sub } => match sub {
            DebugCommands::SendFeature { bytes } => {
                let payload = crate::hex::parse_hex_payload(&bytes)?;
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
                dev.send_raw_feature(&payload)?;
                println!("{}", "Payload sent successfully.".green());
                Ok(())
            }
            DebugCommands::Query { bytes } => {
                let payload = if bytes.is_empty() {
                    [0x82, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
                } else {
                    crate::hex::parse_hex_payload(&bytes)?
                };
                let dev = open_device(cli.device.as_deref(), cli.debug)?;
                println!("Querying hardware with feature payload: {:02x?}", payload);
                let response = dev.query_feature(&payload, 1000)?;
                println!(
                    "Response from hardware ({} bytes): {:02x?}",
                    response.len(),
                    response
                );
                Ok(())
            }
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_helpers_reject_invalid_without_panicking() {
        assert!(crate::hex::parse_hex_payload(&["€€".to_string()]).is_err());
    }
}
