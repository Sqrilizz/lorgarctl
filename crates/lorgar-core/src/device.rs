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
        let mut config_candidates: Vec<&hidapi::DeviceInfo> = api
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

        let has_wired = config_candidates
            .iter()
            .any(|d| d.product_id() == PRODUCT_IDS[0]);
        if has_wired {
            config_candidates.retain(|d| d.product_id() == PRODUCT_IDS[0]);
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

    pub fn send_nvram_packet(&self, packet: &[u8; 32]) -> Result<()> {
        let mut report = [0u8; 33];
        report[1..].copy_from_slice(packet);
        self.transport.send_feature_report(&report)
    }

    pub fn set_sleep_timeout(&self, minutes: u8, active_stage: u8) -> Result<()> {
        let packet = crate::protocol::build_set_sleep_timeout_packet(minutes, active_stage)?;
        self.send_nvram_packet(&packet)
    }

    pub fn set_sensor_config(
        &self,
        active_stage: u8,
        lod_mm: u8,
        sleep_timeout_minutes: u8,
        angle_snapping: bool,
        motion_sync: bool,
        color: (u8, u8, u8),
    ) -> Result<()> {
        let packet = crate::protocol::build_nvram_packet_02(
            active_stage,
            lod_mm,
            sleep_timeout_minutes,
            angle_snapping,
            motion_sync,
            color,
        )?;
        self.send_nvram_packet(&packet)
    }

    pub fn set_lod(&self, lod_mm: u8) -> Result<()> {
        self.set_sensor_config(0, lod_mm, 5, false, true, (0, 0, 0))
    }

    pub fn set_angle_snapping(&self, enabled: bool) -> Result<()> {
        self.set_sensor_config(0, 1, 5, enabled, true, (0, 0, 0))
    }

    pub fn set_motion_sync(&self, enabled: bool) -> Result<()> {
        self.set_sensor_config(0, 1, 5, false, enabled, (0, 0, 0))
    }

    pub fn set_debounce(&self, debounce_ms: u16, rapid_fire: bool) -> Result<()> {
        let packet = crate::protocol::build_nvram_packet_04(debounce_ms, rapid_fire, 50, 3);
        self.send_nvram_packet(&packet)
    }

    pub fn enter_bootloader(&self) -> Result<()> {
        let packet = crate::firmware::build_enter_bootloader_packet();
        self.transport.send_feature_command(&packet)
    }

    pub fn read_flash(&self, addr: u16, len: u8) -> Result<Vec<u8>> {
        let packet = crate::firmware::build_read_flash_packet(addr, len);
        self.query_feature(&packet, 1000)
    }

    pub fn dump_flash(&self, start_addr: u16, total_len: usize) -> Result<Vec<u8>> {
        let mut dumped = Vec::with_capacity(total_len);
        let mut cur_addr = start_addr;
        let mut remaining = total_len;
        while remaining > 0 {
            let chunk_len = remaining.min(10) as u8;
            let resp = self.read_flash(cur_addr, chunk_len)?;
            let data = if resp.len() >= 8 {
                &resp[2..resp.len().min(2 + chunk_len as usize)]
            } else {
                &resp[..]
            };
            dumped.extend_from_slice(data);
            cur_addr = cur_addr.saturating_add(chunk_len as u16);
            remaining -= data.len().min(remaining);
            if data.is_empty() {
                break;
            }
        }
        Ok(dumped)
    }

    pub fn dump_onboard_memory(&self) -> Result<crate::firmware::OnboardMemoryDump> {
        let mut banks = Vec::with_capacity(crate::firmware::ONBOARD_MEMORY_BANKS.len());
        let mut total_bytes = 0;
        for &(op, name) in &crate::firmware::ONBOARD_MEMORY_BANKS {
            let mut payload = [0u8; 8];
            payload[0] = op;
            let resp = self.query_feature(&payload, 1000)?;
            total_bytes += resp.len();
            banks.push(crate::firmware::MemoryBank {
                opcode: op,
                name: name.to_string(),
                data: resp,
            });
        }
        Ok(crate::firmware::OnboardMemoryDump { banks, total_bytes })
    }

    pub fn get_hardware_dpi_stages(&self) -> Result<[u16; 8]> {
        let mut payload = [0u8; 8];
        payload[0] = 0xa6;
        let resp = self.query_feature(&payload, 1000)?;
        if resp.len() < 16 {
            return Err(LorgarError::Transport(
                "DPI table query returned fewer than 16 bytes".into(),
            ));
        }
        let mut stages = [0u16; 8];
        for (i, &chunk) in resp[..16].as_chunks::<2>().0.iter().enumerate() {
            stages[i] = u16::from_le_bytes(chunk);
        }
        Ok(stages)
    }

    pub fn get_hardware_stage_colors(&self) -> Result<[[u8; 3]; 8]> {
        let mut payload = [0u8; 8];
        payload[0] = 0xa3;
        let resp = self.query_feature(&payload, 1000)?;
        if resp.len() < 24 {
            return Err(LorgarError::Transport(
                "DPI stage RGB palette query returned fewer than 24 bytes".into(),
            ));
        }
        let mut colors = [[0u8; 3]; 8];
        for (i, &chunk) in resp[..24].as_chunks::<3>().0.iter().enumerate() {
            colors[i] = chunk;
        }
        Ok(colors)
    }

    pub fn get_hardware_button_mappings(
        &self,
        secondary: bool,
    ) -> Result<Vec<crate::firmware::ButtonMapping>> {
        let opcode = if secondary { 0xa5 } else { 0xa4 };
        let mut payload = [0u8; 8];
        payload[0] = opcode;
        let resp = self.query_feature(&payload, 1000)?;
        let dump = crate::firmware::OnboardMemoryDump {
            banks: vec![crate::firmware::MemoryBank {
                opcode,
                name: if secondary {
                    "button_mapping_secondary".into()
                } else {
                    "button_mapping_primary".into()
                },
                data: resp,
            }],
            total_bytes: 64,
        };
        dump.parse_button_mappings(secondary).ok_or_else(|| {
            LorgarError::Transport("Failed to parse hardware button mappings".into())
        })
    }

    pub fn set_dpi_stages(&self, stages: &[u16; 8]) -> Result<()> {
        for (stage, &dpi) in stages.iter().enumerate() {
            self.set_dpi(stage as u8, dpi)?;
        }
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
