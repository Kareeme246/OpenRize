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
  created_at     INTEGER NOT NULL
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
    /// Sum of every completed run.
    pub accumulated_ms: u64,
    /// Epoch milliseconds at which the current run started, or `None` if paused.
    pub started_at: Option<u64>,
    pub created_at: u64,
}

impl Timer {
    pub fn is_running(&self) -> bool {
        self.started_at.is_some()
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
        let conn = Connection::open(&path).map_err(|error| error.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|error| error.to_string())?;
        conn.execute_batch(SCHEMA)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            clock: Clock::new(),
            conn,
        })
    }

    pub fn snapshot(&self) -> Result<Vec<Timer>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, label, accumulated_ms, started_at, created_at
                 FROM timers
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|error| error.to_string())?;
        let timers = statement
            .query_map([], timer_from_row)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
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
        // Created idle: naming a tracker is not the same as deciding to bill
        // time to it, so the card waits for an explicit Start.
        self.conn
            .execute(
                "INSERT INTO timers (id, label, accumulated_ms, started_at, created_at)
                 VALUES (?1, ?2, 0, NULL, ?3)",
                params![id, label, now as i64],
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
            let accumulated = timer.accumulated_ms + now.saturating_sub(started);
            self.conn
                .execute(
                    "UPDATE timers SET accumulated_ms = ?1, started_at = NULL WHERE id = ?2",
                    params![accumulated as i64, id],
                )
                .map_err(|error| error.to_string())?;
        }
        self.snapshot()
    }

    pub fn reset(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        self.find(id)?;
        self.conn
            .execute(
                "UPDATE timers SET accumulated_ms = 0, started_at = NULL WHERE id = ?1",
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

    pub fn delete(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        let changed = self
            .conn
            .execute("DELETE FROM timers WHERE id = ?1", params![id])
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err(unknown(id));
        }
        self.snapshot()
    }

    fn find(&self, id: &str) -> Result<Timer, String> {
        self.conn
            .query_row(
                "SELECT id, label, accumulated_ms, started_at, created_at
                 FROM timers WHERE id = ?1",
                params![id],
                timer_from_row,
            )
            .map_err(|_| unknown(id))
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
        let next = self
            .read_setting("timer_next_id")
            .and_then(|value| value.parse().ok())
            .unwrap_or(1u64);
        self.write_setting("timer_next_id", &(next + 1).to_string())?;
        Ok(next)
    }
}

fn timer_from_row(row: &Row<'_>) -> rusqlite::Result<Timer> {
    Ok(Timer {
        id: row.get(0)?,
        label: row.get(1)?,
        accumulated_ms: row.get::<_, i64>(2)? as u64,
        started_at: row.get::<_, Option<i64>>(3)?.map(|value| value as u64),
        created_at: row.get::<_, i64>(4)? as u64,
    })
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
}
