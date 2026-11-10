use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_EDGE_MARGIN_PX: u32 = 16;
pub const DEFAULT_GAP_PX: u32 = 12;
pub const DEFAULT_MAX_VISIBLE: usize = 3;
pub(crate) const DEFAULT_DURATION_MS: u64 = 8_000;
pub(crate) const DEFAULT_TIMEOUT_SECONDS: u64 = 300;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct WindowDefaults {
    pub default_width_px: u32,
    pub auto_min_height_px: u32,
    pub auto_max_height_px: u32,
    pub fallback_height_px: u32,
}

impl WindowDefaults {
    pub(crate) const NOTIFICATION: Self = Self {
        default_width_px: 360,
        auto_min_height_px: 120,
        auto_max_height_px: 480,
        fallback_height_px: 220,
    };
    pub(crate) const DIALOG: Self = Self {
        default_width_px: 440,
        auto_min_height_px: 160,
        auto_max_height_px: 720,
        fallback_height_px: 300,
    };
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NotificationConfig {
    #[serde(flatten)]
    pub window: WindowDefaults,
    pub edge_margin_px: u32,
    pub gap_px: u32,
    pub max_visible: usize,
    pub default_duration_ms: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DialogConfig {
    #[serde(flatten)]
    pub window: WindowDefaults,
    pub default_timeout_seconds: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub notification: NotificationConfig,
    pub dialog: DialogConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            notification: NotificationConfig {
                window: WindowDefaults::NOTIFICATION,
                edge_margin_px: DEFAULT_EDGE_MARGIN_PX,
                gap_px: DEFAULT_GAP_PX,
                max_visible: DEFAULT_MAX_VISIBLE,
                default_duration_ms: DEFAULT_DURATION_MS,
            },
            dialog: DialogConfig {
                window: WindowDefaults::DIALOG,
                default_timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ConfigStatus {
    File {
        path: PathBuf,
    },
    Defaults {
        path: Option<PathBuf>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

pub struct LoadedConfig {
    pub settings: Config,
    pub status: ConfigStatus,
}

impl Default for LoadedConfig {
    fn default() -> Self {
        Self {
            settings: Config::default(),
            status: ConfigStatus::Defaults {
                path: None,
                error: None,
            },
        }
    }
}

impl LoadedConfig {
    pub fn load() -> Self {
        match default_path() {
            Ok(path) => Self::load_path(path),
            Err(error) => Self::fallback(None, Some(error.to_string())),
        }
    }

    fn load_path(path: PathBuf) -> Self {
        match fs::read_to_string(&path) {
            Ok(text) => match parse(&text) {
                Ok(settings) => Self {
                    settings,
                    status: ConfigStatus::File { path },
                },
                Err(error) => Self::fallback(Some(path), Some(error)),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Self::fallback(Some(path), None)
            }
            Err(error) => Self::fallback(Some(path), Some(error.to_string())),
        }
    }

    fn fallback(path: Option<PathBuf>, error: Option<String>) -> Self {
        if let Some(error) = &error {
            let location = path
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "path".into());
            eprintln!("Popu configuration ({location}): {error}. Using built-in defaults.");
        }
        Self {
            settings: Config::default(),
            status: ConfigStatus::Defaults { path, error },
        }
    }
}

pub(crate) fn default_path() -> io::Result<PathBuf> {
    #[cfg(windows)]
    let directory = std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("Popu"));
    #[cfg(not(windows))]
    let directory = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".config")))
        .map(|path| path.join("popu"));
    directory
        .filter(|path| path.is_absolute())
        .map(|path| path.join("config.toml"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "user configuration directory unavailable",
            )
        })
}

pub(crate) fn validate_file(path: &Path) -> Result<(), String> {
    fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|text| parse(&text))
        .map(|_| ())
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn parse(text: &str) -> Result<Config, String> {
    let overrides: toml::Table = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut merged = toml::Table::try_from(Config::default()).map_err(|error| error.to_string())?;
    for (section, value) in overrides {
        match (
            merged.get_mut(&section).and_then(toml::Value::as_table_mut),
            value,
        ) {
            (Some(defaults), toml::Value::Table(overrides)) => defaults.extend(overrides),
            (_, value) => {
                merged.insert(section, value);
            }
        }
    }

    let schema = serde_json::from_str(include_str!("../../protocol/config.schema.json"))
        .map_err(|error| format!("invalid embedded configuration schema: {error}"))?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|error| format!("invalid embedded configuration schema: {error}"))?;
    let instance = serde_json::to_value(&merged).map_err(|error| error.to_string())?;
    let errors: Vec<_> = validator
        .iter_errors(&instance)
        .map(|error| {
            let path = error.instance_path();
            format!("configuration{path}: {error}")
        })
        .collect();
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }

    let config: Config = merged
        .try_into()
        .map_err(|error: toml::de::Error| error.to_string())?;
    for (section, window) in [
        ("notification", config.notification.window),
        ("dialog", config.dialog.window),
    ] {
        if !(window.auto_min_height_px..=window.auto_max_height_px)
            .contains(&window.fallback_height_px)
        {
            return Err(format!(
                "{section}: expected auto_min_height_px <= fallback_height_px <= auto_max_height_px (got {} <= {} <= {})",
                window.auto_min_height_px, window.fallback_height_px, window.auto_max_height_px,
            ));
        }
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_merges_with_built_in_defaults() {
        let config = parse(
            "[notification]\nmax_visible = 5\ngap_px = 0\n[dialog]\ndefault_timeout_seconds = 15",
        )
        .unwrap();
        assert_eq!(config.notification.max_visible, 5);
        assert_eq!(config.notification.gap_px, 0);
        assert_eq!(config.notification.window.default_width_px, 360);
        assert_eq!(config.dialog.default_timeout_seconds, 15);
        assert_eq!(config.dialog.window.fallback_height_px, 300);
    }

    #[test]
    fn rejects_invalid_config_and_reports_the_field_or_section() {
        for (text, field) in [
            ("[other]\nmax_visible = 1", "other"),
            ("[notification]\ngapp_px = 1", "gapp_px"),
            ("[notification]\nmax_visible = 0", "max_visible"),
            ("[notification]\nedge_margin_px = 257", "edge_margin_px"),
            ("[notification]\ngap_px = \"12\"", "gap_px"),
            ("[notification]\ngap_px = 1.0", "notification"),
            ("[notification]\ngap_px = 1\ngap_px = 2", "duplicate"),
            (
                "[notification]\ndefault_duration_ms = 499",
                "default_duration_ms",
            ),
            (
                "[dialog]\ndefault_timeout_seconds = 86401",
                "default_timeout_seconds",
            ),
            ("[dialog]\ndefault_width_px = 8193", "default_width_px"),
            (
                "[notification]\nauto_max_height_px = 180",
                "fallback_height_px",
            ),
        ] {
            let error = parse(text).unwrap_err();
            assert!(error.contains(field), "{text}: {error}");
        }
    }

    #[test]
    fn invalid_file_uses_all_defaults_and_missing_file_is_normal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let missing = LoadedConfig::load_path(path.clone());
        assert!(matches!(
            missing.status,
            ConfigStatus::Defaults { error: None, .. }
        ));

        fs::write(
            &path,
            "[notification]\ngap_px = 0\nauto_max_height_px = 180",
        )
        .unwrap();
        let invalid = LoadedConfig::load_path(path.clone());
        assert!(matches!(
            invalid.status,
            ConfigStatus::Defaults { error: Some(_), .. }
        ));
        assert_eq!(invalid.settings.notification.gap_px, DEFAULT_GAP_PX);

        fs::write(&path, "[notification]\ngap_px = 0").unwrap();
        let valid = LoadedConfig::load_path(path);
        assert!(matches!(valid.status, ConfigStatus::File { .. }));
        assert_eq!(valid.settings.notification.gap_px, 0);
    }
}
