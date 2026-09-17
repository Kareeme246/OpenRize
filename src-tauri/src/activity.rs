//! Activity capture — the automatic tracker.
//!
//! A background thread samples the OS foreground window (`active-win-pos-rs`)
//! and user idle time (`user-idle3`) every second and folds consecutive
//! samples into *segments*: one row per contiguous run of the same app + window
//! title. Segments are the unit Home, Sessions, Focus, and Breaks all read.
//!
//! Why segments and not raw samples (decision A2): raw samples would mean one
//! write every second forever, which is a machine-rate problem that wants a
//! dedicated time-series store. Folding at the source means a write only when
//! the user actually switches windows — human rate — so plain SQLite stays
//! comfortable and a day of tracking is a few hundred rows.
//!
//! Sampling vs. pushing (decision A4): the sampler itself always runs at
//! `SAMPLE_SECS` — that is what determines tracking accuracy for whatever app
//! the user is actually using, and it must not degrade just because
//! OpenRize's own window isn't focused (it almost never is; that's the whole
//! point of a background tracker). What *does* adapt to OpenRize's own focus
//! state is how often the result gets pushed to the frontend: every tick
//! while focused, a 30s heartbeat while backgrounded, plus an immediate
//! reconciliation push the instant focus returns (see `spawn_sampler` and
//! `lib.rs`'s `WindowEvent::Focused` handling). A structural change (a
//! segment actually opening or closing) always pushes immediately regardless
//! of focus, since that is a real state transition, not a clock tick.
//!
//! Push payloads carry data (decision A5), the same pattern `timers-changed`
//! already uses: `EVENT_ACTIVITY_CHANGED` carries the full `ActivitySnapshot`
//! and `EVENT_ACTIVITY_TICK` carries the lighter `ActivityTick` (same numbers,
//! no segment list) so the frontend never has to round-trip an `invoke` just
//! to learn what the push already told it.
//!
//! Reads don't share the writer's lock (decision A6): `AppState` keeps a
//! second, read-only `Connection` to the same WAL-mode database, behind its
//! own mutex. That is the entire fix for "a query blocks behind the
//! sampler's tick" — WAL already lets one writer and readers coexist, but a
//! single shared `Mutex<Connection>` used for everything defeats that. A
//! pool is unnecessary here: this app is single-window/single-user, so two
//! reads competing for the one reader connection is a rare, brief thing, not
//! a bottleneck.
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
use std::sync::atomic::Ordering;
use std::time::Duration;

use rusqlite::{params, Connection, Row};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::timers::now_epoch_ms;

/// How often the sampler runs. Always this fast, regardless of whether
/// OpenRize's own window is focused — see decision A4 above. What adapts to
/// focus is the *push* cadence in `spawn_sampler`, not this.
pub const SAMPLE_SECS: u64 = 1;

/// How often the frontend gets a push while OpenRize is backgrounded and
/// nothing structural has changed. Focused, every tick pushes instead.
const HEARTBEAT_MS: u64 = 30_000;

/// Idle before an automatic Break. Rize cites 5 minutes as the per-category
/// default; one global value until per-category settings exist.
pub const DEFAULT_IDLE_THRESHOLD_MS: u64 = 5 * 60 * 1000;

/// Shared with `timers.rs` (decision A8/opt 7): both stores live in the same
/// WAL-mode file, each through its own connection — SQLite is fine with
/// that, and it means one backup artifact covers both.
pub(crate) const DB_FILE: &str = "activity.db";

/// Rollup granularity and retention window for decision A7 (below). A plain
/// UTC-day bucket, not the user's local day: this table is an internal
/// acceleration cache for old data, never shown to the user with day-precision,
/// so it doesn't need the local-midnight machinery `since_ms` callers rely on.
const ONE_DAY_MS: u64 = 24 * 60 * 60 * 1000;
const RETENTION_DAYS: u64 = 90;
const RETENTION_MS: u64 = RETENTION_DAYS * ONE_DAY_MS;

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
-- Decision A7: additive-only acceleration cache for old-date aggregates.
-- Raw segments are never deleted; this table is purely a fast path for
-- summing days far enough in the past that they will never change again.
CREATE TABLE IF NOT EXISTS daily_rollups (
  day_epoch INTEGER NOT NULL,
  kind      TEXT    NOT NULL,
  total_ms  INTEGER NOT NULL,
  PRIMARY KEY (day_epoch, kind)
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

/// The lightweight push used for the 1Hz-focused / 30s-heartbeat cadence
/// (decision A5): the same numbers as `ActivitySnapshot`, minus the segment
/// list, so a per-second push doesn't re-serialize the whole day every time.
/// The segment list only ever changes at a structural change, which always
/// pushes a full `ActivitySnapshot` instead.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityTick {
    pub current: Option<ActivitySegment>,
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
    started_at: u64,
    reviewed: bool,
}

impl Current {
    fn as_segment(&self) -> ActivitySegment {
        ActivitySegment {
            id: self.id,
            app: self.app.clone(),
            title: self.title.clone(),
            kind: self.kind.clone(),
            label: self.label.clone(),
            started_at: self.started_at,
            ended_at: None,
            reviewed: self.reviewed,
        }
    }
}

/// The handful of in-memory fields a read needs, copied out from behind the
/// writer's mutex before the (potentially slower) query runs against the
/// separate reader connection — see decision A6.
struct LiveState {
    current: Option<Current>,
    capture_enabled: bool,
    idle_threshold_ms: u64,
    last_idle_ms: u64,
    watch_since_ms: Option<u64>,
}

struct Totals {
    tracked_ms: u64,
    focus_ms: u64,
    break_ms: u64,
    unreviewed: u64,
}

pub struct ActivityStore {
    conn: Connection,
    current: Option<Current>,
    capture_enabled: bool,
    idle_threshold_ms: u64,
    last_idle_ms: u64,
    /// The day boundary the frontend last asked about, cached so the sampler
    /// thread (which has no timezone information of its own) knows what
    /// range to aggregate for the tick/heartbeat pushes. Refreshed on every
    /// `activity_snapshot` call, which also naturally handles local-midnight
    /// rollover since the frontend recomputes it fresh each time.
    watch_since_ms: Option<u64>,
}

impl ActivityStore {
    pub fn load(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|error| format!("could not create data dir: {error}"))?;
        let path: PathBuf = dir.join(DB_FILE);
        let conn = Connection::open(&path).map_err(|error| error.to_string())?;
        Self::from_conn(conn)
    }

    /// Opens a second, read-only-by-convention connection to the same
    /// database for `AppState`'s reader mutex (decision A6). Must be called
    /// after `load` has created the file and its schema.
    pub fn open_reader(dir: &Path) -> Result<Connection, String> {
        let path = dir.join(DB_FILE);
        Connection::open(&path).map_err(|error| error.to_string())
    }

    fn from_conn(conn: Connection) -> Result<Self, String> {
        // WAL survives an unclean shutdown better and keeps the reader (the
        // commands) from blocking the writer (the sampler). synchronous=NORMAL
        // is the standard WAL trade: a crash can lose the last few seconds of
        // tracking, never the database. PRAGMA optimize is SQLite's own
        // recommendation to run at least once per connection lifetime so the
        // query planner's statistics don't go stale as the table grows.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA optimize;")
            .map_err(|error| error.to_string())?;
        conn.execute_batch(SCHEMA)
            .map_err(|error| error.to_string())?;

        let mut store = Self {
            conn,
            current: None,
            capture_enabled: true,
            idle_threshold_ms: DEFAULT_IDLE_THRESHOLD_MS,
            last_idle_ms: 0,
            watch_since_ms: None,
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
        store.rollup_old_segments(now_epoch_ms())?;
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

    /// Decision A7: rolls whole UTC days older than the retention window into
    /// `daily_rollups`. Additive and idempotent (re-running just overwrites
    /// with the same numbers) — raw segments are never touched or deleted.
    fn rollup_old_segments(&mut self, now: u64) -> Result<(), String> {
        let cutoff = now.saturating_sub(RETENTION_MS);
        self.conn
            .execute(
                "INSERT INTO daily_rollups (day_epoch, kind, total_ms)
                 SELECT started_at / ?1, kind, SUM(ended_at - started_at)
                 FROM segments
                 WHERE ended_at IS NOT NULL AND ended_at < ?2
                 GROUP BY started_at / ?1, kind
                 ON CONFLICT(day_epoch, kind) DO UPDATE SET total_ms = excluded.total_ms",
                params![ONE_DAY_MS as i64, cutoff as i64],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
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
            .map_err(|error| error.to_string())?;
        if let Some(current) = &mut self.current {
            if current.id == id {
                current.reviewed = true;
            }
        }
        Ok(())
    }

    // --- reads -------------------------------------------------------------
    // These use `self.conn` (the writer connection) directly, which is fine
    // for tests and any other in-process caller: the read/write split that
    // matters for concurrency is the separate `activity_reader` connection
    // in `AppState`, used by `snapshot_for`/`emit_full`/`emit_tick` below.

    fn live_state(&self) -> LiveState {
        LiveState {
            current: self.current.clone(),
            capture_enabled: self.capture_enabled,
            idle_threshold_ms: self.idle_threshold_ms,
            last_idle_ms: self.last_idle_ms,
            watch_since_ms: self.watch_since_ms,
        }
    }

    /// Segments overlapping `[since_ms, now]`, oldest first, plus the rollups
    /// the dashboard draws. An ongoing segment is clipped to `now`.
    pub fn snapshot(&self, since_ms: u64, now: u64) -> Result<ActivitySnapshot, String> {
        build_snapshot(&self.conn, &self.live_state(), since_ms, now)
    }

    /// The lightweight equivalent of `snapshot`, for tests exercising
    /// `ActivityTick` without needing `AppState` plumbing.
    pub fn tick_summary(&self, now: u64) -> Result<ActivityTick, String> {
        let live = self.live_state();
        let since_ms = live.watch_since_ms.unwrap_or(0);
        build_tick(&self.conn, &live, since_ms, now)
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
            started_at: now,
            reviewed: false,
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

/// SQL-side aggregation (decision A4/opt 4): sums *closed* segments by kind
/// in one query instead of fetching every row and summing in Rust. The
/// currently open segment (`ended_at IS NULL`) is deliberately excluded here
/// and folded in afterward by `add_current`, from the in-memory `Current` —
/// no query needed for that part.
fn compute_totals(conn: &Connection, since_ms: u64, now: u64) -> Result<Totals, String> {
    let mut statement = conn
        .prepare(
            "SELECT kind,
                    COALESCE(SUM(ended_at - MAX(started_at, ?1)), 0),
                    COALESCE(SUM(CASE WHEN reviewed = 0 THEN 1 ELSE 0 END), 0)
             FROM segments
             WHERE ended_at IS NOT NULL AND ended_at >= ?1 AND started_at < ?2
             GROUP BY kind",
        )
        .map_err(|error| error.to_string())?;

    let mut totals = Totals {
        tracked_ms: 0,
        focus_ms: 0,
        break_ms: 0,
        unreviewed: 0,
    };
    let rows = statement
        .query_map(params![since_ms as i64, now as i64], |row| {
            let kind: String = row.get(0)?;
            let total_ms: i64 = row.get(1)?;
            let unreviewed: i64 = row.get(2)?;
            Ok((kind, total_ms as u64, unreviewed as u64))
        })
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (kind, total_ms, unreviewed) = row.map_err(|error| error.to_string())?;
        match kind.as_str() {
            KIND_FOCUS => totals.focus_ms += total_ms,
            KIND_BREAK => totals.break_ms += total_ms,
            _ => totals.tracked_ms += total_ms,
        }
        totals.unreviewed += unreviewed;
    }
    Ok(totals)
}

/// Folds the currently-open segment's elapsed time into `totals`, matching
/// the clipping the old manual loop did: `[max(started_at, since), now]`.
fn add_current(totals: &mut Totals, current: &Current, since_ms: u64, now: u64) {
    let start = current.started_at.max(since_ms);
    let elapsed = now.saturating_sub(start);
    match current.kind.as_str() {
        KIND_FOCUS => totals.focus_ms += elapsed,
        KIND_BREAK => totals.break_ms += elapsed,
        _ => totals.tracked_ms += elapsed,
    }
    if !current.reviewed {
        totals.unreviewed += 1;
    }
}

fn build_tick(
    conn: &Connection,
    live: &LiveState,
    since_ms: u64,
    now: u64,
) -> Result<ActivityTick, String> {
    let mut totals = compute_totals(conn, since_ms, now)?;
    if let Some(current) = &live.current {
        add_current(&mut totals, current, since_ms, now);
    }
    Ok(ActivityTick {
        current: live.current.as_ref().map(Current::as_segment),
        tracked_ms: totals.tracked_ms,
        focus_ms: totals.focus_ms,
        break_ms: totals.break_ms,
        unreviewed: totals.unreviewed,
        idle_ms: live.last_idle_ms,
        idle_threshold_ms: live.idle_threshold_ms,
        capture_enabled: live.capture_enabled,
    })
}

fn build_snapshot(
    conn: &Connection,
    live: &LiveState,
    since_ms: u64,
    now: u64,
) -> Result<ActivitySnapshot, String> {
    let mut statement = conn
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

    let mut totals = compute_totals(conn, since_ms, now)?;
    if let Some(current) = &live.current {
        add_current(&mut totals, current, since_ms, now);
    }

    Ok(ActivitySnapshot {
        current: segments
            .iter()
            .find(|segment| segment.ended_at.is_none())
            .cloned(),
        segments,
        tracked_ms: totals.tracked_ms,
        focus_ms: totals.focus_ms,
        break_ms: totals.break_ms,
        unreviewed: totals.unreviewed,
        idle_ms: live.last_idle_ms,
        idle_threshold_ms: live.idle_threshold_ms,
        capture_enabled: live.capture_enabled,
    })
}

/// The read path for `commands::activity_snapshot`: caches `since_ms` on the
/// writer (so the sampler's ticks/heartbeats know the boundary going
/// forward), then runs the actual query against the separate reader
/// connection — see decision A6.
pub fn snapshot_for(app: &AppHandle, since_ms: u64) -> Result<ActivitySnapshot, String> {
    let state = app.state::<crate::AppState>();
    let live = {
        let mut store = state
            .activity
            .lock()
            .map_err(|_| "activity store lock poisoned".to_string())?;
        store.watch_since_ms = Some(since_ms);
        store.live_state()
    };
    let reader = state
        .activity_reader
        .lock()
        .map_err(|_| "activity reader lock poisoned".to_string())?;
    build_snapshot(&reader, &live, since_ms, now_epoch_ms())
}

/// Recomputes the full snapshot and broadcasts it on `EVENT_ACTIVITY_CHANGED`.
/// Used both for structural changes (a segment opened/closed) and for the
/// reconciliation push when OpenRize's window regains focus.
pub fn emit_full(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let result = (|| -> Result<ActivitySnapshot, String> {
        let live = {
            let store = state
                .activity
                .lock()
                .map_err(|_| "activity store lock poisoned".to_string())?;
            store.live_state()
        };
        let since_ms = live.watch_since_ms.unwrap_or(0);
        let reader = state
            .activity_reader
            .lock()
            .map_err(|_| "activity reader lock poisoned".to_string())?;
        build_snapshot(&reader, &live, since_ms, now_epoch_ms())
    })();
    match result {
        Ok(snapshot) => {
            let _ = app.emit(crate::EVENT_ACTIVITY_CHANGED, snapshot);
        }
        Err(error) => eprintln!("activity snapshot failed: {error}"),
    }
}

fn emit_tick(app: &AppHandle, now: u64) {
    let state = app.state::<crate::AppState>();
    let result = (|| -> Result<ActivityTick, String> {
        let live = {
            let store = state
                .activity
                .lock()
                .map_err(|_| "activity store lock poisoned".to_string())?;
            store.live_state()
        };
        let since_ms = live.watch_since_ms.unwrap_or(0);
        let reader = state
            .activity_reader
            .lock()
            .map_err(|_| "activity reader lock poisoned".to_string())?;
        build_tick(&reader, &live, since_ms, now)
    })();
    match result {
        Ok(tick) => {
            let _ = app.emit(crate::EVENT_ACTIVITY_TICK, tick);
        }
        Err(error) => eprintln!("activity tick failed: {error}"),
    }
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
///
/// Sampling always runs at `SAMPLE_SECS`. Pushing to the frontend is what
/// adapts to OpenRize's own focus state (decision A4): a structural change
/// (segment open/close) always pushes a full snapshot immediately; otherwise,
/// a lightweight tick pushes every iteration while focused, or only once the
/// 30s heartbeat window has elapsed while backgrounded.
pub fn spawn_sampler(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_push_at: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(SAMPLE_SECS));

            let sample = read_active_window();
            let idle_ms = read_idle_ms();
            let now = now_epoch_ms();

            let state = app.state::<crate::AppState>();
            let tick_result = state
                .activity
                .lock()
                .map_err(|_| "activity store lock poisoned".to_string())
                .and_then(|mut store| store.tick(sample, idle_ms, now));

            match tick_result {
                Ok(true) => {
                    emit_full(&app);
                    last_push_at = now;
                }
                Ok(false) => {
                    let foreground = state.foreground.load(Ordering::Relaxed);
                    if foreground || now.saturating_sub(last_push_at) >= HEARTBEAT_MS {
                        emit_tick(&app, now);
                        last_push_at = now;
                    }
                }
                Err(error) => eprintln!("activity sample failed: {error}"),
            }
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

    #[test]
    fn tick_summary_matches_snapshot_totals_without_a_segment_list() {
        let mut store = store();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();
        store.tick(sample("Code", "main.rs"), 0, 5_000).unwrap();

        let snapshot = store.snapshot(0, 5_000).unwrap();
        let tick = store.tick_summary(5_000).unwrap();

        assert_eq!(tick.tracked_ms, snapshot.tracked_ms);
        assert_eq!(tick.focus_ms, snapshot.focus_ms);
        assert_eq!(tick.break_ms, snapshot.break_ms);
        assert_eq!(tick.unreviewed, snapshot.unreviewed);
        assert_eq!(
            tick.current.map(|segment| segment.app),
            snapshot.current.map(|segment| segment.app)
        );
    }

    #[test]
    fn mark_reviewed_updates_the_open_segment_in_memory_too() {
        let mut store = store();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();
        let id = store.snapshot(0, 1_000).unwrap().current.unwrap().id;

        store.mark_reviewed(id).unwrap();
        let tick = store.tick_summary(2_000).unwrap();
        assert!(tick.current.unwrap().reviewed);
        assert_eq!(tick.unreviewed, 0);
    }

    #[test]
    fn old_closed_segments_are_rolled_up_without_deleting_raw_rows() {
        let mut store = store();
        store
            .open("Code", "main.rs", KIND_ACTIVITY, None, 1_000)
            .unwrap();
        store.close_current(61_000).unwrap();

        let far_future = 1_000 + RETENTION_MS + ONE_DAY_MS;
        store.rollup_old_segments(far_future).unwrap();

        let day_epoch = 1_000u64 / ONE_DAY_MS;
        let total_ms: i64 = store
            .conn
            .query_row(
                "SELECT total_ms FROM daily_rollups WHERE day_epoch = ?1 AND kind = ?2",
                params![day_epoch as i64, KIND_ACTIVITY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(total_ms, 60_000);

        // Raw segment is untouched.
        let snapshot = store.snapshot(0, far_future).unwrap();
        assert_eq!(snapshot.segments.len(), 1);
    }
}
