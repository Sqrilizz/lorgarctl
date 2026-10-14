pub mod capture;
pub mod descriptor;
pub mod device;
pub mod error;
pub mod foreground;
pub mod ipc;
pub mod monitor;
pub mod notify;
pub mod profiles;
pub mod protocol;
pub mod storage;
pub mod transport;

pub use device::{BatteryInfo, DeviceStatus, LorgarDeviceInfo, Msa10w};
pub use error::{LorgarError, Result};
pub use notify::{send_desktop_notification, Urgency};
pub use protocol::{
    build_set_active_stage_payload, build_set_dpi_payload, build_set_lighting_color_payload,
    build_set_lighting_mode_payload, build_set_polling_rate_payload,
    build_set_sleep_timeout_packet, build_set_stage_color_payload, cmd, parse_active_dpi_stage,
    parse_battery_percentage, parse_hex_color, parse_lighting_mode_readback, LightingMode,
    PollingRate, CONFIG_USAGE_PAGE, DPI_STAGE_COUNT, MAX_DPI, MIN_DPI, PRODUCT_IDS, VENDOR_ID,
};
pub use transport::{with_query_retries, HidTransport};
