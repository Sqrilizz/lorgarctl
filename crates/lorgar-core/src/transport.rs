use crate::error::{LorgarError, Result};
use crate::protocol::{cmd, FeaturePacket, InputEvent, InputPacket};
use crate::storage::{runtime_dir, FileLock};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub trait HidTransport: Send {
    fn send_feature_command(&self, payload: &[u8; 8]) -> Result<()>;
    fn query_feature_command(&self, payload: &[u8; 8], timeout_ms: i32) -> Result<Vec<u8>>;
    fn read_input(&self, _timeout_ms: i32) -> Result<Option<Vec<u8>>> {
        Err(LorgarError::Transport(
            "transport does not support input monitoring".into(),
        ))
    }
}

pub struct HidapiTransport {
    device: hidapi::HidDevice,
    pending: RefCell<VecDeque<Vec<u8>>>,
}

impl HidapiTransport {
    pub fn new(device: hidapi::HidDevice) -> Self {
        Self {
            device,
            pending: RefCell::default(),
        }
    }

    fn read_hardware(&self, timeout_ms: i32) -> Result<Option<Vec<u8>>> {
        if !(0..=5000).contains(&timeout_ms) {
            return Err(LorgarError::Config(
                "read timeout must be 0..5000 ms".into(),
            ));
        }
        let mut buffer = [0; 4096];
        let len = self.device.read_timeout(&mut buffer, timeout_ms)?;
        Ok((len > 0).then(|| buffer[..len].to_vec()))
    }
}

pub struct DeviceLock {
    _lock: FileLock,
}

impl DeviceLock {
    pub fn acquire() -> Result<Self> {
        Ok(Self {
            _lock: FileLock::acquire(&runtime_dir()?.join("device.lock"))?,
        })
    }
}

impl HidTransport for HidapiTransport {
    fn send_feature_command(&self, payload: &[u8; 8]) -> Result<()> {
        let report = FeaturePacket::parse(payload)?.report();
        log::debug!("Feature output: {:02x?}", report);
        self.device.send_feature_report(&report)?;
        std::thread::sleep(Duration::from_millis(20));
        Ok(())
    }

    fn query_feature_command(&self, payload: &[u8; 8], timeout_ms: i32) -> Result<Vec<u8>> {
        if !(1..=5000).contains(&timeout_ms) {
            return Err(LorgarError::Config(
                "query timeout must be 1..5000 ms".into(),
            ));
        }
        self.send_feature_command(payload)?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(LorgarError::Timeout);
            }
            let Some(raw) = self.read_hardware(remaining.as_millis().max(1) as i32)? else {
                return Err(LorgarError::Timeout);
            };
            if payload[0] != cmd::QUERY_STATE
                || matches!(InputPacket::parse(&raw)?.event, InputEvent::State { .. })
            {
                return Ok(raw);
            }
            let mut pending = self.pending.borrow_mut();
            if pending.len() >= 1024 {
                return Err(LorgarError::Transport(
                    "too many unrelated reports during query".into(),
                ));
            }
            pending.push_back(raw);
        }
    }

    fn read_input(&self, timeout_ms: i32) -> Result<Option<Vec<u8>>> {
        if let Some(raw) = self.pending.borrow_mut().pop_front() {
            return Ok(Some(raw));
        }
        self.read_hardware(timeout_ms)
    }
}

#[derive(Default)]
pub struct MockTransport {
    pub sent: RefCell<Vec<[u8; 8]>>,
    pub input: RefCell<VecDeque<Result<Vec<u8>>>>,
}

impl HidTransport for MockTransport {
    fn send_feature_command(&self, payload: &[u8; 8]) -> Result<()> {
        self.sent.borrow_mut().push(*payload);
        Ok(())
    }
    fn query_feature_command(&self, payload: &[u8; 8], _timeout_ms: i32) -> Result<Vec<u8>> {
        self.send_feature_command(payload)?;
        self.input
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(LorgarError::Timeout))
    }
    fn read_input(&self, _timeout_ms: i32) -> Result<Option<Vec<u8>>> {
        self.input.borrow_mut().pop_front().transpose()
    }
}
