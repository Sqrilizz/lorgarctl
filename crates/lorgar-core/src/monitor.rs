use crate::device::check_access;
use crate::protocol::{Command, InputEvent, InputPacket};
use crate::transport::HidapiTransport;
use crate::{HidTransport, LorgarDeviceInfo, LorgarError, Msa10w, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Input,
    FeatureOutput,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Connected {
        device: LorgarDeviceInfo,
    },
    Disconnected {
        reason: String,
    },
    BatteryChanged {
        percentage: u8,
    },
    DpiStageChanged {
        stage: u8,
        confidence: String,
    },
    Packet {
        device: LorgarDeviceInfo,
        direction: Direction,
        raw: Vec<u8>,
        parsed: Option<InputPacket>,
    },
    Warning {
        message: String,
    },
    ProfileApplied {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedEvent {
    pub timestamp_ms: u64,
    #[serde(flatten)]
    pub event: Event,
}

impl TimedEvent {
    pub fn new(event: Event) -> Self {
        Self {
            timestamp_ms: timestamp_ms(),
            event,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CachedState {
    pub device: Option<LorgarDeviceInfo>,
    pub battery_percentage: Option<u8>,
    pub battery_observed_ms: Option<u64>,
    pub active_stage: Option<u8>,
    pub applied_profile: Option<String>,
    pub last_error: Option<String>,
}

pub trait Session: Send {
    fn info(&self) -> &LorgarDeviceInfo;
    fn execute(&self, command: &Command) -> Result<()>;
    fn read(&self) -> Result<Vec<(LorgarDeviceInfo, Vec<u8>)>>;
    fn present(&self) -> Result<bool>;
    fn set_sleep_timeout(&self, minutes: u8, active_stage: u8) -> Result<()> {
        let _ = (minutes, active_stage);
        Ok(())
    }
}

pub trait Provider {
    fn open(&mut self) -> Result<Box<dyn Session>>;
}

pub struct HidProvider {
    pub path: Option<String>,
}

struct HidSession {
    device: Msa10w,
    vendor: Option<(LorgarDeviceInfo, HidapiTransport)>,
    pending_error: std::cell::RefCell<Option<LorgarError>>,
}

pub fn usb_parent(path: &str) -> Option<PathBuf> {
    let name = std::path::Path::new(path).file_name()?;
    let sys = std::fs::canonicalize(
        std::path::Path::new("/sys/class/hidraw")
            .join(name)
            .join("device"),
    )
    .ok()?;
    sys.ancestors()
        .find(|p| p.join("idVendor").exists())
        .map(PathBuf::from)
}

impl Provider for HidProvider {
    fn open(&mut self) -> Result<Box<dyn Session>> {
        let device = match &self.path {
            Some(path) => Msa10w::open_path(path)?,
            None => Msa10w::open()?,
        };
        let parent = usb_parent(&device.info().path);
        let vendor_info = Msa10w::list_devices()?
            .into_iter()
            .find(|d| d.usage_page == 0xff11 && parent.is_some() && usb_parent(&d.path) == parent);
        let vendor = if let Some(info) = vendor_info {
            check_access(&info.path)?;
            let api = hidapi::HidApi::new()?;
            let path = std::ffi::CString::new(info.path.as_str())
                .map_err(|_| LorgarError::Config("invalid HID path".into()))?;
            Some((info, HidapiTransport::new(api.open_path(&path)?)))
        } else {
            None
        };
        Ok(Box::new(HidSession {
            device,
            vendor,
            pending_error: std::cell::RefCell::default(),
        }))
    }
}

impl Session for HidSession {
    fn info(&self) -> &LorgarDeviceInfo {
        self.device.info()
    }
    fn execute(&self, command: &Command) -> Result<()> {
        self.device.execute(command)
    }
    fn set_sleep_timeout(&self, minutes: u8, active_stage: u8) -> Result<()> {
        self.device.set_sleep_timeout(minutes, active_stage)
    }
    fn read(&self) -> Result<Vec<(LorgarDeviceInfo, Vec<u8>)>> {
        if let Some(error) = self.pending_error.borrow_mut().take() {
            return Err(error);
        }
        let mut reports = Vec::new();
        for _ in 0..32 {
            let next = match self.device.read_input(0) {
                Ok(raw) => raw,
                Err(error) if reports.is_empty() => return Err(error),
                Err(error) => {
                    *self.pending_error.borrow_mut() = Some(error);
                    return Ok(reports);
                }
            };
            let Some(raw) = next else {
                break;
            };
            reports.push((self.info().clone(), raw));
        }
        if let Some((info, transport)) = &self.vendor {
            for _ in 0..32 {
                let next = match transport.read_input(0) {
                    Ok(raw) => raw,
                    Err(error) if reports.is_empty() => return Err(error),
                    Err(error) => {
                        *self.pending_error.borrow_mut() = Some(error);
                        return Ok(reports);
                    }
                };
                let Some(raw) = next else {
                    break;
                };
                reports.push((info.clone(), raw));
            }
        }
        Ok(reports)
    }
    fn present(&self) -> Result<bool> {
        Ok(Msa10w::list_devices()?
            .iter()
            .any(|d| d.path == self.info().path && d.is_config_interface))
    }
}

pub struct Monitor<P: Provider> {
    provider: P,
    session: Option<Box<dyn Session>>,
    pub state: CachedState,
    next_open_ms: u64,
    next_presence_ms: u64,
    next_query_ms: u64,
    pending_query_ms: Option<u64>,
    query_interval_ms: Option<u64>,
    initial: bool,
    clock_ms: u64,
    deferred_events: Vec<TimedEvent>,
}

impl<P: Provider> Monitor<P> {
    pub fn new(provider: P, query_interval_ms: Option<u64>) -> Self {
        Self {
            provider,
            session: None,
            state: CachedState::default(),
            next_open_ms: 0,
            next_presence_ms: 0,
            next_query_ms: 0,
            pending_query_ms: None,
            query_interval_ms,
            initial: true,
            clock_ms: 0,
            deferred_events: Vec::new(),
        }
    }

    fn disconnect(&mut self, now: u64, reason: String, events: &mut Vec<TimedEvent>) {
        self.session = None;
        self.state = CachedState {
            last_error: Some(reason.clone()),
            ..CachedState::default()
        };
        self.next_open_ms = now.saturating_add(1000);
        self.pending_query_ms = None;
        events.push(TimedEvent::new(Event::Disconnected { reason }));
    }

    pub fn set_sleep_timeout(&mut self, minutes: u8) -> Result<()> {
        let stage = self.state.active_stage.unwrap_or(1);
        let session = self.session.as_ref().ok_or(LorgarError::DeviceNotFound {
            vid: crate::VENDOR_ID,
            pid: crate::PRODUCT_IDS[1],
        })?;
        session.set_sleep_timeout(minutes, stage)
    }

    pub fn execute(&mut self, command: &Command) -> Result<TimedEvent> {
        let packet = command.packet()?;
        let session = self.session.as_ref().ok_or(LorgarError::DeviceNotFound {
            vid: crate::VENDOR_ID,
            pid: crate::PRODUCT_IDS[1],
        })?;
        session.execute(command)?;
        if matches!(command, Command::QueryState) {
            self.pending_query_ms.get_or_insert(self.clock_ms);
        }
        if let Command::SetActiveStage { stage } = command {
            self.state.active_stage = Some(*stage);
        }
        Ok(TimedEvent::new(Event::Packet {
            device: session.info().clone(),
            direction: Direction::FeatureOutput,
            raw: packet.report().to_vec(),
            parsed: None,
        }))
    }

    pub fn apply(
        &mut self,
        name: &str,
        profile: &crate::profiles::Profile,
    ) -> Result<Vec<TimedEvent>> {
        let commands = profile.commands()?;
        let mut events = Vec::new();
        self.state.applied_profile = None;
        for (index, command) in commands.iter().enumerate() {
            match self.execute(command) {
                Ok(event) => events.push(event),
                Err(error) => {
                    self.deferred_events.extend(events);
                    return Err(LorgarError::ProfileApply {
                        name: name.into(),
                        index,
                        source: Box::new(error),
                    });
                }
            }
        }
        if self.session.is_none() {
            return Err(LorgarError::DeviceNotFound {
                vid: crate::VENDOR_ID,
                pid: crate::PRODUCT_IDS[1],
            });
        }
        self.state.applied_profile = Some(name.into());
        events.push(TimedEvent::new(Event::ProfileApplied { name: name.into() }));
        Ok(events)
    }

    pub fn tick(&mut self, now: u64) -> Vec<TimedEvent> {
        self.clock_ms = now;
        let mut events = std::mem::take(&mut self.deferred_events);
        if self.session.is_none() && now >= self.next_open_ms {
            self.next_open_ms = now.saturating_add(1000);
            match self.provider.open() {
                Ok(session) => {
                    self.state = CachedState {
                        device: Some(session.info().clone()),
                        ..CachedState::default()
                    };
                    events.push(TimedEvent::new(Event::Connected {
                        device: session.info().clone(),
                    }));
                    self.session = Some(session);
                    self.next_query_ms = now;
                    self.next_presence_ms = now.saturating_add(1000);
                }
                Err(error) => {
                    let message = error.to_string();
                    if self.initial {
                        events.push(TimedEvent::new(Event::Disconnected {
                            reason: message.clone(),
                        }));
                    } else if self.state.last_error.as_ref() != Some(&message) {
                        events.push(TimedEvent::new(Event::Warning {
                            message: message.clone(),
                        }));
                    }
                    self.state.last_error = Some(message);
                }
            }
            self.initial = false;
        }
        let Some(session) = &self.session else {
            return events;
        };
        if now >= self.next_presence_ms {
            self.next_presence_ms = now.saturating_add(1000);
            match session.present() {
                Ok(true) => (),
                result => {
                    let reason = result
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "HID device removed".into());
                    self.disconnect(now, reason, &mut events);
                    return events;
                }
            }
        }
        if self.query_interval_ms.is_some() && now >= self.next_query_ms {
            match self.execute(&Command::QueryState) {
                Ok(event) => {
                    events.push(event);
                    self.pending_query_ms.get_or_insert(now);
                }
                Err(e) => {
                    self.disconnect(now, e.to_string(), &mut events);
                    return events;
                }
            }
            self.next_query_ms =
                now.saturating_add(self.query_interval_ms.unwrap_or(30000).max(1000));
        }
        let read = self.session.as_ref().map(|s| s.read());
        match read {
            Some(Ok(reports)) => {
                for (device, raw) in reports {
                    let page = if device.is_config_interface {
                        crate::CONFIG_USAGE_PAGE
                    } else {
                        device.usage_page
                    };
                    let parsed = InputPacket::parse_for_usage(page, &raw).ok();
                    if let Some(packet) = &parsed {
                        match packet.event {
                            InputEvent::State { battery_percentage }
                                if device.is_config_interface =>
                            {
                                self.pending_query_ms = None;
                                self.state.last_error = None;
                                if self.state.battery_percentage != Some(battery_percentage) {
                                    events.push(TimedEvent::new(Event::BatteryChanged {
                                        percentage: battery_percentage,
                                    }));
                                }
                                self.state.battery_percentage = Some(battery_percentage);
                                self.state.battery_observed_ms = Some(timestamp_ms());
                                if let Ok(stage) = crate::protocol::parse_active_dpi_stage(&raw) {
                                    if self.state.active_stage != Some(stage) {
                                        events.push(TimedEvent::new(Event::DpiStageChanged {
                                            stage,
                                            confidence: "high".into(),
                                        }));
                                    }
                                    self.state.active_stage = Some(stage);
                                }
                            }
                            InputEvent::DpiStageChanged { stage }
                                if device.usage_page == 0xff11 =>
                            {
                                if self.state.active_stage != Some(stage) {
                                    events.push(TimedEvent::new(Event::DpiStageChanged {
                                        stage,
                                        confidence: "high".into(),
                                    }));
                                }
                                self.state.active_stage = Some(stage);
                            }
                            _ => (),
                        }
                    }
                    events.push(TimedEvent::new(Event::Packet {
                        device,
                        direction: Direction::Input,
                        raw,
                        parsed,
                    }));
                }
            }
            Some(Err(e)) => self.disconnect(now, e.to_string(), &mut events),
            None => (),
        }
        if self
            .pending_query_ms
            .is_some_and(|sent| now.saturating_sub(sent) >= 2000)
        {
            self.pending_query_ms = None;
            self.state.battery_percentage = None;
            self.state.battery_observed_ms = None;
            let message = "state query timed out; HID presence does not establish wireless mouse connectivity".to_string();
            self.state.last_error = Some(message.clone());
            events.push(TimedEvent::new(Event::Warning { message }));
        }
        events
    }
}

pub fn shutdown_flag() -> Result<std::sync::Arc<std::sync::atomic::AtomicBool>> {
    let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stopped.clone();
    ctrlc::set_handler(move || flag.store(true, std::sync::atomic::Ordering::Relaxed))
        .map_err(|e| LorgarError::Transport(e.to_string()))?;
    Ok(stopped)
}
