//! User preferences — theme, accent, close behavior, tray, retention.
//!
//! Storage policy: a single JSON file under the XDG config home
//! (`$XDG_CONFIG_HOME/openrize/settings.json`, or `~/.config/openrize/...`).
//! Preferences are config, not data: they survive deleting the app-data
//! directory, and they are portable between machines the way `.dotfiles` are.
//!
//! The file is written atomically (temp + rename) and a corrupt file is moved
//! aside rather than silently reset, matching `timers.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::timers::now_epoch_ms;

const FILE_NAME: &str = "settings.json";

/// 0 means keep everything. Anything else is a day count, clamped to a decade
/// so a typo cannot ask the sweeper to walk a table for a century.
pub const MAX_RETENTION_DAYS: u32 = 3650;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Accent {
    #[default]
    Green,
    Blue,
    Purple,
    Orange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CloseBehavior {
    Quit,
    #[default]
    Hide,
}

/// What the AI suggests for each entry (Rize's "Suggestion level").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AiSuggest {
    Category,
    #[default]
    CategoryProject,
}

/// Auto-accept thresholds below this would approve too many wrong entries to
/// be useful; the UI offers 85-99.
pub const MIN_AUTO_ACCEPT_PERCENT: u8 = 50;
pub const MAX_AUTO_ACCEPT_PERCENT: u8 = 100;
const MAX_CUSTOM_PROMPT_CHARS: usize = 1_000;

/// Every field is `#[serde(default)]`, so a settings file written by an older
/// build loads with new fields defaulted instead of failing to parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub accent: Accent,
    /// What the window's close button does while the tray is enabled.
    pub close_behavior: CloseBehavior,
    pub tray_enabled: bool,
    /// Activity history older than this many days is deleted. 0 = forever.
    pub retention_days: u32,
    pub ai_suggest: AiSuggest,
    /// Approve an entry without review when every suggested field clears
    /// `auto_accept_percent`.
    pub auto_accept: bool,
    pub auto_accept_percent: u8,
    /// Appended to the Foundation Model's instructions.
    pub ai_custom_prompt: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            accent: Accent::default(),
            close_behavior: CloseBehavior::Hide,
            tray_enabled: true,
            retention_days: 0,
            ai_suggest: AiSuggest::CategoryProject,
            auto_accept: true,
            auto_accept_percent: 95,
            ai_custom_prompt: String::new(),
        }
    }
}

impl Settings {
    /// The only place a value is allowed in. Keeps persistence and validation
    /// in one spot so a command can never write something unloadable.
    fn normalized(mut self) -> Self {
        self.retention_days = self.retention_days.min(MAX_RETENTION_DAYS);
        self.auto_accept_percent = self
            .auto_accept_percent
            .clamp(MIN_AUTO_ACCEPT_PERCENT, MAX_AUTO_ACCEPT_PERCENT);
        if self.ai_custom_prompt.chars().count() > MAX_CUSTOM_PROMPT_CHARS {
            self.ai_custom_prompt = self
                .ai_custom_prompt
                .chars()
                .take(MAX_CUSTOM_PROMPT_CHARS)
                .collect();
        }
        self
    }
}

/// `$XDG_CONFIG_HOME/openrize`, falling back to `$HOME/.config/openrize`.
///
/// Deliberately not Tauri's `app_config_dir`: on macOS that lands under
/// `~/Library/Application Support`, and this app keeps its preferences in the
/// conventional `.config` location on every platform.
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("openrize")
}

pub struct SettingsStore {
    path: PathBuf,
    settings: Settings,
}

impl SettingsStore {
    pub fn load(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|error| format!("could not create config dir: {error}"))?;
        let path = dir.join(FILE_NAME);

        let settings = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .unwrap_or_else(|| {
                if path.exists() {
                    let quarantine = path.with_extension(format!("corrupt-{}", now_epoch_ms()));
                    let _ = fs::rename(&path, &quarantine);
                    eprintln!(
                        "settings.json was unreadable; moved to {}",
                        quarantine.display()
                    );
                }
                Settings::default()
            });

        Ok(Self {
            path,
            settings: settings.normalized(),
        })
    }

    pub fn snapshot(&self) -> Settings {
        self.settings.clone()
    }

    pub fn set(&mut self, next: Settings) -> Result<Settings, String> {
        self.settings = next.normalized();
        self.persist()?;
        Ok(self.snapshot())
    }

    fn persist(&self) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(&self.settings).map_err(|e| e.to_string())?;
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, json).map_err(|error| format!("could not write settings: {error}"))?;
        fs::rename(&temp, &self.path)
            .map_err(|error| format!("could not replace settings: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openrize-settings-{name}-{}", now_epoch_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_round_trip() {
        let dir = temp_dir("defaults");
        let store = SettingsStore::load(&dir).unwrap();
        assert_eq!(store.snapshot(), Settings::default());
    }

    #[test]
    fn values_survive_a_reload() {
        let dir = temp_dir("reload");
        {
            let mut store = SettingsStore::load(&dir).unwrap();
            store
                .set(Settings {
                    theme: Theme::Light,
                    accent: Accent::Orange,
                    close_behavior: CloseBehavior::Quit,
                    tray_enabled: false,
                    retention_days: 30,
                    ai_suggest: AiSuggest::Category,
                    auto_accept: false,
                    auto_accept_percent: 90,
                    ai_custom_prompt: "OpenRize is Coding".to_string(),
                })
                .unwrap();
        }
        let reloaded = SettingsStore::load(&dir).unwrap().snapshot();
        assert_eq!(reloaded.theme, Theme::Light);
        assert_eq!(reloaded.accent, Accent::Orange);
        assert_eq!(reloaded.close_behavior, CloseBehavior::Quit);
        assert!(!reloaded.tray_enabled);
        assert_eq!(reloaded.retention_days, 30);
        assert_eq!(reloaded.ai_suggest, AiSuggest::Category);
        assert!(!reloaded.auto_accept);
        assert_eq!(reloaded.auto_accept_percent, 90);
        assert_eq!(reloaded.ai_custom_prompt, "OpenRize is Coding");
    }

    #[test]
    fn auto_accept_threshold_is_clamped() {
        let dir = temp_dir("threshold");
        let mut store = SettingsStore::load(&dir).unwrap();
        let saved = store
            .set(Settings {
                auto_accept_percent: 5,
                ..Settings::default()
            })
            .unwrap();
        assert_eq!(saved.auto_accept_percent, MIN_AUTO_ACCEPT_PERCENT);
    }

    #[test]
    fn retention_is_clamped() {
        let dir = temp_dir("clamp");
        let mut store = SettingsStore::load(&dir).unwrap();
        let saved = store
            .set(Settings {
                retention_days: u32::MAX,
                ..Settings::default()
            })
            .unwrap();
        assert_eq!(saved.retention_days, MAX_RETENTION_DAYS);
    }

    #[test]
    fn a_corrupt_file_falls_back_to_defaults() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join(FILE_NAME), b"{ not json").unwrap();
        let store = SettingsStore::load(&dir).unwrap();
        assert_eq!(store.snapshot(), Settings::default());
        // The bad file is preserved under a quarantine name, not deleted.
        assert!(fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains("corrupt")));
    }
}
