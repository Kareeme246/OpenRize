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
/// The work week the Calendar, My Timesheet, and Timesheets measure against.
pub const DEFAULT_WEEKLY_TARGET_HOURS: u16 = 40;
pub const MAX_WEEKLY_TARGET_HOURS: u16 = 168;

pub const DEFAULT_TRACKING_START: &str = "07:00";
pub const DEFAULT_TRACKING_END: &str = "19:00";

fn parse_time_minutes(s: &str) -> Option<u32> {
    let mut parts = s.split(':');
    let h: u32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    if h < 24 && m < 60 && parts.next().is_none() {
        Some(h * 60 + m)
    } else {
        None
    }
}

fn normalize_time_str(s: &str, default_val: &str) -> String {
    if let Some(mins) = parse_time_minutes(s) {
        format!("{:02}:{:02}", mins / 60, mins % 60)
    } else {
        default_val.to_string()
    }
}

pub fn is_time_in_window(hour: u32, minute: u32, start_str: &str, end_str: &str) -> bool {
    let start = match parse_time_minutes(start_str) {
        Some(mins) => mins,
        None => return false,
    };
    let end = match parse_time_minutes(end_str) {
        Some(mins) => mins,
        None => return false,
    };

    if start == end {
        return false;
    }

    let current = hour * 60 + minute;
    if start < end {
        current >= start && current < end
    } else {
        current >= start || current < end
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySchedule {
    pub enabled: bool,
    pub start: String,
    pub end: String,
}

impl Default for DaySchedule {
    fn default() -> Self {
        Self {
            enabled: true,
            start: DEFAULT_TRACKING_START.to_string(),
            end: DEFAULT_TRACKING_END.to_string(),
        }
    }
}

impl DaySchedule {
    pub fn normalized(mut self) -> Self {
        self.start = normalize_time_str(&self.start, DEFAULT_TRACKING_START);
        self.end = normalize_time_str(&self.end, DEFAULT_TRACKING_END);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingHours {
    pub enabled: bool,
    pub per_day: bool,
    pub default_start: String,
    pub default_end: String,
    pub monday: DaySchedule,
    pub tuesday: DaySchedule,
    pub wednesday: DaySchedule,
    pub thursday: DaySchedule,
    pub friday: DaySchedule,
    pub saturday: DaySchedule,
    pub sunday: DaySchedule,
}

impl Default for TrackingHours {
    fn default() -> Self {
        Self {
            enabled: true,
            per_day: false,
            default_start: DEFAULT_TRACKING_START.to_string(),
            default_end: DEFAULT_TRACKING_END.to_string(),
            monday: DaySchedule::default(),
            tuesday: DaySchedule::default(),
            wednesday: DaySchedule::default(),
            thursday: DaySchedule::default(),
            friday: DaySchedule::default(),
            saturday: DaySchedule::default(),
            sunday: DaySchedule::default(),
        }
    }
}

impl TrackingHours {
    pub fn normalized(mut self) -> Self {
        self.default_start = normalize_time_str(&self.default_start, DEFAULT_TRACKING_START);
        self.default_end = normalize_time_str(&self.default_end, DEFAULT_TRACKING_END);
        self.monday = self.monday.normalized();
        self.tuesday = self.tuesday.normalized();
        self.wednesday = self.wednesday.normalized();
        self.thursday = self.thursday.normalized();
        self.friday = self.friday.normalized();
        self.saturday = self.saturday.normalized();
        self.sunday = self.sunday.normalized();

        if !self.per_day {
            let single = DaySchedule {
                enabled: true,
                start: self.default_start.clone(),
                end: self.default_end.clone(),
            };
            self.monday = single.clone();
            self.tuesday = single.clone();
            self.wednesday = single.clone();
            self.thursday = single.clone();
            self.friday = single.clone();
            self.saturday = single.clone();
            self.sunday = single;
        }

        self
    }

    /// Whether the timestamp (in epoch milliseconds) is inside the configured tracking window.
    /// Respects the local system timezone and DST.
    pub fn is_inside_window(&self, now_ms: u64) -> bool {
        use chrono::{DateTime, Datelike, Local, Timelike, Weekday};

        if !self.enabled {
            return true;
        }

        let dt = match DateTime::from_timestamp_millis(now_ms as i64) {
            Some(utc) => utc.with_timezone(&Local),
            None => return true,
        };

        let (start_str, end_str) = if self.per_day {
            let day = match dt.weekday() {
                Weekday::Mon => &self.monday,
                Weekday::Tue => &self.tuesday,
                Weekday::Wed => &self.wednesday,
                Weekday::Thu => &self.thursday,
                Weekday::Fri => &self.friday,
                Weekday::Sat => &self.saturday,
                Weekday::Sun => &self.sunday,
            };
            if !day.enabled {
                return false;
            }
            (&day.start, &day.end)
        } else {
            (&self.default_start, &self.default_end)
        };

        is_time_in_window(dt.hour(), dt.minute(), start_str, end_str)
    }
}

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
    /// Expected work hours per week. A day's target is a fifth of it.
    pub weekly_target_hours: u16,
    pub tracking_hours: TrackingHours,
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
            weekly_target_hours: DEFAULT_WEEKLY_TARGET_HOURS,
            tracking_hours: TrackingHours::default(),
        }
    }
}

impl Settings {
    /// The only place a value is allowed in. Keeps persistence and validation
    /// in one spot so a command can never write something unloadable.
    fn normalized(mut self) -> Self {
        self.retention_days = self.retention_days.min(MAX_RETENTION_DAYS);
        self.weekly_target_hours = self.weekly_target_hours.clamp(1, MAX_WEEKLY_TARGET_HOURS);
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
        self.tracking_hours = self.tracking_hours.normalized();
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
                    weekly_target_hours: 32,
                    tracking_hours: TrackingHours {
                        enabled: true,
                        per_day: true,
                        default_start: "08:00".to_string(),
                        default_end: "17:00".to_string(),
                        monday: DaySchedule {
                            enabled: true,
                            start: "09:00".to_string(),
                            end: "18:00".to_string(),
                        },
                        tuesday: DaySchedule {
                            enabled: false,
                            start: "07:00".to_string(),
                            end: "19:00".to_string(),
                        },
                        wednesday: DaySchedule::default(),
                        thursday: DaySchedule::default(),
                        friday: DaySchedule::default(),
                        saturday: DaySchedule::default(),
                        sunday: DaySchedule::default(),
                    },
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
        assert_eq!(reloaded.weekly_target_hours, 32);
        assert!(reloaded.tracking_hours.per_day);
        assert_eq!(reloaded.tracking_hours.monday.start, "09:00");
        assert!(!reloaded.tracking_hours.tuesday.enabled);
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
    fn weekly_target_is_clamped() {
        let dir = temp_dir("target");
        let mut store = SettingsStore::load(&dir).unwrap();
        let zero = store
            .set(Settings {
                weekly_target_hours: 0,
                ..Settings::default()
            })
            .unwrap();
        assert_eq!(zero.weekly_target_hours, 1);
        let huge = store
            .set(Settings {
                weekly_target_hours: 500,
                ..Settings::default()
            })
            .unwrap();
        assert_eq!(huge.weekly_target_hours, MAX_WEEKLY_TARGET_HOURS);
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

    #[test]
    fn test_is_time_in_window_daytime() {
        // 07:00 to 19:00
        assert!(!is_time_in_window(6, 59, "07:00", "19:00"));
        assert!(is_time_in_window(7, 0, "07:00", "19:00"));
        assert!(is_time_in_window(12, 30, "07:00", "19:00"));
        assert!(is_time_in_window(18, 59, "07:00", "19:00"));
        assert!(!is_time_in_window(19, 0, "07:00", "19:00"));
        assert!(!is_time_in_window(23, 0, "07:00", "19:00"));
    }

    #[test]
    fn test_is_time_in_window_zero_length() {
        // Zero length window means no capture
        assert!(!is_time_in_window(7, 0, "07:00", "07:00"));
        assert!(!is_time_in_window(12, 0, "07:00", "07:00"));
    }

    #[test]
    fn test_is_time_in_window_overnight() {
        // 22:00 to 06:00
        assert!(!is_time_in_window(21, 59, "22:00", "06:00"));
        assert!(is_time_in_window(22, 0, "22:00", "06:00"));
        assert!(is_time_in_window(23, 59, "22:00", "06:00"));
        assert!(is_time_in_window(0, 0, "22:00", "06:00"));
        assert!(is_time_in_window(5, 59, "22:00", "06:00"));
        assert!(!is_time_in_window(6, 0, "22:00", "06:00"));
        assert!(!is_time_in_window(12, 0, "22:00", "06:00"));
    }

    #[test]
    fn test_tracking_hours_collapse_syncs_all_days() {
        let mut th = TrackingHours {
            per_day: true,
            default_start: "08:00".to_string(),
            default_end: "16:00".to_string(),
            monday: DaySchedule {
                enabled: true,
                start: "10:00".to_string(),
                end: "20:00".to_string(),
            },
            ..TrackingHours::default()
        };
        assert_eq!(th.monday.start, "10:00");
        // Collapsing to single window:
        th.per_day = false;
        let normalized = th.normalized();
        assert_eq!(normalized.monday.start, "08:00");
        assert_eq!(normalized.monday.end, "16:00");
        assert_eq!(normalized.sunday.start, "08:00");
        assert_eq!(normalized.sunday.end, "16:00");
    }

    #[test]
    fn test_tracking_hours_disabled_day() {
        use chrono::{Local, TimeZone};
        let th = TrackingHours {
            enabled: true,
            per_day: true,
            default_start: "07:00".to_string(),
            default_end: "19:00".to_string(),
            monday: DaySchedule {
                enabled: false,
                start: "07:00".to_string(),
                end: "19:00".to_string(),
            },
            ..TrackingHours::default()
        };
        // Construct a Monday at 12:00 local time
        let dt = Local
            .with_ymd_and_hms(2026, 9, 28, 12, 0, 0)
            .single()
            .expect("local dt"); // 2026-09-28 is a Monday
        let ms = dt.timestamp_millis() as u64;
        assert!(!th.is_inside_window(ms));
    }
}
