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

pub struct DeviceLock {
    _file: std::fs::File,
}

impl DeviceLock {
    pub fn acquire() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join("lorgarctl_device.lock");
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o666)
            .truncate(false)
            .open(&path)?;

        use std::os::unix::io::AsRawFd;
        unsafe {
            let fd = file.as_raw_fd();
            let ret = libc::flock(fd, libc::LOCK_EX);
            if ret != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }

        Ok(Self { _file: file })
    }
}

impl Drop for DeviceLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        unsafe {
            let fd = self._file.as_raw_fd();
            libc::flock(fd, libc::LOCK_UN);
        }
    }
}

impl HidTransport for HidapiTransport {
    fn send_feature_command(&self, payload: &[u8; 8]) -> Result<()> {
        let _lock = DeviceLock::acquire().expect("Failed to acquire IPC device lock");
        std::thread::sleep(std::time::Duration::from_millis(20));

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
        let _lock = DeviceLock::acquire().expect("Failed to acquire IPC device lock");
        std::thread::sleep(std::time::Duration::from_millis(20));

        let mut report = [0u8; 9];
        report[0] = 0x00;
        report[1..9].copy_from_slice(payload);

        log::debug!("Sending Feature Query (9 bytes): {:02x?}", report);

        for attempt in 1..=5 {
            if let Err(e) = self.device.send_feature_report(&report) {
                if attempt == 5 {
                    return Err(LorgarError::Hid(e));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }

            let mut buf = [0u8; 64];
            match self.device.read_timeout(&mut buf, timeout_ms) {
                Ok(bytes_read) => {
                    log::debug!("Attempt {}: Read {} byte(s) response from device", attempt, bytes_read);
                    if bytes_read >= 8 && (buf[0] != 0 || buf[1] != 0 || buf[7] != 0) {
                        return Ok(buf[..bytes_read].to_vec());
                    }
                }
                Err(e) => {
                    log::debug!("Attempt {} read error: {:?}", attempt, e);
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        Err(LorgarError::Transport(
            "Short device state query response from hardware after 5 attempts".into(),
        ))
    }
}
