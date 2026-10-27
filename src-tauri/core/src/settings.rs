//! User preferences - theme, accent, size mode, close behavior, tray, retention.
//!
//! Storage policy: a single JSON file under the XDG config home
//! (`$XDG_CONFIG_HOME/openrize/settings.json`, or `~/.config/openrize/...`;
//! on Windows `%APPDATA%\openrize\settings.json`).
//! Preferences are config, not data: they survive deleting the app-data
//! directory, and they are portable between machines the way `.dotfiles` are.
//!
//! The file is written atomically (temp + rename) and a corrupt file is moved
//! aside rather than silently reset, matching `timers.rs`.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Once;

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

/// Placement of app-owned reminder, live-break, and welcome-back panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NotificationPlacement {
    TopLeft,
    TopRight,
    #[serde(alias = "centerMiddle")]
    TopCenter,
    BottomLeft,
    #[default]
    BottomRight,
}

/// Reminds once per current stopwatch run, rather than accumulated time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StopwatchReminder {
    pub enabled: bool,
    pub after_minutes: u32,
}

impl Default for StopwatchReminder {
    fn default() -> Self {
        Self {
            enabled: true,
            after_minutes: 180,
        }
    }
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
    pub stopwatch_reminder: StopwatchReminder,
    pub notification_placement: NotificationPlacement,
    /// Whether to send anonymous crash reports to the built-in endpoint.
    pub crash_reporting: bool,
    /// Persistent anonymous installation identifier for diagnostics and bug reporting.
    pub install_id: String,
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
            stopwatch_reminder: StopwatchReminder::default(),
            notification_placement: NotificationPlacement::default(),
            crash_reporting: false,
            install_id: String::new(),
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
        self.breaks = self.breaks.normalized();
        self.stopwatch_reminder.after_minutes =
            self.stopwatch_reminder.after_minutes.clamp(30, 720);
        self
    }
}

/// `$XDG_CONFIG_HOME/openrize`, falling back to `$HOME/.config/openrize`.
///
/// Deliberately not Tauri's `app_config_dir`: on macOS that lands under
/// `~/Library/Application Support`, and this app keeps its preferences in the
/// conventional `.config` location on every platform. Windows is the exception
/// to the fallback: `HOME` exists only in shells like Git Bash, so honoring it
/// would give the app (launched from Explorer) and `rize` different files.
/// There the fallback is the roaming `%APPDATA%\openrize`, and the first call
/// moves a settings file left in an older location over (`migrate_legacy`).
pub fn config_dir() -> PathBuf {
    let env = ConfigEnv::from_process();
    let dir = env.resolve(cfg!(windows));
    // An explicit XDG_CONFIG_HOME is an isolated instance (dev scripts, tests):
    // never pull the user's real settings into it.
    if cfg!(windows) && !env.has_xdg() {
        static MIGRATED: Once = Once::new();
        MIGRATED.call_once(|| migrate_legacy(&dir, &env.legacy_dirs()));
    }
    dir
}

/// The environment `config_dir` reads, separated so tests can supply it.
#[derive(Default)]
struct ConfigEnv {
    xdg: Option<OsString>,
    home: Option<OsString>,
    userprofile: Option<OsString>,
    /// Windows roaming AppData, where the platform keeps per-user config.
    roaming: Option<PathBuf>,
}

impl ConfigEnv {
    fn from_process() -> Self {
        Self {
            xdg: std::env::var_os("XDG_CONFIG_HOME"),
            home: std::env::var_os("HOME"),
            userprofile: std::env::var_os("USERPROFILE"),
            roaming: if cfg!(windows) {
                dirs::config_dir()
            } else {
                None
            },
        }
    }

    fn has_xdg(&self) -> bool {
        self.xdg.as_ref().is_some_and(|value| !value.is_empty())
    }

    fn resolve(&self, windows: bool) -> PathBuf {
        let base = self
            .xdg
            .as_ref()
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| self.roaming.clone().filter(|_| windows))
            .or_else(|| {
                self.home
                    .as_ref()
                    .map(|home| PathBuf::from(home).join(".config"))
            })
            .or_else(|| self.userprofile.as_ref().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("openrize")
    }

    /// Where Windows builds before the roaming location kept settings: the app
    /// launched from Explorer (no `HOME`) wrote the first, `rize` run from Git
    /// Bash (`HOME` set) the second.
    fn legacy_dirs(&self) -> Vec<PathBuf> {
        match &self.userprofile {
            Some(profile) => {
                let profile = PathBuf::from(profile);
                vec![
                    profile.join("openrize"),
                    profile.join(".config").join("openrize"),
                ]
            }
            None => Vec::new(),
        }
    }
}

/// What a config directory can hold. Anything else in a legacy directory is
/// not ours (it may be a checkout named `openrize`) and stays put.
fn is_config_file(name: &str) -> bool {
    name == FILE_NAME || name.starts_with("settings.corrupt-")
}

/// Moves settings from the newest legacy directory that has any into `dir`,
/// only when `dir` has no settings of its own, and removes that directory when
/// nothing else is left in it. Safe for the app and `rize` starting together
/// and never replaces a file already in `dir`: each file is copied under a
/// private name, then published with a hard link, which fails instead of
/// overwriting, so a reader sees a whole `settings.json` or none. Failures are
/// logged and leave the legacy files where they were.
fn migrate_legacy(dir: &Path, legacy_dirs: &[PathBuf]) {
    if dir.join(FILE_NAME).exists() {
        return;
    }
    let source = legacy_dirs
        .iter()
        .filter_map(|legacy| {
            let modified = fs::metadata(legacy.join(FILE_NAME)).ok()?.modified().ok()?;
            Some((modified, legacy))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, legacy)| legacy);
    let Some(source) = source else { return };
    if let Err(error) = move_config_files(source, dir) {
        eprintln!(
            "could not move settings from {} to {}: {error}",
            source.display(),
            dir.display()
        );
    }
}

fn move_config_files(source: &Path, dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut names: Vec<String> = fs::read_dir(source)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_config_file(name))
        .collect();
    // settings.json goes last: its arrival is what tells other starters to skip.
    names.sort_by_key(|name| name == FILE_NAME);
    for name in names {
        let from = source.join(&name);
        let to = dir.join(&name);
        let staging = dir.join(format!("{name}.moving-{}", uuid::Uuid::now_v7().simple()));
        let published = fs::copy(&from, &staging).and_then(|_| fs::hard_link(&staging, &to));
        let _ = fs::remove_file(&staging);
        match published {
            Ok(()) => {}
            // Another process published it first: theirs is the same content,
            // so only our source remains to remove.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound && to.exists() => {}
            Err(error) => return Err(error),
        }
        match fs::remove_file(&from) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    // Fails, as wanted, unless the directory is empty.
    let _ = fs::remove_dir(source);
    Ok(())
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
        if !fields.contains_key(*last) {
            return Err(format!("unknown setting {key}"));
        }
        fields.insert((*last).to_string(), value);
        let next: Settings = serde_json::from_value(document)
            .map_err(|error| format!("invalid value for {key}: {error}"))?;
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
    fn notification_placements_round_trip_and_legacy_center_migrates() {
        let dir = temp_dir("notification-placement");
        let mut store = SettingsStore::load(&dir).unwrap();
        for placement in [
            NotificationPlacement::TopLeft,
            NotificationPlacement::TopRight,
            NotificationPlacement::TopCenter,
            NotificationPlacement::BottomLeft,
            NotificationPlacement::BottomRight,
        ] {
            store
                .set(Settings {
                    notification_placement: placement,
                    ..Settings::default()
                })
                .unwrap();
            assert_eq!(
                SettingsStore::load(&dir)
                    .unwrap()
                    .snapshot()
                    .notification_placement,
                placement
            );
        }
        fs::write(
            dir.join(FILE_NAME),
            r#"{"notificationPlacement":"centerMiddle","accent":"orange"}"#,
        )
        .unwrap();
        let mut legacy = SettingsStore::load(&dir).unwrap();
        let migrated = legacy.snapshot();
        assert_eq!(
            migrated.notification_placement,
            NotificationPlacement::TopCenter
        );
        assert_eq!(migrated.accent, Accent::Orange);
        legacy.set(migrated).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(FILE_NAME)).unwrap()).unwrap();
        assert_eq!(saved["notificationPlacement"], "topCenter");
        assert_eq!(
            SettingsStore::load(&dir)
                .unwrap()
                .snapshot()
                .notification_placement,
            NotificationPlacement::TopCenter
        );
    }

    #[test]
    fn stopwatch_reminder_defaults_bounds_and_saved_values() {
        let dir = temp_dir("stopwatch-reminder");
        let mut store = SettingsStore::load(&dir).unwrap();
        assert_eq!(
            store.snapshot().stopwatch_reminder,
            StopwatchReminder::default()
        );
        let old: Settings = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert!(old.stopwatch_reminder.enabled);
        assert_eq!(old.stopwatch_reminder.after_minutes, 180);
        for (input, expected) in [(0, 30), (240, 240), (u32::MAX, 720)] {
            store
                .set(Settings {
                    stopwatch_reminder: StopwatchReminder {
                        enabled: false,
                        after_minutes: input,
                    },
                    ..Settings::default()
                })
                .unwrap();
            let saved = SettingsStore::load(&dir)
                .unwrap()
                .snapshot()
                .stopwatch_reminder;
            assert!(!saved.enabled);
            assert_eq!(saved.after_minutes, expected);
        }
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
                    stopwatch_reminder: StopwatchReminder::default(),
                    notification_placement: NotificationPlacement::TopLeft,
                    crash_reporting: true,
                    install_id: "test-install-id".to_string(),
                })
                .unwrap();
        }
        let reloaded = SettingsStore::load(&dir).unwrap().snapshot();
        assert_eq!(reloaded.theme, Theme::Light);
        assert_eq!(reloaded.accent, Accent::Orange);
        assert_eq!(reloaded.shape, Shape::Sharper);
        assert_eq!(reloaded.install_id, "test-install-id");
        assert_eq!(reloaded.size_mode, SizeMode::VeryRelaxed);
        assert_eq!(reloaded.close_behavior, CloseBehavior::Quit);
        assert!(!reloaded.tray_enabled);
        assert_eq!(reloaded.retention_days, 30);
        assert_eq!(reloaded.ai_suggest, AiSuggest::Category);
        assert!(!reloaded.auto_accept);
        assert_eq!(reloaded.auto_accept_percent, 90);
        assert_eq!(reloaded.ai_custom_prompt, "OpenRize is Coding");
        assert_eq!(reloaded.weekly_target_hours, 32);
        assert_eq!(
            reloaded.notification_placement,
            NotificationPlacement::TopLeft
        );
        assert!(reloaded.crash_reporting);
        assert!(reloaded.tracking_hours.per_day);
        assert_eq!(reloaded.tracking_hours.monday.start, "09:00");
        assert!(!reloaded.tracking_hours.tuesday.enabled);
    }

    #[test]
    fn keys_from_removed_settings_are_ignored_and_dropped_on_the_next_save() {
        let dir = temp_dir("removed-keys");
        fs::write(
            dir.join(FILE_NAME),
            r#"{"weeklyTargetHours":32,"advancedWorkflowTracking":true,"extensions":{"tmux":false}}"#,
        )
        .unwrap();
        let mut store = SettingsStore::load(&dir).unwrap();
        assert_eq!(store.snapshot().weekly_target_hours, 32);

        store.patch("retentionDays", serde_json::json!(7)).unwrap();
        let saved = fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        assert!(!saved.contains("advancedWorkflowTracking"));
        assert!(!saved.contains("extensions"));
        let _ = fs::remove_dir_all(dir);
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

    fn os(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    #[test]
    fn config_dir_prefers_xdg_then_roaming_on_windows_and_home_elsewhere() {
        let env = ConfigEnv {
            xdg: os("/x"),
            home: os("/h"),
            userprofile: os("/u"),
            roaming: Some(PathBuf::from("/roaming")),
        };
        assert_eq!(env.resolve(true), PathBuf::from("/x/openrize"));
        assert_eq!(env.resolve(false), PathBuf::from("/x/openrize"));

        let env = ConfigEnv { xdg: os(""), ..env };
        // HOME (set by Git Bash) must not move Windows off the roaming dir.
        assert_eq!(env.resolve(true), PathBuf::from("/roaming/openrize"));
        // The other platforms ignore the roaming slot entirely.
        assert_eq!(env.resolve(false), PathBuf::from("/h/.config/openrize"));

        let env = ConfigEnv { home: None, ..env };
        assert_eq!(env.resolve(false), PathBuf::from("/u/openrize"));
        let bare = ConfigEnv::default();
        assert_eq!(bare.resolve(false), PathBuf::from("./openrize"));
        assert!(!bare.has_xdg());
    }

    #[test]
    fn legacy_dirs_are_the_two_old_profile_locations() {
        let env = ConfigEnv {
            userprofile: os("/u"),
            ..ConfigEnv::default()
        };
        assert_eq!(
            env.legacy_dirs(),
            vec![
                PathBuf::from("/u/openrize"),
                PathBuf::from("/u/.config/openrize")
            ]
        );
        assert!(ConfigEnv::default().legacy_dirs().is_empty());
    }

    #[test]
    fn legacy_settings_move_to_the_new_dir_and_the_empty_old_dir_goes() {
        let root = temp_dir("migrate");
        let new = root.join("roaming").join("openrize");
        let legacy = root.join("profile").join("openrize");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join(FILE_NAME), r#"{"accent":"blue"}"#).unwrap();
        fs::write(legacy.join("settings.corrupt-1"), "junk").unwrap();

        migrate_legacy(&new, std::slice::from_ref(&legacy));

        assert_eq!(
            SettingsStore::load(&new).unwrap().snapshot().accent,
            Accent::Blue
        );
        assert!(new.join("settings.corrupt-1").is_file());
        assert!(!legacy.exists());
        let leftovers: Vec<_> = fs::read_dir(&new).unwrap().collect();
        assert_eq!(leftovers.len(), 2, "no staging files are left behind");

        // Running again, as a second process would, changes nothing.
        migrate_legacy(&new, &[legacy]);
        assert!(new.join(FILE_NAME).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migration_never_replaces_new_settings_and_keeps_foreign_files() {
        let root = temp_dir("migrate-keep");
        let new = root.join("new");
        let legacy = root.join("old");
        fs::create_dir_all(&new).unwrap();
        fs::create_dir_all(&legacy).unwrap();
        fs::write(new.join(FILE_NAME), "new").unwrap();
        fs::write(legacy.join(FILE_NAME), "old").unwrap();
        migrate_legacy(&new, std::slice::from_ref(&legacy));
        assert_eq!(fs::read_to_string(new.join(FILE_NAME)).unwrap(), "new");
        assert_eq!(fs::read_to_string(legacy.join(FILE_NAME)).unwrap(), "old");

        // A directory that is really something else keeps its other files.
        fs::remove_file(new.join(FILE_NAME)).unwrap();
        fs::write(legacy.join("README.md"), "mine").unwrap();
        migrate_legacy(&new, std::slice::from_ref(&legacy));
        assert_eq!(fs::read_to_string(new.join(FILE_NAME)).unwrap(), "old");
        assert!(legacy.join("README.md").is_file());
        assert!(!legacy.join(FILE_NAME).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migration_takes_the_most_recently_written_legacy_dir() {
        let root = temp_dir("migrate-newest");
        let new = root.join("new");
        let older = root.join("older");
        let newer = root.join("newer");
        for dir in [&older, &newer] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(older.join(FILE_NAME), "older").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(newer.join(FILE_NAME), "newer").unwrap();
        migrate_legacy(&new, &[older.clone(), newer]);
        assert_eq!(fs::read_to_string(new.join(FILE_NAME)).unwrap(), "newer");
        assert!(older.join(FILE_NAME).is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_migrations_leave_one_whole_settings_file() {
        let root = temp_dir("migrate-race");
        let new = root.join("new");
        let legacy = root.join("old");
        fs::create_dir_all(&legacy).unwrap();
        let body = format!(r#"{{"aiCustomPrompt":"{}"}}"#, "x".repeat(2000));
        fs::write(legacy.join(FILE_NAME), &body).unwrap();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let (new, legacy) = (new.clone(), legacy.clone());
                std::thread::spawn(move || migrate_legacy(&new, &[legacy]))
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(fs::read_to_string(new.join(FILE_NAME)).unwrap(), body);
        assert!(!legacy.exists());
        let _ = fs::remove_dir_all(root);
    }
}
