use crate::error::{LorgarError, Result};

pub trait HidTransport: Send {
    fn send_feature_command(&self, payload: &[u8; 8]) -> Result<()>;
    fn query_feature_command(&self, payload: &[u8; 8], timeout_ms: i32) -> Result<Vec<u8>>;
}

pub struct HidapiTransport {
    device: hidapi::HidDevice,
}

impl HidapiTransport {
    pub fn new(device: hidapi::HidDevice) -> Self {
        Self { device }
    }
}

impl HidTransport for HidapiTransport {
    fn send_feature_command(&self, payload: &[u8; 8]) -> Result<()> {
        let mut report = [0u8; 9];
        report[0] = 0x00;
        report[1..9].copy_from_slice(payload);

        log::debug!("Sending Feature Report (9 bytes): {:02x?}", report);

        let result = self.device.send_feature_report(&report);
        match result {
            Ok(()) => {
                log::debug!("Feature report sent successfully");
                Ok(())
            }
            Err(e) => Err(LorgarError::Hid(e)),
        }
    }

    fn query_feature_command(&self, payload: &[u8; 8], timeout_ms: i32) -> Result<Vec<u8>> {
        let mut report = [0u8; 9];
        report[0] = 0x00;
        report[1..9].copy_from_slice(payload);

        log::debug!("Sending Feature Query (9 bytes): {:02x?}", report);
        self.device.send_feature_report(&report)?;

        let mut buf = [0u8; 64];
        let bytes_read = self.device.read_timeout(&mut buf, timeout_ms)?;
        log::debug!("Read {} byte(s) response from device", bytes_read);

        Ok(buf[..bytes_read].to_vec())
    }
}
