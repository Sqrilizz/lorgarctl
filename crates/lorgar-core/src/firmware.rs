use crate::error::{LorgarError, Result};

pub const FIRMWARE_HEADER_SIZE: usize = 0x2000;
pub const FIRMWARE_CHECKSUM_SEED: u32 = 0x55555555;
pub const FLASH_WRITE_CHUNK_SIZE: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FirmwareMetadata {
    pub product_name: String,
    pub ic_name: String,
    pub sensor_name: String,
    pub boot_in_ep: String,
    pub boot_out_ep: String,
    pub normal_in_ep: String,
    pub normal_out_ep: String,
    pub flash_base_addr: u32,
    pub file_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareContainer {
    pub meta: FirmwareMetadata,
    pub payload: Vec<u8>,
}

impl FirmwareContainer {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < FIRMWARE_HEADER_SIZE {
            return Err(LorgarError::Firmware(format!(
                "firmware file too small: {} bytes (minimum header size is {} bytes)",
                data.len(),
                FIRMWARE_HEADER_SIZE
            )));
        }

        let stored_checksum = u32::from_le_bytes(
            data[0..4]
                .try_into()
                .map_err(|_| LorgarError::Firmware("invalid checksum slice".into()))?,
        );
        let declared_len = u32::from_le_bytes(
            data[4..8]
                .try_into()
                .map_err(|_| LorgarError::Firmware("invalid length slice".into()))?,
        ) as usize;

        let check_len = declared_len.min(data.len());
        if check_len < 8 {
            return Err(LorgarError::Firmware("invalid declared length".into()));
        }
        let sum: u32 = data[8..check_len]
            .iter()
            .fold(0u32, |acc, &b| acc.wrapping_add(b as u32));
        let computed_checksum = FIRMWARE_CHECKSUM_SEED.wrapping_sub(sum);

        if stored_checksum != computed_checksum {
            return Err(LorgarError::Firmware(format!(
                "checksum mismatch: expected 0x{computed_checksum:08x}, got 0x{stored_checksum:08x}"
            )));
        }

        let read_str = |start: usize, max_len: usize| -> String {
            if start >= data.len() {
                return String::new();
            }
            let end = (start + max_len).min(data.len());
            let slice = &data[start..end];
            let nul_pos = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
            String::from_utf8_lossy(&slice[..nul_pos])
                .trim()
                .to_string()
        };

        let product_name = read_str(0x37, 64);
        let ic_name = read_str(0x77, 64);
        let sensor_name = read_str(0xb7, 64);
        let boot_in_ep = read_str(0xf7, 64);
        let boot_out_ep = read_str(0x137, 64);
        let normal_in_ep = read_str(0x177, 64);
        let normal_out_ep = read_str(0x277, 64);

        let flash_base_addr = u32::from_be_bytes(
            data[0x21e..0x222]
                .try_into()
                .map_err(|_| LorgarError::Firmware("invalid flash base address slice".into()))?,
        );

        let payload = if data.len() > FIRMWARE_HEADER_SIZE {
            data[FIRMWARE_HEADER_SIZE..].to_vec()
        } else {
            Vec::new()
        };

        Ok(Self {
            meta: FirmwareMetadata {
                product_name,
                ic_name,
                sensor_name,
                boot_in_ep,
                boot_out_ep,
                normal_in_ep,
                normal_out_ep,
                flash_base_addr,
                file_size: data.len() as u32,
            },
            payload,
        })
    }

    pub fn build(meta: &FirmwareMetadata, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; FIRMWARE_HEADER_SIZE + payload.len()];
        let header_len = FIRMWARE_HEADER_SIZE as u32;
        out[4..8].copy_from_slice(&header_len.to_le_bytes());

        let write_str = |buf: &mut [u8], start: usize, s: &str| {
            let bytes = s.as_bytes();
            let len = bytes.len().min(63);
            buf[start..start + len].copy_from_slice(&bytes[..len]);
            buf[start + len] = 0;
        };

        write_str(&mut out, 0x37, &meta.product_name);
        write_str(&mut out, 0x77, &meta.ic_name);
        write_str(&mut out, 0xb7, &meta.sensor_name);
        write_str(&mut out, 0xf7, &meta.boot_in_ep);
        write_str(&mut out, 0x137, &meta.boot_out_ep);
        write_str(&mut out, 0x177, &meta.normal_in_ep);
        write_str(&mut out, 0x277, &meta.normal_out_ep);

        out[0x21e..0x222].copy_from_slice(&meta.flash_base_addr.to_be_bytes());

        if !payload.is_empty() {
            out[FIRMWARE_HEADER_SIZE..].copy_from_slice(payload);
        }

        let sum: u32 = out[8..FIRMWARE_HEADER_SIZE]
            .iter()
            .fold(0u32, |acc, &b| acc.wrapping_add(b as u32));
        let chk = FIRMWARE_CHECKSUM_SEED.wrapping_sub(sum);
        out[0..4].copy_from_slice(&chk.to_le_bytes());

        out
    }
}

pub fn verify_header(data: &[u8]) -> Result<FirmwareMetadata> {
    let container = FirmwareContainer::parse(data)?;
    Ok(container.meta)
}

pub fn create_firmware(
    product: &str,
    ic: &str,
    sensor: &str,
    flash_base_addr: u32,
    payload: &[u8],
) -> Vec<u8> {
    let meta = FirmwareMetadata {
        product_name: product.to_string(),
        ic_name: ic.to_string(),
        sensor_name: sensor.to_string(),
        boot_in_ep: String::new(),
        boot_out_ep: String::new(),
        normal_in_ep: String::new(),
        normal_out_ep: String::new(),
        flash_base_addr,
        file_size: (FIRMWARE_HEADER_SIZE + payload.len()) as u32,
    };
    FirmwareContainer::build(&meta, payload)
}

/// Packet opcode to read flash chunk from device (Opcode 0x08, Sub 0x08)
pub fn build_read_flash_packet(addr: u16, chunk_size: u8) -> [u8; 8] {
    let mut pkt = [0u8; 8];
    pkt[0] = 0x08;
    pkt[1] = 0x08;
    pkt[2] = 0x00;
    pkt[3] = (addr >> 8) as u8;
    pkt[4] = (addr & 0xff) as u8;
    pkt[5] = chunk_size.min(10);
    pkt[6] = 0x00;
    let sum: u8 = pkt[..7].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    pkt[7] = 0x55u8.wrapping_sub(sum);
    pkt
}

/// Packet command to jump device to bootloader update mode (Opcode 0x01, Sub 0x01, 0x08, 0x0d)
pub fn build_enter_bootloader_packet() -> [u8; 8] {
    let mut pkt = [0u8; 8];
    pkt[0] = 0x01;
    pkt[1] = 0x01;
    pkt[2] = 0x00;
    pkt[3] = 0x08;
    pkt[4] = 0x0d;
    pkt[5] = 0x00;
    pkt[6] = 0x00;
    let sum: u8 = pkt[..7].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    pkt[7] = 0x55u8.wrapping_sub(sum);
    pkt
}

/// Build flash write chunk packet for writing 32 bytes of code
/// Uses opcode 0xC0 (regular block) or 0xC1 (final block)
pub fn build_flash_write_chunk(addr: u32, chunk: &[u8], is_final: bool) -> Result<[u8; 40]> {
    if chunk.len() > FLASH_WRITE_CHUNK_SIZE {
        return Err(LorgarError::Firmware(format!(
            "chunk exceeds {FLASH_WRITE_CHUNK_SIZE} bytes"
        )));
    }
    let mut pkt = [0u8; 40];
    pkt[0] = if is_final { 0xc1 } else { 0xc0 };
    pkt[1] = chunk.len() as u8;
    pkt[2] = (addr >> 24) as u8;
    pkt[3] = (addr >> 16) as u8;
    pkt[4] = (addr >> 8) as u8;
    pkt[5] = (addr & 0xff) as u8;
    pkt[6..6 + chunk.len()].copy_from_slice(chunk);
    let sum: u8 = pkt[..39].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    pkt[39] = 0x55u8.wrapping_sub(sum);
    Ok(pkt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_firmware_container_roundtrip() {
        let meta = FirmwareMetadata {
            product_name: "LORGAR MSA10W".into(),
            ic_name: "571R".into(),
            sensor_name: "PAW3395".into(),
            boot_in_ep: "EP1".into(),
            boot_out_ep: "EP2".into(),
            normal_in_ep: "EP3".into(),
            normal_out_ep: "EP4".into(),
            flash_base_addr: 0x08000000,
            file_size: 0,
        };
        let payload = vec![0x90; 128];
        let bytes = FirmwareContainer::build(&meta, &payload);
        assert_eq!(bytes.len(), FIRMWARE_HEADER_SIZE + payload.len());

        let parsed = FirmwareContainer::parse(&bytes).expect("parse failed");
        assert_eq!(parsed.meta.product_name, "LORGAR MSA10W");
        assert_eq!(parsed.meta.ic_name, "571R");
        assert_eq!(parsed.meta.sensor_name, "PAW3395");
        assert_eq!(parsed.meta.flash_base_addr, 0x08000000);
        assert_eq!(parsed.payload, payload);
    }

    #[test]
    fn test_firmware_corrupted_checksum() {
        let meta = FirmwareMetadata {
            product_name: "Test".into(),
            ic_name: "571R".into(),
            sensor_name: "PAW3395".into(),
            boot_in_ep: String::new(),
            boot_out_ep: String::new(),
            normal_in_ep: String::new(),
            normal_out_ep: String::new(),
            flash_base_addr: 0,
            file_size: 0,
        };
        let mut bytes = FirmwareContainer::build(&meta, &[]);
        bytes[0] ^= 0xff; // corrupt checksum
        assert!(matches!(
            FirmwareContainer::parse(&bytes),
            Err(LorgarError::Firmware(_))
        ));
    }

    #[test]
    fn test_read_flash_packet() {
        let p = build_read_flash_packet(0x0120, 10);
        assert_eq!(p[0], 0x08);
        assert_eq!(p[1], 0x08);
        assert_eq!(p[3], 0x01);
        assert_eq!(p[4], 0x20);
        assert_eq!(p[5], 10);
        let sum: u8 = p[..7].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(p[7], 0x55u8.wrapping_sub(sum));
    }

    #[test]
    fn test_enter_bootloader_packet() {
        let p = build_enter_bootloader_packet();
        assert_eq!(p[0], 0x01);
        assert_eq!(p[1], 0x01);
        assert_eq!(p[3], 0x08);
        assert_eq!(p[4], 0x0d);
        let sum: u8 = p[..7].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(p[7], 0x55u8.wrapping_sub(sum));
    }

    #[test]
    fn test_flash_write_chunk() {
        let chunk = [0xaa; 32];
        let p = build_flash_write_chunk(0x08001000, &chunk, false).unwrap();
        assert_eq!(p[0], 0xc0);
        assert_eq!(p[1], 32);
        assert_eq!(p[2], 0x08);
        assert_eq!(p[3], 0x00);
        assert_eq!(p[4], 0x10);
        assert_eq!(p[5], 0x00);
        assert_eq!(&p[6..38], &chunk[..32]);
        let sum: u8 = p[..39].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        assert_eq!(p[39], 0x55u8.wrapping_sub(sum));

        let p_final = build_flash_write_chunk(0x08001000, &chunk, true).unwrap();
        assert_eq!(p_final[0], 0xc1);
    }
}
