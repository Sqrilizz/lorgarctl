use crate::monitor::{CachedState, TimedEvent};
use crate::protocol::Command;
use crate::{LorgarError, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Status,
    Devices,
    Execute { command: Command },
    SetSleepTimeout { minutes: u8 },
    ApplyProfile { name: String },
    ApplyDraft { profile: crate::profiles::Profile },
    Events { after: u64 },
    Reload,
    GetDpiTable,
    SetDpiStages { stages: Vec<u16> },
    GetButtonMappings { secondary: bool },
    DumpMemory,
    DumpFlash { addr: u16, len: usize },
    SetLod { mm: u8 },
    SetDebounce { ms: u16, rapid_fire: bool },
    SetAngleSnapping { enabled: bool },
    SetMotionSync { enabled: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencedEvent {
    pub sequence: u64,
    #[serde(flatten)]
    pub event: TimedEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub version: u32,
    pub ok: bool,
    pub error: Option<String>,
    pub error_code: Option<u8>,
    pub state: Option<CachedState>,
    pub devices: Vec<crate::LorgarDeviceInfo>,
    pub events: Vec<SequencedEvent>,
    pub gap: bool,
    pub cursor: u64,
    pub config_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi_stages: Option<Vec<u16>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_colors: Option<Vec<[u8; 3]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button_mappings: Option<Vec<crate::firmware::ButtonMapping>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_dump: Option<crate::firmware::OnboardMemoryDump>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flash_dump: Option<Vec<u8>>,
}

impl Default for Response {
    fn default() -> Self {
        Self {
            version: VERSION,
            ok: true,
            error: None,
            error_code: None,
            state: None,
            devices: Vec::new(),
            events: Vec::new(),
            gap: false,
            cursor: 0,
            config_path: None,
            dpi_stages: None,
            stage_colors: None,
            button_mappings: None,
            memory_dump: None,
            flash_dump: None,
        }
    }
}

pub struct EventLog {
    events: VecDeque<SequencedEvent>,
    sequence: u64,
    capacity: usize,
}

impl EventLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::new(),
            sequence: 0,
            capacity: capacity.max(1),
        }
    }
    pub fn push(&mut self, event: TimedEvent) {
        self.sequence += 1;
        if self.events.len() == self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(SequencedEvent {
            sequence: self.sequence,
            event,
        });
    }
    pub fn since(&self, after: u64) -> Response {
        let events: Vec<_> = self
            .events
            .iter()
            .filter(|e| e.sequence > after)
            .take(16)
            .cloned()
            .collect();
        let cursor = events.last().map(|e| e.sequence).unwrap_or(self.sequence);
        let gap = after > self.sequence
            || self
                .events
                .front()
                .is_some_and(|e| after.saturating_add(1) < e.sequence);
        Response {
            events,
            gap,
            cursor,
            ..Response::default()
        }
    }
}

pub fn socket_path() -> Result<PathBuf> {
    Ok(crate::storage::runtime_dir()?.join("lorgard.sock"))
}

pub fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_FRAME {
        return Err(LorgarError::Config("IPC frame too large".into()));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let len = u32::from_le_bytes(length) as usize;
    if len > MAX_FRAME {
        return Err(LorgarError::Config("IPC frame too large".into()));
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn request(path: &Path, action: Action) -> Result<Response> {
    let mut stream = UnixStream::connect(path).map_err(|e| {
        LorgarError::Transport(format!(
            "cannot connect to lorgard at {}: {e}; start lorgard or use direct mode",
            path.display()
        ))
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    write_frame(
        &mut stream,
        &Request {
            version: VERSION,
            action,
        },
    )?;
    let response: Response = read_timed_frame(&mut stream, Duration::from_secs(5))?;
    if response.version != VERSION {
        return Err(LorgarError::Config(
            "daemon protocol version mismatch".into(),
        ));
    }
    Ok(response)
}

struct DeadlineReader<'a> {
    stream: &'a mut UnixStream,
    deadline: std::time::Instant,
}

impl Read for DeadlineReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self
            .deadline
            .saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "IPC deadline exceeded",
            ));
        }
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(bytes)
    }
}

pub fn read_timed_frame<T: DeserializeOwned>(
    stream: &mut UnixStream,
    timeout: Duration,
) -> Result<T> {
    read_frame(&mut DeadlineReader {
        stream,
        deadline: std::time::Instant::now() + timeout,
    })
}

pub fn exit_code(error: &LorgarError) -> u8 {
    match error {
        LorgarError::Remote { code, .. } => *code,
        LorgarError::ProfileApply { source, .. } => exit_code(source),
        LorgarError::DeviceNotFound { .. } | LorgarError::ConfigInterfaceNotFound { .. } => 3,
        LorgarError::PermissionDenied { .. } => 4,
        LorgarError::DeviceBusy => 5,
        LorgarError::Timeout => 6,
        LorgarError::Config(_)
        | LorgarError::Json(_)
        | LorgarError::InvalidStage { .. }
        | LorgarError::InvalidDpi { .. }
        | LorgarError::InvalidPollingRate { .. }
        | LorgarError::InvalidColorFormat { .. }
        | LorgarError::HexParseError(_)
        | LorgarError::InvalidFeatureLength { .. } => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_and_limits() {
        let request = Request {
            version: 1,
            action: Action::Execute {
                command: Command::QueryState,
            },
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &request).unwrap();
        let decoded: Request = read_frame(&mut bytes.as_slice()).unwrap();
        assert!(matches!(
            decoded.action,
            Action::Execute {
                command: Command::QueryState
            }
        ));
        assert!(read_frame::<Request>(&mut &u32::MAX.to_le_bytes()[..]).is_err());
        bytes.pop();
        assert!(read_frame::<Request>(&mut bytes.as_slice()).is_err());
    }
    #[test]
    fn new_actions_and_responses_roundtrip() {
        let req = Request {
            version: VERSION,
            action: Action::GetButtonMappings { secondary: true },
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &req).unwrap();
        let decoded: Request = read_frame(&mut bytes.as_slice()).unwrap();
        assert!(matches!(
            decoded.action,
            Action::GetButtonMappings { secondary: true }
        ));

        let resp = Response {
            button_mappings: Some(vec![crate::firmware::ButtonMapping {
                button_index: 0,
                name: "Button 1".into(),
                raw: [0xf0, 0, 0, 2],
                action: crate::firmware::ButtonAction::LeftClick,
            }]),
            dpi_stages: Some(vec![800, 1600, 2400]),
            memory_dump: Some(crate::firmware::OnboardMemoryDump {
                banks: vec![crate::firmware::MemoryBank {
                    opcode: 0xa4,
                    name: "test".into(),
                    data: vec![1, 2, 3],
                }],
                total_bytes: 3,
            }),
            ..Response::default()
        };
        let mut rbytes = Vec::new();
        write_frame(&mut rbytes, &resp).unwrap();
        let rdecoded: Response = read_frame(&mut rbytes.as_slice()).unwrap();
        assert_eq!(rdecoded.dpi_stages, Some(vec![800, 1600, 2400]));
        assert_eq!(
            rdecoded.button_mappings.as_ref().unwrap()[0].name,
            "Button 1"
        );
        assert_eq!(rdecoded.memory_dump.as_ref().unwrap().total_bytes, 3);
    }
    #[test]
    fn lagging_subscriber_gets_explicit_gap() {
        let mut log = EventLog::new(2);
        for _ in 0..4 {
            log.push(TimedEvent::new(crate::monitor::Event::Disconnected {
                reason: "test".into(),
            }));
        }
        assert!(log.since(0).gap);
        assert_eq!(log.since(0).events.len(), 2);
        assert!(!log.since(3).gap);
        assert!(log.since(20).gap);
    }
}
