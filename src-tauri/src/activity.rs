//! Activity capture — the automatic tracker.
//!
//! Samples the OS foreground window via macOS Accessibility API (`AXUIElement`),
//! whether that app keeps the display awake (IOKit power assertions, how a
//! playing video shows up), and user idle time (`user-idle3`) every second and
//! folds consecutive samples into *segments*: one row per contiguous run of the
//! same app + window title + url.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::capture::WindowSample;
use crate::models::{
    AppContribution, AppRecord, Category, Client, EntryDetail, EntryEvent, NewCategory, NewClient,
    NewProject, NewTimeEntry, Project, TimeEntry, TitleItem, UpdateCategory, UpdateClient,
    UpdateProject, UpdateTimeEntry,
};
use crate::timers::now_epoch_ms;

pub const SAMPLE_SECS: u64 = 1;
const HEARTBEAT_MS: u64 = 30_000;
pub const DEFAULT_IDLE_THRESHOLD_MS: u64 = 5 * 60 * 1000;
pub(crate) const DB_FILE: &str = "activity.db";

const ONE_DAY_MS: u64 = 24 * 60 * 60 * 1000;
const RETENTION_DAYS: u64 = 90;
const RETENTION_MS: u64 = RETENTION_DAYS * ONE_DAY_MS;

pub const KIND_ACTIVITY: &str = "activity";
pub const KIND_FOCUS: &str = "focus";
pub const KIND_BREAK: &str = "break";

const IDLE_LABEL: &str = "Idle";

/// How long a statement waits on another connection's write lock. The AI
/// worker reads on its own connection, so contention is brief but real.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The window the background worker keeps rebuilt (see ai/worker.rs).
pub const REBUILD_WINDOW_MS: u64 = ONE_DAY_MS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub app_id: Option<String>,
    pub bundle_id: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
    pub entry_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshot {
    pub current: Option<ActivitySegment>,
    pub segments: Vec<ActivitySegment>,
    pub tracked_ms: u64,
    pub focus_ms: u64,
    pub break_ms: u64,
    pub unreviewed: u64,
    pub idle_ms: u64,
    pub idle_threshold_ms: u64,
    pub capture_enabled: bool,
    pub in_tracking_hours: bool,
    pub tracking_active: bool,
}

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
    pub in_tracking_hours: bool,
    pub tracking_active: bool,
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
    app_id: Option<String>,
    bundle_id: Option<String>,
    url: Option<String>,
    domain: Option<String>,
    entry_id: Option<String>,
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
            app_id: self.app_id.clone(),
            bundle_id: self.bundle_id.clone(),
            url: self.url.clone(),
            domain: self.domain.clone(),
            entry_id: self.entry_id.clone(),
        }
    }
}

pub struct LiveState {
    current: Option<Current>,
    capture_enabled: bool,
    idle_threshold_ms: u64,
    last_idle_ms: u64,
    watch_since_ms: Option<u64>,
    in_tracking_hours: bool,
    tracking_active: bool,
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
    /// When the foreground app was last seen keeping the display awake.
    watched_at_ms: Option<u64>,
    watch_since_ms: Option<u64>,
    tracking_hours: crate::settings::TrackingHours,
    manual_tracking: bool,
}

impl ActivityStore {
    pub fn load(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|error| format!("could not create data dir: {error}"))?;
        let path: PathBuf = dir.join(DB_FILE);
        let conn = Connection::open(&path).map_err(|error| error.to_string())?;
        Self::from_conn(conn)
    }

    pub fn open_reader(dir: &Path) -> Result<Connection, String> {
        let path = dir.join(DB_FILE);
        let conn = Connection::open(&path).map_err(|error| error.to_string())?;
        conn.busy_timeout(BUSY_TIMEOUT)
            .map_err(|error| error.to_string())?;
        Ok(conn)
    }

    pub fn from_conn(mut conn: Connection) -> Result<Self, String> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA optimize;")
            .map_err(|error| error.to_string())?;
        conn.busy_timeout(BUSY_TIMEOUT)
            .map_err(|error| error.to_string())?;

        crate::migrations::run_migrations(&mut conn).map_err(|error| error.to_string())?;

        let mut store = Self {
            conn,
            current: None,
            capture_enabled: true,
            idle_threshold_ms: DEFAULT_IDLE_THRESHOLD_MS,
            last_idle_ms: 0,
            watched_at_ms: None,
            watch_since_ms: None,
            tracking_hours: crate::settings::TrackingHours::default(),
            manual_tracking: false,
        };
        store.capture_enabled = store
            .read_setting("capture_enabled")
            .and_then(|value| value.parse().ok())
            .unwrap_or(true);
        store.idle_threshold_ms = store
            .read_setting("idle_threshold_ms")
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_IDLE_THRESHOLD_MS);

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

    /// The writer connection, for the AI worker's writes (see ai/store.rs).
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
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

    pub fn set_tracking_hours(&mut self, hours: crate::settings::TrackingHours) {
        self.tracking_hours = hours;
    }

    pub fn is_in_tracking_window(&self, now: u64) -> bool {
        self.tracking_hours.is_inside_window(now)
    }

    pub fn tick(
        &mut self,
        sample: Option<WindowSample>,
        idle_ms: u64,
        now: u64,
    ) -> Result<bool, String> {
        if sample.as_ref().is_some_and(|s| s.keeps_display_awake) {
            self.watched_at_ms = Some(now);
        }
        // Watching a video gives no input, so while an activity segment is
        // open, the foreground app keeping the display awake counts as the
        // user being there: idle runs from the last input or the last moment
        // they were watching, whichever is later. That also rides out the
        // gaps where a player briefly drops its assertion. It never ends a
        // break, though: only input brings an idle user back.
        let idle_ms = match (&self.current, self.watched_at_ms) {
            (Some(current), Some(watched_at)) if current.kind == KIND_ACTIVITY => {
                idle_ms.min(now.saturating_sub(watched_at))
            }
            _ => idle_ms,
        };
        self.last_idle_ms = idle_ms;

        if !self.capture_enabled {
            self.manual_tracking = false;
            return self.close_current(now);
        }

        let in_window = self.tracking_hours.is_inside_window(now);
        let idle = idle_ms >= self.idle_threshold_ms;
        let is_active_work = self
            .current
            .as_ref()
            .is_some_and(|cur| cur.kind == KIND_ACTIVITY);

        // Outside tracking hours with no active work session and no manual override:
        // suppress new capture. Close any idle break that was open.
        if !in_window && !self.manual_tracking && !is_active_work {
            if self.current.is_some() {
                self.close_current(now)?;
                return Ok(true);
            }
            return Ok(false);
        }

        let current = self.current.as_ref().map(|cur| {
            (
                cur.app.clone(),
                cur.title.clone(),
                cur.kind.clone(),
                cur.label.clone(),
                cur.bundle_id.clone(),
                cur.url.clone(),
            )
        });

        match current {
            None => {
                if !in_window && !self.manual_tracking {
                    return Ok(false);
                }
                if idle {
                    self.open(
                        "Idle",
                        "No activity",
                        KIND_BREAK,
                        Some(IDLE_LABEL),
                        now,
                        None,
                        None,
                        None,
                    )
                } else if let Some(sample) = sample {
                    self.open(
                        &sample.app,
                        &sample.title,
                        KIND_ACTIVITY,
                        None,
                        now,
                        sample.bundle_id.as_deref(),
                        sample.url.as_deref(),
                        sample.domain.as_deref(),
                    )
                } else {
                    Ok(false)
                }
            }
            Some((app, title, kind, label, bundle_id, url)) => {
                if idle {
                    if kind != KIND_ACTIVITY {
                        return Ok(false);
                    }
                    // The break began at the last input, not when the
                    // threshold was crossed; the time in between was idle.
                    let started_at = self.current.as_ref().map_or(now, |cur| cur.started_at);
                    let idle_since = now.saturating_sub(idle_ms).max(started_at);
                    self.close_current(idle_since)?;
                    self.manual_tracking = false;

                    // If outside window, do not open an Idle break segment
                    if !in_window {
                        return Ok(true);
                    }

                    return self.open(
                        "Idle",
                        "No activity",
                        KIND_BREAK,
                        Some(IDLE_LABEL),
                        idle_since,
                        None,
                        None,
                        None,
                    );
                }

                if kind == KIND_BREAK && label.as_deref() == Some(IDLE_LABEL) {
                    self.close_current(now)?;
                    if !in_window && !self.manual_tracking {
                        return Ok(true);
                    }
                    return match sample {
                        Some(sample) => self.open(
                            &sample.app,
                            &sample.title,
                            KIND_ACTIVITY,
                            None,
                            now,
                            sample.bundle_id.as_deref(),
                            sample.url.as_deref(),
                            sample.domain.as_deref(),
                        ),
                        None => Ok(true),
                    };
                }

                if kind != KIND_ACTIVITY {
                    return Ok(false);
                }

                match sample {
                    Some(sample)
                        if sample.app != app
                            || sample.title != title
                            || sample.url != url
                            || sample.bundle_id != bundle_id =>
                    {
                        self.close_current(now)?;
                        self.open(
                            &sample.app,
                            &sample.title,
                            KIND_ACTIVITY,
                            None,
                            now,
                            sample.bundle_id.as_deref(),
                            sample.url.as_deref(),
                            sample.domain.as_deref(),
                        )
                    }
                    _ => Ok(false),
                }
            }
        }
    }

    pub fn start_session(
        &mut self,
        kind: &str,
        label: Option<&str>,
        now: u64,
    ) -> Result<bool, String> {
        if kind != KIND_FOCUS && kind != KIND_BREAK {
            return Err(format!("cannot start a session of kind {kind}"));
        }
        self.manual_tracking = true;
        self.close_current(now)?;
        let display = label.map(str::trim).filter(|text| !text.is_empty());
        let app = if kind == KIND_FOCUS { "Focus" } else { "Break" };
        let title = display.unwrap_or(app);
        self.open(app, title, kind, display, now, None, None, None)
    }

    pub fn stop_session(&mut self, now: u64) -> Result<bool, String> {
        self.manual_tracking = false;
        self.close_current(now)
    }

    pub fn close_active_segment(&mut self, now: u64) -> Result<bool, String> {
        self.manual_tracking = false;
        self.close_current(now)
    }

    pub fn set_capture_enabled(&mut self, enabled: bool, now: u64) -> Result<(), String> {
        if enabled {
            self.capture_enabled = true;
            self.write_setting("capture_enabled", "true")?;
            if !self.is_in_tracking_window(now) {
                self.manual_tracking = true;
            }
        } else {
            if self.manual_tracking && !self.is_in_tracking_window(now) {
                self.manual_tracking = false;
                self.close_current(now)?;
            } else {
                self.manual_tracking = false;
                self.capture_enabled = false;
                self.write_setting("capture_enabled", "false")?;
                self.close_current(now)?;
            }
        }
        Ok(())
    }

    pub fn set_idle_threshold_ms(&mut self, ms: u64) -> Result<(), String> {
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

    pub fn purge_older_than(&mut self, days: u32, now: u64) -> Result<u64, String> {
        if days == 0 {
            return Ok(0);
        }
        let cutoff = now.saturating_sub(u64::from(days) * ONE_DAY_MS);
        let transaction = self.conn.transaction().map_err(|error| error.to_string())?;
        let removed = transaction
            .execute(
                "DELETE FROM segments WHERE ended_at IS NOT NULL AND ended_at < ?1",
                params![cutoff as i64],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM daily_rollups WHERE day_epoch < ?1",
                params![(cutoff / ONE_DAY_MS) as i64],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(removed as u64)
    }

    pub fn live_state(&self, now: u64) -> LiveState {
        let in_tracking_hours = self.tracking_hours.is_inside_window(now);
        let tracking_active = self.capture_enabled
            && (in_tracking_hours
                || self.manual_tracking
                || self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.kind == KIND_ACTIVITY));
        LiveState {
            current: self.current.clone(),
            capture_enabled: self.capture_enabled,
            idle_threshold_ms: self.idle_threshold_ms,
            last_idle_ms: self.last_idle_ms,
            watch_since_ms: self.watch_since_ms,
            in_tracking_hours,
            tracking_active,
        }
    }

    pub fn snapshot(&self, since_ms: u64, now: u64) -> Result<ActivitySnapshot, String> {
        build_snapshot(&self.conn, &self.live_state(now), since_ms, now)
    }

    pub fn tick_summary(&self, now: u64) -> Result<ActivityTick, String> {
        let live = self.live_state(now);
        let since_ms = live.watch_since_ms.unwrap_or(0);
        build_tick(&self.conn, &live, since_ms, now)
    }

    #[allow(clippy::too_many_arguments)]
    fn open(
        &mut self,
        app: &str,
        title: &str,
        kind: &str,
        label: Option<&str>,
        now: u64,
        bundle_id: Option<&str>,
        url: Option<&str>,
        domain: Option<&str>,
    ) -> Result<bool, String> {
        self.conn
            .execute(
                "INSERT INTO segments (app, title, kind, label, started_at, ended_at, reviewed, bundle_id, url, domain)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, 0, ?6, ?7, ?8)",
                params![app, title, kind, label, now as i64, bundle_id, url, domain],
            )
            .map_err(|error| error.to_string())?;

        let segment_id = self.conn.last_insert_rowid();

        // Browser tabs are tracked per domain, not per browser app, so a
        // domain (when present) takes priority over the bundle id as the
        // identifier; errors here are swallowed because a failed apps-table
        // upsert must not stop the segment itself from being recorded.
        if kind == KIND_ACTIVITY {
            let identifier = domain.unwrap_or(bundle_id.unwrap_or(app));
            let app_kind = if domain.is_some() { "site" } else { "app" };
            let app_id = uuid::Uuid::now_v7().to_string();
            let _ = self.conn.execute(
                "INSERT INTO apps (id, kind, identifier, display_name, excluded, first_seen, last_seen, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5, ?5, ?5)
                 ON CONFLICT(identifier) DO UPDATE SET last_seen = ?5, updated_at = ?5;",
                params![app_id, app_kind, identifier, app, now as i64],
            );
        }

        self.current = Some(Current {
            id: segment_id,
            app: app.to_string(),
            title: title.to_string(),
            kind: kind.to_string(),
            label: label.map(str::to_string),
            started_at: now,
            reviewed: false,
            app_id: None,
            bundle_id: bundle_id.map(str::to_string),
            url: url.map(str::to_string),
            domain: domain.map(str::to_string),
            entry_id: None,
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

    // --- P1: Categories CRUD --------------------------------------------

    pub fn list_categories(&self) -> Result<Vec<Category>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, color, description, ai_prompt, billable_default, counts_as_work, archived, sort, created_at, updated_at, deleted_at
                 FROM categories
                 WHERE deleted_at IS NULL
                 ORDER BY sort ASC, name ASC;",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    description: row.get(3)?,
                    ai_prompt: row.get(4)?,
                    billable_default: row.get::<_, i64>(5)? != 0,
                    counts_as_work: row.get::<_, i64>(6)? != 0,
                    archived: row.get::<_, i64>(7)? != 0,
                    sort: row.get(8)?,
                    created_at: row.get::<_, i64>(9)? as u64,
                    updated_at: row.get::<_, i64>(10)? as u64,
                    deleted_at: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                })
            })
            .map_err(|e| e.to_string())?;

        let mut list = Vec::new();
        for cat in rows {
            list.push(cat.map_err(|e| e.to_string())?);
        }
        Ok(list)
    }

    pub fn create_category(&mut self, cat: NewCategory, now: u64) -> Result<Category, String> {
        let id = uuid::Uuid::now_v7().to_string();
        let billable = cat.billable_default.unwrap_or(false) as i64;
        let counts = cat.counts_as_work.unwrap_or(true) as i64;
        let sort = cat.sort.unwrap_or(0);

        self.conn
            .execute(
                "INSERT INTO categories (id, name, color, description, ai_prompt, billable_default, counts_as_work, archived, sort, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?9);",
                params![id, cat.name, cat.color, cat.description, cat.ai_prompt, billable, counts, sort, now as i64],
            )
            .map_err(|e| e.to_string())?;

        Ok(Category {
            id,
            name: cat.name,
            color: cat.color,
            description: cat.description,
            ai_prompt: cat.ai_prompt,
            billable_default: billable != 0,
            counts_as_work: counts != 0,
            archived: false,
            sort,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    }

    pub fn update_category(
        &mut self,
        id: &str,
        patch: UpdateCategory,
        now: u64,
    ) -> Result<Category, String> {
        let mut cat = self.get_category(id)?;
        if let Some(name) = patch.name {
            cat.name = name;
        }
        if let Some(color) = patch.color {
            cat.color = color;
        }
        if let Some(desc) = patch.description {
            cat.description = Some(desc);
        }
        if let Some(prompt) = patch.ai_prompt {
            cat.ai_prompt = Some(prompt);
        }
        if let Some(b) = patch.billable_default {
            cat.billable_default = b;
        }
        if let Some(c) = patch.counts_as_work {
            cat.counts_as_work = c;
        }
        if let Some(a) = patch.archived {
            cat.archived = a;
        }
        if let Some(s) = patch.sort {
            cat.sort = s;
        }
        cat.updated_at = now;

        self.conn
            .execute(
                "UPDATE categories SET name = ?1, color = ?2, description = ?3, ai_prompt = ?4, billable_default = ?5, counts_as_work = ?6, archived = ?7, sort = ?8, updated_at = ?9
                 WHERE id = ?10;",
                params![
                    cat.name,
                    cat.color,
                    cat.description,
                    cat.ai_prompt,
                    cat.billable_default as i64,
                    cat.counts_as_work as i64,
                    cat.archived as i64,
                    cat.sort,
                    now as i64,
                    id,
                ],
            )
            .map_err(|e| e.to_string())?;

        Ok(cat)
    }

    pub fn delete_category(&mut self, id: &str, now: u64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE categories SET deleted_at = ?1, archived = 1, updated_at = ?1 WHERE id = ?2;",
                params![now as i64, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_category(&self, id: &str) -> Result<Category, String> {
        self.conn
            .query_row(
                "SELECT id, name, color, description, ai_prompt, billable_default, counts_as_work, archived, sort, created_at, updated_at, deleted_at
                 FROM categories WHERE id = ?1;",
                params![id],
                |row| {
                    Ok(Category {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        color: row.get(2)?,
                        description: row.get(3)?,
                        ai_prompt: row.get(4)?,
                        billable_default: row.get::<_, i64>(5)? != 0,
                        counts_as_work: row.get::<_, i64>(6)? != 0,
                        archived: row.get::<_, i64>(7)? != 0,
                        sort: row.get(8)?,
                        created_at: row.get::<_, i64>(9)? as u64,
                        updated_at: row.get::<_, i64>(10)? as u64,
                        deleted_at: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                    })
                },
            )
            .map_err(|e| e.to_string())
    }

    // --- P1: Projects CRUD ----------------------------------------------

    pub fn list_projects(&self) -> Result<Vec<Project>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, client_id, name, color, description, ai_hints, status, due_date, budget_kind, budget_value, budget_period, billable_default, hourly_rate, created_at, updated_at, deleted_at
                 FROM projects
                 WHERE deleted_at IS NULL
                 ORDER BY name ASC;",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    client_id: row.get(1)?,
                    name: row.get(2)?,
                    color: row.get(3)?,
                    description: row.get(4)?,
                    ai_hints: row.get(5)?,
                    status: row.get(6)?,
                    due_date: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
                    budget_kind: row.get(8)?,
                    budget_value: row.get(9)?,
                    budget_period: row.get(10)?,
                    billable_default: row.get::<_, i64>(11)? != 0,
                    hourly_rate: row.get(12)?,
                    created_at: row.get::<_, i64>(13)? as u64,
                    updated_at: row.get::<_, i64>(14)? as u64,
                    deleted_at: row.get::<_, Option<i64>>(15)?.map(|v| v as u64),
                })
            })
            .map_err(|e| e.to_string())?;

        let mut list = Vec::new();
        for p in rows {
            list.push(p.map_err(|e| e.to_string())?);
        }
        Ok(list)
    }

    pub fn create_project(&mut self, proj: NewProject, now: u64) -> Result<Project, String> {
        let id = uuid::Uuid::now_v7().to_string();
        let status = proj.status.unwrap_or_else(|| "active".to_string());
        let budget_kind = proj.budget_kind.unwrap_or_else(|| "none".to_string());
        let budget_period = proj.budget_period.unwrap_or_else(|| "total".to_string());
        let billable = proj.billable_default.unwrap_or(false) as i64;

        self.conn.execute(
            "INSERT INTO projects (id, client_id, name, color, description, ai_hints, status, due_date, budget_kind, budget_value, budget_period, billable_default, hourly_rate, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14);",
            params![
                id,
                proj.client_id,
                proj.name,
                proj.color,
                proj.description,
                proj.ai_hints,
                status,
                proj.due_date.map(|v| v as i64),
                budget_kind,
                proj.budget_value,
                budget_period,
                billable,
                proj.hourly_rate,
                now as i64,
            ],
        ).map_err(|e| e.to_string())?;
        crate::projects::sync_hint_rules(&self.conn, &id, proj.ai_hints.as_deref(), now)?;

        Ok(Project {
            id,
            client_id: proj.client_id,
            name: proj.name,
            color: proj.color,
            description: proj.description,
            ai_hints: proj.ai_hints,
            status,
            due_date: proj.due_date,
            budget_kind,
            budget_value: proj.budget_value,
            budget_period,
            billable_default: billable != 0,
            hourly_rate: proj.hourly_rate,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    }

    pub fn update_project(
        &mut self,
        id: &str,
        patch: UpdateProject,
        now: u64,
    ) -> Result<Project, String> {
        let mut proj = self.get_project(id)?;
        if let Some(client_id) = patch.client_id {
            let client_id = client_id.filter(|value| !value.is_empty());
            if client_id != proj.client_id {
                // An invoice bills a client; moving invoiced time to another
                // one would change what was already sent.
                let invoiced: bool = self
                    .conn
                    .query_row(
                        "SELECT EXISTS (SELECT 1 FROM time_entries
                         WHERE project_id = ?1 AND deleted_at IS NULL AND invoice_id IS NOT NULL);",
                        params![id],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                if invoiced {
                    return Err(
                        "This project has invoiced time, so its client can't change. Void the invoice first."
                            .to_string(),
                    );
                }
            }
            proj.client_id = client_id;
        }
        if let Some(name) = patch.name {
            proj.name = name;
        }
        if let Some(color) = patch.color {
            proj.color = color;
        }
        if let Some(desc) = patch.description {
            proj.description = desc;
        }
        let hints_changed = patch
            .ai_hints
            .as_ref()
            .is_some_and(|hints| *hints != proj.ai_hints);
        if let Some(hints) = patch.ai_hints {
            proj.ai_hints = hints;
        }
        if let Some(status) = patch.status {
            proj.status = status;
        }
        if let Some(due) = patch.due_date {
            proj.due_date = due;
        }
        if let Some(bk) = patch.budget_kind {
            proj.budget_kind = bk;
        }
        if let Some(bv) = patch.budget_value {
            proj.budget_value = bv;
        }
        if let Some(bp) = patch.budget_period {
            proj.budget_period = bp;
        }
        if let Some(b) = patch.billable_default {
            proj.billable_default = b;
        }
        if let Some(rate) = patch.hourly_rate {
            proj.hourly_rate = rate;
        }
        proj.updated_at = now;

        self.conn.execute(
            "UPDATE projects SET client_id = ?1, name = ?2, color = ?3, description = ?4, ai_hints = ?5, status = ?6, due_date = ?7, budget_kind = ?8, budget_value = ?9, budget_period = ?10, billable_default = ?11, hourly_rate = ?12, updated_at = ?13
             WHERE id = ?14;",
            params![
                proj.client_id,
                proj.name,
                proj.color,
                proj.description,
                proj.ai_hints,
                proj.status,
                proj.due_date.map(|v| v as i64),
                proj.budget_kind,
                proj.budget_value,
                proj.budget_period,
                proj.billable_default as i64,
                proj.hourly_rate,
                now as i64,
                id,
            ],
        ).map_err(|e| e.to_string())?;

        if hints_changed {
            crate::projects::sync_hint_rules(&self.conn, id, proj.ai_hints.as_deref(), now)?;
        }
        Ok(proj)
    }

    /// Only a project with no entries can be deleted; one with history is
    /// completed or archived instead, so its time keeps its project.
    pub fn delete_project(&mut self, id: &str, now: u64) -> Result<(), String> {
        let has_entries: bool = self
            .conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM time_entries WHERE project_id = ?1 AND deleted_at IS NULL);",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if has_entries {
            return Err("This project has time entries. Archive it instead.".to_string());
        }
        self.conn
            .execute(
                "UPDATE projects SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2;",
                params![now as i64, id],
            )
            .map_err(|e| e.to_string())?;
        crate::projects::sync_hint_rules(&self.conn, id, None, now)?;
        Ok(())
    }

    fn get_project(&self, id: &str) -> Result<Project, String> {
        self.conn.query_row(
            "SELECT id, client_id, name, color, description, ai_hints, status, due_date, budget_kind, budget_value, budget_period, billable_default, hourly_rate, created_at, updated_at, deleted_at
             FROM projects WHERE id = ?1;",
            params![id],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    client_id: row.get(1)?,
                    name: row.get(2)?,
                    color: row.get(3)?,
                    description: row.get(4)?,
                    ai_hints: row.get(5)?,
                    status: row.get(6)?,
                    due_date: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
                    budget_kind: row.get(8)?,
                    budget_value: row.get(9)?,
                    budget_period: row.get(10)?,
                    billable_default: row.get::<_, i64>(11)? != 0,
                    hourly_rate: row.get(12)?,
                    created_at: row.get::<_, i64>(13)? as u64,
                    updated_at: row.get::<_, i64>(14)? as u64,
                    deleted_at: row.get::<_, Option<i64>>(15)?.map(|v| v as u64),
                })
            },
        ).map_err(|e| e.to_string())
    }

    // --- P1: Clients CRUD -----------------------------------------------

    pub fn list_clients(&self) -> Result<Vec<Client>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, email, address, default_rate, currency, created_at, updated_at, deleted_at
                 FROM clients
                 WHERE deleted_at IS NULL
                 ORDER BY name ASC;",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Client {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    email: row.get(2)?,
                    address: row.get(3)?,
                    default_rate: row.get(4)?,
                    currency: row.get(5)?,
                    created_at: row.get::<_, i64>(6)? as u64,
                    updated_at: row.get::<_, i64>(7)? as u64,
                    deleted_at: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
                })
            })
            .map_err(|e| e.to_string())?;

        let mut list = Vec::new();
        for c in rows {
            list.push(c.map_err(|e| e.to_string())?);
        }
        Ok(list)
    }

    pub fn create_client(&mut self, client: NewClient, now: u64) -> Result<Client, String> {
        let id = uuid::Uuid::now_v7().to_string();
        self.conn.execute(
            "INSERT INTO clients (id, name, email, address, default_rate, currency, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7);",
            params![id, client.name, client.email, client.address, client.default_rate, client.currency, now as i64],
        ).map_err(|e| e.to_string())?;

        Ok(Client {
            id,
            name: client.name,
            email: client.email,
            address: client.address,
            default_rate: client.default_rate,
            currency: client.currency,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    }

    pub fn update_client(
        &mut self,
        id: &str,
        patch: UpdateClient,
        now: u64,
    ) -> Result<Client, String> {
        let mut client = self.get_client(id)?;
        if let Some(name) = patch.name {
            client.name = name;
        }
        if let Some(email) = patch.email {
            client.email = Some(email);
        }
        if let Some(addr) = patch.address {
            client.address = Some(addr);
        }
        if let Some(rate) = patch.default_rate {
            client.default_rate = Some(rate);
        }
        if let Some(curr) = patch.currency {
            client.currency = Some(curr);
        }
        client.updated_at = now;

        self.conn.execute(
            "UPDATE clients SET name = ?1, email = ?2, address = ?3, default_rate = ?4, currency = ?5, updated_at = ?6
             WHERE id = ?7;",
            params![
                client.name,
                client.email,
                client.address,
                client.default_rate,
                client.currency,
                now as i64,
                id,
            ],
        ).map_err(|e| e.to_string())?;

        Ok(client)
    }

    pub fn delete_client(&mut self, id: &str, now: u64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE clients SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2;",
                params![now as i64, id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_client(&self, id: &str) -> Result<Client, String> {
        self.conn.query_row(
            "SELECT id, name, email, address, default_rate, currency, created_at, updated_at, deleted_at
             FROM clients WHERE id = ?1;",
            params![id],
            |row| {
                Ok(Client {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    email: row.get(2)?,
                    address: row.get(3)?,
                    default_rate: row.get(4)?,
                    currency: row.get(5)?,
                    created_at: row.get::<_, i64>(6)? as u64,
                    updated_at: row.get::<_, i64>(7)? as u64,
                    deleted_at: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
                })
            },
        ).map_err(|e| e.to_string())
    }

    // --- P1: Time Entries -----------------------------------------------

    pub fn list_time_entries(&self, start_ms: u64, end_ms: u64) -> Result<Vec<TimeEntry>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, ended_at, description, category_id, project_id, status, approved_by, source, billable, invoice_id, created_at, updated_at, deleted_at, description_origin
                 FROM time_entries
                 WHERE deleted_at IS NULL AND ended_at >= ?1 AND started_at <= ?2
                 ORDER BY started_at ASC;",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![start_ms as i64, end_ms as i64], time_entry_from_row)
            .map_err(|e| e.to_string())?;

        let mut ai = crate::ai::store::summaries(&self.conn, start_ms, end_ms)?;
        let mut apps = crate::reports::dominant_apps(&self.conn, start_ms, end_ms)?;
        let mut list = Vec::new();
        for entry in rows {
            let mut entry = entry.map_err(|e| e.to_string())?;
            entry.ai = ai.remove(&entry.id);
            entry.dominant_app = apps.remove(&entry.id);
            list.push(entry);
        }
        Ok(list)
    }

    /// The entries with these ids, in the order given (missing ids skipped).
    pub fn time_entries(&self, ids: &[String]) -> Result<Vec<TimeEntry>, String> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok(entry) = self.time_entry(id) {
                if entry.deleted_at.is_none() {
                    out.push(entry);
                }
            }
        }
        Ok(out)
    }

    fn time_entry(&self, id: &str) -> Result<TimeEntry, String> {
        self.conn
            .query_row(
                "SELECT id, started_at, ended_at, description, category_id, project_id, status, approved_by, source, billable, invoice_id, created_at, updated_at, deleted_at, description_origin
                 FROM time_entries WHERE id = ?1;",
                params![id],
                time_entry_from_row,
            )
            .map_err(|e| e.to_string())
    }

    fn log_event(&self, entry_id: &str, kind: &str, actor: &str, payload: Option<&str>, now: u64) {
        let event_id = uuid::Uuid::now_v7().to_string();
        let _ = self.conn.execute(
            "INSERT INTO entry_events (id, entry_id, kind, actor, payload, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            params![event_id, entry_id, kind, actor, payload, now as i64],
        );
    }

    pub fn get_entry_detail(&self, id: &str) -> Result<EntryDetail, String> {
        let entry = self.conn.query_row(
            "SELECT id, started_at, ended_at, description, category_id, project_id, status, approved_by, source, billable, invoice_id, created_at, updated_at, deleted_at, description_origin
             FROM time_entries WHERE id = ?1;",
            params![id],
            time_entry_from_row,
        ).map_err(|e| e.to_string())?;

        let mut stmt = self.conn.prepare(
            "SELECT id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id
             FROM segments
             WHERE entry_id = ?1 OR (entry_id IS NULL AND started_at >= ?2 AND ended_at <= ?3 AND kind != 'break')
             ORDER BY started_at ASC;",
        ).map_err(|e| e.to_string())?;

        let segment_rows = stmt
            .query_map(
                params![id, entry.started_at as i64, entry.ended_at as i64],
                segment_from_row,
            )
            .map_err(|e| e.to_string())?;

        let mut segments = Vec::new();
        let mut app_duration: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        let mut title_duration: std::collections::HashMap<(String, String), (u64, u64)> =
            std::collections::HashMap::new();
        let mut total_work_ms = 0u64;

        for s in segment_rows {
            let seg = s.map_err(|e| e.to_string())?;
            if seg.kind != "break" {
                let dur = seg
                    .ended_at
                    .unwrap_or(entry.ended_at)
                    .saturating_sub(seg.started_at);
                *app_duration.entry(seg.app.clone()).or_insert(0) += dur;
                total_work_ms += dur;

                let key = (seg.title.clone(), seg.app.clone());
                let entry_td = title_duration.entry(key).or_insert((seg.started_at, 0));
                entry_td.1 += dur;
            }
            segments.push(seg);
        }

        let mut apps = Vec::new();
        for (app, dur) in app_duration {
            let percentage = if total_work_ms > 0 {
                (dur as f64 / total_work_ms as f64) * 100.0
            } else {
                0.0
            };
            apps.push(AppContribution {
                app,
                duration_ms: dur,
                percentage,
            });
        }
        apps.sort_by_key(|a| std::cmp::Reverse(a.duration_ms));

        let mut titles = Vec::new();
        for ((title, app), (started_at, dur)) in title_duration {
            titles.push(TitleItem {
                title,
                app,
                started_at,
                duration_ms: dur,
            });
        }
        titles.sort_by_key(|a| std::cmp::Reverse(a.duration_ms));

        let mut evt_stmt = self
            .conn
            .prepare(
                "SELECT id, entry_id, kind, actor, payload, at
                 FROM entry_events
                 WHERE entry_id = ?1
                 ORDER BY at ASC;",
            )
            .map_err(|e| e.to_string())?;

        let event_rows = evt_stmt
            .query_map(params![id], |row| {
                Ok(EntryEvent {
                    id: row.get(0)?,
                    entry_id: row.get(1)?,
                    kind: row.get(2)?,
                    actor: row.get(3)?,
                    payload: row.get(4)?,
                    at: row.get::<_, i64>(5)? as u64,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut events = Vec::new();
        for evt in event_rows {
            events.push(evt.map_err(|e| e.to_string())?);
        }

        let suggestions = crate::ai::store::latest_suggestions(&self.conn, id)?;
        let rule_suggestion = crate::ai::store::rule_suggestion(
            &self.conn,
            id,
            entry.category_id.as_deref(),
            entry.project_id.as_deref(),
            &suggestions,
        )?;
        let job = crate::ai::store::job_for(&self.conn, id)?;

        Ok(EntryDetail {
            entry,
            segments,
            apps,
            titles,
            events,
            suggestions,
            rule_suggestion,
            job,
        })
    }

    pub fn update_time_entry(
        &mut self,
        id: &str,
        patch: UpdateTimeEntry,
        now: u64,
    ) -> Result<TimeEntry, String> {
        let mut entry = self.conn.query_row(
            "SELECT id, started_at, ended_at, description, category_id, project_id, status, approved_by, source, billable, invoice_id, created_at, updated_at, deleted_at, description_origin
             FROM time_entries WHERE id = ?1;",
            params![id],
            time_entry_from_row,
        ).map_err(|e| e.to_string())?;

        if let Some(desc) = patch.description {
            if desc != entry.description {
                entry.description_origin = "user".to_string();
            }
            entry.description = desc;
        }
        if let Some(cat) = patch.category_id {
            entry.category_id = if cat.is_empty() { None } else { Some(cat) };
        }
        if let Some(proj) = patch.project_id {
            entry.project_id = if proj.is_empty() { None } else { Some(proj) };
        }
        if let Some(s) = patch.started_at {
            entry.started_at = s;
        }
        if let Some(e) = patch.ended_at {
            entry.ended_at = e;
        }
        if let Some(st) = patch.status {
            entry.status = st;
        }
        if let Some(b) = patch.billable {
            entry.billable = b;
        }
        entry.updated_at = now;

        self.conn.execute(
            "UPDATE time_entries SET started_at = ?1, ended_at = ?2, description = ?3, category_id = ?4, project_id = ?5, status = ?6, billable = ?7, updated_at = ?8, description_origin = ?9
             WHERE id = ?10;",
            params![
                entry.started_at as i64,
                entry.ended_at as i64,
                entry.description,
                entry.category_id,
                entry.project_id,
                entry.status,
                entry.billable as i64,
                now as i64,
                entry.description_origin,
                id,
            ],
        ).map_err(|e| e.to_string())?;

        // Re-categorizing an entry the AI already had approved (or the user
        // accepted) is a correction: the suggestion becomes `changed`.
        crate::ai::store::record_edit(
            &self.conn,
            id,
            entry.category_id.as_deref(),
            entry.project_id.as_deref(),
            now,
        )?;

        self.log_event(id, "edited", "user", None, now);

        Ok(entry)
    }

    pub fn approve_time_entries(
        &mut self,
        ids: &[String],
        approved_by: &str,
        now: u64,
    ) -> Result<(), String> {
        for id in ids {
            let entry = self.time_entry(id)?;
            self.conn.execute(
                "UPDATE time_entries SET status = 'approved', approved_by = ?1, updated_at = ?2 WHERE id = ?3;",
                params![approved_by, now as i64, id],
            ).map_err(|e| e.to_string())?;

            // Feedback loop: the suggestion's outcome (accepted or changed)
            // feeds calibration and rule suggestions, and the entry joins the
            // kNN store as soon as it has a vector.
            crate::ai::store::record_approval(
                &self.conn,
                id,
                entry.category_id.as_deref(),
                entry.project_id.as_deref(),
                now,
            )?;
            let has_vector: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM entry_embeddings WHERE entry_id = ?1);",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !has_vector {
                crate::ai::store::enqueue(&self.conn, id, crate::ai::store::JOB_EMBED, now)?;
            }

            self.log_event(id, "accepted", approved_by, None, now);
        }
        Ok(())
    }

    /// Reject clears the suggestion and leaves the entry pending with no
    /// category, recording negative feedback. A project the AI pre-filled is
    /// cleared too; one the user picked stays.
    pub fn reject_time_entry(&mut self, id: &str, now: u64) -> Result<(), String> {
        let entry = self.time_entry(id)?;
        let rejected = crate::ai::store::record_rejection(&self.conn, id, now)?;
        let prefilled_project = rejected.iter().any(|s| {
            s.field == crate::ai::FIELD_PROJECT
                && s.value_id.is_some()
                && s.value_id == entry.project_id
        });
        self.conn
            .execute(
                "UPDATE time_entries SET category_id = NULL,
                   project_id = CASE WHEN ?3 THEN NULL ELSE project_id END,
                   status = 'pending', approved_by = NULL, updated_at = ?1
                 WHERE id = ?2;",
                params![now as i64, id, prefilled_project],
            )
            .map_err(|e| e.to_string())?;

        self.log_event(id, "rejected", "user", None, now);
        Ok(())
    }

    /// Queues an entry for (re)classification, e.g. "Couldn't categorize ·
    /// Retry".
    pub fn queue_classification(&mut self, id: &str, now: u64) -> Result<(), String> {
        let entry = self.time_entry(id)?;
        if entry.status == "approved" {
            return Err("approved entries are frozen".to_string());
        }
        crate::ai::store::enqueue(&self.conn, id, crate::ai::store::JOB_CLASSIFY, now)
    }

    /// Accepts or dismisses an inline rule suggestion. Accepting creates a
    /// `suggested` rule; dismissing stores a disabled one so the same prompt
    /// never comes back.
    pub fn resolve_rule_suggestion(
        &mut self,
        suggestion: &crate::models::RuleSuggestion,
        accept: bool,
        now: u64,
    ) -> Result<(), String> {
        let field = crate::ai::Field::parse(&suggestion.field)?;
        if !matches!(suggestion.match_kind.as_str(), "app" | "domain") {
            return Err(format!("unsupported rule kind {}", suggestion.match_kind));
        }
        let value = suggestion.value_id.as_deref();
        let (category, project) = match field {
            crate::ai::Field::Category => (value, None),
            crate::ai::Field::Project => (None, value),
        };
        crate::ai::store::create_rule(
            &self.conn,
            &suggestion.match_kind,
            &suggestion.pattern,
            category,
            project,
            if accept { "suggested" } else { "dismissed" },
            accept,
            now,
        )?;
        Ok(())
    }

    pub fn split_time_entry(
        &mut self,
        id: &str,
        at_ms: u64,
        now: u64,
    ) -> Result<(TimeEntry, TimeEntry), String> {
        let original = self.conn.query_row(
            "SELECT id, started_at, ended_at, description, category_id, project_id, status, approved_by, source, billable, invoice_id, created_at, updated_at, deleted_at, description_origin
             FROM time_entries WHERE id = ?1;",
            params![id],
            time_entry_from_row,
        ).map_err(|e| e.to_string())?;

        if at_ms <= original.started_at || at_ms >= original.ended_at {
            return Err("Split point must be strictly inside the entry duration".to_string());
        }

        self.conn
            .execute(
                "UPDATE time_entries SET ended_at = ?1, updated_at = ?2 WHERE id = ?3;",
                params![at_ms as i64, now as i64, id],
            )
            .map_err(|e| e.to_string())?;

        let first = TimeEntry {
            ended_at: at_ms,
            updated_at: now,
            ..original.clone()
        };

        let new_id = uuid::Uuid::now_v7().to_string();
        self.conn.execute(
            "INSERT INTO time_entries (id, started_at, ended_at, description, category_id, project_id, status, approved_by, source, billable, invoice_id, created_at, updated_at, description_origin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13);",
            params![
                new_id,
                at_ms as i64,
                original.ended_at as i64,
                original.description,
                original.category_id,
                original.project_id,
                original.status,
                original.approved_by,
                original.source,
                original.billable as i64,
                original.invoice_id,
                now as i64,
                original.description_origin,
            ],
        ).map_err(|e| e.to_string())?;

        self.conn
            .execute(
                "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2 AND started_at >= ?3;",
                params![new_id, id, at_ms as i64],
            )
            .map_err(|e| e.to_string())?;

        let second = TimeEntry {
            id: new_id.clone(),
            started_at: at_ms,
            ended_at: original.ended_at,
            created_at: now,
            updated_at: now,
            ..original
        };

        self.log_event(id, "split", "user", None, now);

        Ok((first, second))
    }

    pub fn delete_time_entry(&mut self, id: &str, now: u64) -> Result<(), String> {
        self.delete_time_entries(&[id.to_string()], now)
    }

    pub fn delete_time_entries(&mut self, ids: &[String], now: u64) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut delete_entry = tx
                .prepare("UPDATE time_entries SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2;")
                .map_err(|e| e.to_string())?;
            let mut delete_segments = tx
                .prepare("DELETE FROM segments WHERE entry_id = ?1;")
                .map_err(|e| e.to_string())?;
            for id in ids {
                delete_entry
                    .execute(params![now as i64, id])
                    .map_err(|e| e.to_string())?;
                delete_segments
                    .execute(params![id])
                    .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn create_manual_entry(
        &mut self,
        new_entry: NewTimeEntry,
        now: u64,
    ) -> Result<TimeEntry, String> {
        if new_entry.ended_at <= new_entry.started_at {
            return Err("Entry end must be after its start".to_string());
        }
        let unclassified = new_entry.description.trim().is_empty()
            && new_entry.category_id.is_none()
            && new_entry.project_id.is_none();
        if new_entry.description.trim().is_empty() && !unclassified {
            return Err("Entry description cannot be empty".to_string());
        }
        let id = uuid::Uuid::now_v7().to_string();
        let billable = new_entry.billable.unwrap_or(false) as i64;
        let status = if unclassified { "pending" } else { "approved" };
        let approved_by = if unclassified { None } else { Some("user") };

        self.conn.execute(
            "INSERT INTO time_entries (id, started_at, ended_at, description, category_id, project_id, status, approved_by, source, billable, created_at, updated_at, description_origin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'manual', ?9, ?10, ?10, 'user');",
            params![
                id,
                new_entry.started_at as i64,
                new_entry.ended_at as i64,
                new_entry.description,
                new_entry.category_id,
                new_entry.project_id,
                status,
                approved_by,
                billable,
                now as i64,
            ],
        ).map_err(|e| e.to_string())?;

        self.log_event(&id, "created", "user", None, now);
        if unclassified && new_entry.ended_at <= now {
            crate::ai::store::enqueue(&self.conn, &id, crate::ai::store::JOB_CLASSIFY, now)?;
        } else if !unclassified {
            // A labeled hand-made entry contributes to kNN.
            crate::ai::store::enqueue(&self.conn, &id, crate::ai::store::JOB_EMBED, now)?;
        }

        Ok(TimeEntry {
            id,
            started_at: new_entry.started_at,
            ended_at: new_entry.ended_at,
            description: new_entry.description,
            category_id: new_entry.category_id,
            project_id: new_entry.project_id,
            status: if unclassified && new_entry.ended_at <= now {
                "processing"
            } else {
                status
            }
            .to_string(),
            approved_by: approved_by.map(str::to_string),
            source: "manual".to_string(),
            billable: billable != 0,
            invoice_id: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
            description_origin: "user".to_string(),
            ai: None,
            dominant_app: None,
        })
    }

    pub fn rebuild_time_entries_in_range(
        &mut self,
        start_ms: u64,
        end_ms: u64,
        now: u64,
    ) -> Result<Vec<TimeEntry>, String> {
        self.rebuild_range(start_ms, end_ms, now)?;
        self.list_time_entries(start_ms, end_ms)
    }

    /// Re-runs the entry builder over the range and saves the result.
    ///
    /// Only the open (`building`) entry is re-segmented; every closed entry is
    /// frozen, so its id, suggestion, and edits survive. An entry that has
    /// just closed is queued for classification. Returns whether anything a
    /// view shows changed (an entry closed, appeared, or went away), as
    /// opposed to the open entry merely growing.
    pub fn rebuild_range(&mut self, start_ms: u64, end_ms: u64, now: u64) -> Result<bool, String> {
        use crate::entry_builder::{build_entries, BuiltTimeEntry, EntrySettings, SegmentInput};

        let tx = self.conn.transaction().map_err(|e| e.to_string())?;

        let segments: Vec<SegmentInput> = {
            let mut stmt = tx
                .prepare(
                    "SELECT id, app, title, kind, label, started_at, ended_at, entry_id
                     FROM segments
                     WHERE started_at >= ?1 AND started_at <= ?2
                     ORDER BY started_at ASC;",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![start_ms as i64, end_ms as i64], |row| {
                    Ok(SegmentInput {
                        id: row.get(0)?,
                        app: row.get(1)?,
                        title: row.get(2)?,
                        kind: row.get(3)?,
                        label: row.get(4)?,
                        started_at: row.get::<_, i64>(5)? as u64,
                        ended_at: row.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                        entry_id: row.get(7)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<rusqlite::Result<_>>()
                .map_err(|e| e.to_string())?
        };

        // Every entry touching the range, deleted ones included: a deleted
        // entry's time stays unassigned instead of being rebuilt.
        let existing: Vec<TimeEntry> = {
            let mut stmt = tx
                .prepare(
                    "SELECT id, started_at, ended_at, description, category_id, project_id, status, approved_by, source, billable, invoice_id, created_at, updated_at, deleted_at, description_origin
                     FROM time_entries
                     WHERE ended_at >= ?1 AND started_at <= ?2;",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![start_ms as i64, end_ms as i64], time_entry_from_row)
                .map_err(|e| e.to_string())?;
            rows.collect::<rusqlite::Result<_>>()
                .map_err(|e| e.to_string())?
        };
        let is_open = |e: &TimeEntry| e.deleted_at.is_none() && e.status == "building";
        let frozen: Vec<BuiltTimeEntry> = existing
            .iter()
            .filter(|e| !is_open(e))
            .map(|e| BuiltTimeEntry {
                id: e.id.clone(),
                started_at: e.started_at,
                ended_at: e.ended_at,
                description: e.description.clone(),
                category_id: e.category_id.clone(),
                project_id: e.project_id.clone(),
                status: e.status.clone(),
                approved_by: e.approved_by.clone(),
                source: e.source.clone(),
                billable: e.billable,
                invoice_id: e.invoice_id.clone(),
                created_at: e.created_at,
                updated_at: e.updated_at,
                deleted_at: e.deleted_at,
                segment_ids: Vec::new(),
            })
            .collect();
        let frozen_ids: std::collections::HashSet<&str> =
            frozen.iter().map(|e| e.id.as_str()).collect();

        let built = build_entries(&segments, &frozen, &EntrySettings::default(), now);
        let linked: std::collections::HashMap<i64, Option<&str>> = segments
            .iter()
            .map(|s| (s.id, s.entry_id.as_deref()))
            .collect();

        let mut changed = false;
        let mut kept = std::collections::HashSet::new();
        for entry in built.iter().filter(|e| !frozen_ids.contains(e.id.as_str())) {
            kept.insert(entry.id.clone());
            let before = existing.iter().find(|e| e.id == entry.id);
            tx.execute(
                "INSERT INTO time_entries (id, started_at, ended_at, description, category_id, project_id, status, source, billable, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, 'auto', 0, ?6, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                   started_at = excluded.started_at,
                   ended_at = excluded.ended_at,
                   status = excluded.status,
                   description = CASE WHEN time_entries.description_origin = 'template'
                                      THEN excluded.description ELSE time_entries.description END,
                   updated_at = excluded.updated_at;",
                params![
                    entry.id,
                    entry.started_at as i64,
                    entry.ended_at as i64,
                    entry.description,
                    entry.status,
                    now as i64,
                ],
            )
            .map_err(|e| e.to_string())?;
            // Only newly linked segments: a long session is rebuilt often.
            for segment_id in &entry.segment_ids {
                if linked.get(segment_id) == Some(&Some(entry.id.as_str())) {
                    continue;
                }
                tx.execute(
                    "UPDATE segments SET entry_id = ?1 WHERE id = ?2;",
                    params![entry.id, segment_id],
                )
                .map_err(|e| e.to_string())?;
            }

            let status_changed = before.map(|e| e.status.as_str()) != Some(entry.status.as_str());
            changed |= status_changed;
            if status_changed && entry.status == "pending" {
                crate::ai::store::enqueue(&tx, &entry.id, crate::ai::store::JOB_CLASSIFY, now)?;
            }
        }

        // An open entry whose segments all moved elsewhere no longer exists.
        for stale in existing
            .iter()
            .filter(|e| is_open(e) && !kept.contains(&e.id))
        {
            tx.execute(
                "UPDATE time_entries SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2;",
                params![now as i64, stale.id],
            )
            .map_err(|e| e.to_string())?;
            changed = true;
        }

        // Entries from before the AI pipeline existed (or whose job was lost)
        // get classified the first time their day is rebuilt.
        changed |= crate::ai::store::enqueue_unclassified(&tx, start_ms, end_ms, now)? > 0;

        tx.commit().map_err(|e| e.to_string())?;
        Ok(changed)
    }

    // --- P1: Apps -------------------------------------------------------

    pub fn list_apps(&self) -> Result<Vec<AppRecord>, String> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, identifier, display_name, default_category_id, default_project_id, excluded, first_seen, last_seen, created_at, updated_at, deleted_at
             FROM apps
             WHERE deleted_at IS NULL
             ORDER BY last_seen DESC;",
        ).map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(AppRecord {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    identifier: row.get(2)?,
                    display_name: row.get(3)?,
                    default_category_id: row.get(4)?,
                    default_project_id: row.get(5)?,
                    excluded: row.get::<_, i64>(6)? != 0,
                    first_seen: row.get::<_, i64>(7)? as u64,
                    last_seen: row.get::<_, i64>(8)? as u64,
                    created_at: row.get::<_, i64>(9)? as u64,
                    updated_at: row.get::<_, i64>(10)? as u64,
                    deleted_at: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                })
            })
            .map_err(|e| e.to_string())?;

        let mut list = Vec::new();
        for a in rows {
            list.push(a.map_err(|e| e.to_string())?);
        }
        Ok(list)
    }

    pub fn update_app(
        &mut self,
        id: &str,
        default_category_id: Option<String>,
        default_project_id: Option<String>,
        excluded: Option<bool>,
        now: u64,
    ) -> Result<AppRecord, String> {
        if let Some(cat) = default_category_id.as_deref() {
            self.conn
                .execute(
                    "UPDATE apps SET default_category_id = ?1, updated_at = ?2 WHERE id = ?3;",
                    params![cat, now as i64, id],
                )
                .map_err(|e| e.to_string())?;
        }
        if let Some(proj) = default_project_id.as_deref() {
            self.conn
                .execute(
                    "UPDATE apps SET default_project_id = ?1, updated_at = ?2 WHERE id = ?3;",
                    params![proj, now as i64, id],
                )
                .map_err(|e| e.to_string())?;
        }
        if let Some(exc) = excluded {
            self.conn
                .execute(
                    "UPDATE apps SET excluded = ?1, updated_at = ?2 WHERE id = ?3;",
                    params![exc as i64, now as i64, id],
                )
                .map_err(|e| e.to_string())?;
        }

        self.conn.query_row(
            "SELECT id, kind, identifier, display_name, default_category_id, default_project_id, excluded, first_seen, last_seen, created_at, updated_at, deleted_at
             FROM apps WHERE id = ?1;",
            params![id],
            |row| {
                Ok(AppRecord {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    identifier: row.get(2)?,
                    display_name: row.get(3)?,
                    default_category_id: row.get(4)?,
                    default_project_id: row.get(5)?,
                    excluded: row.get::<_, i64>(6)? != 0,
                    first_seen: row.get::<_, i64>(7)? as u64,
                    last_seen: row.get::<_, i64>(8)? as u64,
                    created_at: row.get::<_, i64>(9)? as u64,
                    updated_at: row.get::<_, i64>(10)? as u64,
                    deleted_at: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                })
            },
        ).map_err(|e| e.to_string())
    }
}

/// Maps the `time_entries` column list used throughout this module (the 14
/// P1 columns plus `description_origin`).
pub(crate) fn time_entry_from_row(row: &Row<'_>) -> rusqlite::Result<TimeEntry> {
    Ok(TimeEntry {
        id: row.get(0)?,
        started_at: row.get::<_, i64>(1)? as u64,
        ended_at: row.get::<_, i64>(2)? as u64,
        description: row.get(3)?,
        category_id: row.get(4)?,
        project_id: row.get(5)?,
        status: row.get(6)?,
        approved_by: row.get(7)?,
        source: row.get(8)?,
        billable: row.get::<_, i64>(9)? != 0,
        invoice_id: row.get(10)?,
        created_at: row.get::<_, i64>(11)? as u64,
        updated_at: row.get::<_, i64>(12)? as u64,
        deleted_at: row.get::<_, Option<i64>>(13)?.map(|v| v as u64),
        description_origin: row.get(14)?,
        ai: None,
        dominant_app: None,
    })
}

pub(crate) fn segment_from_row(row: &Row<'_>) -> rusqlite::Result<ActivitySegment> {
    Ok(ActivitySegment {
        id: row.get(0)?,
        app: row.get(1)?,
        title: row.get(2)?,
        kind: row.get(3)?,
        label: row.get(4)?,
        started_at: row.get::<_, i64>(5)? as u64,
        ended_at: row.get::<_, Option<i64>>(6)?.map(|ms| ms as u64),
        reviewed: row.get::<_, i64>(7)? != 0,
        app_id: row.get(8).ok(),
        bundle_id: row.get(9).ok(),
        url: row.get(10).ok(),
        domain: row.get(11).ok(),
        entry_id: row.get(12).ok(),
    })
}

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
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)? as u64,
            ))
        })
        .map_err(|error| error.to_string())?;

    for row in rows {
        let (kind, duration_ms, unreviewed) = row.map_err(|error| error.to_string())?;
        match kind.as_str() {
            KIND_ACTIVITY => {
                totals.tracked_ms += duration_ms;
                totals.unreviewed += unreviewed;
            }
            KIND_FOCUS => {
                totals.tracked_ms += duration_ms;
                totals.focus_ms += duration_ms;
            }
            KIND_BREAK => {
                totals.break_ms += duration_ms;
            }
            _ => {}
        }
    }
    Ok(totals)
}

fn fetch_segments(
    conn: &Connection,
    since_ms: u64,
    now: u64,
) -> Result<Vec<ActivitySegment>, String> {
    let mut statement = conn
        .prepare(
            "SELECT id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id
             FROM segments
             WHERE ended_at IS NULL OR (ended_at >= ?1 AND started_at < ?2)
             ORDER BY started_at ASC",
        )
        .map_err(|error| error.to_string())?;

    let rows = statement
        .query_map(params![since_ms as i64, now as i64], segment_from_row)
        .map_err(|error| error.to_string())?;

    let mut segments = Vec::new();
    for row in rows {
        segments.push(row.map_err(|error| error.to_string())?);
    }
    Ok(segments)
}

fn add_current(totals: &mut Totals, current: Option<&Current>, since_ms: u64, now: u64) {
    if let Some(current) = current {
        let effective_start = current.started_at.max(since_ms);
        let active_ms = now.saturating_sub(effective_start);
        match current.kind.as_str() {
            KIND_ACTIVITY => {
                totals.tracked_ms += active_ms;
                if !current.reviewed {
                    totals.unreviewed += 1;
                }
            }
            KIND_FOCUS => {
                totals.tracked_ms += active_ms;
                totals.focus_ms += active_ms;
            }
            KIND_BREAK => {
                totals.break_ms += active_ms;
            }
            _ => {}
        }
    }
}

fn build_snapshot(
    conn: &Connection,
    live: &LiveState,
    since_ms: u64,
    now: u64,
) -> Result<ActivitySnapshot, String> {
    let mut totals = compute_totals(conn, since_ms, now)?;
    add_current(&mut totals, live.current.as_ref(), since_ms, now);

    let mut segments = fetch_segments(conn, since_ms, now)?;
    if let Some(current) = &live.current {
        if !segments.iter().any(|s| s.id == current.id) {
            segments.push(current.as_segment());
        }
    }

    Ok(ActivitySnapshot {
        current: live.current.as_ref().map(Current::as_segment),
        segments,
        tracked_ms: totals.tracked_ms,
        focus_ms: totals.focus_ms,
        break_ms: totals.break_ms,
        unreviewed: totals.unreviewed,
        idle_ms: live.last_idle_ms,
        idle_threshold_ms: live.idle_threshold_ms,
        capture_enabled: live.capture_enabled,
        in_tracking_hours: live.in_tracking_hours,
        tracking_active: live.tracking_active,
    })
}

pub fn build_tick(
    conn: &Connection,
    live: &LiveState,
    since_ms: u64,
    now: u64,
) -> Result<ActivityTick, String> {
    let mut totals = compute_totals(conn, since_ms, now)?;
    add_current(&mut totals, live.current.as_ref(), since_ms, now);

    Ok(ActivityTick {
        current: live.current.as_ref().map(Current::as_segment),
        tracked_ms: totals.tracked_ms,
        focus_ms: totals.focus_ms,
        break_ms: totals.break_ms,
        unreviewed: totals.unreviewed,
        idle_ms: live.last_idle_ms,
        idle_threshold_ms: live.idle_threshold_ms,
        capture_enabled: live.capture_enabled,
        in_tracking_hours: live.in_tracking_hours,
        tracking_active: live.tracking_active,
    })
}

pub fn snapshot_for(app: &AppHandle, since_ms: u64) -> Result<ActivitySnapshot, String> {
    let state = app.state::<crate::AppState>();
    let now = now_epoch_ms();
    let live = {
        let mut store = state
            .activity
            .lock()
            .map_err(|_| "activity store lock poisoned".to_string())?;
        store.watch_since_ms = Some(since_ms);
        store.live_state(now)
    };
    let reader = state
        .activity_reader
        .lock()
        .map_err(|_| "activity reader lock poisoned".to_string())?;
    build_snapshot(&reader, &live, since_ms, now)
}

pub fn emit_full(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let now = now_epoch_ms();
    let result = (|| -> Result<ActivitySnapshot, String> {
        let live = {
            let store = state
                .activity
                .lock()
                .map_err(|_| "activity store lock poisoned".to_string())?;
            store.live_state(now)
        };
        let since_ms = live.watch_since_ms.unwrap_or(0);
        let reader = state
            .activity_reader
            .lock()
            .map_err(|_| "activity reader lock poisoned".to_string())?;
        build_snapshot(&reader, &live, since_ms, now)
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
            store.live_state(now)
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

pub(crate) fn read_idle_ms() -> u64 {
    user_idle3::UserIdle::get_time()
        .map(|idle| idle.duration().as_millis() as u64)
        .unwrap_or(0)
}

pub fn spawn_sampler(app: AppHandle) {
    std::thread::spawn(move || {
        // App Nap assertion: prevents throttling while capture runs in background
        let _app_nap =
            crate::capture::AppNapAssertion::begin("OpenRize background activity capture");

        let mut last_push_at: u64 = 0;
        let mut last_sample_time: u64 = 0;

        loop {
            std::thread::sleep(Duration::from_secs(SAMPLE_SECS));

            let now = now_epoch_ms();
            let sample = crate::capture::read_active_window();
            let idle_ms = read_idle_ms();

            let state = app.state::<crate::AppState>();

            // Sleep / lid-close defense: if a gap of >10s occurred between 1s ticks,
            // close active segment at last_sample_time + 1000 so sleep time is not counted.
            if last_sample_time > 0 && now.saturating_sub(last_sample_time) > 10_000 {
                if let Ok(mut store) = state.activity.lock() {
                    let _ = store.close_active_segment(last_sample_time + 1000);
                }
            }
            last_sample_time = now;

            let tick_result = state
                .activity
                .lock()
                .map_err(|_| "activity store lock poisoned".to_string())
                .and_then(|mut store| store.tick(sample, idle_ms, now));

            match tick_result {
                Ok(true) => {
                    emit_full(&app);
                    // A segment opened or closed, so an entry may have just
                    // closed: let the AI worker rebuild and classify it.
                    crate::ai::nudge(&app);
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
        let mut s = ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
            .expect("schema");
        s.tracking_hours.enabled = false;
        s
    }

    fn sample(app: &str, title: &str) -> Option<WindowSample> {
        Some(WindowSample {
            app: app.to_string(),
            title: title.to_string(),
            bundle_id: None,
            url: None,
            domain: None,
            keeps_display_awake: false,
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
    fn deleting_an_entry_also_deletes_its_activity_segments() {
        let mut store = store();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();
        store.tick(sample("Slack", "#general"), 0, 5_000).unwrap();
        let entries = store
            .rebuild_time_entries_in_range(0, 5_000, 5_000)
            .unwrap();
        let code_entry = entries
            .iter()
            .find(|entry| entry.description.contains("Code"))
            .unwrap();

        store.delete_time_entry(&code_entry.id, 6_000).unwrap();

        let remaining = store.snapshot(0, 6_000).unwrap();
        assert_eq!(remaining.segments.len(), 1);
        assert_eq!(remaining.segments[0].app, "Slack");
        let linked: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM segments WHERE entry_id = ?1;",
                params![code_entry.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(linked, 0);
    }

    #[test]
    fn idle_crosses_into_an_automatic_break_and_back() {
        let mut store = store();
        store.set_idle_threshold_ms(60_000).unwrap();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();

        store
            .tick(sample("Code", "main.rs"), 90_000, 100_000)
            .unwrap();
        assert_eq!(
            store.snapshot(0, 100_000).unwrap().current.unwrap().kind,
            KIND_BREAK
        );

        store.tick(sample("Code", "main.rs"), 0, 130_000).unwrap();
        let snapshot = store.snapshot(0, 130_000).unwrap();
        let current = snapshot.current.unwrap();
        assert_eq!(current.kind, KIND_ACTIVITY);
        // The break is backdated to the last input, 90s before it was noticed.
        assert_eq!(snapshot.segments[0].ended_at, Some(10_000));
        assert_eq!(snapshot.break_ms, 120_000);
    }

    /// Ticks once a second through `[from, to)` with Zen in front. `inputs`
    /// are the moments the user touched the keyboard or mouse, and
    /// `watching` says whether Zen keeps the display awake at a moment.
    fn watch_zen(
        store: &mut ActivityStore,
        (from, to): (u64, u64),
        inputs: &[u64],
        watching: impl Fn(u64) -> bool,
    ) {
        for now in (from..to).step_by(1_000) {
            let last_input = inputs.iter().rev().find(|&&at| at <= now).unwrap();
            let mut zen = sample("Zen", "").unwrap();
            zen.keeps_display_awake = watching(now);
            store.tick(Some(zen), now - last_input, now).unwrap();
        }
    }

    /// Three hours of a show in Zen, touched only now and then, the way the
    /// evening of 2026-09-25 was captured: the player drops its display
    /// assertion for 40 seconds every 10 minutes (between episodes, ads) and
    /// the show is paused at 3h, after which nobody touches the Mac.
    const SHOW_END: u64 = 180 * MIN;
    const SHOW_INPUTS: [u64; 6] = [0, 21 * MIN, 47 * MIN, 88 * MIN, 131 * MIN, 170 * MIN];

    fn show_playing(at: u64) -> bool {
        at < SHOW_END && (at < 10 * MIN || at % (10 * MIN) >= 40_000)
    }

    #[test]
    fn a_show_watched_in_the_foreground_is_one_continuous_session() {
        let mut store = store();
        watch_zen(
            &mut store,
            (0, SHOW_END + 10 * MIN),
            &SHOW_INPUTS,
            show_playing,
        );

        let segments = store.snapshot(0, SHOW_END + 10 * MIN).unwrap().segments;
        let spans: Vec<_> = segments
            .iter()
            .map(|s| (s.kind.as_str(), s.started_at, s.ended_at))
            .collect();
        // The break starts when the show stopped playing, not at the last
        // input 10 minutes before it.
        let paused = SHOW_END - 1_000;
        assert_eq!(
            spans,
            vec![(KIND_ACTIVITY, 0, Some(paused)), (KIND_BREAK, paused, None)]
        );

        let entries = store
            .rebuild_time_entries_in_range(0, SHOW_END + 10 * MIN, SHOW_END + 10 * MIN)
            .unwrap();
        let entries: Vec<_> = entries
            .iter()
            .map(|e| (e.started_at, e.ended_at, e.status.as_str()))
            .collect();
        // Closed, so it is queued for classification.
        assert_eq!(entries, vec![(0, paused, "processing")]);
    }

    #[test]
    fn the_same_evening_without_a_display_assertion_is_idle() {
        // The counterfactual: identical input, but Zen never keeps the
        // display awake, so every stretch without input is a break and each
        // input leaves only an instant of activity, too short to be an entry.
        let mut store = store();
        watch_zen(&mut store, (0, SHOW_END + 10 * MIN), &SHOW_INPUTS, |_| {
            false
        });

        let snapshot = store.snapshot(0, SHOW_END + 10 * MIN).unwrap();
        assert_eq!(snapshot.tracked_ms, 0);
        let entries = store
            .rebuild_time_entries_in_range(0, SHOW_END + 10 * MIN, SHOW_END + 10 * MIN)
            .unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn a_video_does_not_end_an_idle_break() {
        // Autoplay after the user left must not bring them back: only input
        // ends a break.
        let mut store = store();
        watch_zen(&mut store, (0, 10 * MIN), &[0], |_| false);
        watch_zen(&mut store, (10 * MIN, 20 * MIN), &[0], |_| true);

        let snapshot = store.snapshot(0, 20 * MIN).unwrap();
        assert_eq!(snapshot.current.unwrap().kind, KIND_BREAK);
        assert_eq!(snapshot.segments.len(), 2);
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
        store.set_capture_enabled(false, 5_000).unwrap();
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
    fn categories_crud_works() {
        let mut store = store();
        let cats = store.list_categories().unwrap();
        assert_eq!(cats.len(), 12); // Seeded default 12 categories

        let created = store
            .create_category(
                NewCategory {
                    name: "Custom Category".to_string(),
                    color: "#123456".to_string(),
                    description: Some("Custom desc".to_string()),
                    ai_prompt: None,
                    billable_default: Some(true),
                    counts_as_work: Some(true),
                    sort: Some(13),
                },
                1_000,
            )
            .unwrap();
        assert_eq!(created.name, "Custom Category");

        let updated = store
            .update_category(
                &created.id,
                UpdateCategory {
                    name: Some("Renamed Category".to_string()),
                    color: None,
                    description: None,
                    ai_prompt: None,
                    billable_default: None,
                    counts_as_work: None,
                    archived: None,
                    sort: None,
                },
                2_000,
            )
            .unwrap();
        assert_eq!(updated.name, "Renamed Category");

        store.delete_category(&created.id, 3_000).unwrap();
        let remaining = store.list_categories().unwrap();
        assert_eq!(remaining.len(), 12);
    }

    #[test]
    fn projects_and_clients_crud_works() {
        let mut store = store();
        let client = store
            .create_client(
                NewClient {
                    name: "Acme Corp".to_string(),
                    email: Some("contact@acme.com".to_string()),
                    address: None,
                    default_rate: Some(120.0),
                    currency: Some("USD".to_string()),
                },
                1_000,
            )
            .unwrap();
        assert_eq!(client.name, "Acme Corp");

        let proj = store
            .create_project(
                NewProject {
                    client_id: Some(client.id.clone()),
                    name: "Acme Web".to_string(),
                    color: "#75a4e5".to_string(),
                    description: Some("Web platform".to_string()),
                    ai_hints: Some("acme, web".to_string()),
                    status: Some("active".to_string()),
                    due_date: None,
                    budget_kind: Some("hours".to_string()),
                    budget_value: Some(50.0),
                    budget_period: Some("total".to_string()),
                    billable_default: Some(true),
                    hourly_rate: Some(120.0),
                },
                2_000,
            )
            .unwrap();
        assert_eq!(proj.name, "Acme Web");

        let projects = store.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
    }

    const MIN: u64 = 60_000;

    /// Code then Slack for 20 minutes, 10 minutes away, then back to Code:
    /// the first session closes, the second is still building.
    fn tracked_morning(store: &mut ActivityStore) {
        store.set_idle_threshold_ms(5 * MIN).unwrap();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();
        store
            .tick(sample("Slack", "#general"), 0, 12 * MIN)
            .unwrap();
        store
            .tick(sample("Slack", "#general"), 5 * MIN, 25 * MIN)
            .unwrap();
        store.tick(sample("Code", "main.rs"), 0, 35 * MIN).unwrap();
    }

    fn live_entries(store: &ActivityStore) -> Vec<TimeEntry> {
        store.list_time_entries(0, u64::MAX / 2).unwrap()
    }

    #[test]
    fn rebuilding_the_same_day_never_duplicates_entries() {
        let mut store = store();
        tracked_morning(&mut store);
        let first = store
            .rebuild_time_entries_in_range(0, 60 * MIN, 36 * MIN)
            .unwrap();
        let second = store
            .rebuild_time_entries_in_range(0, 60 * MIN, 37 * MIN)
            .unwrap();
        let third = store
            .rebuild_time_entries_in_range(0, 60 * MIN, 38 * MIN)
            .unwrap();
        assert_eq!(first.len(), 2);
        let ids = |list: &[TimeEntry]| list.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&first), ids(&second));
        assert_eq!(ids(&second), ids(&third));
        // The open block kept its id while it grew.
        assert_eq!(third[1].status, "building");
        assert_eq!(third[1].ended_at, 38 * MIN);
    }

    #[test]
    fn live_classification_runs_at_thirty_then_every_fifteen_minutes() {
        let mut store = store();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();
        let start = 1_000;
        for minute in [29, 30, 44, 45, 59, 60] {
            let now = start + minute * MIN;
            store.rebuild_range(0, now, now).unwrap();
            let count = crate::ai::store::enqueue_live_due(store.conn(), now).unwrap();
            assert_eq!(count, usize::from(matches!(minute, 30 | 45 | 60)));
            if count > 0 {
                let entry = live_entries(&store).remove(0);
                let job = crate::ai::store::next_due(store.conn(), now)
                    .unwrap()
                    .unwrap();
                assert_eq!(entry.id, job.entry_id);
                crate::ai::store::set_job_state(store.conn(), &job.id, "done", now).unwrap();
            }
        }
        store.stop_session(start + 61 * MIN).unwrap();
        store
            .rebuild_range(0, start + 70 * MIN, start + 70 * MIN)
            .unwrap();
        assert_eq!(
            crate::ai::store::enqueue_live_due(store.conn(), start + 75 * MIN).unwrap(),
            0
        );
    }

    #[test]
    fn overnight_session_stays_in_previous_five_am_day() {
        let mut store = store();
        let hour = 60 * MIN;
        store
            .tick(sample("Code", "night.rs"), 0, 23 * hour + 30 * MIN)
            .unwrap();
        store.stop_session(28 * hour).unwrap();
        let entries = store
            .rebuild_time_entries_in_range(5 * hour, 29 * hour, 29 * hour)
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].started_at, 23 * hour + 30 * MIN);
        assert_eq!(entries[0].ended_at, 28 * hour);
        assert_eq!(entries[0].status, "processing");
    }

    #[test]
    fn blank_manual_entry_is_classified_only_after_its_end() {
        let mut store = store();
        let entry = store
            .create_manual_entry(
                NewTimeEntry {
                    started_at: 1_000,
                    ended_at: 16 * MIN,
                    description: String::new(),
                    category_id: None,
                    project_id: None,
                    billable: None,
                },
                2_000,
            )
            .unwrap();
        assert_eq!(entry.status, "pending");
        store.rebuild_range(0, 20 * MIN, 15 * MIN).unwrap();
        assert!(crate::ai::store::next_due(store.conn(), 15 * MIN)
            .unwrap()
            .is_none());
        store.rebuild_range(0, 20 * MIN, 16 * MIN).unwrap();
        assert_eq!(live_entries(&store)[0].status, "processing");
        assert!(crate::ai::store::next_due(store.conn(), 16 * MIN)
            .unwrap()
            .is_some());
    }

    #[test]
    fn a_closed_entry_is_queued_for_classification() {
        let mut store = store();
        tracked_morning(&mut store);
        store.rebuild_range(0, 60 * MIN, 36 * MIN).unwrap();
        let entries = live_entries(&store);
        assert_eq!(entries[0].status, "processing");
        assert_eq!(
            entries[0].ai.as_ref().and_then(|ai| ai.state.as_deref()),
            Some("queued")
        );
        assert_eq!(entries[1].status, "building");
        assert_eq!(crate::ai::store::queued_count(&store.conn).unwrap(), 1);
    }

    #[test]
    fn an_edited_description_survives_a_rebuild() {
        let mut store = store();
        store.tick(sample("Code", "main.rs"), 0, 1_000).unwrap();
        store.rebuild_range(0, 60 * MIN, 5 * MIN).unwrap();
        let id = live_entries(&store)[0].id.clone();
        store
            .update_time_entry(
                &id,
                UpdateTimeEntry {
                    description: Some("Fixed the rebuild".into()),
                    category_id: None,
                    project_id: None,
                    started_at: None,
                    ended_at: None,
                    status: None,
                    billable: None,
                },
                6 * MIN,
            )
            .unwrap();
        store.rebuild_range(0, 60 * MIN, 7 * MIN).unwrap();
        let entry = &live_entries(&store)[0];
        assert_eq!(entry.id, id);
        assert_eq!(entry.description, "Fixed the rebuild");
        assert_eq!(entry.ended_at, 7 * MIN);
    }

    /// Seeds a pending entry with a suggestion, as the worker would.
    fn suggested_entry(
        store: &mut ActivityStore,
        started: u64,
        value: &str,
        dominant_key: &str,
    ) -> String {
        let id = uuid::Uuid::now_v7().to_string();
        store
            .conn
            .execute(
                "INSERT INTO time_entries (id, started_at, ended_at, description, category_id, status, source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'Figma', ?4, 'pending', 'auto', 0, 0);",
                params![id, started as i64, (started + 30 * MIN) as i64, value],
            )
            .unwrap();
        let decision = crate::ai::arbiter::Decision {
            value: Some(value.to_string()),
            confidence: 0.7,
            raw_confidence: 0.7,
            alternatives: vec![],
            signals: vec![],
            engine: "full",
            tier: "model",
        };
        let dominant = crate::models::Dominant {
            kind: "domain".into(),
            key: dominant_key.into(),
            label: dominant_key.into(),
            share: 0.9,
        };
        crate::ai::store::insert_suggestion(
            &store.conn,
            &id,
            crate::ai::Field::Category,
            &decision,
            Some(&dominant),
            "test",
            None,
            started,
        )
        .unwrap();
        id
    }

    fn set_category(store: &mut ActivityStore, id: &str, category: &str) {
        store
            .update_time_entry(
                id,
                UpdateTimeEntry {
                    description: None,
                    category_id: Some(category.into()),
                    project_id: None,
                    started_at: None,
                    ended_at: None,
                    status: None,
                    billable: None,
                },
                1,
            )
            .unwrap();
    }

    #[test]
    fn approval_records_whether_the_suggestion_was_kept() {
        let mut store = store();
        let kept = suggested_entry(&mut store, 0, "coding", "github.com");
        let changed = suggested_entry(&mut store, 40 * MIN, "coding", "github.com");
        set_category(&mut store, &changed, "review");
        store
            .approve_time_entries(&[kept.clone(), changed.clone()], "user", 2)
            .unwrap();

        let outcome = |id: &str| {
            crate::ai::store::latest_suggestions(&store.conn, id).unwrap()[0]
                .outcome
                .clone()
        };
        assert_eq!(outcome(&kept).as_deref(), Some("accepted"));
        assert_eq!(outcome(&changed).as_deref(), Some("changed"));
        // Both now wait for a vector so they can join the kNN store.
        assert_eq!(crate::ai::store::queued_count(&store.conn).unwrap(), 2);
    }

    #[test]
    fn reject_clears_the_prefilled_category_and_records_it() {
        let mut store = store();
        let id = suggested_entry(&mut store, 0, "coding", "github.com");
        store.reject_time_entry(&id, 5).unwrap();
        let detail = store.get_entry_detail(&id).unwrap();
        assert_eq!(detail.entry.category_id, None);
        assert_eq!(detail.entry.status, "pending");
        assert_eq!(detail.suggestions[0].outcome.as_deref(), Some("rejected"));
        let ai = live_entries(&store)[0].ai.clone();
        assert_eq!(ai.and_then(|ai| ai.category_confidence), None);
    }

    #[test]
    fn three_consistent_corrections_offer_a_rule_once() {
        let mut store = store();
        for i in 0..2 {
            let id = suggested_entry(&mut store, i * 40 * MIN, "coding", "figma.com");
            set_category(&mut store, &id, "design");
            store.approve_time_entries(&[id], "user", 2).unwrap();
        }
        let third = suggested_entry(&mut store, 200 * MIN, "coding", "figma.com");
        assert!(store
            .get_entry_detail(&third)
            .unwrap()
            .rule_suggestion
            .is_none());

        set_category(&mut store, &third, "design");
        let offer = store
            .get_entry_detail(&third)
            .unwrap()
            .rule_suggestion
            .expect("a rule suggestion after the third correction");
        assert_eq!(offer.pattern, "figma.com");
        assert_eq!(offer.value_id.as_deref(), Some("design"));
        assert_eq!(offer.corrections, 3);

        store.resolve_rule_suggestion(&offer, false, 3).unwrap();
        assert!(store
            .get_entry_detail(&third)
            .unwrap()
            .rule_suggestion
            .is_none());
        // A dismissed suggestion is stored disabled, so T0 never applies it.
        let rules = crate::ai::store::load_rules(&store.conn).unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn rejections_count_toward_a_rule_suggestion() {
        let mut store = store();
        for i in 0..2 {
            let id = suggested_entry(&mut store, i * 40 * MIN, "coding", "figma.com");
            store.reject_time_entry(&id, 2).unwrap();
            set_category(&mut store, &id, "design");
            store.approve_time_entries(&[id], "user", 3).unwrap();
        }
        let third = suggested_entry(&mut store, 200 * MIN, "coding", "figma.com");
        store.reject_time_entry(&third, 4).unwrap();
        set_category(&mut store, &third, "design");
        let offer = store
            .get_entry_detail(&third)
            .unwrap()
            .rule_suggestion
            .expect("rejections followed by a pick are corrections");
        assert_eq!(offer.corrections, 3);
        assert_eq!(offer.value_id.as_deref(), Some("design"));
    }

    #[test]
    fn recategorizing_an_approved_entry_is_a_correction() {
        let mut store = store();
        let auto = suggested_entry(&mut store, 0, "coding", "github.com");
        store
            .conn
            .execute(
                "UPDATE suggestions SET outcome = 'auto' WHERE entry_id = ?1;",
                params![auto],
            )
            .unwrap();
        let accepted = suggested_entry(&mut store, 40 * MIN, "coding", "github.com");
        store
            .approve_time_entries(&[auto.clone(), accepted.clone()], "user", 2)
            .unwrap();
        set_category(&mut store, &auto, "review");
        set_category(&mut store, &accepted, "coding");

        let outcome = |id: &str| {
            crate::ai::store::latest_suggestions(&store.conn, id).unwrap()[0]
                .outcome
                .clone()
        };
        assert_eq!(outcome(&auto).as_deref(), Some("changed"));
        // Saving the same value again is not a correction.
        assert_eq!(outcome(&accepted).as_deref(), Some("accepted"));
    }

    #[test]
    fn calibration_learns_from_verdicts_on_model_suggestions() {
        let mut store = store();
        let kept = suggested_entry(&mut store, 0, "coding", "github.com");
        let fixed = suggested_entry(&mut store, 40 * MIN, "coding", "github.com");
        let open = suggested_entry(&mut store, 80 * MIN, "coding", "github.com");
        set_category(&mut store, &fixed, "review");
        store
            .approve_time_entries(&[kept, fixed], "user", 2)
            .unwrap();
        let _ = open;

        let now = 10 * MIN;
        let mut samples = crate::ai::store::calibration_samples(&store.conn, now).unwrap();
        samples.sort_by_key(|a| a.1);
        assert_eq!(samples, vec![(0.7, false), (0.7, true)]);
        assert_eq!(
            crate::ai::store::outcome_count(&store.conn, now).unwrap(),
            2
        );
        assert_eq!(
            crate::ai::store::verdicts_since(&store.conn, 1, now).unwrap(),
            2
        );
        assert_eq!(
            crate::ai::store::verdicts_since(&store.conn, 2, now).unwrap(),
            0
        );
    }

    #[test]
    fn reset_forgets_what_was_learned_but_keeps_the_entries() {
        let mut store = store();
        let id = suggested_entry(&mut store, 0, "coding", "github.com");
        store
            .approve_time_entries(std::slice::from_ref(&id), "user", 2)
            .unwrap();
        crate::ai::store::upsert_embedding(&store.conn, &id, &[1.0, 0.0], "h", "Xcode", "m", 3)
            .unwrap();
        crate::ai::store::insert_artifact(
            &store.conn,
            "category",
            "/tmp/m.mlmodelc",
            20,
            Some(0.8),
            true,
            4,
        )
        .unwrap();
        let curve = crate::ai::calibration::Calibrator {
            x: vec![0.0, 1.0],
            y: vec![0.0, 1.0],
            samples: 50,
        };
        crate::ai::store::insert_calibration(&store.conn, &curve, 5).unwrap();
        assert_eq!(crate::ai::store::labeled_count(&store.conn).unwrap(), 1);
        assert!(crate::ai::store::active_calibration(&store.conn)
            .unwrap()
            .is_some());

        let paths = crate::ai::store::reset_learned(&store.conn, 10).unwrap();
        assert_eq!(paths, vec!["/tmp/m.mlmodelc".to_string()]);
        assert!(crate::ai::store::active_calibration(&store.conn)
            .unwrap()
            .is_none());
        assert!(crate::ai::store::active_artifact(&store.conn, "category")
            .unwrap()
            .is_none());
        assert_eq!(crate::ai::store::labeled_count(&store.conn).unwrap(), 0);
        assert_eq!(crate::ai::store::outcome_count(&store.conn, 20).unwrap(), 0);
        assert_eq!(crate::ai::store::learning_since(&store.conn).unwrap(), 10);
        let entry = store.get_entry_detail(&id).unwrap().entry;
        assert_eq!(entry.status, "approved");
        assert_eq!(entry.category_id.as_deref(), Some("coding"));
    }

    #[test]
    fn outside_tracking_hours_suppresses_new_capture() {
        use chrono::TimeZone;
        let mut store =
            ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
                .expect("schema");
        store.set_tracking_hours(crate::settings::TrackingHours {
            enabled: true,
            per_day: false,
            default_start: "07:00".to_string(),
            default_end: "19:00".to_string(),
            ..crate::settings::TrackingHours::default()
        });

        let dt = chrono::Local
            .with_ymd_and_hms(2026, 9, 25, 20, 0, 0)
            .single()
            .expect("local dt");
        let now = dt.timestamp_millis() as u64;

        let changed = store.tick(sample("Code", "main.rs"), 0, now).unwrap();
        assert!(!changed);
        let snapshot = store.snapshot(0, now).unwrap();
        assert!(snapshot.current.is_none());
        assert!(snapshot.segments.is_empty());
        assert!(!snapshot.in_tracking_hours);
        assert!(!snapshot.tracking_active);
    }

    #[test]
    fn session_in_progress_finishes_naturally_outside_window() {
        use chrono::TimeZone;
        let mut store =
            ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
                .expect("schema");
        store.set_idle_threshold_ms(300_000).unwrap();
        store.set_tracking_hours(crate::settings::TrackingHours {
            enabled: true,
            per_day: false,
            default_start: "07:00".to_string(),
            default_end: "19:00".to_string(),
            ..crate::settings::TrackingHours::default()
        });

        // Start session at 18:58 (inside window)
        let dt_start = chrono::Local
            .with_ymd_and_hms(2026, 9, 25, 18, 58, 0)
            .single()
            .expect("local dt");
        let start_ms = dt_start.timestamp_millis() as u64;
        store.tick(sample("Code", "main.rs"), 0, start_ms).unwrap();
        assert!(store.snapshot(0, start_ms).unwrap().current.is_some());

        // At 19:02 (outside window): switch to Slack. Session continues!
        let dt_outside = chrono::Local
            .with_ymd_and_hms(2026, 9, 25, 19, 2, 0)
            .single()
            .expect("local dt");
        let outside_ms = dt_outside.timestamp_millis() as u64;
        store
            .tick(sample("Slack", "#general"), 0, outside_ms)
            .unwrap();

        let live = store.live_state(outside_ms);
        assert!(!live.in_tracking_hours);
        assert!(live.tracking_active);
        assert_eq!(live.current.as_ref().unwrap().app, "Slack");

        // At 19:10: idle detected (idle for 5 min, since 19:05). Idle ends it naturally.
        let dt_idle = chrono::Local
            .with_ymd_and_hms(2026, 9, 25, 19, 10, 0)
            .single()
            .expect("local dt");
        let idle_ms = dt_idle.timestamp_millis() as u64;
        let changed = store
            .tick(sample("Slack", "#general"), 300_000, idle_ms)
            .unwrap();
        assert!(changed);

        let snapshot = store.snapshot(0, idle_ms).unwrap();
        assert!(snapshot.current.is_none());
        assert!(!snapshot.tracking_active);

        let dt_cutoff = chrono::Local
            .with_ymd_and_hms(2026, 9, 25, 19, 5, 0)
            .single()
            .expect("local dt");
        assert_eq!(
            snapshot.segments.last().unwrap().ended_at,
            Some(dt_cutoff.timestamp_millis() as u64)
        );

        // Further activity outside window does not start new capture
        let dt_later = chrono::Local
            .with_ymd_and_hms(2026, 9, 25, 19, 15, 0)
            .single()
            .expect("local dt");
        let later_ms = dt_later.timestamp_millis() as u64;
        store.tick(sample("Code", "main.rs"), 0, later_ms).unwrap();
        assert!(store.snapshot(0, later_ms).unwrap().current.is_none());
    }

    #[test]
    fn manual_tracking_works_outside_window() {
        use chrono::TimeZone;
        let mut store =
            ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
                .expect("schema");
        store.set_tracking_hours(crate::settings::TrackingHours {
            enabled: true,
            per_day: false,
            default_start: "07:00".to_string(),
            default_end: "19:00".to_string(),
            ..crate::settings::TrackingHours::default()
        });

        let dt = chrono::Local
            .with_ymd_and_hms(2026, 9, 25, 20, 0, 0)
            .single()
            .expect("local dt");
        let now = dt.timestamp_millis() as u64;

        store.set_capture_enabled(true, now).unwrap();
        assert!(store.manual_tracking);

        store.tick(sample("Code", "main.rs"), 0, now).unwrap();
        let snapshot = store.snapshot(0, now).unwrap();
        assert!(snapshot.current.is_some());
        assert_eq!(snapshot.current.unwrap().app, "Code");
        assert!(snapshot.tracking_active);

        store.set_capture_enabled(false, now + 10_000).unwrap();
        assert!(!store.manual_tracking);
        assert!(store.capture_enabled);

        let snapshot = store.snapshot(0, now + 10_000).unwrap();
        assert!(snapshot.current.is_none());
        assert!(!snapshot.tracking_active);

        store
            .tick(sample("Code", "main.rs"), 0, now + 20_000)
            .unwrap();
        assert!(store.snapshot(0, now + 20_000).unwrap().current.is_none());
    }
}
