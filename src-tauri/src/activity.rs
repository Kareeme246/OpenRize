//! Activity capture — the automatic tracker.
//!
//! A background thread samples the OS foreground window (`active-win-pos-rs`)
//! and user idle time (`user-idle3`) every few seconds and folds consecutive
//! samples into *segments*: one row per contiguous run of the same app + window
//! title. Segments are the unit Home, Sessions, Focus, and Breaks all read.
//!
//! Why segments and not raw samples (decision A2): raw samples would mean one
//! write every few seconds forever, which is a machine-rate problem that wants a
//! dedicated time-series store. Folding at the source means a write only when
//! the user actually switches windows — human rate — so plain SQLite stays
//! comfortable and a day of tracking is a few hundred rows.
//!
//! Clock policy matches `timers.rs`: everything persisted is wall-clock epoch
//! milliseconds. `now_epoch_ms` is reused from there so there is one definition.
//!
//! Idle policy (decision A3): crossing the idle threshold closes the current
//! segment and opens an automatic Break. Coming back closes that Break and
//! resumes capture. Manual Focus/Break sessions are *not* interrupted by
//! capture — while one is open, automatic segment creation is paused.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, Row};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::timers::now_epoch_ms;

/// How often the sampler runs. Long enough to be invisible in power use, short
/// enough that a window switch is attributed to roughly the right minute.
pub const SAMPLE_SECS: u64 = 3;

/// Idle before an automatic Break. Rize cites 5 minutes as the per-category
/// default; one global value until per-category settings exist.
pub const DEFAULT_IDLE_THRESHOLD_MS: u64 = 5 * 60 * 1000;

const DB_FILE: &str = "activity.db";

/// Session kinds, stored as text so the DB stays readable and a future kind
/// (Meeting) is a value change, not a migration.
pub const KIND_ACTIVITY: &str = "activity";
pub const KIND_FOCUS: &str = "focus";
pub const KIND_BREAK: &str = "break";

/// Label carried by the idle-generated Break, so `tick` can tell an automatic
/// break (safe to end when the user returns) from a manual one (leave alone).
const IDLE_LABEL: &str = "Idle";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS segments (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  app        TEXT    NOT NULL,
  title      TEXT    NOT NULL,
  kind       TEXT    NOT NULL,
  label      TEXT,
  started_at INTEGER NOT NULL,
  ended_at   INTEGER,
  reviewed   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_segments_started_at ON segments (started_at);
CREATE INDEX IF NOT EXISTS idx_segments_ended_at ON segments (ended_at);
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySegment {
    pub id: i64,
    pub app: String,
    pub title: String,
    pub kind: String,
    pub label: Option<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub reviewed: bool,
}

/// Everything the dashboard needs from one round trip, aggregated over a
/// caller-supplied range. The range is passed in (not computed here) because
/// "today" is a *local* midnight and Rust has no timezone off the standard
/// library — the frontend already knows its own offset.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshot {
    pub current: Option<ActivitySegment>,
    pub segments: Vec<ActivitySegment>,
    /// Activity + Focus; Break is reported separately so a lunch hour does not
    /// inflate the working total.
    pub tracked_ms: u64,
    pub focus_ms: u64,
    pub break_ms: u64,
    pub unreviewed: u64,
    pub idle_ms: u64,
    pub idle_threshold_ms: u64,
    pub capture_enabled: bool,
}

/// The subset of `ActiveWindow` this module cares about, so the state machine
/// is testable without a real desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSample {
    pub app: String,
    pub title: String,
}

#[derive(Debug, Clone)]
struct Current {
    id: i64,
    app: String,
    title: String,
    kind: String,
    label: Option<String>,
}

pub struct ActivityStore {
    conn: Connection,
    current: Option<Current>,
    capture_enabled: bool,
    idle_threshold_ms: u64,
    last_idle_ms: u64,
}

impl ActivityStore {
    pub fn load(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|error| format!("could not create data dir: {error}"))?;
        let path: PathBuf = dir.join(DB_FILE);
        let conn = Connection::open(&path).map_err(|error| error.to_string())?;
        Self::from_conn(conn)
    }

    fn from_conn(conn: Connection) -> Result<Self, String> {
        // WAL survives an unclean shutdown better and keeps the reader (the
        // commands) from blocking the writer (the sampler). synchronous=NORMAL
        // is the standard WAL trade: a crash can lose the last few seconds of
        // tracking, never the database.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|error| error.to_string())?;
        conn.execute_batch(SCHEMA)
            .map_err(|error| error.to_string())?;

        let mut store = Self {
            conn,
            current: None,
            capture_enabled: true,
            idle_threshold_ms: DEFAULT_IDLE_THRESHOLD_MS,
            last_idle_ms: 0,
        };
        store.capture_enabled = store
            .read_setting("capture_enabled")
            .and_then(|value| value.parse().ok())
            .unwrap_or(true);
        store.idle_threshold_ms = store
            .read_setting("idle_threshold_ms")
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_IDLE_THRESHOLD_MS);
        // A segment left open by a crash/kill has no end and would read as
        // "still active" forever. Stamp it ended at its own start: honest
        // zero-length, not a fake hours-long session.
        store
            .conn
            .execute(
                "UPDATE segments SET ended_at = started_at WHERE ended_at IS NULL",
                [],
            )
            .map_err(|error| error.to_string())?;
        Ok(store)
    }

    fn read_setting(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .ok()
    }

    fn write_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    // --- capture state machine ------------------------------------------

    /// Folds one sample into the segment log. Returns whether the open segment
    /// changed, so the caller can decide if the frontend needs telling.
    pub fn tick(
        &mut self,
        sample: Option<WindowSample>,
        idle_ms: u64,
        now: u64,
    ) -> Result<bool, String> {
        self.last_idle_ms = idle_ms;

        if !self.capture_enabled {
            return self.close_current(now);
        }

        let idle = idle_ms >= self.idle_threshold_ms;
        let current = self.current.as_ref().map(|cur| {
            (
                cur.app.clone(),
                cur.title.clone(),
                cur.kind.clone(),
                cur.label.clone(),
            )
        });

        match current {
            None => {
                if idle {
                    self.open("Idle", "No activity", KIND_BREAK, Some(IDLE_LABEL), now)
                } else if let Some(sample) = sample {
                    self.open(&sample.app, &sample.title, KIND_ACTIVITY, None, now)
                } else {
                    Ok(false)
                }
            }
            Some((app, title, kind, label)) => {
                if idle {
                    // A manual Focus/Break is not interrupted by idle: no
                    // keyboard input is not the same as not working (reading
                    // is still focus). Only automatic capture yields to idle.
                    if kind != KIND_ACTIVITY {
                        return Ok(false);
                    }
                    self.close_current(now)?;
                    return self.open("Idle", "No activity", KIND_BREAK, Some(IDLE_LABEL), now);
                }

                if kind == KIND_BREAK && label.as_deref() == Some(IDLE_LABEL) {
                    // The user is back. End the automatic break and resume.
                    self.close_current(now)?;
                    return match sample {
                        Some(sample) => {
                            self.open(&sample.app, &sample.title, KIND_ACTIVITY, None, now)
                        }
                        None => Ok(true),
                    };
                }

                if kind != KIND_ACTIVITY {
                    // A manual Focus/Break owns the timeline; capture is paused.
                    return Ok(false);
                }

                match sample {
                    Some(sample) if sample.app != app || sample.title != title => {
                        self.close_current(now)?;
                        self.open(&sample.app, &sample.title, KIND_ACTIVITY, None, now)
                    }
                    // Same window, or the sample failed on this tick — keep the
                    // segment running rather than splitting it on a transient
                    // error.
                    _ => Ok(false),
                }
            }
        }
    }

    /// Manual Start Focus / Start Break. Whatever was open closes first; the
    /// `start_session` command rejects any other kind.
    pub fn start_session(
        &mut self,
        kind: &str,
        label: Option<&str>,
        now: u64,
    ) -> Result<bool, String> {
        if kind != KIND_FOCUS && kind != KIND_BREAK {
            return Err(format!("cannot start a session of kind {kind}"));
        }
        self.close_current(now)?;
        let display = label.map(str::trim).filter(|text| !text.is_empty());
        let app = if kind == KIND_FOCUS { "Focus" } else { "Break" };
        let title = display.unwrap_or(app);
        self.open(app, title, kind, display, now)
    }

    pub fn stop_session(&mut self, now: u64) -> Result<bool, String> {
        self.close_current(now)
    }

    pub fn set_capture_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.capture_enabled = enabled;
        self.write_setting("capture_enabled", if enabled { "true" } else { "false" })
    }

    pub fn set_idle_threshold_ms(&mut self, ms: u64) -> Result<(), String> {
        // Guard the floor: a threshold under the sample interval would flap
        // between Break and Activity every tick.
        self.idle_threshold_ms = ms.max(SAMPLE_SECS * 1000);
        self.write_setting("idle_threshold_ms", &self.idle_threshold_ms.to_string())
    }

    pub fn mark_reviewed(&mut self, id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE segments SET reviewed = 1 WHERE id = ?1",
                params![id],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    // --- reads -----------------------------------------------------------

    /// Segments overlapping `[since_ms, now]`, oldest first, plus the rollups
    /// the dashboard draws. An ongoing segment is clipped to `now`.
    pub fn snapshot(&self, since_ms: u64, now: u64) -> Result<ActivitySnapshot, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, app, title, kind, label, started_at, ended_at, reviewed
                 FROM segments
                 WHERE ended_at IS NULL OR ended_at >= ?1
                 ORDER BY started_at ASC",
            )
            .map_err(|error| error.to_string())?;

        // SQLite has no unsigned integer, so epoch milliseconds cross the
        // boundary as i64. That overflows in year 292 million.
        let segments: Vec<ActivitySegment> = statement
            .query_map(params![since_ms as i64], segment_from_row)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;

        let mut tracked_ms = 0;
        let mut focus_ms = 0;
        let mut break_ms = 0;
        let mut unreviewed = 0;
        for segment in &segments {
            let start = segment.started_at.max(since_ms);
            let end = segment.ended_at.unwrap_or(now).min(now);
            let duration = end.saturating_sub(start);
            match segment.kind.as_str() {
                KIND_FOCUS => focus_ms += duration,
                KIND_BREAK => break_ms += duration,
                _ => tracked_ms += duration,
            }
            if !segment.reviewed {
                unreviewed += 1;
            }
        }

        Ok(ActivitySnapshot {
            current: segments
                .iter()
                .find(|segment| segment.ended_at.is_none())
                .cloned(),
            segments,
            tracked_ms,
            focus_ms,
            break_ms,
            unreviewed,
            idle_ms: self.last_idle_ms,
            idle_threshold_ms: self.idle_threshold_ms,
            capture_enabled: self.capture_enabled,
        })
    }

    // --- segment primitives ---------------------------------------------

    fn open(
        &mut self,
        app: &str,
        title: &str,
        kind: &str,
        label: Option<&str>,
        now: u64,
    ) -> Result<bool, String> {
        self.conn
            .execute(
                "INSERT INTO segments (app, title, kind, label, started_at, ended_at, reviewed)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, 0)",
                params![app, title, kind, label, now as i64],
            )
            .map_err(|error| error.to_string())?;
        self.current = Some(Current {
            id: self.conn.last_insert_rowid(),
            app: app.to_string(),
            title: title.to_string(),
            kind: kind.to_string(),
            label: label.map(str::to_string),
        });
        Ok(true)
    }

    fn close_current(&mut self, now: u64) -> Result<bool, String> {
        let Some(current) = self.current.take() else {
            return Ok(false);
        };
        self.conn
            .execute(
                "UPDATE segments SET ended_at = ?1 WHERE id = ?2",
                params![now as i64, current.id],
            )
            .map_err(|error| error.to_string())?;
        Ok(true)
    }
}

fn segment_from_row(row: &Row<'_>) -> rusqlite::Result<ActivitySegment> {
    Ok(ActivitySegment {
        id: row.get(0)?,
        app: row.get(1)?,
        title: row.get(2)?,
        kind: row.get(3)?,
        label: row.get(4)?,
        started_at: row.get::<_, i64>(5)? as u64,
        ended_at: row.get::<_, Option<i64>>(6)?.map(|ms| ms as u64),
        reviewed: row.get::<_, i64>(7)? != 0,
    })
}

/// Reads the OS foreground window. Failure (no foreground window, permission
/// not granted) is reported as `None`, not an error: capture simply has nothing
/// to record this tick.
fn read_active_window() -> Option<WindowSample> {
    match active_win_pos_rs::get_active_window() {
        Ok(window) if !window.app_name.is_empty() => Some(WindowSample {
            app: window.app_name,
            title: window.title,
        }),
        _ => None,
    }
}

fn read_idle_ms() -> u64 {
    user_idle3::UserIdle::get_time()
        .map(|idle| idle.duration().as_millis() as u64)
        .unwrap_or(0)
}

/// Starts the sampler thread. Runs for the life of the process, including while
/// the window is hidden — that is the whole point of a background tracker.
pub fn spawn_sampler(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(SAMPLE_SECS));

        let sample = read_active_window();
        let idle_ms = read_idle_ms();
        let now = now_epoch_ms();

        let result = app
            .state::<crate::AppState>()
            .activity
            .lock()
            .map_err(|_| "activity store lock poisoned".to_string())
            .and_then(|mut store| store.tick(sample, idle_ms, now));

        match result {
            // The payload is a bare ping: the frontend re-queries with its own
            // local-midnight bound, which Rust cannot compute.
            Ok(_) => {
                let _ = app.emit(crate::EVENT_ACTIVITY_CHANGED, ());
            }
            Err(error) => eprintln!("activity sample failed: {error}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> ActivityStore {
        ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
            .expect("schema")
    }

    fn sample(app: &str, title: &str) -> Option<WindowSample> {
        Some(WindowSample {
            app: app.to_string(),
            title: title.to_string(),
        })
    }

    #[test]
    fn a_window_switch_closes_one_segment_and_opens_another() {
        let mut store = store();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();
        assert!(!store.tick(sample("Code", "main.rs"), 0, 4_000).unwrap());
        assert!(store.tick(sample("Slack", "#general"), 0, 7_000).unwrap());

        let snapshot = store.snapshot(0, 7_000).unwrap();
        assert_eq!(snapshot.segments.len(), 2);
        assert_eq!(snapshot.segments[0].app, "Code");
        assert_eq!(snapshot.segments[0].ended_at, Some(7_000));
        assert_eq!(snapshot.segments[1].app, "Slack");
        assert_eq!(snapshot.segments[1].ended_at, None);
        assert_eq!(snapshot.tracked_ms, 6_000);
    }

    #[test]
    fn idle_crosses_into_an_automatic_break_and_back() {
        let mut store = store();
        store.set_idle_threshold_ms(60_000).unwrap();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();

        // 90s idle: close activity, open the auto break.
        store
            .tick(sample("Code", "main.rs"), 90_000, 100_000)
            .unwrap();
        assert_eq!(
            store.snapshot(0, 100_000).unwrap().current.unwrap().kind,
            KIND_BREAK
        );

        // User is back: the break ends and a fresh activity segment starts.
        store.tick(sample("Code", "main.rs"), 0, 130_000).unwrap();
        let snapshot = store.snapshot(0, 130_000).unwrap();
        let current = snapshot.current.unwrap();
        assert_eq!(current.kind, KIND_ACTIVITY);
        assert_eq!(snapshot.break_ms, 30_000);
    }

    #[test]
    fn a_manual_focus_is_not_interrupted_by_capture() {
        let mut store = store();
        store
            .start_session(KIND_FOCUS, Some("Deep work"), 1_000)
            .unwrap();
        store.tick(sample("Slack", "#random"), 0, 2_000).unwrap();

        let snapshot = store.snapshot(0, 2_000).unwrap();
        assert_eq!(snapshot.segments.len(), 1);
        assert_eq!(snapshot.current.unwrap().kind, KIND_FOCUS);
        assert_eq!(snapshot.focus_ms, 1_000);
    }

    #[test]
    fn pausing_capture_closes_the_open_segment() {
        let mut store = store();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();
        store.set_capture_enabled(false).unwrap();
        store.tick(sample("Code", "main.rs"), 0, 5_000).unwrap();

        let snapshot = store.snapshot(0, 5_000).unwrap();
        assert_eq!(snapshot.current, None);
        assert_eq!(snapshot.segments[0].ended_at, Some(5_000));
    }

    #[test]
    fn an_unclean_shutdown_does_not_leave_a_segment_running_forever() {
        let dir = std::env::temp_dir().join(format!("openrize-activity-{}", now_epoch_ms()));
        fs::create_dir_all(&dir).unwrap();
        {
            let mut store = ActivityStore::load(&dir).unwrap();
            store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();
        }
        let reloaded = ActivityStore::load(&dir).unwrap();
        let snapshot = reloaded.snapshot(0, 9_999_999).unwrap();
        assert_eq!(snapshot.current, None);
        assert_eq!(snapshot.segments[0].ended_at, Some(1_000));
    }

    #[test]
    fn idle_does_not_interrupt_a_manual_focus() {
        let mut store = store();
        store.set_idle_threshold_ms(60_000).unwrap();
        store
            .start_session(KIND_FOCUS, Some("Deep work"), 1_000)
            .unwrap();

        store
            .tick(sample("Code", "main.rs"), 120_000, 121_000)
            .unwrap();
        let snapshot = store.snapshot(0, 121_000).unwrap();
        assert_eq!(snapshot.current.unwrap().kind, KIND_FOCUS);
        assert_eq!(snapshot.break_ms, 0);
    }

    #[test]
    fn a_bad_kind_is_rejected() {
        let mut store = store();
        assert!(store.start_session("meeting", None, 1_000).is_err());
    }
}
