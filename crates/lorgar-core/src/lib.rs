pub mod capture;
pub mod descriptor;
pub mod device;
pub mod error;
pub mod firmware;
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
pub use firmware::{
    build_enter_bootloader_packet, build_flash_write_chunk, build_read_flash_packet,
    create_firmware, verify_header, ButtonAction, ButtonMapping, FirmwareContainer,
    FirmwareMetadata, MemoryBank, OnboardMemoryDump, FIRMWARE_CHECKSUM_SEED, FIRMWARE_HEADER_SIZE,
    FLASH_WRITE_CHUNK_SIZE, ONBOARD_MEMORY_BANKS,
};
pub use notify::{send_desktop_notification, Urgency};
pub use protocol::{
    build_nvram_packet_01, build_nvram_packet_02, build_nvram_packet_03, build_nvram_packet_04,
    build_nvram_packet_05_06, build_nvram_packet_07, build_set_active_stage_payload,
    build_set_dpi_payload, build_set_lighting_color_payload, build_set_lighting_mode_payload,
    build_set_polling_rate_payload, build_set_sleep_timeout_packet, build_set_stage_color_payload,
    cmd, parse_active_dpi_stage, parse_battery_percentage, parse_hex_color,
    parse_lighting_mode_readback, LightingMode, PollingRate, CONFIG_USAGE_PAGE, DPI_STAGE_COUNT,
    MAX_DPI, MIN_DPI, PRODUCT_IDS, VENDOR_ID,
};
pub use transport::{with_query_retries, HidTransport};
