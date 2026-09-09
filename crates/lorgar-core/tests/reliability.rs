use lorgar_core::monitor::{Event, Monitor, Provider, Session};
use lorgar_core::protocol::{Command, FeaturePacket, InputEvent, InputPacket};
use lorgar_core::transport::MockTransport;
use lorgar_core::{
    HidTransport, LightingMode, LorgarDeviceInfo, LorgarError, Msa10w, PollingRate, Result,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

fn info() -> LorgarDeviceInfo {
    LorgarDeviceInfo {
        vendor_id: 0x093a,
        product_id: 0xc601,
        path: "/dev/mock".into(),
        serial_number: None,
        manufacturer_string: None,
        product_string: None,
        usage_page: 0xff10,
        usage: 1,
        interface_number: 2,
        is_config_interface: true,
    }
}

fn state(percentage: u8) -> Vec<u8> {
    let mut raw = vec![0; 64];
    raw[..16].copy_from_slice(&[
        1, 1, 5, 0x80, 1, 0, 0, percentage, 1, 0x19, 0x40, 0x1f, 0x10, 0x27, 0xe0, 0x2e,
    ]);
    raw
}

#[test]
fn all_documented_commands_roundtrip_and_report_ids() {
    let mut commands = Vec::new();
    for (stage, dpi) in [(0, 800), (1, 1400), (1, 1600), (1, 3000)] {
        commands.push(Command::SetDpi { stage, dpi });
    }
    for stage in 0..8 {
        commands.push(Command::SetActiveStage { stage });
    }
    for rate in [
        PollingRate::Hz125,
        PollingRate::Hz250,
        PollingRate::Hz500,
        PollingRate::Hz1000,
    ] {
        commands.push(Command::SetPolling { rate });
    }
    for mode in [
        LightingMode::Static,
        LightingMode::Breathing,
        LightingMode::ColorCycle,
        LightingMode::Tapping,
        LightingMode::Wave,
    ] {
        commands.push(Command::SetLightingMode { mode });
    }
    commands.extend([
        Command::SetLightingColor {
            red: 0xaa,
            green: 0xbb,
            blue: 0xcc,
        },
        Command::QueryState,
    ]);
    for command in commands {
        let packet = command.packet().unwrap();
        assert_eq!(
            FeaturePacket::parse(&packet.raw).unwrap().command,
            Some(command)
        );
        assert_eq!(packet.report()[0], 0);
        assert_eq!(&packet.report()[1..], &packet.raw);
    }
    assert_eq!(
        Command::QueryState.packet().unwrap().raw,
        [0x82, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        Command::SetActiveStage { stage: 3 }.packet().unwrap().raw,
        [0x21, 0, 0, 3, 0, 0, 0, 0]
    );
}

#[test]
fn unknown_fields_and_malformed_reports_remain_lossless() {
    assert_eq!(
        InputPacket::parse_for_usage(0xff10, &state(85))
            .unwrap()
            .report_id,
        None
    );
    assert_eq!(
        InputPacket::parse_for_usage(0xff11, &[5, 15, 1, 0, 53])
            .unwrap()
            .report_id,
        Some(5)
    );
    for raw in [
        [0x27, 0, 1, 255, 0, 0, 0, 0],
        [0x20, 9, 0, 1, 0, 0, 0, 0],
        [0x21, 0, 0, 255, 0, 0, 0, 0],
    ] {
        let packet = FeaturePacket::parse(&raw).unwrap();
        assert_eq!(packet.raw, raw);
        assert!(packet.command.is_none());
    }
    for size in [0, 1, 7, 9, 64] {
        assert!(FeaturePacket::parse(&vec![0; size]).is_err());
    }
    for raw in [
        vec![5, 0x0f, 0, 0, 0x7d],
        vec![5, 0x0f, 1, 0x50, 1],
        vec![5, 0x0b, 1, 4, 9],
        vec![5, 0x0b, 8, 0, 0],
    ] {
        let packet = InputPacket::parse(&raw).unwrap();
        assert!(matches!(packet.event, InputEvent::UnknownVendor { .. }));
        assert_eq!(packet.raw, raw);
    }
    assert_eq!(
        InputPacket::parse(&[5, 0x0b, 1, 0, 0]).unwrap().event,
        InputEvent::DpiStageChanged { stage: 1 }
    );
    assert_eq!(
        InputPacket::parse(&state(85)).unwrap().event,
        InputEvent::State {
            battery_percentage: 85
        }
    );
    assert!(lorgar_core::parse_battery_percentage(&state(101)).is_err());
    assert!(lorgar_core::parse_battery_percentage(&state(85)[..8]).is_err());
    assert!(InputPacket::parse(&[]).is_err());
}

#[test]
fn recorded_hardware_evidence_remains_inspectable_and_unknown() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/captures");
    for (name, count) in [
        ("2026-09-05-query-1000ms.jsonl", 8),
        ("2026-09-05-query-1500ms.jsonl", 6),
    ] {
        let (header, events) = lorgar_core::capture::read_capture(&root.join(name)).unwrap();
        assert_eq!(header.version, 1);
        let vendor: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.event {
                Event::Packet { raw, device, .. } if device.usage_page == 0xff11 => Some(raw),
                _ => None,
            })
            .collect();
        assert_eq!(vendor.len(), count);
        for raw in vendor {
            assert_eq!(raw.as_slice(), &[5, 15, 1, 0, 53]);
            assert!(matches!(
                InputPacket::parse(raw).unwrap().event,
                InputEvent::UnknownVendor { event_code: 15 }
            ));
        }
    }
}

#[test]
fn every_single_byte_input_is_safe() {
    for byte in 0..=255 {
        assert_eq!(InputPacket::parse(&[byte]).unwrap().raw, [byte]);
    }
    for value in ["€€", "ééé", "💥ab", "ab💥", "#€€", "gggggg"] {
        assert!(lorgar_core::parse_hex_color(value).is_err());
    }
}

#[test]
fn mock_device_does_not_infer_charging_or_undocumented_settings() {
    let mock = MockTransport::default();
    mock.input.borrow_mut().push_back(Ok(state(85)));
    let mut device_info = info();
    device_info.product_id = 0xc600;
    let device = Msa10w::from_transport(device_info, Box::new(mock));
    let status = device.get_status().unwrap();
    assert_eq!(status.battery_percentage, 85);
    assert_eq!(status.is_charging, None);
    assert_eq!(status.active_stage, None);
    assert_eq!(status.polling_rate, None);
    assert_eq!(status.lighting_mode, None);
    assert_eq!(&status.raw_response[..], state(85));
}

#[test]
fn mock_transport_propagates_disconnect_and_timeout() {
    let mock = MockTransport::default();
    assert!(matches!(
        mock.query_feature_command(&[0; 8], 100),
        Err(LorgarError::Timeout)
    ));
    mock.input
        .borrow_mut()
        .push_back(Err(LorgarError::Transport("unplugged".into())));
    assert!(mock.read_input(0).is_err());
    assert_eq!(mock.sent.borrow().len(), 1);
}

#[test]
fn discovery_rejects_conflicting_pages_and_virtual_batteries() {
    assert!(lorgar_core::device::is_config_interface(0xff10, 2));
    assert!(lorgar_core::device::is_config_interface(0, 2));
    assert!(!lorgar_core::device::is_config_interface(0xff11, 2));
    assert!(!lorgar_core::device::is_config_interface(0x85, -1));
}

#[derive(Default)]
struct Shared {
    present: bool,
    fail_write: bool,
    reads: VecDeque<Vec<u8>>,
    writes: usize,
    opens: usize,
}
struct TestSession {
    shared: Arc<Mutex<Shared>>,
    info: LorgarDeviceInfo,
}
impl Session for TestSession {
    fn info(&self) -> &LorgarDeviceInfo {
        &self.info
    }
    fn execute(&self, _: &Command) -> Result<()> {
        let mut state = self.shared.lock().unwrap();
        if state.fail_write {
            return Err(LorgarError::Transport("disconnected".into()));
        }
        state.writes += 1;
        Ok(())
    }
    fn read(&self) -> Result<Vec<(LorgarDeviceInfo, Vec<u8>)>> {
        Ok(self
            .shared
            .lock()
            .unwrap()
            .reads
            .drain(..)
            .map(|raw| (self.info.clone(), raw))
            .collect())
    }
    fn present(&self) -> Result<bool> {
        Ok(self.shared.lock().unwrap().present)
    }
}
struct TestProvider(Arc<Mutex<Shared>>);
impl Provider for TestProvider {
    fn open(&mut self) -> Result<Box<dyn Session>> {
        let mut shared = self.0.lock().unwrap();
        shared.opens += 1;
        if !shared.present {
            return Err(LorgarError::DeviceNotFound {
                vid: 0x093a,
                pid: 0xc601,
            });
        }
        Ok(Box::new(TestSession {
            shared: self.0.clone(),
            info: info(),
        }))
    }
}

#[test]
fn reconnect_backoff_cache_invalidation_and_recovery() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut monitor = Monitor::new(TestProvider(shared.clone()), Some(30000));
    assert!(matches!(
        monitor.tick(0)[0].event,
        Event::Disconnected { .. }
    ));
    for now in 1..1000 {
        assert!(monitor.tick(now).is_empty());
    }
    assert_eq!(shared.lock().unwrap().opens, 1);
    shared.lock().unwrap().present = true;
    shared.lock().unwrap().reads.push_back(state(85));
    assert!(monitor
        .tick(1000)
        .iter()
        .any(|e| matches!(e.event, Event::Connected { .. })));
    assert_eq!(monitor.state.battery_percentage, Some(85));
    assert_eq!(shared.lock().unwrap().writes, 1);
    shared.lock().unwrap().present = false;
    assert!(monitor
        .tick(2000)
        .iter()
        .any(|e| matches!(e.event, Event::Disconnected { .. })));
    assert!(monitor.state.battery_percentage.is_none());
    shared.lock().unwrap().present = true;
    assert!(monitor.tick(2999).is_empty());
    assert!(monitor
        .tick(3000)
        .iter()
        .any(|e| matches!(e.event, Event::Connected { .. })));
}

#[test]
fn response_timeout_does_not_claim_receiver_removed() {
    let shared = Arc::new(Mutex::new(Shared {
        present: true,
        ..Shared::default()
    }));
    let mut monitor = Monitor::new(TestProvider(shared), Some(30000));
    monitor.tick(0);
    assert!(monitor
        .tick(2000)
        .iter()
        .any(|e| matches!(e.event, Event::Warning { .. })));
    assert!(monitor.state.device.is_some());
    assert!(monitor.state.battery_percentage.is_none());
}

#[test]
fn invalid_profile_never_writes_and_partial_apply_is_reported() {
    let shared = Arc::new(Mutex::new(Shared {
        present: true,
        ..Shared::default()
    }));
    let mut monitor = Monitor::new(TestProvider(shared.clone()), None);
    monitor.tick(0);
    let profile = lorgar_core::profiles::Profile {
        polling: Some(999),
        ..Default::default()
    };
    assert!(monitor.apply("bad", &profile).is_err());
    assert_eq!(shared.lock().unwrap().writes, 0);
    let profile = lorgar_core::profiles::Profile {
        polling: Some(1000),
        ..Default::default()
    };
    monitor.apply("good", &profile).unwrap();
    assert_eq!(monitor.state.applied_profile.as_deref(), Some("good"));
    shared.lock().unwrap().fail_write = true;
    assert!(monitor
        .apply("good", &profile)
        .unwrap_err()
        .to_string()
        .contains("earlier writes may have applied"));
    assert_eq!(monitor.state.applied_profile, None);
}
