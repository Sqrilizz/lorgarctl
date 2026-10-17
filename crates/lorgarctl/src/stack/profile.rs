use crate::Cli;
use clap::Subcommand;
use colored::*;
use lorgar_core::ipc::Action;
use lorgar_core::profiles::{self, Profile};
use lorgar_core::protocol::Command;
use lorgar_core::{LorgarError, Result};
use serde_json::json;
use std::io::Write;

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    #[command(about = "Create an empty named profile")]
    Create { name: String },
    #[command(about = "Update selected settings, preserving all others")]
    Set {
        name: String,
        #[arg(long, value_name = "STAGE=DPI")]
        dpi_stage: Vec<String>,
        #[arg(long)]
        active_stage: Option<u8>,
        #[arg(long)]
        polling: Option<u16>,
        #[arg(long, value_parser = crate::parse_lighting_mode)]
        rgb: Option<lorgar_core::LightingMode>,
        #[arg(long, value_name = "RRGGBB")]
        color: Option<String>,
    },
    #[command(about = "Send confirmed commands and remember the profile name")]
    Apply { name: String },
    #[command(about = "List saved profile names")]
    List,
    #[command(about = "Show settings and preserved extension fields")]
    Show { name: String },
    #[command(about = "Delete a profile unless application rules reference it")]
    Delete { name: String },
    #[command(about = "Print the XDG configuration file path")]
    Path,
}

pub fn apply_dpi_preset(cli: &Cli, name: &str) -> Result<()> {
    let (stages, active): (Vec<(u8, u16)>, u8) = match name.to_ascii_lowercase().as_str() {
        "fps" | "cs" | "shooter" => (vec![(0, 400), (1, 800), (2, 1600), (3, 3200)], 1),
        "balanced" | "standard" | "default" => (vec![(0, 800), (1, 1200), (2, 1600), (3, 2400)], 1),
        "high" | "speed" | "fast" => (vec![(0, 1600), (1, 2400), (2, 3200), (3, 6400)], 2),
        _ => {
            return Err(LorgarError::Config(format!(
                "unknown preset '{name}'. Supported presets: fps, balanced, high"
            )))
        }
    };

    for (stage, dpi) in &stages {
        super::common::execute(
            cli,
            Command::SetDpi {
                stage: *stage,
                dpi: *dpi,
            },
        )?;
    }
    super::common::execute(cli, Command::SetActiveStage { stage: active })?;

    if !cli.json {
        let stages_str = stages
            .iter()
            .map(|(s, d)| {
                if *s == active {
                    format!("{s}: {d} DPI (active)")
                } else {
                    format!("{s}: {d} DPI")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            std::io::stdout().lock(),
            "{}",
            format!("Applied DPI preset '{name}': [{stages_str}]")
                .green()
                .bold()
        )?;
        return Ok(());
    }

    super::common::output(
        &json!({
            "ok": true,
            "preset": name,
            "active_stage": active,
            "stages": stages.iter().map(|(s, d)| json!({"stage": s, "dpi": d})).collect::<Vec<_>>(),
        }),
        cli.json,
    )
}

pub fn profile(cli: &Cli, sub: &ProfileCommand) -> Result<()> {
    let path = cli
        .config
        .clone()
        .map(Ok)
        .unwrap_or_else(profiles::config_path)?;
    match sub {
        ProfileCommand::Path => super::common::output(&json!({"path": path}), cli.json),
        ProfileCommand::List => super::common::output(
            &profiles::load(&path)?.profiles.keys().collect::<Vec<_>>(),
            cli.json,
        ),
        ProfileCommand::Show { name } => {
            super::common::output(profiles::load(&path)?.profile(name)?, cli.json)
        }
        ProfileCommand::Apply { name } => {
            if super::common::is_daemon_mode(cli) {
                super::common::response(cli, Action::ApplyProfile { name: name.clone() })?;
            } else {
                let config = profiles::load(&path)?;
                config.writable()?;
                let commands = config.profile(name)?.commands()?;
                let device = crate::open_device(cli.device.as_deref(), false)?;
                for (index, command) in commands.iter().enumerate() {
                    device
                        .execute(command)
                        .map_err(|source| LorgarError::ProfileApply {
                            name: name.clone(),
                            index,
                            source: Box::new(source),
                        })?;
                }
                profiles::update(&path, |config| {
                    config.last_profile = Some(name.clone());
                    Ok(())
                })
                .map_err(|e| {
                    LorgarError::Config(format!(
                        "profile applied, but saving last profile failed: {e}"
                    ))
                })?;
            }
            super::common::output(
                &json!({"ok": true, "profile": name, "hardware_readback_verified": false}),
                cli.json,
            )
        }
        _ => {
            profiles::update(&path, |config| {
                match sub {
                    ProfileCommand::Create { name } => {
                        profiles::validate_name(name)?;
                        if config.profiles.contains_key(name) {
                            return Err(LorgarError::Config(format!(
                                "profile '{name}' already exists"
                            )));
                        }
                        config.profiles.insert(name.clone(), Profile::default());
                    }
                    ProfileCommand::Delete { name } => {
                        config.profile(name)?;
                        if config.applications.default.as_ref() == Some(name)
                            || config.applications.rules.iter().any(|r| &r.profile == name)
                        {
                            return Err(LorgarError::Config(
                                "profile is referenced by application rules; update those rules first"
                                    .into(),
                            ));
                        }
                        config.profiles.remove(name);
                        if config.last_profile.as_ref() == Some(name) {
                            config.last_profile = None;
                        }
                    }
                    ProfileCommand::Set {
                        name,
                        dpi_stage,
                        active_stage,
                        polling,
                        rgb,
                        color,
                    } => {
                        let profile = config.profiles.get_mut(name).ok_or_else(|| {
                            LorgarError::Config(format!("profile '{name}' does not exist"))
                        })?;
                        for value in dpi_stage {
                            let (stage, dpi) = value.split_once('=').ok_or_else(|| {
                                LorgarError::Config(
                                    "--dpi-stage requires STAGE=DPI, e.g. 0=800".into(),
                                )
                            })?;
                            let stage = stage
                                .parse::<u8>()
                                .map_err(|_| LorgarError::Config("invalid stage index".into()))?;
                            let dpi = dpi
                                .parse::<u16>()
                                .map_err(|_| LorgarError::Config("invalid DPI value".into()))?;
                            profile.dpi_stages.insert(stage, dpi);
                        }
                        if let Some(stage) = active_stage {
                            profile.active_stage = Some(*stage);
                        }
                        if let Some(polling) = polling {
                            profile.polling = Some(*polling);
                        }
                        if let Some(rgb) = rgb {
                            profile.rgb = Some(*rgb);
                        }
                        if let Some(color) = color {
                            profile.color = Some(color.clone());
                        }
                    }
                    _ => (),
                }
                Ok(())
            })?;
            super::common::output(&json!({"ok": true, "config": path}), cli.json)
        }
    }
}
