//! Timer store — the single source of truth for stopwatch state.
//!
//! Clock policy (decision D2): the *persisted* anchor is a wall-clock epoch in
//! milliseconds, because an `Instant` cannot survive a restart. While the
//! process is alive, `now` comes from a monotonic `Instant` anchored to a
//! startup epoch, so a system-clock change mid-session cannot corrupt a running
//! stopwatch.
//!
//! Persistence policy (decision A8/opt 7): timers live in a `timers` table in
//! the same SQLite file activity capture already uses (`activity.db`), each
//! store through its own connection — WAL permits that, and it means one
//! backup artifact covers both instead of a JSON file and a database. This
//! used to be a synchronous whole-file JSON rewrite per mutation; that was a
//! reasonable choice while this was the only persisted store; it stopped
//! being one the moment there was already a database file sitting right next
//! to it with no shared story between the two. Timers still mutate at human
//! rate, so there's no contention concern moving to row-level SQL statements.
//!
//! A store that finds a legacy `timers.json` on disk imports it once (see
//! `migrate_from_json`) and quarantines the file rather than deleting it —
//! same "don't destroy, set aside" policy this module already used for a
//! corrupt file.

use std::fs;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::activity::DB_FILE;

/// The legacy on-disk format this module migrates away from. Only used to
/// parse a pre-existing `timers.json`, if one is found. `version` is carried
/// through for shape compatibility but was never validated even when this
/// was the live format, so there's nothing to check on the way in either.
const FILE_NAME: &str = "timers.json";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Only used to parse a legacy `timers.json` during migration.
#[derive(Debug, Serialize, Deserialize)]
struct TimersFile {
    version: u32,
    next_id: u64,
    timers: Vec<Timer>,
}

pub struct TimerStore {
    clock: Clock,
    conn: Connection,
}

impl TimerStore {
    /// Opens the shared database, creates the `timers` table if needed, and
    /// imports a legacy `timers.json` the first time one is found.
    pub fn load(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|error| format!("could not create data dir: {error}"))?;
        let path = dir.join(DB_FILE);
        let conn = Connection::open(&path).map_err(|error| error.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|error| error.to_string())?;
        conn.execute_batch(SCHEMA)
            .map_err(|error| error.to_string())?;
        migrate_from_json(&conn, dir)?;
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

/// One-time import of a legacy `timers.json`, if one exists and the `timers`
/// table is still empty (never overwrites rows already in SQLite). The JSON
/// file is quarantined afterward, matching the corrupt-file policy below —
/// data is set aside, never destroyed.
fn migrate_from_json(conn: &Connection, dir: &Path) -> Result<(), String> {
    let path = dir.join(FILE_NAME);
    if !path.exists() {
        return Ok(());
    }

    let existing: i64 = conn
        .query_row("SELECT COUNT(*) FROM timers", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if existing > 0 {
        return Ok(());
    }

    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    match serde_json::from_slice::<TimersFile>(&bytes) {
        Ok(file) => {
            for timer in &file.timers {
                conn.execute(
                    "INSERT INTO timers (id, label, accumulated_ms, started_at, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        timer.id,
                        timer.label,
                        timer.accumulated_ms as i64,
                        timer.started_at.map(|value| value as i64),
                        timer.created_at as i64,
                    ],
                )
                .map_err(|error| error.to_string())?;
            }
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('timer_next_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![file.next_id.to_string()],
            )
            .map_err(|error| error.to_string())?;

            let quarantine = path.with_extension(format!("json.migrated-{}", now_epoch_ms()));
            let _ = fs::rename(&path, &quarantine);
        }
        Err(_) => {
            let quarantine = path.with_extension(format!("corrupt-{}", now_epoch_ms()));
            let _ = fs::rename(&path, &quarantine);
            eprintln!(
                "timers.json was unreadable; moved to {}",
                quarantine.display()
            );
        }
    }
    Ok(())
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

    #[test]
    fn a_legacy_json_file_is_migrated_once_and_quarantined() {
        let dir = temp_dir("migrate");
        let legacy = TimersFile {
            version: 1,
            next_id: 2,
            timers: vec![Timer {
                id: "t1".to_string(),
                label: "Legacy timer".to_string(),
                accumulated_ms: 5_000,
                started_at: None,
                created_at: 1_000,
            }],
        };
        fs::write(
            dir.join(FILE_NAME),
            serde_json::to_vec(&legacy).expect("serialize legacy file"),
        )
        .expect("write legacy file");

        let mut store = TimerStore::load(&dir).expect("timer store");
        let timers = store.snapshot().unwrap();
        assert_eq!(timers.len(), 1);
        assert_eq!(timers[0].label, "Legacy timer");
        assert_eq!(timers[0].accumulated_ms, 5_000);

        // The next id picks up from the migrated counter, not from scratch.
        let after_create = store.create_at("New one", 2_000).unwrap();
        assert_eq!(after_create[1].id, "t2");

        assert!(!dir.join(FILE_NAME).exists());
        let quarantined = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.file_name().to_string_lossy().contains("migrated"));
        assert!(quarantined, "expected a quarantined copy of timers.json");
    }
}
