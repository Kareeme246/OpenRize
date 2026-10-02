//! User preferences - theme, accent, size mode, close behavior, tray, retention.
//!
//! Storage policy: a single JSON file under the XDG config home
//! (`$XDG_CONFIG_HOME/openrize/settings.json`, or `~/.config/openrize/...`).
//! Preferences are config, not data: they survive deleting the app-data
//! directory, and they are portable between machines the way `.dotfiles` are.
//!
//! The file is written atomically (temp + rename) and a corrupt file is moved
//! aside rather than silently reset, matching `timers.rs`.

use std::collections::BTreeMap;
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
    System,
    Light,
    #[default]
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
pub enum SizeMode {
    Compact,
    #[default]
    Normal,
    Relaxed,
    VeryRelaxed,
}

/// How panels are drawn: spaced rounded cards, or edge-to-edge sections split
/// by plain dividers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Shape {
    #[default]
    Rounded,
    Sharper,
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

pub const DEFAULT_WORK_MINUTES: u16 = 50;
pub const DEFAULT_BREAK_MINUTES: u16 = 5;
pub const DEFAULT_SNOOZE_MINUTES: u16 = 5;
/// The snooze lengths the reminder's chevron offers; the default is one of them.
pub const SNOOZE_CHOICES: [u16; 3] = [5, 10, 15];
pub const MAX_BREAK_MESSAGE_CHARS: usize = 120;
pub const MAX_SCHEDULED_BREAKS: usize = 12;
const MAX_SCHEDULE_LABEL_CHARS: usize = 40;

/// Days a scheduled break repeats on. Interval reminders have no days of
/// their own: tracking hours alone decide when they fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Weekdays {
    pub mon: bool,
    pub tue: bool,
    pub wed: bool,
    pub thu: bool,
    pub fri: bool,
    pub sat: bool,
    pub sun: bool,
}

impl Default for Weekdays {
    fn default() -> Self {
        Self {
            mon: true,
            tue: true,
            wed: true,
            thu: true,
            fri: true,
            sat: false,
            sun: false,
        }
    }
}

impl Weekdays {
    /// Whether the schedule repeats on `weekday`.
    pub fn includes(&self, weekday: chrono::Weekday) -> bool {
        use chrono::Weekday::{Fri, Mon, Sat, Sun, Thu, Tue, Wed};
        match weekday {
            Mon => self.mon,
            Tue => self.tue,
            Wed => self.wed,
            Thu => self.thu,
            Fri => self.fri,
            Sat => self.sat,
            Sun => self.sun,
        }
    }
}

/// A fixed-time, recurring break such as "Lunch 12:30, 45 min, weekdays".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScheduledBreak {
    pub id: String,
    pub label: String,
    /// `HH:MM`, local time.
    pub at: String,
    pub minutes: u16,
    pub days: Weekdays,
    pub enabled: bool,
}

impl Default for ScheduledBreak {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: "Break".to_string(),
            at: "12:30".to_string(),
            minutes: 30,
            days: Weekdays::default(),
            enabled: true,
        }
    }
}

impl ScheduledBreak {
    /// Minutes after local midnight the break is due.
    pub fn at_minutes(&self) -> u32 {
        parse_time_minutes(&self.at).unwrap_or(0)
    }

    fn normalized(mut self) -> Self {
        if self.id.trim().is_empty() {
            self.id = uuid::Uuid::now_v7().to_string();
        }
        let label: String = self
            .label
            .trim()
            .chars()
            .take(MAX_SCHEDULE_LABEL_CHARS)
            .collect();
        self.label = if label.is_empty() {
            "Break".to_string()
        } else {
            label
        };
        self.at = normalize_time_str(&self.at, "12:30");
        self.minutes = self.minutes.clamp(1, 180);
        self
    }
}

/// Break reminders (Settings > Notifications). Interval reminders fire after
/// `work_minutes` of continuous work; scheduled breaks fire at fixed times.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BreakSettings {
    pub enabled: bool,
    pub work_minutes: u16,
    pub break_minutes: u16,
    /// The reminder's snooze button snoozes for this long; its chevron offers
    /// every length in `SNOOZE_CHOICES`.
    pub snooze_minutes: u16,
    /// An optional extra line on the reminder. Empty shows nothing.
    pub message: String,
    /// Pause running stopwatches for a break and resume them after it.
    pub pause_stopwatches: bool,
    pub chime: bool,
    pub schedules: Vec<ScheduledBreak>,
}

impl Default for BreakSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            work_minutes: DEFAULT_WORK_MINUTES,
            break_minutes: DEFAULT_BREAK_MINUTES,
            snooze_minutes: DEFAULT_SNOOZE_MINUTES,
            message: String::new(),
            pause_stopwatches: false,
            chime: false,
            schedules: Vec::new(),
        }
    }
}

impl BreakSettings {
    fn normalized(mut self) -> Self {
        self.work_minutes = self.work_minutes.clamp(15, 180);
        self.break_minutes = self.break_minutes.clamp(1, 60);
        if !SNOOZE_CHOICES.contains(&self.snooze_minutes) {
            self.snooze_minutes = DEFAULT_SNOOZE_MINUTES;
        }
        let message: String = self
            .message
            .trim()
            .chars()
            .take(MAX_BREAK_MESSAGE_CHARS)
            .collect();
        // Cutting at the limit can leave a trailing space; normalizing must
        // be idempotent or the value changes on every reload.
        self.message = message.trim_end().to_string();
        self.schedules.truncate(MAX_SCHEDULED_BREAKS);
        self.schedules = self
            .schedules
            .into_iter()
            .map(ScheduledBreak::normalized)
            .collect();
        self
    }
}

/// Every field is `#[serde(default)]`, so a settings file written by an older
/// build loads with new fields defaulted instead of failing to parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub accent: Accent,
    pub shape: Shape,
    pub size_mode: SizeMode,
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
    pub breaks: BreakSettings,
    /// Extensions (agent bridges) the person switched on or off by hand,
    /// keyed by extension id. An id with no entry follows auto-detection: on
    /// when the tool is installed. An explicit value, on or off, persists and
    /// is never overridden by detection.
    pub extensions: BTreeMap<String, bool>,
    /// Advanced workflow tracking: the experimental master switch for coding
    /// agent tracking (the agent bridge, jobs, threads and agent time). Off by
    /// default; while off nothing runs, nothing new is recorded, and every
    /// surface for it is hidden. What was recorded before is kept.
    pub advanced_workflow_tracking: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            accent: Accent::default(),
            shape: Shape::default(),
            size_mode: SizeMode::default(),
            close_behavior: CloseBehavior::Hide,
            tray_enabled: true,
            retention_days: 0,
            ai_suggest: AiSuggest::CategoryProject,
            auto_accept: true,
            auto_accept_percent: 95,
            ai_custom_prompt: String::new(),
            weekly_target_hours: DEFAULT_WEEKLY_TARGET_HOURS,
            tracking_hours: TrackingHours::default(),
            breaks: BreakSettings::default(),
            extensions: BTreeMap::new(),
            advanced_workflow_tracking: false,
        }
    }
}

/// Extension ids a settings file may carry; anything else is dropped.
pub const EXTENSION_IDS: [&str; 2] = ["herdr", "tmux"];

impl Settings {
    /// Whether an extension runs: the person's explicit choice, else whether
    /// its tool was detected.
    pub fn extension_enabled(&self, id: &str, detected: bool) -> bool {
        self.extensions.get(id).copied().unwrap_or(detected)
    }

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
        self.breaks = self.breaks.normalized();
        self.extensions
            .retain(|id, _| EXTENSION_IDS.contains(&id.as_str()));
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

/// Runs in every webview before the page's own scripts, and seeds the
/// `localStorage` copy of the saved theme and shape that `index.html` paints
/// from. Without it a user upgrading from a build that had no copy (or a fresh
/// profile) gets the defaults on the first frame and only switches once the
/// settings arrive over IPC. A copy that already exists is left alone: it is
/// kept current by `applyAppearance` in `src/lib/settings.ts`, which owns the
/// key names. Read-only, so a damaged file is still quarantined by `load`.
pub fn appearance_init_script(dir: &Path) -> String {
    let settings = fs::read(dir.join(FILE_NAME))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .unwrap_or_default();
    // The enums serialize to quoted JSON strings, which are valid JS literals.
    let theme = serde_json::to_string(&settings.theme).unwrap_or_default();
    let shape = serde_json::to_string(&settings.shape).unwrap_or_default();
    format!(
        "try {{ const seed = (key, value) => {{ if (localStorage.getItem(key) === null) localStorage.setItem(key, value); }}; seed(\"openrize.theme\", {theme}); seed(\"openrize.shape\", {shape}); }} catch (_) {{}}"
    )
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

    /// Sets one field by its camelCase path (`breaks.enabled`), leaving every
    /// other field as stored, so a change made elsewhere a moment earlier is
    /// never written back over. The value must have the field's type.
    pub fn patch(&mut self, key: &str, value: serde_json::Value) -> Result<Settings, String> {
        let mut document = serde_json::to_value(&self.settings).map_err(|e| e.to_string())?;
        let parts: Vec<&str> = key.split('.').collect();
        let (last, parents) = parts.split_last().ok_or("empty settings key")?;
        let mut slot = &mut document;
        for part in parents {
            slot = slot
                .get_mut(*part)
                .filter(|value| value.is_object())
                .ok_or_else(|| format!("unknown setting {key}"))?;
        }
        let fields = slot
            .as_object_mut()
            .ok_or_else(|| format!("unknown setting {key}"))?;
        // Extensions are a map keyed by id, so an id may not be stored yet.
        if !fields.contains_key(*last) && parents != ["extensions"] {
            return Err(format!("unknown setting {key}"));
        }
        fields.insert((*last).to_string(), value);
        let next: Settings = serde_json::from_value(document)
            .map_err(|error| format!("invalid value for {key}: {error}"))?;
        if parents == ["extensions"] && !EXTENSION_IDS.contains(last) {
            return Err(format!("unknown extension {last}"));
        }
        self.set(next)
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
    fn patch_changes_one_field_and_validates_it() {
        let dir = temp_dir("patch");
        let mut store = SettingsStore::load(&dir).unwrap();
        store
            .set(Settings {
                retention_days: 30,
                ..Settings::default()
            })
            .unwrap();
        let next = store
            .patch("weeklyTargetHours", serde_json::json!(32))
            .unwrap();
        assert_eq!(next.weekly_target_hours, 32);
        assert_eq!(next.retention_days, 30);
        let breaks = !next.breaks.enabled;
        assert_eq!(
            store
                .patch("breaks.enabled", serde_json::json!(breaks))
                .unwrap()
                .breaks
                .enabled,
            breaks
        );
        assert!(
            !store
                .patch("extensions.tmux", serde_json::json!(false))
                .unwrap()
                .extensions["tmux"]
        );
        // Clamped like any other write.
        assert_eq!(
            store
                .patch("weeklyTargetHours", serde_json::json!(1_000))
                .unwrap()
                .weekly_target_hours,
            MAX_WEEKLY_TARGET_HOURS
        );
        for (key, value) in [
            ("nope", serde_json::json!(1)),
            ("breaks.nope", serde_json::json!(1)),
            ("extensions.unknown", serde_json::json!(true)),
            ("breaks.enabled", serde_json::json!("maybe")),
            ("weeklyTargetHours.deeper", serde_json::json!(1)),
        ] {
            assert!(store.patch(key, value).is_err(), "accepted {key}");
        }
        // Failed patches leave the file as it was.
        let reloaded = SettingsStore::load(&dir).unwrap().snapshot();
        assert_eq!(reloaded, store.snapshot());
        let _ = fs::remove_dir_all(dir);
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
                    shape: Shape::Sharper,
                    size_mode: SizeMode::VeryRelaxed,
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
                    breaks: BreakSettings::default(),
                    extensions: BTreeMap::new(),
                    advanced_workflow_tracking: true,
                })
                .unwrap();
        }
        let reloaded = SettingsStore::load(&dir).unwrap().snapshot();
        assert_eq!(reloaded.theme, Theme::Light);
        assert_eq!(reloaded.accent, Accent::Orange);
        assert_eq!(reloaded.shape, Shape::Sharper);
        assert_eq!(reloaded.size_mode, SizeMode::VeryRelaxed);
        assert_eq!(reloaded.close_behavior, CloseBehavior::Quit);
        assert!(!reloaded.tray_enabled);
        assert_eq!(reloaded.retention_days, 30);
        assert_eq!(reloaded.ai_suggest, AiSuggest::Category);
        assert!(!reloaded.auto_accept);
        assert_eq!(reloaded.auto_accept_percent, 90);
        assert_eq!(reloaded.ai_custom_prompt, "OpenRize is Coding");
        assert_eq!(reloaded.weekly_target_hours, 32);
        assert!(reloaded.advanced_workflow_tracking);
        assert!(reloaded.tracking_hours.per_day);
        assert_eq!(reloaded.tracking_hours.monday.start, "09:00");
        assert!(!reloaded.tracking_hours.tuesday.enabled);
    }

    #[test]
    fn advanced_workflow_tracking_is_off_until_switched_on() {
        let dir = temp_dir("workflow-tracking");
        let mut store = SettingsStore::load(&dir).unwrap();
        assert!(!store.snapshot().advanced_workflow_tracking);

        store
            .patch("advancedWorkflowTracking", serde_json::json!(true))
            .unwrap();
        let reloaded = SettingsStore::load(&dir).unwrap().snapshot();
        assert!(reloaded.advanced_workflow_tracking);
    }

    #[test]
    fn an_extension_follows_detection_until_the_person_chooses() {
        let dir = temp_dir("extensions");
        let mut store = SettingsStore::load(&dir).unwrap();
        let fresh = store.snapshot();
        // No choice yet: detection decides, so a newly installed tool is on.
        assert!(fresh.extension_enabled("herdr", true));
        assert!(!fresh.extension_enabled("herdr", false));

        let mut next = fresh;
        next.extensions.insert("herdr".to_string(), false);
        next.extensions.insert("not-an-extension".to_string(), true);
        store.set(next).unwrap();

        // An explicit off survives a restart and is not undone by detection.
        let reloaded = SettingsStore::load(&dir).unwrap().snapshot();
        assert!(!reloaded.extension_enabled("herdr", true));
        assert!(reloaded.extension_enabled("tmux", true));
        assert!(!reloaded.extensions.contains_key("not-an-extension"));
    }

    #[test]
    fn theme_defaults_to_dark_but_keeps_saved_choices() {
        assert_eq!(Settings::default().theme, Theme::Dark);
        let unset: Settings = serde_json::from_str(r#"{"accent":"blue"}"#).unwrap();
        assert_eq!(unset.theme, Theme::Dark);
        let system: Settings = serde_json::from_str(r#"{"theme":"system"}"#).unwrap();
        assert_eq!(system.theme, Theme::System);
        let light: Settings = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(light.theme, Theme::Light);
    }

    #[test]
    fn appearance_init_script_seeds_the_saved_choices() {
        let dir = temp_dir("init-script");
        let mut store = SettingsStore::load(&dir).unwrap();
        store
            .set(Settings {
                theme: Theme::Light,
                shape: Shape::Sharper,
                ..Settings::default()
            })
            .unwrap();
        let script = appearance_init_script(&dir);
        assert!(script.contains("seed(\"openrize.theme\", \"light\")"));
        assert!(script.contains("seed(\"openrize.shape\", \"sharper\")"));
        // Only fills a missing copy; an existing one is current.
        assert!(script.contains("localStorage.getItem(key) === null"));

        let system = temp_dir("init-script-system");
        fs::write(system.join(FILE_NAME), r#"{"theme":"system"}"#).unwrap();
        let script = appearance_init_script(&system);
        assert!(script.contains("\"system\""));
        assert!(script.contains("\"rounded\""));
    }

    #[test]
    fn appearance_init_script_falls_back_to_the_defaults() {
        let missing = temp_dir("init-script-missing");
        let script = appearance_init_script(&missing);
        assert!(script.contains("seed(\"openrize.theme\", \"dark\")"));
        assert!(script.contains("seed(\"openrize.shape\", \"rounded\")"));

        let corrupt = temp_dir("init-script-corrupt");
        fs::write(corrupt.join(FILE_NAME), "not json").unwrap();
        let script = appearance_init_script(&corrupt);
        assert!(script.contains("\"dark\""));
        // Reading must not quarantine the file; `load` owns that.
        assert!(corrupt.join(FILE_NAME).exists());
    }

    #[test]
    fn older_settings_default_to_rounded_shape() {
        let loaded: Settings = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(loaded.shape, Shape::Rounded);
        let sharper: Settings = serde_json::from_str(r#"{"shape":"sharper"}"#).unwrap();
        assert_eq!(sharper.shape, Shape::Sharper);
    }

    #[test]
    fn older_settings_default_to_normal_size() {
        let loaded: Settings = serde_json::from_str(r#"{"theme":"dark","accent":"blue"}"#).unwrap();
        assert_eq!(loaded.size_mode, SizeMode::Normal);
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
    fn break_reminders_default_to_fifty_minutes_of_work_then_five_off_the_clock() {
        let defaults = Settings::default().breaks;
        assert!(defaults.enabled);
        assert_eq!(defaults.work_minutes, 50);
        assert_eq!(defaults.break_minutes, 5);
        assert_eq!(defaults.snooze_minutes, 5);
        assert_eq!(defaults.message, "");
        // Pausing stopwatches for a break is opt-in.
        assert!(!defaults.pause_stopwatches);
        assert!(defaults.schedules.is_empty());
        // An older settings file loads with the defaults filled in.
        let loaded: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(loaded.breaks, defaults);
    }

    #[test]
    fn break_settings_are_clamped_and_schedules_get_ids() {
        let dir = temp_dir("breaks");
        let mut store = SettingsStore::load(&dir).unwrap();
        let saved = store
            .set(Settings {
                breaks: BreakSettings {
                    work_minutes: 1,
                    break_minutes: 500,
                    snooze_minutes: 7,
                    message: "  x".repeat(100),
                    schedules: vec![ScheduledBreak {
                        label: "   ".to_string(),
                        at: "25:99".to_string(),
                        minutes: 0,
                        ..ScheduledBreak::default()
                    }],
                    ..BreakSettings::default()
                },
                ..Settings::default()
            })
            .unwrap();
        assert_eq!(saved.breaks.work_minutes, 15);
        assert_eq!(saved.breaks.break_minutes, 60);
        assert_eq!(saved.breaks.snooze_minutes, DEFAULT_SNOOZE_MINUTES);
        assert!(saved.breaks.message.chars().count() <= MAX_BREAK_MESSAGE_CHARS);
        let schedule = &saved.breaks.schedules[0];
        assert!(!schedule.id.is_empty());
        assert_eq!(schedule.label, "Break");
        assert_eq!(schedule.at, "12:30");
        assert_eq!(schedule.minutes, 1);
        let reloaded = SettingsStore::load(&dir).unwrap().snapshot();
        assert_eq!(reloaded.breaks, saved.breaks);
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
