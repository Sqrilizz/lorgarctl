use crate::protocol::Command;
use crate::storage::{atomic_save, FileLock};
use crate::{LightingMode, LorgarError, PollingRate, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub type Extensions = BTreeMap<String, Value>;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Profile {
    pub dpi_stages: BTreeMap<u8, u16>,
    pub active_stage: Option<u8>,
    pub polling: Option<u16>,
    pub rgb: Option<LightingMode>,
    pub color: Option<String>,
    #[serde(flatten)]
    pub extra: Extensions,
}

impl Profile {
    pub fn commands(&self) -> Result<Vec<Command>> {
        let mut commands = Vec::new();
        for (&stage, &dpi) in &self.dpi_stages {
            commands.push(Command::SetDpi { stage, dpi });
        }
        if let Some(hz) = self.polling {
            commands.push(Command::SetPolling {
                rate: PollingRate::from_hz(hz)?,
            });
        }
        if let Some(mode) = self.rgb {
            commands.push(Command::SetLightingMode { mode });
        }
        if let Some(color) = &self.color {
            let (red, green, blue) = crate::parse_hex_color(color)?;
            commands.push(Command::SetLightingColor { red, green, blue });
        }
        if let Some(stage) = self.active_stage {
            commands.push(Command::SetActiveStage { stage });
        }
        for command in &commands {
            command.packet()?;
        }
        Ok(commands)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppRule {
    pub process: String,
    pub profile: String,
    #[serde(flatten)]
    pub extra: Extensions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Applications {
    pub default: Option<String>,
    pub rules: Vec<AppRule>,
    #[serde(flatten)]
    pub extra: Extensions,
}

impl Applications {
    pub fn select(&self, process: Option<&str>) -> Option<&str> {
        self.rules
            .iter()
            .find(|rule| Some(rule.process.as_str()) == process)
            .map(|r| r.profile.as_str())
            .or(self.default.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub profiles: BTreeMap<String, Profile>,
    pub last_profile: Option<String>,
    pub restore_on_reconnect: bool,
    pub applications: Applications,
    #[serde(flatten)]
    pub extra: Extensions,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            profiles: BTreeMap::new(),
            last_profile: None,
            restore_on_reconnect: false,
            applications: Applications::default(),
            extra: Extensions::new(),
        }
    }
}

impl Config {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut value: Value = serde_json::from_slice(bytes)?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| LorgarError::Config("config must be an object".into()))?;
        if !object.contains_key("version") || object.get("version") == Some(&Value::from(0)) {
            object.insert("version".into(), Value::from(1));
        }
        Ok(serde_json::from_value(value)?)
    }

    pub fn writable(&self) -> Result<()> {
        if self.version != 1 {
            return Err(LorgarError::Config(format!(
                "config version {} is read-only; upgrade lorgarctl",
                self.version
            )));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.writable()?;
        for (name, profile) in &self.profiles {
            validate_name(name)?;
            profile.commands()?;
        }
        for name in self
            .last_profile
            .iter()
            .chain(self.applications.default.iter())
            .chain(self.applications.rules.iter().map(|r| &r.profile))
        {
            self.profile(name)?;
        }
        if self.applications.rules.iter().any(|r| r.process.is_empty()) {
            return Err(LorgarError::Config(
                "application process cannot be empty".into(),
            ));
        }
        Ok(())
    }

    pub fn profile(&self, name: &str) -> Result<&Profile> {
        self.profiles
            .get(name)
            .ok_or_else(|| LorgarError::Config(format!("profile '{name}' does not exist")))
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(LorgarError::Config(
            "profile names must contain 1..64 ASCII letters, digits, '_' or '-'".into(),
        ));
    }
    Ok(())
}

pub fn config_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(
            std::env::var_os("HOME")
                .ok_or_else(|| LorgarError::Config("HOME or XDG_CONFIG_HOME is required".into()))?,
        )
        .join(".config"),
    };
    if !base.is_absolute() {
        return Err(LorgarError::Config(
            "config directory must be absolute".into(),
        ));
    }
    Ok(base.join("lorgar/config.json"))
}

pub fn load(path: &Path) -> Result<Config> {
    match std::fs::read(path) {
        Ok(bytes) => Config::parse(&bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn update<T>(path: &Path, edit: impl FnOnce(&mut Config) -> Result<T>) -> Result<T> {
    let parent = path
        .parent()
        .ok_or_else(|| LorgarError::Config("config path requires parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let _lock = FileLock::acquire(&parent.join("config.lock"))?;
    let mut config = load(path)?;
    config.writable()?;
    let result = edit(&mut config)?;
    config.validate()?;
    atomic_save(path, &serde_json::to_vec_pretty(&config)?)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_and_unknown_fields_roundtrip() {
        let config = Config::parse(
            br#"{"profiles":{"gaming":{"polling":1000,"future":{"x":1}}},"new_setting":42}"#,
        )
        .unwrap();
        assert_eq!(config.version, 1);
        let encoded = serde_json::to_vec(&config).unwrap();
        assert_eq!(Config::parse(&encoded).unwrap(), config);
        assert_eq!(config.profiles["gaming"].extra["future"]["x"], 1);
        let future = Config::parse(br#"{"version":9}"#).unwrap();
        assert!(future.writable().is_err());
    }
    #[test]
    fn failed_edit_preserves_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        update(&path, |c| {
            c.profiles.insert("gaming".into(), Profile::default());
            Ok(())
        })
        .unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(update(&path, |c| {
            c.profiles.get_mut("gaming").unwrap().polling = Some(3);
            Ok(())
        })
        .is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    #[test]
    fn invalid_settings_and_rule_priority() {
        assert!(Profile {
            active_stage: Some(8),
            ..Profile::default()
        }
        .commands()
        .is_err());
        let apps = Applications {
            default: Some("desktop".into()),
            rules: vec![AppRule {
                process: "cs2".into(),
                profile: "gaming".into(),
                extra: Extensions::new(),
            }],
            ..Applications::default()
        };
        assert_eq!(apps.select(Some("cs2")), Some("gaming"));
        assert_eq!(apps.select(Some("other")), Some("desktop"));
    }
}
