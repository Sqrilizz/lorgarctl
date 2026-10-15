use crate::error::{LorgarError, Result};

pub const VENDOR_ID: u16 = 0x093a;
pub const PRODUCT_IDS: &[u16] = &[0xc600, 0xc601];
pub const CONFIG_USAGE_PAGE: u16 = 0xff10;
pub const DPI_STAGE_COUNT: u8 = 8;
pub const MIN_DPI: u16 = 100;
pub const MAX_DPI: u16 = 26000;
pub const DPI_STEP: u16 = 50;

pub mod cmd {
    pub const SET_POLLING_RATE: u8 = 0x20;
    pub const SET_ACTIVE_STAGE: u8 = 0x21;
    pub const SET_LIGHTING_MODE: u8 = 0x22;
    pub const SET_LIGHTING_COLOR: u8 = 0x23;
    pub const SET_DPI: u8 = 0x26;
    pub const SET_STAGE_COLOR: u8 = 0x27;
    pub const QUERY_STATE: u8 = 0x82;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PollingRate {
    Hz125,
    Hz250,
    Hz500,
    Hz1000,
}

impl PollingRate {
    pub fn hz(self) -> u16 {
        match self {
            PollingRate::Hz125 => 125,
            PollingRate::Hz250 => 250,
            PollingRate::Hz500 => 500,
            PollingRate::Hz1000 => 1000,
        }
    }

    pub fn interval_ms(self) -> u8 {
        match self {
            PollingRate::Hz125 => 8,
            PollingRate::Hz250 => 4,
            PollingRate::Hz500 => 2,
            PollingRate::Hz1000 => 1,
        }
    }

    pub fn from_hz(hz: u16) -> Result<Self> {
        match hz {
            125 => Ok(PollingRate::Hz125),
            250 => Ok(PollingRate::Hz250),
            500 => Ok(PollingRate::Hz500),
            1000 => Ok(PollingRate::Hz1000),
            _ => Err(LorgarError::InvalidPollingRate { hz }),
        }
    }
}

pub fn build_set_polling_rate_payload(rate: PollingRate) -> [u8; 8] {
    [
        cmd::SET_POLLING_RATE,
        0x00,
        0x00,
        rate.interval_ms(),
        0x00,
        0x00,
        0x00,
        0x00,
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LightingMode {
    Off,
    Static,
    Breathing,
    ColorCycle,
    Tapping,
    Wave,
}

impl LightingMode {
    pub fn opcode_params(self) -> (u8, u8) {
        match self {
            LightingMode::Off => (0x00, 0x00),
            LightingMode::Static => (0x01, 0x05),
            LightingMode::Breathing => (0x02, 0x05),
            LightingMode::ColorCycle => (0x03, 0x05),
            LightingMode::Tapping => (0x04, 0x05),
            LightingMode::Wave => (0x05, 0x01),
        }
    }
}

pub fn build_set_lighting_mode_payload(mode: LightingMode) -> [u8; 8] {
    let (mm, pp) = mode.opcode_params();
    [cmd::SET_LIGHTING_MODE, 0x00, 0x00, mm, 0x80, pp, 0x00, 0x00]
}

pub fn build_set_lighting_color_payload(r: u8, g: u8, b: u8) -> [u8; 8] {
    [cmd::SET_LIGHTING_COLOR, 0x00, 0x00, r, g, b, 0x00, 0x00]
}

pub fn build_set_stage_color_payload(stage: u8, r: u8, g: u8, b: u8) -> Result<[u8; 8]> {
    if stage >= DPI_STAGE_COUNT {
        return Err(LorgarError::InvalidStage { stage });
    }
    Ok([cmd::SET_STAGE_COLOR, 0x00, stage, r, g, b, 0x00, 0x00])
}

pub fn parse_hex_color(hex_str: &str) -> Result<(u8, u8, u8)> {
    let s = hex_str.trim().trim_start_matches('#');
    if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(LorgarError::InvalidColorFormat {
            color: hex_str.to_string(),
        });
    }

    let r = u8::from_str_radix(&s[0..2], 16).map_err(|_| LorgarError::InvalidColorFormat {
        color: hex_str.to_string(),
    })?;
    let g = u8::from_str_radix(&s[2..4], 16).map_err(|_| LorgarError::InvalidColorFormat {
        color: hex_str.to_string(),
    })?;
    let b = u8::from_str_radix(&s[4..6], 16).map_err(|_| LorgarError::InvalidColorFormat {
        color: hex_str.to_string(),
    })?;

    Ok((r, g, b))
}

pub fn build_set_dpi_payload(stage: u8, dpi: u16) -> Result<[u8; 8]> {
    if stage >= DPI_STAGE_COUNT {
        return Err(LorgarError::InvalidStage { stage });
    }
    if !(MIN_DPI..=MAX_DPI).contains(&dpi) || !dpi.is_multiple_of(DPI_STEP) {
        return Err(LorgarError::InvalidDpi {
            dpi,
            min: MIN_DPI,
            max: MAX_DPI,
        });
    }

    let [dpi_lo, dpi_hi] = dpi.to_le_bytes();
    Ok([cmd::SET_DPI, 0x00, stage, dpi_lo, dpi_hi, 0x00, 0x00, 0x00])
}

pub fn build_set_active_stage_payload(stage: u8) -> Result<[u8; 8]> {
    if stage >= DPI_STAGE_COUNT {
        return Err(LorgarError::InvalidStage { stage });
    }
    Ok([
        cmd::SET_ACTIVE_STAGE,
        0x00,
        0x00,
        stage,
        0x00,
        0x00,
        0x00,
        0x00,
    ])
}

pub fn parse_battery_percentage(raw: &[u8]) -> Result<u8> {
    match InputPacket::parse(raw)?.event {
        InputEvent::State { battery_percentage } => Ok(battery_percentage),
        _ => Err(LorgarError::InvalidPacket(
            "not a documented state response".into(),
        )),
    }
}

pub fn parse_active_dpi_stage(raw: &[u8]) -> Result<u8> {
    if raw.len() == 64 && (raw[0] == 1 || raw[0] == 2) && raw[1] < DPI_STAGE_COUNT && raw[7] <= 100
    {
        Ok(raw[1])
    } else {
        Err(LorgarError::InvalidPacket(
            "not a documented state response".into(),
        ))
    }
}

pub fn parse_lighting_mode_readback(raw: &[u8]) -> Result<Option<LightingMode>> {
    if raw.len() == 64 && (raw[0] == 1 || raw[0] == 2) && raw[1] < DPI_STAGE_COUNT && raw[7] <= 100
    {
        match raw[2] {
            0 => Ok(Some(LightingMode::Off)),
            1 => Ok(Some(LightingMode::Static)),
            2 => Ok(Some(LightingMode::Breathing)),
            3 => Ok(Some(LightingMode::ColorCycle)),
            4 => Ok(Some(LightingMode::Tapping)),
            5 => Ok(Some(LightingMode::Wave)),
            _ => Ok(None),
        }
    } else {
        Err(LorgarError::InvalidPacket(
            "not a documented state response".into(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    SetDpi {
        stage: u8,
        dpi: u16,
    },
    SetActiveStage {
        stage: u8,
    },
    SetPolling {
        rate: PollingRate,
    },
    SetLightingMode {
        mode: LightingMode,
    },
    SetLightingColor {
        red: u8,
        green: u8,
        blue: u8,
    },
    SetStageColor {
        stage: u8,
        red: u8,
        green: u8,
        blue: u8,
    },
    QueryState,
}

impl Command {
    pub fn packet(&self) -> Result<FeaturePacket> {
        let raw = match *self {
            Self::SetDpi { stage, dpi } => build_set_dpi_payload(stage, dpi)?,
            Self::SetActiveStage { stage } => build_set_active_stage_payload(stage)?,
            Self::SetPolling { rate } => build_set_polling_rate_payload(rate),
            Self::SetLightingMode { mode } => build_set_lighting_mode_payload(mode),
            Self::SetLightingColor { red, green, blue } => {
                build_set_lighting_color_payload(red, green, blue)
            }
            Self::SetStageColor {
                stage,
                red,
                green,
                blue,
            } => build_set_stage_color_payload(stage, red, green, blue)?,
            Self::QueryState => [cmd::QUERY_STATE, 0, 0, 0, 0, 0, 0, 0],
        };
        Ok(FeaturePacket {
            raw,
            command: Some(self.clone()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FeaturePacket {
    pub raw: [u8; 8],
    pub command: Option<Command>,
}

impl FeaturePacket {
    pub fn report(&self) -> [u8; 9] {
        let mut report = [0; 9];
        report[1..].copy_from_slice(&self.raw);
        report
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let raw: [u8; 8] = bytes
            .try_into()
            .map_err(|_| LorgarError::InvalidFeatureLength { len: bytes.len() })?;
        let candidate = match raw[0] {
            cmd::SET_DPI => Some(Command::SetDpi {
                stage: raw[2],
                dpi: u16::from_le_bytes([raw[3], raw[4]]),
            }),
            cmd::SET_ACTIVE_STAGE => Some(Command::SetActiveStage { stage: raw[3] }),
            cmd::SET_POLLING_RATE => match raw[3] {
                1 => Some(PollingRate::Hz1000),
                2 => Some(PollingRate::Hz500),
                4 => Some(PollingRate::Hz250),
                8 => Some(PollingRate::Hz125),
                _ => None,
            }
            .map(|rate| Command::SetPolling { rate }),
            cmd::SET_LIGHTING_MODE => match raw[3] {
                0 => Some(LightingMode::Off),
                1 => Some(LightingMode::Static),
                2 => Some(LightingMode::Breathing),
                3 => Some(LightingMode::ColorCycle),
                4 => Some(LightingMode::Tapping),
                5 => Some(LightingMode::Wave),
                _ => None,
            }
            .map(|mode| Command::SetLightingMode { mode }),
            cmd::SET_LIGHTING_COLOR => Some(Command::SetLightingColor {
                red: raw[3],
                green: raw[4],
                blue: raw[5],
            }),
            cmd::SET_STAGE_COLOR => Some(Command::SetStageColor {
                stage: raw[2],
                red: raw[3],
                green: raw[4],
                blue: raw[5],
            }),
            cmd::QUERY_STATE => Some(Command::QueryState),
            _ => None,
        };
        let command = candidate.filter(|c| c.packet().is_ok_and(|p| p.raw == raw));
        Ok(Self { raw, command })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputEvent {
    State { battery_percentage: u8 },
    DpiStageChanged { stage: u8 },
    UnknownVendor { event_code: u8 },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InputPacket {
    pub report_id: Option<u8>,
    pub event: InputEvent,
    pub raw: Vec<u8>,
}

impl InputPacket {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let first = *raw
            .first()
            .ok_or_else(|| LorgarError::InvalidPacket("empty input report".into()))?;
        let event = match raw {
            [5, 0x0b, stage, 0, 0] if *stage < DPI_STAGE_COUNT => {
                InputEvent::DpiStageChanged { stage: *stage }
            }
            [5, event_code, ..] => InputEvent::UnknownVendor {
                event_code: *event_code,
            },
            [1 | 2, stage, ..]
                if raw.len() == 64
                    && *stage < DPI_STAGE_COUNT
                    && (raw.len() <= 2 || raw[2] < DPI_STAGE_COUNT)
                    && raw[7] <= 100
                    && (raw.len() <= 8 || raw[8] <= 1) =>
            {
                InputEvent::State {
                    battery_percentage: raw[7],
                }
            }
            _ => InputEvent::Unknown,
        };
        Ok(Self {
            report_id: if matches!(event, InputEvent::State { .. }) {
                None
            } else {
                Some(first)
            },
            event,
            raw: raw.to_vec(),
        })
    }

    pub fn parse_for_usage(usage_page: u16, raw: &[u8]) -> Result<Self> {
        let mut packet = Self::parse(raw)?;
        match usage_page {
            CONFIG_USAGE_PAGE => {
                packet.report_id = None;
                if !matches!(packet.event, InputEvent::State { .. }) {
                    packet.event = InputEvent::Unknown;
                }
            }
            0xff11 => {
                packet.report_id = raw.first().copied();
                if matches!(packet.event, InputEvent::State { .. }) {
                    packet.event = InputEvent::Unknown;
                }
            }
            _ => {
                packet.report_id = None;
                packet.event = InputEvent::Unknown;
            }
        }
        Ok(packet)
    }
}

fn compute_nvram_checksum(packet: &mut [u8; 32]) {
    let sum: u8 = packet[..31].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    packet[31] = !sum;
}

/// Packet 0x01: Master DPI Stage & Lighting Configuration
pub fn build_nvram_packet_01(
    active_stage: u8,
    mode: LightingMode,
    brightness: u8,
    speed: u8,
    direction: u8,
) -> Result<[u8; 32]> {
    if active_stage >= DPI_STAGE_COUNT {
        return Err(LorgarError::InvalidStage {
            stage: active_stage,
        });
    }
    let speed = speed.clamp(1, 5);
    let mut packet = [0u8; 32];
    packet[0] = 0x01;
    packet[1] = active_stage;
    packet[2] = match mode {
        LightingMode::Off => 0x00,
        LightingMode::Static => 0x01,
        LightingMode::Breathing => 0x02,
        LightingMode::ColorCycle => 0x03,
        LightingMode::Tapping => 0x04,
        LightingMode::Wave => 0x05,
    };
    packet[3] = brightness;
    packet[4] = speed;
    packet[5] = direction.min(1);
    compute_nvram_checksum(&mut packet);
    Ok(packet)
}

/// Packet 0x02: Sensor, Power & Performance Configuration
pub fn build_nvram_packet_02(
    active_stage: u8,
    lod: u8,
    sleep_minutes: u8,
    angle_snap: bool,
    motion_sync: bool,
    color: (u8, u8, u8),
) -> Result<[u8; 32]> {
    if active_stage >= DPI_STAGE_COUNT {
        return Err(LorgarError::InvalidStage {
            stage: active_stage,
        });
    }
    if !(1..=10).contains(&sleep_minutes) {
        return Err(LorgarError::Config(format!(
            "sleep timeout must be between 1 and 10 minutes (got {sleep_minutes})"
        )));
    }
    let lod_val = if lod == 2 { 0x02 } else { 0x01 };
    let mut packet = [0u8; 32];
    packet[0] = 0x02;
    packet[1] = active_stage;
    packet[2] = lod_val;
    packet[3] = sleep_minutes;
    packet[5] = if angle_snap { 0x01 } else { 0x00 };
    packet[6] = if motion_sync { 0x01 } else { 0x00 };
    packet[8] = color.0;
    packet[9] = color.1;
    packet[10] = color.2;
    compute_nvram_checksum(&mut packet);
    Ok(packet)
}

/// Backwards-compatible sleep timeout packet builder
pub fn build_set_sleep_timeout_packet(minutes: u8, active_stage: u8) -> Result<[u8; 32]> {
    build_nvram_packet_02(active_stage, 1, minutes, false, false, (0, 0, 0))
}

/// Packet 0x03: 8-Stage DPI Indicator Palette
pub fn build_nvram_packet_03(palette: &[(u8, u8, u8); 8]) -> [u8; 32] {
    let mut packet = [0u8; 32];
    packet[0] = 0x03;
    for (i, &(r, g, b)) in palette.iter().enumerate() {
        let base = 1 + i * 3;
        packet[base] = r;
        packet[base + 1] = g;
        packet[base + 2] = b;
    }
    compute_nvram_checksum(&mut packet);
    packet
}

/// Packet 0x04: Button Debounce & Rapid-Fire Settings
pub fn build_nvram_packet_04(
    debounce_ms: u16,
    rapid_fire_enabled: bool,
    fire_interval_ms: u8,
    fire_repeat_count: u8,
) -> [u8; 32] {
    let mut packet = [0u8; 32];
    packet[0] = 0x04;
    let [d_lo, d_hi] = debounce_ms.to_le_bytes();
    packet[1] = d_lo;
    packet[2] = d_hi;
    packet[3] = if rapid_fire_enabled { 0x01 } else { 0x00 };
    packet[4] = fire_interval_ms;
    packet[5] = fire_repeat_count;
    compute_nvram_checksum(&mut packet);
    packet
}

/// Packets 0x05 & 0x06: Full Hardware DPI Banks (0..3 and 4..7)
pub fn build_nvram_packet_05_06(stages: &[u16; 8]) -> Result<([u8; 32], [u8; 32])> {
    for &dpi in stages {
        if !(MIN_DPI..=MAX_DPI).contains(&dpi) {
            return Err(LorgarError::InvalidDpi {
                dpi,
                min: MIN_DPI,
                max: MAX_DPI,
            });
        }
    }
    let mut p5 = [0u8; 32];
    p5[0] = 0x05;
    for i in 0..4 {
        let [lo, hi] = stages[i].to_le_bytes();
        p5[1 + i * 2] = lo;
        p5[1 + i * 2 + 1] = hi;
    }
    compute_nvram_checksum(&mut p5);

    let mut p6 = [0u8; 32];
    p6[0] = 0x06;
    for i in 0..4 {
        let [lo, hi] = stages[4 + i].to_le_bytes();
        p6[1 + i * 2] = lo;
        p6[1 + i * 2 + 1] = hi;
    }
    compute_nvram_checksum(&mut p6);

    Ok((p5, p6))
}

/// Packet 0x07: Physical Button Remapping Table (24 bytes)
pub fn build_nvram_packet_07(mappings: &[u8; 24]) -> [u8; 32] {
    let mut packet = [0u8; 32];
    packet[0] = 0x07;
    packet[1..25].copy_from_slice(mappings);
    compute_nvram_checksum(&mut packet);
    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_dpi_800_stage_0() {
        let payload = build_set_dpi_payload(0, 800).unwrap();
        assert_eq!(payload, [0x26, 0x00, 0x00, 0x20, 0x03, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_set_dpi_1400_stage_1() {
        let payload = build_set_dpi_payload(1, 1400).unwrap();
        assert_eq!(payload, [0x26, 0x00, 0x01, 0x78, 0x05, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_set_dpi_1600_stage_1() {
        let payload = build_set_dpi_payload(1, 1600).unwrap();
        assert_eq!(payload, [0x26, 0x00, 0x01, 0x40, 0x06, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_set_dpi_3000_stage_1() {
        let payload = build_set_dpi_payload(1, 3000).unwrap();
        assert_eq!(payload, [0x26, 0x00, 0x01, 0xb8, 0x0b, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_invalid_stage() {
        let res = build_set_dpi_payload(8, 1400);
        assert!(matches!(res, Err(LorgarError::InvalidStage { stage: 8 })));
    }

    #[test]
    fn test_invalid_dpi() {
        assert!(build_set_dpi_payload(0, 50).is_err());
        assert!(build_set_dpi_payload(0, 837).is_err());
        assert!(build_set_dpi_payload(0, 26050).is_err());
        assert!(build_set_dpi_payload(0, 26000).is_ok());
    }

    #[test]
    fn test_set_lighting_mode_payloads() {
        assert_eq!(
            build_set_lighting_mode_payload(LightingMode::Off),
            [0x22, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            build_set_lighting_mode_payload(LightingMode::Static),
            [0x22, 0x00, 0x00, 0x01, 0x80, 0x05, 0x00, 0x00]
        );
        assert_eq!(
            build_set_lighting_mode_payload(LightingMode::Breathing),
            [0x22, 0x00, 0x00, 0x02, 0x80, 0x05, 0x00, 0x00]
        );
        assert_eq!(
            build_set_lighting_mode_payload(LightingMode::ColorCycle),
            [0x22, 0x00, 0x00, 0x03, 0x80, 0x05, 0x00, 0x00]
        );
        assert_eq!(
            build_set_lighting_mode_payload(LightingMode::Tapping),
            [0x22, 0x00, 0x00, 0x04, 0x80, 0x05, 0x00, 0x00]
        );
        assert_eq!(
            build_set_lighting_mode_payload(LightingMode::Wave),
            [0x22, 0x00, 0x00, 0x05, 0x80, 0x01, 0x00, 0x00]
        );
    }

    #[test]
    fn test_set_lighting_color_payloads() {
        assert_eq!(
            build_set_lighting_color_payload(0xff, 0x00, 0x00),
            [0x23, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            build_set_lighting_color_payload(0x00, 0xff, 0x00),
            [0x23, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            build_set_lighting_color_payload(0x00, 0x00, 0xff),
            [0x23, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00]
        );
    }

    #[test]
    fn test_set_stage_color_payloads() {
        assert_eq!(
            build_set_stage_color_payload(0, 0x32, 0xcd, 0x32).unwrap(),
            [0x27, 0x00, 0x00, 0x32, 0xcd, 0x32, 0x00, 0x00]
        );
        assert_eq!(
            build_set_stage_color_payload(7, 0x4c, 0x11, 0x30).unwrap(),
            [0x27, 0x00, 0x07, 0x4c, 0x11, 0x30, 0x00, 0x00]
        );
        assert!(build_set_stage_color_payload(8, 0, 0, 0).is_err());
    }

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("ff0000").unwrap(), (255, 0, 0));
        assert_eq!(parse_hex_color("#00ff00").unwrap(), (0, 255, 0));
        assert_eq!(parse_hex_color("#a28466").unwrap(), (162, 132, 102));
        assert!(parse_hex_color("invalid").is_err());
    }

    #[test]
    fn test_set_sleep_timeout_packet() {
        assert!(build_set_sleep_timeout_packet(0, 1).is_err());
        assert!(build_set_sleep_timeout_packet(11, 1).is_err());
        let p = build_set_sleep_timeout_packet(5, 1).unwrap();
        assert_eq!(p[0], 0x02);
        assert_eq!(p[1], 1);
        assert_eq!(p[3], 5);
        let sum: u8 = p.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0xff);
    }

    #[test]
    fn test_set_polling_rate_payloads() {
        assert_eq!(
            build_set_polling_rate_payload(PollingRate::Hz125),
            [0x20, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            build_set_polling_rate_payload(PollingRate::Hz250),
            [0x20, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            build_set_polling_rate_payload(PollingRate::Hz500),
            [0x20, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            build_set_polling_rate_payload(PollingRate::Hz1000),
            [0x20, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn test_nvram_packet_01() {
        let p = build_nvram_packet_01(2, LightingMode::Wave, 200, 4, 1).unwrap();
        assert_eq!(p[0], 0x01);
        assert_eq!(p[1], 2);
        assert_eq!(p[2], 0x05);
        assert_eq!(p[3], 200);
        assert_eq!(p[4], 4);
        assert_eq!(p[5], 1);
        let sum: u8 = p.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0xff);
    }

    #[test]
    fn test_nvram_packet_02() {
        let p = build_nvram_packet_02(0, 2, 8, true, true, (0xff, 0xaa, 0x55)).unwrap();
        assert_eq!(p[0], 0x02);
        assert_eq!(p[1], 0);
        assert_eq!(p[2], 0x02);
        assert_eq!(p[3], 8);
        assert_eq!(p[5], 1);
        assert_eq!(p[6], 1);
        assert_eq!(p[8], 0xff);
        assert_eq!(p[9], 0xaa);
        assert_eq!(p[10], 0x55);
        let sum: u8 = p.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0xff);
    }

    #[test]
    fn test_nvram_packet_03() {
        let palette = [
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (255, 255, 0),
            (255, 0, 255),
            (0, 255, 255),
            (255, 255, 255),
            (128, 128, 128),
        ];
        let p = build_nvram_packet_03(&palette);
        assert_eq!(p[0], 0x03);
        assert_eq!(p[1], 255);
        assert_eq!(p[2], 0);
        assert_eq!(p[3], 0);
        assert_eq!(p[22], 128);
        assert_eq!(p[23], 128);
        assert_eq!(p[24], 128);
        let sum: u8 = p.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0xff);
    }

    #[test]
    fn test_nvram_packet_04() {
        let p = build_nvram_packet_04(12, true, 40, 3);
        assert_eq!(p[0], 0x04);
        assert_eq!(p[1], 12);
        assert_eq!(p[2], 0);
        assert_eq!(p[3], 1);
        assert_eq!(p[4], 40);
        assert_eq!(p[5], 3);
        let sum: u8 = p.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0xff);
    }

    #[test]
    fn test_nvram_packet_05_06() {
        let stages = [800, 1200, 1600, 2400, 3200, 4800, 6400, 12000];
        let (p5, p6) = build_nvram_packet_05_06(&stages).unwrap();
        assert_eq!(p5[0], 0x05);
        assert_eq!(&p5[1..3], &800u16.to_le_bytes());
        assert_eq!(&p5[7..9], &2400u16.to_le_bytes());
        let sum5: u8 = p5.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum5, 0xff);

        assert_eq!(p6[0], 0x06);
        assert_eq!(&p6[1..3], &3200u16.to_le_bytes());
        assert_eq!(&p6[7..9], &12000u16.to_le_bytes());
        let sum6: u8 = p6.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum6, 0xff);
    }

    #[test]
    fn test_nvram_packet_07() {
        let mappings = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        ];
        let p = build_nvram_packet_07(&mappings);
        assert_eq!(p[0], 0x07);
        assert_eq!(&p[1..25], &mappings);
        let sum: u8 = p.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0xff);
    }
}
