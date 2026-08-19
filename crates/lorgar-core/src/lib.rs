pub mod device;
pub mod error;
pub mod protocol;
pub mod transport;

pub use device::{BatteryInfo, DeviceStatus, LorgarDeviceInfo, Msa10w};
pub use error::{LorgarError, Result};
pub use protocol::{
    build_set_active_stage_payload, build_set_dpi_payload, build_set_lighting_color_payload,
    build_set_lighting_mode_payload, build_set_polling_rate_payload, cmd, parse_battery_percentage,
    parse_hex_color, LightingMode, PollingRate, CONFIG_USAGE_PAGE, DPI_STAGE_COUNT, MAX_DPI,
    MIN_DPI, PRODUCT_IDS, VENDOR_ID,
};
pub use transport::HidTransport;
