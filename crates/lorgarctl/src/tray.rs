//! Legacy system-tray integration (`--features tray`, on by default).
//!
//! Kept for compatibility with existing `lorgarctl tray` users. The `ksni`
//! backend pulls a duplicate legacy tree (clap v2 / bitflags v1); lean builds
//! should use `cargo build -p lorgarctl --no-default-features`. While the tray
//! runs it owns direct HID; do not run it together with `lorgard`.

use lorgar_core::Result;

#[cfg(feature = "tray")]
use colored::*;

#[cfg(feature = "tray")]
pub struct LorgarTray {
    pub battery_percentage: Option<u8>,
    pub active_stage: Option<u8>,
    pub polling_rate_str: String,
    pub rgb_mode_str: String,
    pub stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(feature = "tray")]
impl ksni::Tray for LorgarTray {
    fn icon_name(&self) -> String {
        "input-mouse".to_string()
    }

    fn title(&self) -> String {
        match self.battery_percentage {
            Some(percentage) => format!("LORGAR MSA10W ({percentage}%)"),
            None => "LORGAR MSA10W (unknown)".into(),
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.title(),
            description: format!(
                "Stage: {:?} | Polling: {} | RGB: {}",
                self.active_stage, self.polling_rate_str, self.rgb_mode_str
            ),
            icon_name: "input-mouse".to_string(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: match self.battery_percentage {
                    Some(percentage) => format!("Battery: {percentage}%"),
                    None => "Battery: unknown".into(),
                },
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("Active stage: {:?}", self.active_stage),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("Polling rate: {}", self.polling_rate_str),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("RGB mode: {}", self.rgb_mode_str),
                enabled: false,
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".to_string(),
                activate: Box::new(|tray: &mut LorgarTray| {
                    tray.stopped
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn run(
    device: Option<&str>,
    debug: bool,
    open: &dyn Fn(Option<&str>, bool) -> Result<lorgar_core::Msa10w>,
    notify: &dyn Fn(&str, &str, bool),
    wait_or_stop: &dyn Fn(&std::sync::atomic::AtomicBool, u64),
) -> Result<()> {
    #[cfg(not(feature = "tray"))]
    {
        let _ = (device, debug, open, notify, wait_or_stop);
        eprintln!("lorgarctl: tray support was not enabled at compile time");
        Err(lorgar_core::LorgarError::Config(
            "tray feature disabled".into(),
        ))
    }

    #[cfg(feature = "tray")]
    {
        println!(
            "{}",
            "Starting LORGAR MSA10W System Tray Daemon..."
                .bold()
                .green()
        );
        println!("Icon registered in System Tray. Low battery notifications active.");

        let stopped = crate::shutdown_flag()?;
        let initial_status = open(device, debug).and_then(|dev| dev.get_status()).ok();

        let initial_bat = initial_status.as_ref().map(|s| s.battery_percentage);
        let initial_stage = initial_status.as_ref().and_then(|s| s.active_stage);
        let initial_poll = initial_status
            .as_ref()
            .and_then(|s| s.polling_rate)
            .map(|r| format!("{} Hz", r.hz()))
            .unwrap_or_else(|| "N/A".into());
        let initial_rgb = initial_status
            .as_ref()
            .and_then(|s| s.lighting_mode)
            .map(|m| format!("{m:?}"))
            .unwrap_or_else(|| "N/A".into());

        let tray = LorgarTray {
            battery_percentage: initial_bat,
            active_stage: initial_stage,
            polling_rate_str: initial_poll,
            rgb_mode_str: initial_rgb,
            stopped: stopped.clone(),
        };

        let service = ksni::TrayService::new(tray);
        let handle = service.handle();
        service.spawn();
        let mut low_notified = false;

        while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
            wait_or_stop(&stopped, 120);
            if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }

            match open(device, false).and_then(|dev| dev.get_status()) {
                Ok(st) => {
                    let bat = st.battery_percentage;
                    let stage = st.active_stage;
                    let poll = st
                        .polling_rate
                        .map(|r| format!("{} Hz", r.hz()))
                        .unwrap_or_else(|| "N/A".into());
                    let rgb = st
                        .lighting_mode
                        .map(|m| format!("{m:?}"))
                        .unwrap_or_else(|| "N/A".into());

                    if bat <= 20 && !low_notified {
                        let is_critical = bat <= 10;
                        notify(
                            &format!("LORGAR MSA10W battery low ({bat}%)"),
                            &format!("Mouse battery level is at {bat}%. Please connect charger."),
                            is_critical,
                        );
                        low_notified = true;
                    } else if bat > 20 {
                        low_notified = false;
                    }

                    handle.update(move |t: &mut LorgarTray| {
                        t.battery_percentage = Some(bat);
                        t.active_stage = stage;
                        t.polling_rate_str = poll;
                        t.rgb_mode_str = rgb;
                    });
                }
                Err(_) => {
                    handle.update(|t: &mut LorgarTray| {
                        t.battery_percentage = None;
                        t.active_stage = None;
                    });
                }
            }
        }
        handle.shutdown();
        Ok(())
    }
}

#[cfg(all(test, feature = "tray"))]
mod tests {
    use super::*;

    #[test]
    fn tray_never_fabricates_zero_battery() {
        let mut tray = LorgarTray {
            battery_percentage: None,
            active_stage: None,
            polling_rate_str: "N/A".into(),
            rgb_mode_str: "N/A".into(),
            stopped: Default::default(),
        };
        assert_eq!(ksni::Tray::title(&tray), "LORGAR MSA10W (unknown)");
        tray.battery_percentage = Some(57);
        assert_eq!(ksni::Tray::title(&tray), "LORGAR MSA10W (57%)");
    }
}
