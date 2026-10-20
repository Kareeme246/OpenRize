//! Timer store — the single source of truth for stopwatch state.
//!
//! Clock policy (decision D2): the *persisted* anchor is a wall-clock epoch in
//! milliseconds, because an `Instant` cannot survive a restart. While the
//! process is alive, `now` comes from a monotonic `Instant` anchored to a
//! startup epoch, so a system-clock change mid-session cannot corrupt a running
//! stopwatch.
//!
//! Persistence policy: timers live in a `timers` table in
//! the same SQLite file activity capture already uses (`activity.db`), each
//! store through its own connection Timers still mutate at human
//! rate, so there's no contention concern moving to row-level SQL statements.

use std::fs;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, Row};
use serde::Serialize;

use crate::activity::DB_FILE;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS timers (
  id             TEXT PRIMARY KEY,
  label          TEXT NOT NULL,
  accumulated_ms INTEGER NOT NULL,
  started_at     INTEGER,
  created_at     INTEGER NOT NULL,
  ordinal        INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS timer_segments (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  timer_id    TEXT NOT NULL,
  duration_ms INTEGER NOT NULL,
  recorded_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS timer_saved_states (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  timer_id   TEXT NOT NULL,
  label      TEXT NOT NULL,
  elapsed_ms INTEGER NOT NULL,
  saved_at   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    pub id: String,
    pub label: String,
    /// Sum of `segments`; the segments are the only stored time.
    pub accumulated_ms: u64,
    /// Completed stretches of time, oldest first. Pausing records one, and
    /// they can be added, edited and deleted by hand.
    pub segments: Vec<TimerSegment>,
    /// Epoch milliseconds at which the current run started, or `None` if paused.
    pub started_at: Option<u64>,
    pub created_at: u64,
    pub ordinal: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerSegment {
    pub id: i64,
    pub duration_ms: u64,
    /// Epoch milliseconds at which the segment was recorded.
    pub recorded_at: u64,
}

/// A snapshot of a timer taken on demand: what it read, under which name, and
/// when. Saves only ever accumulate; they are removed with their timer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerSavedState {
    pub id: i64,
    pub timer_id: String,
    /// The timer's label when it was saved; later renames do not rewrite it.
    pub label: String,
    pub elapsed_ms: u64,
    /// Epoch milliseconds at which the state was saved.
    pub saved_at: u64,
}

/// Selects a timer with its total computed from its segments.
const TIMER_SELECT: &str = "SELECT id, label,
       COALESCE((SELECT SUM(duration_ms) FROM timer_segments WHERE timer_id = timers.id), 0),
       started_at, created_at, ordinal
     FROM timers";

impl Timer {
    pub fn is_running(&self) -> bool {
        self.started_at.is_some()
    }

    /// Total time at `now`: the segments plus the run in progress, if any.
    pub fn elapsed_at(&self, now: u64) -> u64 {
        self.accumulated_ms + self.started_at.map_or(0, |start| now.saturating_sub(start))
    }
}

/// Wall-clock milliseconds since the Unix epoch.
pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

/// Monotonic time anchored to a wall-clock origin captured at process start.
#[derive(Debug)]
struct Clock {
    origin: Instant,
    origin_epoch_ms: u64,
}

impl Clock {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            origin_epoch_ms: now_epoch_ms(),
        }
    }

    fn now_ms(&self) -> u64 {
        self.origin_epoch_ms + self.origin.elapsed().as_millis() as u64
    }
}

pub struct TimerStore {
    clock: Clock,
    conn: Connection,
}

impl TimerStore {
    /// Opens the shared database and creates the `timers` table if needed.
    pub fn load(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|error| format!("could not create data dir: {error}"))?;
        let path = dir.join(DB_FILE);
        let mut conn = Connection::open(&path).map_err(|error| error.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|error| error.to_string())?;
        conn.execute_batch(SCHEMA)
            .map_err(|error| error.to_string())?;
        Self::migrate(&mut conn)?;
        Ok(Self {
            clock: Clock::new(),
            conn,
        })
    }

    fn migrate(conn: &mut Connection) -> Result<(), String> {
        let mut stmt = conn
            .prepare("PRAGMA table_info(timers)")
            .map_err(|error| error.to_string())?;
        let has_ordinal = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .any(|col| col == "ordinal");
        drop(stmt);

        if !has_ordinal {
            let tx = conn.transaction().map_err(|error| error.to_string())?;
            tx.execute_batch("ALTER TABLE timers ADD COLUMN ordinal INTEGER NOT NULL DEFAULT 0;")
                .map_err(|error| error.to_string())?;

            let mut stmt = tx
                .prepare("SELECT id FROM timers ORDER BY created_at ASC, id ASC")
                .map_err(|error| error.to_string())?;
            let ids: Vec<String> = stmt
                .query_map([], |row| row.get(0))
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            drop(stmt);

            for (index, id) in ids.iter().enumerate() {
                tx.execute(
                    "UPDATE timers SET ordinal = ?1 WHERE id = ?2",
                    params![index as i64, id],
                )
                .map_err(|error| error.to_string())?;
            }
            tx.commit().map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Vec<Timer>, String> {
        let mut statement = self
            .conn
            .prepare(&format!(
                "{TIMER_SELECT} ORDER BY ordinal ASC, created_at ASC, id ASC"
            ))
            .map_err(|error| error.to_string())?;
        let mut timers = statement
            .query_map([], timer_from_row)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        for timer in &mut timers {
            timer.segments = self.segments_of(&timer.id)?;
        }
        Ok(timers)
    }

    // --- mutations -------------------------------------------------------
    // The `_at` variants take an explicit `now` and are what the tests drive;
    // the public ones stamp the real clock.

    pub fn create(&mut self, label: &str) -> Result<Vec<Timer>, String> {
        let now = self.clock.now_ms();
        self.create_at(label, now)
    }

    pub fn create_at(&mut self, label: &str, now: u64) -> Result<Vec<Timer>, String> {
        let label = label.trim();
        if label.is_empty() {
            return Err("a tracker needs a name".to_string());
        }
        let next = self.take_next_id()?;
        let id = format!("t{next}");
        let next_ordinal: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM timers",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        // Created idle: naming a tracker is not the same as deciding to bill
        // time to it, so the card waits for an explicit Start.
        self.conn
            .execute(
                "INSERT INTO timers (id, label, accumulated_ms, started_at, created_at, ordinal)
                 VALUES (?1, ?2, 0, NULL, ?3, ?4)", // accumulated_ms is a retired column
                params![id, label, now as i64, next_ordinal],
            )
            .map_err(|error| error.to_string())?;
        self.snapshot()
    }

    pub fn start(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        let now = self.clock.now_ms();
        self.start_at(id, now)
    }

    pub fn start_at(&mut self, id: &str, now: u64) -> Result<Vec<Timer>, String> {
        let timer = self.find(id)?;
        if timer.started_at.is_none() {
            self.conn
                .execute(
                    "UPDATE timers SET started_at = ?1 WHERE id = ?2",
                    params![now as i64, id],
                )
                .map_err(|error| error.to_string())?;
        }
        self.snapshot()
    }

    pub fn pause(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        let now = self.clock.now_ms();
        self.pause_at(id, now)
    }

    pub fn pause_at(&mut self, id: &str, now: u64) -> Result<Vec<Timer>, String> {
        let timer = self.find(id)?;
        if let Some(started) = timer.started_at {
            self.conn
                .execute(
                    "UPDATE timers SET started_at = NULL WHERE id = ?1",
                    params![id],
                )
                .map_err(|error| error.to_string())?;
            self.insert_segment(id, now.saturating_sub(started), now)?;
        }
        self.snapshot()
    }

    pub fn reset(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        self.find(id)?;
        self.conn
            .execute(
                "UPDATE timers SET started_at = NULL WHERE id = ?1",
                params![id],
            )
            .map_err(|error| error.to_string())?;
        self.conn
            .execute(
                "DELETE FROM timer_segments WHERE timer_id = ?1",
                params![id],
            )
            .map_err(|error| error.to_string())?;
        self.snapshot()
    }

    pub fn rename(&mut self, id: &str, label: &str) -> Result<Vec<Timer>, String> {
        let label = label.trim();
        if label.is_empty() {
            return Err("a tracker needs a name".to_string());
        }
        self.find(id)?;
        self.conn
            .execute(
                "UPDATE timers SET label = ?1 WHERE id = ?2",
                params![label, id],
            )
            .map_err(|error| error.to_string())?;
        self.snapshot()
    }

    /// Updates the display order of timers to match the given list of IDs.
    pub fn reorder(&mut self, ids: &[String]) -> Result<Vec<Timer>, String> {
        let tx = self.conn.transaction().map_err(|error| error.to_string())?;
        for (index, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE timers SET ordinal = ?1 WHERE id = ?2",
                params![index as i64, id],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        self.snapshot()
    }

    /// Moves a timer up (negative delta) or down (positive delta) in the list.
    pub fn move_timer(&mut self, id: &str, delta: i32) -> Result<Vec<Timer>, String> {
        let timers = self.snapshot()?;
        let pos = timers
            .iter()
            .position(|t| t.id == id)
            .ok_or_else(|| unknown(id))?;
        let target = if delta < 0 {
            pos.saturating_sub((-delta) as usize)
        } else {
            (pos + (delta as usize)).min(timers.len().saturating_sub(1))
        };
        if target == pos {
            return Ok(timers);
        }
        let mut ids: Vec<String> = timers.into_iter().map(|t| t.id).collect();
        let item = ids.remove(pos);
        ids.insert(target, item);
        self.reorder(&ids)
    }

    /// Adds a stretch of time to the total.
    pub fn add_segment(&mut self, id: &str, duration_ms: u64) -> Result<Vec<Timer>, String> {
        let now = self.clock.now_ms();
        self.add_segment_at(id, duration_ms, now)
    }

    pub fn add_segment_at(
        &mut self,
        id: &str,
        duration_ms: u64,
        now: u64,
    ) -> Result<Vec<Timer>, String> {
        self.find(id)?;
        check_duration(duration_ms)?;
        self.insert_segment(id, duration_ms, now)?;
        self.snapshot()
    }

    pub fn update_segment(
        &mut self,
        id: &str,
        segment_id: i64,
        duration_ms: u64,
    ) -> Result<Vec<Timer>, String> {
        self.find(id)?;
        check_duration(duration_ms)?;
        let changed = self
            .conn
            .execute(
                "UPDATE timer_segments SET duration_ms = ?1 WHERE id = ?2 AND timer_id = ?3",
                params![duration_ms as i64, segment_id, id],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err(unknown_segment(segment_id));
        }
        self.snapshot()
    }

    pub fn delete_segment(&mut self, id: &str, segment_id: i64) -> Result<Vec<Timer>, String> {
        self.find(id)?;
        let changed = self
            .conn
            .execute(
                "DELETE FROM timer_segments WHERE id = ?1 AND timer_id = ?2",
                params![segment_id, id],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err(unknown_segment(segment_id));
        }
        self.snapshot()
    }

    /// Records the timer's reading right now. The timer itself is untouched,
    /// so this works the same whether it is running, paused or at rest.
    pub fn save_state(&mut self, id: &str) -> Result<TimerSavedState, String> {
        let now = self.clock.now_ms();
        self.save_state_at(id, now)
    }

    pub fn save_state_at(&mut self, id: &str, now: u64) -> Result<TimerSavedState, String> {
        let timer = self.find(id)?;
        let elapsed_ms = timer.elapsed_at(now);
        self.conn
            .execute(
                "INSERT INTO timer_saved_states (timer_id, label, elapsed_ms, saved_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, timer.label, elapsed_ms as i64, now as i64],
            )
            .map_err(|error| error.to_string())?;
        Ok(TimerSavedState {
            id: self.conn.last_insert_rowid(),
            timer_id: timer.id,
            label: timer.label,
            elapsed_ms,
            saved_at: now,
        })
    }

    /// A timer's saved states, newest first.
    pub fn saved_states(&self, id: &str) -> Result<Vec<TimerSavedState>, String> {
        self.find(id)?;
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, timer_id, label, elapsed_ms, saved_at FROM timer_saved_states
                 WHERE timer_id = ?1 ORDER BY saved_at DESC, id DESC",
            )
            .map_err(|error| error.to_string())?;
        let states = statement
            .query_map(params![id], |row| {
                Ok(TimerSavedState {
                    id: row.get(0)?,
                    timer_id: row.get(1)?,
                    label: row.get(2)?,
                    elapsed_ms: row.get::<_, i64>(3)? as u64,
                    saved_at: row.get::<_, i64>(4)? as u64,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string());
        states
    }

    pub fn delete(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        // One transaction, so a timer can never be left without its rows or
        // its rows left without a timer.
        let tx = self.conn.transaction().map_err(|error| error.to_string())?;
        let changed = tx
            .execute("DELETE FROM timers WHERE id = ?1", params![id])
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err(unknown(id));
        }
        for table in ["timer_segments", "timer_saved_states"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE timer_id = ?1"),
                params![id],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        self.snapshot()
    }

    fn insert_segment(&self, id: &str, duration_ms: u64, now: u64) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO timer_segments (timer_id, duration_ms, recorded_at)
                 VALUES (?1, ?2, ?3)",
                params![id, duration_ms as i64, now as i64],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn segments_of(&self, id: &str) -> Result<Vec<TimerSegment>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, duration_ms, recorded_at FROM timer_segments
                 WHERE timer_id = ?1 ORDER BY recorded_at ASC, id ASC",
            )
            .map_err(|error| error.to_string())?;
        let segments = statement
            .query_map(params![id], |row| {
                Ok(TimerSegment {
                    id: row.get(0)?,
                    duration_ms: row.get::<_, i64>(1)? as u64,
                    recorded_at: row.get::<_, i64>(2)? as u64,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string());
        segments
    }

    fn find(&self, id: &str) -> Result<Timer, String> {
        let mut timer = self
            .conn
            .query_row(
                &format!("{TIMER_SELECT} WHERE id = ?1"),
                params![id],
                timer_from_row,
            )
            .map_err(|_| unknown(id))?;
        timer.segments = self.segments_of(id)?;
        Ok(timer)
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

    /// IDs keep the historical `t<n>` shape and never get reused after a
    /// delete, so the counter lives in `settings` rather than being derived
    /// from the current row set.
    fn take_next_id(&self) -> Result<u64, String> {
        let stored_next = self
            .read_setting("timer_next_id")
            .and_then(|value| value.parse().ok());
        let next = match stored_next {
            Some(n) => n,
            None => {
                let mut max_id = 0u64;
                if let Ok(mut stmt) = self.conn.prepare("SELECT id FROM timers") {
                    if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
                        for row in rows.flatten() {
                            if let Some(suffix) = row.strip_prefix('t') {
                                if let Ok(val) = suffix.parse::<u64>() {
                                    max_id = max_id.max(val);
                                }
                            }
                        }
                    }
                }
                max_id + 1
            }
        };
        self.write_setting("timer_next_id", &(next + 1).to_string())?;
        Ok(next)
    }
}

fn timer_from_row(row: &Row<'_>) -> rusqlite::Result<Timer> {
    Ok(Timer {
        id: row.get(0)?,
        label: row.get(1)?,
        accumulated_ms: row.get::<_, i64>(2)? as u64,
        segments: Vec::new(),
        started_at: row.get::<_, Option<i64>>(3)?.map(|value| value as u64),
        created_at: row.get::<_, i64>(4)? as u64,
        ordinal: row.get::<_, i64>(5)?,
    })
}

fn check_duration(duration_ms: u64) -> Result<(), String> {
    if duration_ms == 0 {
        return Err("a segment needs some time in it".to_string());
    }
    Ok(())
}

fn unknown_segment(segment_id: i64) -> String {
    format!("no time segment with id {segment_id}")
}

fn unknown(id: &str) -> String {
    format!("no tracker with id {id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openrize-{name}-{}", now_epoch_ms()));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn store(name: &str) -> TimerStore {
        TimerStore::load(&temp_dir(name)).expect("timer store")
    }

    #[test]
    fn pause_accumulates_only_the_time_that_ran() {
        let mut store = store("pause");
        let id = store.create_at("Write the handoff", 1_000).unwrap()[0]
            .id
            .clone();
        store.start_at(&id, 1_000).unwrap();

        store.pause_at(&id, 4_500).unwrap();
        let paused = &store.snapshot().unwrap()[0];
        assert_eq!(paused.accumulated_ms, 3_500);
        assert!(!paused.is_running());

        // Pausing again must not double-count.
        store.pause_at(&id, 9_000).unwrap();
        assert_eq!(store.snapshot().unwrap()[0].accumulated_ms, 3_500);
    }

    #[test]
    fn segments_are_the_only_time_and_can_be_edited() {
        let mut store = store("segments");
        let id = store.create_at("Edit me", 1_000).unwrap()[0].id.clone();
        store.start_at(&id, 1_000).unwrap();
        store.pause_at(&id, 61_000).unwrap();
        store.add_segment_at(&id, 30_000, 70_000).unwrap();

        let timer = store.snapshot().unwrap().remove(0);
        assert_eq!(timer.accumulated_ms, 90_000);
        assert_eq!(timer.segments.len(), 2);

        let first = timer.segments[0].id;
        let second = timer.segments[1].id;
        store.update_segment(&id, first, 10_000).unwrap();
        assert_eq!(store.snapshot().unwrap()[0].accumulated_ms, 40_000);

        store.delete_segment(&id, second).unwrap();
        assert_eq!(store.snapshot().unwrap()[0].accumulated_ms, 10_000);

        assert!(store.add_segment_at(&id, 0, 80_000).is_err());
        assert!(store.update_segment(&id, second, 5_000).is_err());
    }

    #[test]
    fn saving_records_the_exact_reading_without_disturbing_the_timer() {
        let mut store = store("save-state");
        let id = store.create_at("Deep work", 0).unwrap()[0].id.clone();

        // At rest, nothing tracked yet.
        let at_rest = store.save_state_at(&id, 500).unwrap();
        assert_eq!(at_rest.elapsed_ms, 0);
        assert_eq!(at_rest.label, "Deep work");

        // Running: 37m52s since the last start.
        store.start_at(&id, 1_000).unwrap();
        let running = store.save_state_at(&id, 1_000 + 2_272_000).unwrap();
        assert_eq!(running.elapsed_ms, 2_272_000);
        assert_eq!(running.saved_at, 1_000 + 2_272_000);
        assert_eq!(running.timer_id, id);
        let timer = &store.snapshot().unwrap()[0];
        assert_eq!(timer.started_at, Some(1_000));
        assert!(timer.segments.is_empty());

        // Paused: the stored segments are the whole reading.
        store.pause_at(&id, 61_000).unwrap();
        let paused = store.save_state_at(&id, 90_000).unwrap();
        assert_eq!(paused.elapsed_ms, 60_000);
        let timer = &store.snapshot().unwrap()[0];
        assert_eq!(timer.accumulated_ms, 60_000);
        assert_eq!(timer.started_at, None);

        assert!(store.save_state_at("nope", 1).is_err());
    }

    #[test]
    fn saved_states_accumulate_newest_first_and_survive_rename_and_reload() {
        let dir = temp_dir("saved-states");
        let mut first = TimerStore::load(&dir).expect("timer store");
        let id = first.create_at("Draft", 0).unwrap()[0].id.clone();
        let other = first.create_at("Other", 0).unwrap()[1].id.clone();
        first.start_at(&id, 0).unwrap();
        first.save_state_at(&id, 10_000).unwrap();
        first.rename(&id, "Final").unwrap();
        first.save_state_at(&id, 20_000).unwrap();
        first.save_state_at(&other, 30_000).unwrap();
        first.reset(&id).unwrap();

        let second = TimerStore::load(&dir).expect("timer store");
        let states = second.saved_states(&id).unwrap();
        let readings: Vec<(&str, u64)> = states
            .iter()
            .map(|state| (state.label.as_str(), state.elapsed_ms))
            .collect();
        // Newest first; the earlier save keeps the name it was saved under.
        assert_eq!(readings, [("Final", 20_000), ("Draft", 10_000)]);
        assert_eq!(second.saved_states(&other).unwrap().len(), 1);
        assert!(second.saved_states("nope").is_err());
    }

    #[test]
    fn deleting_a_timer_deletes_its_saved_states_and_only_its_own() {
        let mut store = store("delete-saved-states");
        let id = store.create_at("Gone", 0).unwrap()[0].id.clone();
        let kept = store.create_at("Kept", 0).unwrap()[1].id.clone();
        store.save_state_at(&id, 1_000).unwrap();
        store.save_state_at(&kept, 1_000).unwrap();

        store.delete(&id).unwrap();

        let count = |timer_id: &str| -> i64 {
            store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM timer_saved_states WHERE timer_id = ?1",
                    params![timer_id],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(count(&id), 0);
        assert_eq!(count(&kept), 1);
        assert!(store.delete(&id).is_err());
    }

    #[test]
    fn the_saved_states_table_is_added_to_an_existing_database() {
        let dir = temp_dir("saved-states-migration");
        {
            // A database from before saved states: timers and segments only.
            let conn = Connection::open(dir.join(DB_FILE)).unwrap();
            conn.execute_batch(
                "CREATE TABLE timers (
                   id TEXT PRIMARY KEY, label TEXT NOT NULL, accumulated_ms INTEGER NOT NULL,
                   started_at INTEGER, created_at INTEGER NOT NULL);
                 CREATE TABLE timer_segments (
                   id INTEGER PRIMARY KEY AUTOINCREMENT, timer_id TEXT NOT NULL,
                   duration_ms INTEGER NOT NULL, recorded_at INTEGER NOT NULL);
                 INSERT INTO timers VALUES ('t1', 'Old timer', 0, 5000, 1000);
                 INSERT INTO timer_segments (timer_id, duration_ms, recorded_at)
                   VALUES ('t1', 90000, 2000);",
            )
            .unwrap();
        }

        let mut store = TimerStore::load(&dir).expect("timer store");
        let timers = store.snapshot().unwrap();
        assert_eq!(timers.len(), 1);
        assert_eq!(timers[0].accumulated_ms, 90_000);
        assert_eq!(timers[0].started_at, Some(5_000));
        assert!(store.saved_states("t1").unwrap().is_empty());

        let saved = store.save_state_at("t1", 15_000).unwrap();
        assert_eq!(saved.elapsed_ms, 100_000);
        assert_eq!(store.saved_states("t1").unwrap().len(), 1);
    }

    #[test]
    fn reset_zeroes_and_stops_the_timer() {
        let mut store = store("reset");
        let id = store.create_at("Reset me", 1_000).unwrap()[0].id.clone();
        store.start_at(&id, 1_000).unwrap();

        store.reset(&id).unwrap();
        let timer = &store.snapshot().unwrap()[0];
        assert_eq!(timer.accumulated_ms, 0);
        assert_eq!(timer.started_at, None);
        assert!(!timer.is_running());
    }

    #[test]
    fn a_new_tracker_is_created_idle() {
        let mut store = store("idle-create");
        let timer = &store.create_at("Not started yet", 1_000).unwrap()[0];
        assert_eq!(timer.started_at, None);
        assert!(!timer.is_running());
        assert_eq!(timer.accumulated_ms, 0);
    }

    #[test]
    fn bad_input_returns_an_error_instead_of_panicking() {
        let mut store = store("bad-input");
        assert!(store.create_at("   ", 1_000).is_err());
        assert!(store.pause("nope").is_err());
        assert!(store.delete("nope").is_err());
        assert!(store.rename("nope", "still nope").is_err());
    }

    #[test]
    fn a_running_timer_survives_a_reload() {
        let dir = temp_dir("reload");
        let mut first = TimerStore::load(&dir).expect("timer store");
        let id = first.create_at("Survive a restart", 1_000).unwrap()[0]
            .id
            .clone();
        first.start_at(&id, 1_000).unwrap();
        first.pause_at(&id, 4_000).unwrap();

        let second = TimerStore::load(&dir).expect("timer store");
        let timers = second.snapshot().unwrap();
        assert_eq!(timers[0].label, "Survive a restart");
        assert_eq!(timers[0].accumulated_ms, 3_000);
        assert_eq!(timers.len(), 1);
    }

    #[test]
    fn ordinal_column_is_added_to_an_existing_database_with_stable_default_order() {
        let dir = temp_dir("ordinal-migration");
        {
            // A database from before the ordinal column:
            let conn = Connection::open(dir.join(DB_FILE)).unwrap();
            conn.execute_batch(
                "CREATE TABLE timers (
                   id TEXT PRIMARY KEY, label TEXT NOT NULL, accumulated_ms INTEGER NOT NULL,
                   started_at INTEGER, created_at INTEGER NOT NULL);
                 INSERT INTO timers VALUES ('t1', 'First', 0, NULL, 1000);
                 INSERT INTO timers VALUES ('t2', 'Third', 0, NULL, 3000);
                 INSERT INTO timers VALUES ('t3', 'Second', 0, NULL, 2000);",
            )
            .unwrap();
        }

        let mut store = TimerStore::load(&dir).expect("timer store");
        let timers = store.snapshot().unwrap();
        assert_eq!(timers.len(), 3);
        // Default order matches historical order: ORDER BY created_at ASC, id ASC
        assert_eq!(timers[0].label, "First");
        assert_eq!(timers[0].ordinal, 0);
        assert_eq!(timers[1].label, "Second");
        assert_eq!(timers[1].ordinal, 1);
        assert_eq!(timers[2].label, "Third");
        assert_eq!(timers[2].ordinal, 2);

        // A new timer lands at the end
        let timers = store.create_at("Fourth", 4000).unwrap();
        assert_eq!(timers.len(), 4);
        assert_eq!(timers[3].label, "Fourth");
        assert_eq!(timers[3].ordinal, 3);
    }

    #[test]
    fn order_persists_across_reorder_and_reload() {
        let dir = temp_dir("reorder-persistence");
        let id_a;
        let id_b;
        let id_c;
        {
            let mut store = TimerStore::load(&dir).expect("timer store");
            let t = store.create_at("Alpha", 1000).unwrap();
            id_a = t[0].id.clone();
            let t = store.create_at("Beta", 2000).unwrap();
            id_b = t[1].id.clone();
            let t = store.create_at("Gamma", 3000).unwrap();
            id_c = t[2].id.clone();

            // Reorder to Gamma, Alpha, Beta
            let reordered = store
                .reorder(&[id_c.clone(), id_a.clone(), id_b.clone()])
                .unwrap();
            assert_eq!(reordered[0].label, "Gamma");
            assert_eq!(reordered[1].label, "Alpha");
            assert_eq!(reordered[2].label, "Beta");
        }

        // Reload the store from disk
        let store = TimerStore::load(&dir).expect("timer store");
        let timers = store.snapshot().unwrap();
        assert_eq!(timers.len(), 3);
        assert_eq!(timers[0].id, id_c);
        assert_eq!(timers[0].label, "Gamma");
        assert_eq!(timers[1].id, id_a);
        assert_eq!(timers[1].label, "Alpha");
        assert_eq!(timers[2].id, id_b);
        assert_eq!(timers[2].label, "Beta");
    }

    #[test]
    fn move_timer_shifts_position_and_clamps_at_boundaries() {
        let mut store = store("move-timer");
        let t = store.create_at("One", 1000).unwrap();
        let id1 = t[0].id.clone();
        let t = store.create_at("Two", 2000).unwrap();
        let id2 = t[1].id.clone();
        let t = store.create_at("Three", 3000).unwrap();
        let id3 = t[2].id.clone();

        // Moving the first item up is a no-op
        let res = store.move_timer(&id1, -1).unwrap();
        assert_eq!(res[0].id, id1);

        // Moving the third item down is a no-op
        let res = store.move_timer(&id3, 1).unwrap();
        assert_eq!(res[2].id, id3);

        // Move "Two" up
        let res = store.move_timer(&id2, -1).unwrap();
        assert_eq!(res[0].id, id2);
        assert_eq!(res[1].id, id1);
        assert_eq!(res[2].id, id3);

        // Move "Two" down
        let res = store.move_timer(&id2, 1).unwrap();
        assert_eq!(res[0].id, id1);
        assert_eq!(res[1].id, id2);
        assert_eq!(res[2].id, id3);
    }
}
