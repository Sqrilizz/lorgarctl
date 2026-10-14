use crate::error::{LorgarError, Result};
use crate::protocol::{
    build_set_active_stage_payload, build_set_dpi_payload, build_set_lighting_color_payload,
    build_set_lighting_mode_payload, build_set_polling_rate_payload, build_set_stage_color_payload,
    LightingMode, PollingRate, CONFIG_USAGE_PAGE, PRODUCT_IDS, VENDOR_ID,
};
use crate::transport::{DeviceLock, HidTransport, HidapiTransport};
use hidapi::HidApi;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    _lock: Option<DeviceLock>,
}

impl Msa10w {
    pub fn list_devices() -> Result<Vec<LorgarDeviceInfo>> {
        let api = HidApi::new()?;
        let mut devices = Vec::new();

        for dev in api.device_list() {
            if dev.vendor_id() == VENDOR_ID && PRODUCT_IDS.contains(&dev.product_id()) {
                let usage_page = dev.usage_page();
                let interface_number = dev.interface_number();
                let is_config = is_config_interface(usage_page, interface_number);

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
                    && is_config_interface(d.usage_page(), d.interface_number())
            })
            .collect();

        if config_candidates.is_empty() {
            return Err(LorgarError::DeviceNotFound {
                vid: VENDOR_ID,
                pid: PRODUCT_IDS[1],
            });
        }

        let mut paths: Vec<_> = config_candidates.iter().map(|d| d.path()).collect();
        paths.sort();
        paths.dedup();
        if paths.len() > 1 {
            return Err(LorgarError::Config(
                "multiple configuration devices found; select one with --device".into(),
            ));
        }
        let path = paths[0]
            .to_str()
            .map_err(|_| LorgarError::Config("non-UTF8 HID path".into()))?;
        Self::open_path(path)
    }

    pub fn open_path(path: &str) -> Result<Self> {
        let lock = DeviceLock::acquire()?;
        let api = HidApi::new()?;
        let c_path = std::ffi::CString::new(path).map_err(|_| LorgarError::DeviceOpenFailed {
            path: path.to_string(),
        })?;
        let info = api
            .device_list()
            .find(|d| {
                d.path().to_string_lossy() == path
                    && d.vendor_id() == VENDOR_ID
                    && PRODUCT_IDS.contains(&d.product_id())
                    && is_config_interface(d.usage_page(), d.interface_number())
            })
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
                is_config_interface: is_config_interface(dev.usage_page(), dev.interface_number()),
            })
            .ok_or(LorgarError::ConfigInterfaceNotFound {
                usage_page: CONFIG_USAGE_PAGE,
            })?;
        check_access(path)?;
        let device = api.open_path(&c_path)?;
        let transport = Box::new(HidapiTransport::new(device));
        Ok(Self {
            info,
            transport,
            _lock: Some(lock),
        })
    }

    pub fn from_transport(info: LorgarDeviceInfo, transport: Box<dyn HidTransport + Send>) -> Self {
        Self {
            info,
            transport,
            _lock: None,
        }
    }

    pub fn read_input(&self, timeout_ms: i32) -> Result<Option<Vec<u8>>> {
        self.transport.read_input(timeout_ms)
    }

    pub fn execute(&self, command: &crate::protocol::Command) -> Result<()> {
        self.transport.send_feature_command(&command.packet()?.raw)
    }
}

pub fn is_config_interface(page: u16, interface: i32) -> bool {
    page == CONFIG_USAGE_PAGE || (page == 0 && interface == 2)
}

pub fn check_access(path: &str) -> Result<()> {
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(LorgarError::PermissionDenied { path: path.into() })
        }
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Clone)]
pub struct BatteryInfo {
    pub percentage: u8,
    pub is_charging: Option<bool>,
    pub raw_response: [u8; 64],
}

#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub battery_percentage: u8,
    pub is_charging: Option<bool>,
    pub active_stage: Option<u8>,
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

    pub fn set_sleep_timeout(&self, minutes: u8, active_stage: u8) -> Result<()> {
        let packet = crate::protocol::build_set_sleep_timeout_packet(minutes, active_stage)?;
        let mut report = [0u8; 33];
        report[1..].copy_from_slice(&packet);
        let api = HidApi::new()?;
        let c_path = std::ffi::CString::new(self.info.path.as_str()).map_err(|_| {
            LorgarError::DeviceOpenFailed {
                path: self.info.path.clone(),
            }
        })?;
        let dev = api.open_path(&c_path)?;
        dev.send_feature_report(&report)?;
        std::thread::sleep(std::time::Duration::from_millis(20));
        Ok(())
    }

    pub fn set_stage_color(&self, stage: u8, r: u8, g: u8, b: u8) -> Result<[u8; 8]> {
        let payload = build_set_stage_color_payload(stage, r, g, b)?;
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
        let payload = crate::protocol::Command::QueryState.packet()?.raw;
        self.query_feature(&payload, 1000)
    }

    pub fn get_battery(&self) -> Result<BatteryInfo> {
        let raw = self.query_device_state()?;
        if raw.len() < 8 {
            return Err(LorgarError::Transport(
                "Short device state query response from hardware".into(),
            ));
        }

        let percentage = crate::protocol::parse_battery_percentage(&raw)?;
        let is_charging = None;
        let mut raw_response = [0u8; 64];
        let len = raw.len().min(64);
        raw_response[..len].copy_from_slice(&raw[..len]);

        Ok(BatteryInfo {
            percentage,
            is_charging,
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

        let battery_percentage = crate::protocol::parse_battery_percentage(&raw)?;
        let active_stage = crate::protocol::parse_active_dpi_stage(&raw).ok();
        let lighting_mode = crate::protocol::parse_lighting_mode_readback(&raw)
            .ok()
            .flatten();
        let (is_charging, polling_rate) = (None, None);

        let mut raw_response = [0u8; 64];
        let len = raw.len().min(64);
        raw_response[..len].copy_from_slice(&raw[..len]);

        Ok(DeviceStatus {
            battery_percentage,
            is_charging,
            active_stage,
            polling_rate,
            lighting_mode,
            raw_response,
        })
    }
}
