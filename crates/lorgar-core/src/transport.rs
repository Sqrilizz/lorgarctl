use crate::error::{LorgarError, Result};
use crate::protocol::{cmd, FeaturePacket, InputEvent, InputPacket};
use crate::storage::{runtime_dir, FileLock};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The receiver intermittently drops a state request instead of answering it.
/// Measured on 2026-10-07: two of ten single-shot `lorgarctl battery` queries
/// timed out against a mouse that the kernel driver was reading successfully at
/// the same moment. The kernel already retries three times at startup, so
/// user space retries the same number of times before reporting a timeout, at
/// the same 500 ms spacing the kernel uses between its startup attempts. The
/// spacing is deliberately not shorter: this device acknowledges a request with
/// a vendor event and then sometimes answers nothing at all, and a tight retry
/// burst is the most likely way to keep it in that state.
const QUERY_ATTEMPTS: usize = 3;
const QUERY_RETRY_DELAY_MS: u64 = 500;

/// Run one query attempt, retrying a dropped request with a short pause.
///
/// Only [`LorgarError::Timeout`] is retried. A transport error means the device
/// is gone, and repeating the request cannot change that, so it is returned
/// immediately instead of stretching the failure by two extra pauses.
pub fn with_query_retries<F>(mut attempt_once: F) -> Result<Vec<u8>>
where
    F: FnMut() -> Result<Vec<u8>>,
{
    let mut last = LorgarError::Timeout;
    for attempt in 1..=QUERY_ATTEMPTS {
        match attempt_once() {
            Ok(raw) => return Ok(raw),
            Err(LorgarError::Timeout) => {
                last = LorgarError::Timeout;
                if attempt < QUERY_ATTEMPTS {
                    std::thread::sleep(Duration::from_millis(QUERY_RETRY_DELAY_MS));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(last)
}

pub trait HidTransport: Send {
    fn send_feature_command(&self, payload: &[u8; 8]) -> Result<()>;
    fn send_feature_report(&self, report: &[u8]) -> Result<()>;
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

    fn query_once(&self, payload: &[u8; 8], timeout_ms: i32) -> Result<Vec<u8>> {
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
        self.send_feature_report(&report)
    }

    fn send_feature_report(&self, report: &[u8]) -> Result<()> {
        log::debug!("Feature report output: {:02x?}", report);
        self.device.send_feature_report(report)?;
        std::thread::sleep(Duration::from_millis(20));
        Ok(())
    }

    fn query_feature_command(&self, payload: &[u8; 8], timeout_ms: i32) -> Result<Vec<u8>> {
        if !(1..=5000).contains(&timeout_ms) {
            return Err(LorgarError::Config(
                "query timeout must be 1..5000 ms".into(),
            ));
        }
        with_query_retries(|| self.query_once(payload, timeout_ms))
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
    pub sent_reports: RefCell<Vec<Vec<u8>>>,
    pub input: RefCell<VecDeque<Result<Vec<u8>>>>,
}

impl HidTransport for MockTransport {
    fn send_feature_command(&self, payload: &[u8; 8]) -> Result<()> {
        self.sent.borrow_mut().push(*payload);
        Ok(())
    }
    fn send_feature_report(&self, report: &[u8]) -> Result<()> {
        self.sent_reports.borrow_mut().push(report.to_vec());
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
