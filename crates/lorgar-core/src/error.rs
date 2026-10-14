use thiserror::Error;

#[derive(Error, Debug)]
pub enum LorgarError {
    #[error("{message}")]
    Remote { code: u8, message: String },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration or message error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid packet: {0}")]
    InvalidPacket(String),

    #[error("Permission denied for '{path}'; install udev/99-lorgar-msa10w.rules and reconnect the device")]
    PermissionDenied { path: String },

    #[error("Device is in use; use --daemon or stop the other direct HID client")]
    DeviceBusy,

    #[error("Device response timed out")]
    Timeout,

    #[error("Configuration error: {0}")]
    Config(String),
    #[error("No LORGAR MSA10W device found (VID: 0x{vid:04x}, PID: 0x{pid:04x})")]
    DeviceNotFound { vid: u16, pid: u16 },

    #[error("No LORGAR MSA10W device found matching configuration interface (Usage Page 0x{usage_page:04x})")]
    ConfigInterfaceNotFound { usage_page: u16 },

    #[error("Device at path '{path}' could not be opened")]
    DeviceOpenFailed { path: String },

    #[error("HID API error: {0}")]
    Hid(#[from] hidapi::HidError),

    #[error("Invalid DPI stage '{stage}'. Must be between 0 and {}", crate::protocol::DPI_STAGE_COUNT - 1)]
    InvalidStage { stage: u8 },

    #[error("Unsupported polling rate: {hz} Hz. Supported values: 125, 250, 500, 1000")]
    InvalidPollingRate { hz: u16 },

    #[error("Invalid DPI value '{dpi}'. Valid range is {min}..={max}")]
    InvalidDpi { dpi: u16, min: u16, max: u16 },

    #[error("Invalid raw feature report payload length ({len} bytes). Must be exactly 8 bytes.")]
    InvalidFeatureLength { len: usize },

    #[error("Invalid hex color '{color}'. Expected format RRGGBB or #RRGGBB.")]
    InvalidColorFormat { color: String },

    #[error("Failed to parse hex payload: {0}")]
    HexParseError(String),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Firmware error: {0}")]
    Firmware(String),

    #[error(
        "profile '{name}' stopped at command {index}; earlier writes may have applied: {source}"
    )]
    ProfileApply {
        name: String,
        index: usize,
        #[source]
        source: Box<LorgarError>,
    },
}

pub type Result<T> = std::result::Result<T, LorgarError>;
