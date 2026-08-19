use crate::error::{LorgarError, Result};

pub const VENDOR_ID: u16 = 0x093a;
pub const PRODUCT_IDS: &[u16] = &[0xc600, 0xc601];
pub const CONFIG_USAGE_PAGE: u16 = 0xff10;
pub const DPI_STAGE_COUNT: u8 = 8;
pub const MIN_DPI: u16 = 100;
pub const MAX_DPI: u16 = 12000;

pub mod cmd {
    pub const SET_POLLING_RATE: u8 = 0x20;
    pub const SET_DPI: u8 = 0x26;
    pub const SET_ACTIVE_STAGE: u8 = 0x21;
    pub const SET_LIGHTING_MODE: u8 = 0x22;
    pub const SET_LIGHTING_COLOR: u8 = 0x23;
    pub const QUERY_STATE: u8 = 0x82;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    [cmd::SET_POLLING_RATE, 0x00, 0x00, rate.interval_ms(), 0x00, 0x00, 0x00, 0x00]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightingMode {
    Static,
    Breathing,
    ColorCycle,
    Tapping,
    Wave,
}

impl LightingMode {
    pub fn opcode_params(self) -> (u8, u8) {
        match self {
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

pub fn parse_hex_color(hex_str: &str) -> Result<(u8, u8, u8)> {
    let s = hex_str.trim().trim_start_matches('#');
    if s.len() != 6 {
        return Err(LorgarError::InvalidColorFormat {
            color: hex_str.to_string(),
        });
    }

    let r = u8::from_str_radix(&s[0..2], 16)
        .map_err(|_| LorgarError::InvalidColorFormat { color: hex_str.to_string() })?;
    let g = u8::from_str_radix(&s[2..4], 16)
        .map_err(|_| LorgarError::InvalidColorFormat { color: hex_str.to_string() })?;
    let b = u8::from_str_radix(&s[4..6], 16)
        .map_err(|_| LorgarError::InvalidColorFormat { color: hex_str.to_string() })?;

    Ok((r, g, b))
}

pub fn build_set_dpi_payload(stage: u8, dpi: u16) -> Result<[u8; 8]> {
    if stage >= DPI_STAGE_COUNT {
        return Err(LorgarError::InvalidStage { stage });
    }
    if !(MIN_DPI..=MAX_DPI).contains(&dpi) {
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
    Ok([cmd::SET_ACTIVE_STAGE, 0x00, 0x00, stage, 0x00, 0x00, 0x00, 0x00])
}

pub fn parse_battery_percentage(raw: &[u8]) -> Result<u8> {
    if raw.len() < 8 {
        return Err(LorgarError::Transport(
            "Device query response too short".into(),
        ));
    }
    Ok(raw[7])
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
        let res = build_set_dpi_payload(0, 50);
        assert!(matches!(res, Err(LorgarError::InvalidDpi { .. })));
    }

    #[test]
    fn test_set_lighting_mode_payloads() {
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
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("ff0000").unwrap(), (255, 0, 0));
        assert_eq!(parse_hex_color("#00ff00").unwrap(), (0, 255, 0));
        assert_eq!(parse_hex_color("#a28466").unwrap(), (162, 132, 102));
        assert!(parse_hex_color("invalid").is_err());
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
    fn test_invalid_polling_rate() {
        assert!(matches!(
            PollingRate::from_hz(750),
            Err(LorgarError::InvalidPollingRate { hz: 750 })
        ));
    }
}
