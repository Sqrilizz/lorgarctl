use hidapi::HidApi;
use crate::error::{LorgarError, Result};
use crate::protocol::{
    build_set_active_stage_payload, build_set_dpi_payload, build_set_lighting_color_payload,
    build_set_lighting_mode_payload, build_set_polling_rate_payload, LightingMode, PollingRate,
    CONFIG_USAGE_PAGE, PRODUCT_IDS, VENDOR_ID,
};
use crate::transport::{HidTransport, HidapiTransport};

#[derive(Debug, Clone)]
pub struct LorgarDeviceInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub path: String,
    pub serial_number: Option<String>,
    pub manufacturer_string: Option<String>,
    pub product_string: Option<String>,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
    pub is_config_interface: bool,
}

pub struct Msa10w {
    info: LorgarDeviceInfo,
    transport: Box<dyn HidTransport + Send>,
}

impl Msa10w {
    pub fn list_devices() -> Result<Vec<LorgarDeviceInfo>> {
        let api = HidApi::new()?;
        let mut devices = Vec::new();

        for dev in api.device_list() {
            if dev.vendor_id() == VENDOR_ID && PRODUCT_IDS.contains(&dev.product_id()) {
                let usage_page = dev.usage_page();
                let interface_number = dev.interface_number();
                let is_config = usage_page == CONFIG_USAGE_PAGE || interface_number == 2;

                devices.push(LorgarDeviceInfo {
                    vendor_id: dev.vendor_id(),
                    product_id: dev.product_id(),
                    path: dev.path().to_string_lossy().to_string(),
                    serial_number: dev.serial_number().map(String::from),
                    manufacturer_string: dev.manufacturer_string().map(String::from),
                    product_string: dev.product_string().map(String::from),
                    usage_page,
                    usage: dev.usage(),
                    interface_number,
                    is_config_interface: is_config,
                });
            }
        }

        Ok(devices)
    }

    pub fn open() -> Result<Self> {
        let api = HidApi::new()?;
        let config_candidates: Vec<&hidapi::DeviceInfo> = api
            .device_list()
            .filter(|d| {
                d.vendor_id() == VENDOR_ID
                    && PRODUCT_IDS.contains(&d.product_id())
                    && (d.usage_page() == CONFIG_USAGE_PAGE || d.interface_number() == 2)
            })
            .collect();

        if config_candidates.is_empty() {
            return Err(LorgarError::DeviceNotFound {
                vid: VENDOR_ID,
                pid: PRODUCT_IDS[1],
            });
        }

        for dev_info in &config_candidates {
            let path = dev_info.path().to_str().unwrap_or_default();
            if let Ok(dev) = Self::open_path(path) {
                if dev.query_device_state().is_ok() {
                    return Ok(dev);
                }
            }
        }

        let first_path = config_candidates[0].path().to_str().unwrap_or_default();
        Self::open_path(first_path)
    }

    pub fn open_path(path: &str) -> Result<Self> {
        let api = HidApi::new()?;
        let c_path = std::ffi::CString::new(path).map_err(|_| LorgarError::DeviceOpenFailed {
            path: path.to_string(),
        })?;
        let device = api.open_path(&c_path).map_err(|_| LorgarError::DeviceOpenFailed {
            path: path.to_string(),
        })?;

        let info = api
            .device_list()
            .find(|d| d.path().to_string_lossy() == path)
            .map(|dev| LorgarDeviceInfo {
                vendor_id: dev.vendor_id(),
                product_id: dev.product_id(),
                path: dev.path().to_string_lossy().to_string(),
                serial_number: dev.serial_number().map(String::from),
                manufacturer_string: dev.manufacturer_string().map(String::from),
                product_string: dev.product_string().map(String::from),
                usage_page: dev.usage_page(),
                usage: dev.usage(),
                interface_number: dev.interface_number(),
                is_config_interface: dev.usage_page() == CONFIG_USAGE_PAGE || dev.interface_number() == 2,
            })
            .unwrap_or_else(|| LorgarDeviceInfo {
                vendor_id: VENDOR_ID,
                product_id: PRODUCT_IDS[1],
                path: path.to_string(),
                serial_number: None,
                manufacturer_string: None,
                product_string: Some("LORGAR MSA10W".into()),
                usage_page: CONFIG_USAGE_PAGE,
                usage: 0,
                interface_number: 2,
                is_config_interface: true,
            });

        let transport = Box::new(HidapiTransport::new(device));
        Ok(Self { info, transport })
    }
}

#[derive(Debug, Clone)]
pub struct BatteryInfo {
    pub percentage: u8,
    pub raw_response: [u8; 64],
}

#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub battery_percentage: u8,
    pub active_stage: u8,
    pub polling_rate: Option<PollingRate>,
    pub lighting_mode: Option<LightingMode>,
    pub raw_response: [u8; 64],
}

impl Msa10w {
    pub fn info(&self) -> &LorgarDeviceInfo {
        &self.info
    }

    pub fn set_dpi(&self, stage: u8, dpi: u16) -> Result<[u8; 8]> {
        let payload = build_set_dpi_payload(stage, dpi)?;
        self.transport.send_feature_command(&payload)?;
        Ok(payload)
    }

    pub fn set_active_dpi_stage(&self, stage: u8) -> Result<[u8; 8]> {
        let payload = build_set_active_stage_payload(stage)?;
        self.transport.send_feature_command(&payload)?;
        Ok(payload)
    }

    pub fn set_polling_rate(&self, rate: PollingRate) -> Result<[u8; 8]> {
        let payload = build_set_polling_rate_payload(rate);
        self.transport.send_feature_command(&payload)?;
        Ok(payload)
    }

    pub fn set_lighting_mode(&self, mode: LightingMode) -> Result<[u8; 8]> {
        let payload = build_set_lighting_mode_payload(mode);
        self.transport.send_feature_command(&payload)?;
        Ok(payload)
    }

    pub fn set_lighting_color(&self, r: u8, g: u8, b: u8) -> Result<[u8; 8]> {
        let payload = build_set_lighting_color_payload(r, g, b);
        self.transport.send_feature_command(&payload)?;
        Ok(payload)
    }

    pub fn send_raw_feature(&self, payload: &[u8; 8]) -> Result<()> {
        self.transport.send_feature_command(payload)
    }

    pub fn query_feature(&self, payload: &[u8; 8], timeout_ms: i32) -> Result<Vec<u8>> {
        self.transport.query_feature_command(payload, timeout_ms)
    }

    pub fn query_device_state(&self) -> Result<Vec<u8>> {
        let payload = [0x82, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        self.query_feature(&payload, 1000)
    }

    pub fn get_battery(&self) -> Result<BatteryInfo> {
        let raw = self.query_device_state()?;
        if raw.len() < 8 {
            return Err(LorgarError::Transport(
                "Short device state query response from hardware".into(),
            ));
        }

        let percentage = raw[7];
        let mut raw_response = [0u8; 64];
        let len = raw.len().min(64);
        raw_response[..len].copy_from_slice(&raw[..len]);

        Ok(BatteryInfo {
            percentage,
            raw_response,
        })
    }

    pub fn get_status(&self) -> Result<DeviceStatus> {
        let raw = self.query_device_state()?;
        if raw.len() < 9 {
            return Err(LorgarError::Transport(
                "Short device state query response from hardware".into(),
            ));
        }

        let mode_byte = raw[2];
        let active_stage = raw[4];
        let battery_percentage = raw[7];
        let interval_byte = raw[8];

        let lighting_mode = match mode_byte {
            0x01 => Some(LightingMode::Static),
            0x02 => Some(LightingMode::Breathing),
            0x03 => Some(LightingMode::ColorCycle),
            0x04 => Some(LightingMode::Tapping),
            0x05 => Some(LightingMode::Wave),
            _ => None,
        };

        let polling_rate = match interval_byte {
            8 => Some(PollingRate::Hz125),
            4 => Some(PollingRate::Hz250),
            2 => Some(PollingRate::Hz500),
            1 => Some(PollingRate::Hz1000),
            _ => None,
        };

        let mut raw_response = [0u8; 64];
        let len = raw.len().min(64);
        raw_response[..len].copy_from_slice(&raw[..len]);

        Ok(DeviceStatus {
            battery_percentage,
            active_stage,
            polling_rate,
            lighting_mode,
            raw_response,
        })
    }
}
