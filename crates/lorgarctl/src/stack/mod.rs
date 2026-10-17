pub mod capture;
pub mod common;
pub mod control;
pub mod doctor;
pub mod menu;
pub mod profile;

pub use common::is_daemon_mode;
pub use profile::ProfileCommand;

use crate::{Cli, Commands};
use clap::CommandFactory;
use lorgar_core::capture as core_capture;
use lorgar_core::Result;
use serde_json::json;

pub fn run(cli: &Cli) -> Option<Result<()>> {
    Some(match &cli.command {
        Commands::Menu => menu::run_menu(cli),
        Commands::Monitor => capture::watch(cli, None),
        Commands::Capture {
            output,
            duration,
            query_state,
            query_interval_ms,
            label,
        } => capture::watch(
            cli,
            Some((output, *duration, *query_state, label, *query_interval_ms)),
        ),
        Commands::DiffCaptures { before, after } => (|| {
            let (_, before) = core_capture::read_capture(before)?;
            let (_, after) = core_capture::read_capture(after)?;
            common::output(
                &json!({
                    "alignment": "ordinal within VID/PID/interface/direction/report/length streams; insertion can shift alignment",
                    "offsets_include_report_id": true,
                    "streams": core_capture::diff(&before, &after)
                }),
                cli.json,
            )
        })(),
        Commands::Profile { sub } => profile::profile(cli, sub),
        Commands::Completions { shell } => {
            clap_complete::generate(
                *shell,
                &mut Cli::command(),
                "lorgarctl",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Commands::Doctor => doctor::doctor(cli),
        Commands::InspectCapture { path } => (|| {
            let (header, events) = core_capture::read_capture(path)?;
            common::output(
                &json!({"type": "capture_header", "header": header}),
                cli.json,
            )?;
            for event in events {
                capture::event_output(&event, cli.json)?;
            }
            Ok(())
        })(),
        Commands::Tray | Commands::Upower | Commands::Debug { .. }
            if !is_daemon_mode(cli) && !cli.json =>
        {
            return None
        }
        _ => control::control(cli),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn command_tree_and_compatibility() {
        Cli::command().debug_assert();
        for args in [
            vec!["lorgarctl", "status"],
            vec!["lorgarctl", "sleep"],
            vec!["lorgarctl", "sleep", "5"],
            vec!["lorgarctl", "dpi"],
            vec!["lorgarctl", "dpi", "list"],
            vec!["lorgarctl", "dpi", "preset", "fps"],
            vec!["lorgarctl", "polling"],
            vec!["lorgarctl", "rgb"],
            vec!["lorgarctl", "--direct", "status"],
            vec!["lorgarctl", "battery", "--json"],
            vec!["lorgarctl", "monitor", "--json"],
            vec!["lorgarctl", "--daemon", "dpi", "set", "1", "1600"],
            vec!["lorgarctl", "dpi", "1", "1400"],
            vec![
                "lorgarctl",
                "profile",
                "set",
                "gaming",
                "--dpi-stage",
                "0=800",
                "--rgb",
                "static",
                "--color",
                "ff5500",
            ],
            vec!["lorgarctl", "lod", "1"],
            vec!["lorgarctl", "debounce", "8"],
            vec!["lorgarctl", "angle-snap", "on"],
            vec!["lorgarctl", "motion-sync", "off"],
            vec![
                "lorgarctl",
                "firmware",
                "info",
                "crates/lorgarctl/src/main.rs",
            ],
            vec!["lorgarctl", "firmware", "bootloader"],
            vec!["lorgarctl", "firmware", "dump", "--len", "64"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }

    #[test]
    fn unicode_hex_never_panics() {
        assert!(crate::hex::parse_hex_payload(&["€€".into()]).is_err());
        assert!(lorgar_core::parse_hex_color("€€").is_err());
        assert!(lorgar_core::protocol::InputPacket::parse(&[]).is_err());
    }
}
