//! Timer store — the single source of truth for stopwatch state.
//!
//! Clock policy: every instant is a wall-clock epoch in milliseconds, stamped
//! from `now_epoch_ms` and nothing else. The stopwatch card, `rize` and the
//! tray all read the wall clock, and a run is `now - started_at`, so a start
//! stamped from any other clock (a monotonic `Instant` stops ticking while a
//! Mac sleeps) would make the card read the gap between the two clocks the
//! moment Start was pressed.
//!
//! A timer's time is its segments, each a start and an end like a time entry,
//! plus the run in progress. The run in progress is also a start and an end
//! (`run_span`: started_at and now), and that one span is what the live
//! reading, the pause and the save all use.
//!
//! Persistence policy: timers live in a `timers` table in
//! the same SQLite file activity capture already uses (`activity.db`), each
//! store through its own connection Timers still mutate at human
//! rate, so there's no contention concern moving to row-level SQL statements.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, Row};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::activity::DB_FILE;

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
    /// The group this timer belongs to, or `None`. A group is the timers that
    /// share a name (compared without regard to case), nothing more.
    pub group: Option<String>,
}

/// A stretch of time on a timer, defined as a time entry is: a start and an
/// end in epoch milliseconds. Its length is the difference, never stored.
#[derive(Debug, Clone)]
pub struct TimerSegment {
    pub id: i64,
    pub started_at: u64,
    pub ended_at: u64,
}

impl TimerSegment {
    pub fn duration_ms(&self) -> u64 {
        self.ended_at.saturating_sub(self.started_at)
    }
}

/// The JSON carries the derived `durationMs` beside the start and end, so a
/// reader never has to subtract.
impl Serialize for TimerSegment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut segment = serializer.serialize_struct("TimerSegment", 4)?;
        segment.serialize_field("id", &self.id)?;
        segment.serialize_field("startedAt", &self.started_at)?;
        segment.serialize_field("endedAt", &self.ended_at)?;
        segment.serialize_field("durationMs", &self.duration_ms())?;
        segment.end()
    }
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
       COALESCE((SELECT SUM(ended_at - started_at) FROM timer_segments WHERE timer_id = timers.id), 0),
       started_at, created_at, ordinal, group_name
     FROM timers";

impl Timer {
    pub fn is_running(&self) -> bool {
        self.started_at.is_some()
    }

    /// The run in progress as a start and an end: when it started and `now`,
    /// or `None` while paused. The one computation behind the live reading,
    /// the pause and the save.
    pub fn run_span_at(&self, now: u64) -> Option<(u64, u64)> {
        self.started_at.map(|started| run_span(started, now))
    }

    /// Total time at `now`: the segments plus the run in progress, if any.
    pub fn elapsed_at(&self, now: u64) -> u64 {
        self.accumulated_ms + self.run_span_at(now).map_or(0, |(start, end)| end - start)
    }
}

/// A run that started at `started_at`, ended at `now`. A clock that stepped
/// back leaves a zero-length run, never a negative one.
fn run_span(started_at: u64, now: u64) -> (u64, u64) {
    (started_at, now.max(started_at))
}

/// Wall-clock milliseconds since the Unix epoch.
pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

pub struct TimerStore {
    conn: Connection,
}

impl TimerStore {
    /// Opens the shared database and migrates timer state if needed.
    pub fn load(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|error| format!("could not create data dir: {error}"))?;
        let path = dir.join(DB_FILE);
        let mut conn = Connection::open(&path).map_err(|error| error.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|error| error.to_string())?;
        crate::migrations::run_migrations(&mut conn).map_err(|error| error.to_string())?;
        Ok(Self { conn })
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
        let now = now_epoch_ms();
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
        let now = now_epoch_ms();
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
        let now = now_epoch_ms();
        self.pause_at(id, now)
    }

    pub fn pause_at(&mut self, id: &str, now: u64) -> Result<Vec<Timer>, String> {
        let timer = self.find(id)?;
        if let Some((started_at, ended_at)) = timer.run_span_at(now) {
            // One transaction, so a pause can never stop the clock without
            // keeping the time it ran.
            let tx = self.conn.transaction().map_err(|error| error.to_string())?;
            tx.execute(
                "UPDATE timers SET started_at = NULL WHERE id = ?1",
                params![id],
            )
            .map_err(|error| error.to_string())?;
            insert_segment(&tx, id, started_at, ended_at)?;
            tx.commit().map_err(|error| error.to_string())?;
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

    /// Puts a timer in the group called `group`, or takes it out of any group
    /// when `group` is `None` or blank. A name that matches an existing group
    /// without regard to case joins that group and takes its spelling, so
    /// "Launch" and "launch" never become two groups.
    pub fn set_group(&mut self, id: &str, group: Option<&str>) -> Result<Vec<Timer>, String> {
        self.find(id)?;
        let group = match group.map(str::trim).filter(|name| !name.is_empty()) {
            None => None,
            Some(name) => Some(self.canonical_group(name)?),
        };
        self.conn
            .execute(
                "UPDATE timers SET group_name = ?1 WHERE id = ?2",
                params![group, id],
            )
            .map_err(|error| error.to_string())?;
        self.snapshot()
    }

    pub fn start_group(&mut self, group: &str) -> Result<Vec<Timer>, String> {
        let now = now_epoch_ms();
        self.start_group_at(group, now)
    }

    /// Starts every paused timer in the group at the same instant, so the
    /// set reads one clock; a timer already running keeps its own start.
    pub fn start_group_at(&mut self, group: &str, now: u64) -> Result<Vec<Timer>, String> {
        let members = self.members(group)?;
        let tx = self.conn.transaction().map_err(|error| error.to_string())?;
        for timer in members.iter().filter(|timer| !timer.is_running()) {
            tx.execute(
                "UPDATE timers SET started_at = ?1 WHERE id = ?2",
                params![now as i64, timer.id],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        self.snapshot()
    }

    pub fn pause_group(&mut self, group: &str) -> Result<Vec<Timer>, String> {
        let now = now_epoch_ms();
        self.pause_group_at(group, now)
    }

    /// Pauses every running timer in the group, each recording its segment as
    /// `pause` does, all in one transaction.
    pub fn pause_group_at(&mut self, group: &str, now: u64) -> Result<Vec<Timer>, String> {
        let members = self.members(group)?;
        let tx = self.conn.transaction().map_err(|error| error.to_string())?;
        for timer in &members {
            if let Some((started_at, ended_at)) = timer.run_span_at(now) {
                tx.execute(
                    "UPDATE timers SET started_at = NULL WHERE id = ?1",
                    params![timer.id],
                )
                .map_err(|error| error.to_string())?;
                insert_segment(&tx, &timer.id, started_at, ended_at)?;
            }
        }
        tx.commit().map_err(|error| error.to_string())?;
        self.snapshot()
    }

    /// The timers in `group`, oldest ordinal first; an error if there are none.
    fn members(&self, group: &str) -> Result<Vec<Timer>, String> {
        let group = group.trim();
        let members: Vec<Timer> = self
            .snapshot()?
            .into_iter()
            .filter(|timer| {
                timer
                    .group
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(group))
            })
            .collect();
        if members.is_empty() {
            return Err(unknown_group(group));
        }
        Ok(members)
    }

    /// The spelling an existing group already uses for `name`, else `name`.
    fn canonical_group(&self, name: &str) -> Result<String, String> {
        Ok(self
            .snapshot()?
            .into_iter()
            .filter_map(|timer| timer.group)
            .find(|existing| existing.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| name.to_string()))
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

    /// Adds a stretch of time to the total, from `started_at` to `ended_at`.
    pub fn add_segment(
        &mut self,
        id: &str,
        started_at: u64,
        ended_at: u64,
    ) -> Result<Vec<Timer>, String> {
        self.find(id)?;
        check_span(started_at, ended_at)?;
        insert_segment(&self.conn, id, started_at, ended_at)?;
        self.snapshot()
    }

    /// Changes a segment's start and/or end; whichever is `None` stays as it
    /// is, and the pair that results has to be a valid span.
    pub fn update_segment(
        &mut self,
        id: &str,
        segment_id: i64,
        started_at: Option<u64>,
        ended_at: Option<u64>,
    ) -> Result<Vec<Timer>, String> {
        let timer = self.find(id)?;
        if started_at.is_none() && ended_at.is_none() {
            return Err("nothing to change: give a new start, a new end, or both".to_string());
        }
        let current = timer
            .segments
            .iter()
            .find(|segment| segment.id == segment_id)
            .ok_or_else(|| unknown_segment(segment_id))?;
        let started_at = started_at.unwrap_or(current.started_at);
        let ended_at = ended_at.unwrap_or(current.ended_at);
        check_span(started_at, ended_at)?;
        self.conn
            .execute(
                "UPDATE timer_segments SET started_at = ?1, ended_at = ?2
                 WHERE id = ?3 AND timer_id = ?4",
                params![started_at as i64, ended_at as i64, segment_id, id],
            )
            .map_err(|error| error.to_string())?;
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
        let now = now_epoch_ms();
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

    fn segments_of(&self, id: &str) -> Result<Vec<TimerSegment>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, started_at, ended_at FROM timer_segments
                 WHERE timer_id = ?1 ORDER BY started_at ASC, id ASC",
            )
            .map_err(|error| error.to_string())?;
        let segments = statement
            .query_map(params![id], |row| {
                Ok(TimerSegment {
                    id: row.get(0)?,
                    started_at: row.get::<_, i64>(1)? as u64,
                    ended_at: row.get::<_, i64>(2)? as u64,
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
        group: row.get(6)?,
    })
}

fn insert_segment(
    conn: &Connection,
    id: &str,
    started_at: u64,
    ended_at: u64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO timer_segments (timer_id, started_at, ended_at) VALUES (?1, ?2, ?3)",
        params![id, started_at as i64, ended_at as i64],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// A segment is a start and an end, and may be zero length; it cannot end
/// before it starts.
pub fn check_span(started_at: u64, ended_at: u64) -> Result<(), String> {
    if ended_at < started_at {
        return Err(
            "a segment cannot end before it starts: set the end to the start or later, or move the start earlier"
                .to_string(),
        );
    }
    if ended_at > i64::MAX as u64 {
        return Err("that time is too far in the future to store".to_string());
    }
    Ok(())
}

fn unknown_segment(segment_id: i64) -> String {
    format!("no time segment with id {segment_id}")
}

fn unknown_group(group: &str) -> String {
    format!("no group named {group}")
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
    fn a_timer_started_at_a_known_instant_reads_now_minus_that_instant_and_nothing_else() {
        let mut store = store("reads-now-minus-start");
        let id = store.create_at("Stopwatch", 1_000).unwrap()[0].id.clone();

        // Never run: zero, whatever the clock says.
        let fresh = store.snapshot().unwrap().remove(0);
        assert_eq!(fresh.elapsed_at(0), 0);
        assert_eq!(fresh.elapsed_at(1_000), 0);
        assert_eq!(fresh.elapsed_at(u64::MAX), 0);

        // Running: exactly `now - started_at`, second by second.
        store.start_at(&id, 50_000).unwrap();
        let running = store.snapshot().unwrap().remove(0);
        for seconds in 0..4 {
            assert_eq!(
                running.elapsed_at(50_000 + seconds * 1_000),
                seconds * 1_000
            );
        }
        // A clock that stepped back never reads negative or wraps.
        assert_eq!(running.elapsed_at(49_000), 0);

        // Pausing keeps what the display showed, to the millisecond.
        let shown = running.elapsed_at(53_250);
        store.pause_at(&id, 53_250).unwrap();
        let paused = store.snapshot().unwrap().remove(0);
        assert_eq!(paused.elapsed_at(900_000), shown);
        assert_eq!(paused.segments[0].started_at, 50_000);
        assert_eq!(paused.segments[0].ended_at, 53_250);
    }

    #[test]
    fn start_is_stamped_from_the_wall_clock_the_card_reads() {
        // `Start` used to be stamped from a monotonic `Instant` anchored at
        // launch, which stops ticking while a Mac sleeps: after a 7h16m45s
        // sleep the card (wall clock) read 7:16:45 the moment a stopwatch
        // started, then fell back to 0:00 on Pause. A fresh process cannot
        // show that drift, so this pins the contract the fix restored: the
        // public `start` stamps `now_epoch_ms`, the clock the card reads.
        let mut store = store("start-uses-wall-clock");
        let id = store.create("Stopwatch").unwrap()[0].id.clone();
        let before = now_epoch_ms();
        let timer = store.start(&id).unwrap().remove(0);
        let after = now_epoch_ms();

        let started = timer.started_at.expect("running");
        assert!((before..=after).contains(&started), "stamped {started}");
        assert!(timer.elapsed_at(after) <= after - before);
    }

    #[test]
    fn timers_sharing_a_group_name_form_one_group_whatever_the_case() {
        let mut store = store("group-names");
        let a = store.create_at("A", 1).unwrap()[0].id.clone();
        store.create_at("B", 2).unwrap();
        let b = store.snapshot().unwrap()[1].id.clone();

        store.set_group(&a, Some("  Launch ")).unwrap();
        let timers = store.set_group(&b, Some("launch")).unwrap();
        assert_eq!(timers[0].group.as_deref(), Some("Launch"));
        // Joins the existing group and takes its spelling.
        assert_eq!(timers[1].group.as_deref(), Some("Launch"));

        let timers = store.set_group(&a, Some("   ")).unwrap();
        assert_eq!(timers[0].group, None);
        assert!(store.set_group("t99", Some("x")).is_err());
    }

    #[test]
    fn a_group_starts_and_pauses_as_a_set_at_one_instant() {
        let mut store = store("group-run");
        for label in ["A", "B", "C"] {
            store.create_at(label, 1).unwrap();
        }
        let ids: Vec<String> = store.snapshot().unwrap().into_iter().map(|t| t.id).collect();
        store.set_group(&ids[0], Some("Launch")).unwrap();
        store.set_group(&ids[1], Some("Launch")).unwrap();
        // B is already running from its own start; C is outside the group.
        store.start_at(&ids[1], 10_000).unwrap();

        let timers = store.start_group_at("launch", 20_000).unwrap();
        assert_eq!(timers[0].started_at, Some(20_000));
        assert_eq!(timers[1].started_at, Some(10_000));
        assert_eq!(timers[2].started_at, None);

        let timers = store.pause_group_at("Launch", 50_000).unwrap();
        assert!(timers.iter().all(|timer| !timer.is_running()));
        assert_eq!(timers[0].accumulated_ms, 30_000);
        assert_eq!(timers[1].accumulated_ms, 40_000);
        assert_eq!(timers[2].accumulated_ms, 0);
        assert_eq!(timers[1].segments[0].started_at, 10_000);

        assert!(store.start_group_at("nobody", 1).is_err());
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
        store.add_segment(&id, 70_000, 100_000).unwrap();

        let timer = store.snapshot().unwrap().remove(0);
        assert_eq!(timer.accumulated_ms, 90_000);
        assert_eq!(timer.segments.len(), 2);
        assert_eq!(
            (timer.segments[0].started_at, timer.segments[0].ended_at),
            (1_000, 61_000)
        );
        assert_eq!(timer.segments[1].duration_ms(), 30_000);

        let first = timer.segments[0].id;
        let second = timer.segments[1].id;
        // Change only the end, then only the start, then both.
        store
            .update_segment(&id, first, None, Some(11_000))
            .unwrap();
        assert_eq!(store.snapshot().unwrap()[0].accumulated_ms, 40_000);
        store.update_segment(&id, first, Some(6_000), None).unwrap();
        assert_eq!(store.snapshot().unwrap()[0].accumulated_ms, 35_000);
        store
            .update_segment(&id, first, Some(1_000), Some(11_000))
            .unwrap();
        assert_eq!(store.snapshot().unwrap()[0].accumulated_ms, 40_000);

        store.delete_segment(&id, second).unwrap();
        assert_eq!(store.snapshot().unwrap()[0].accumulated_ms, 10_000);

        assert!(store.update_segment(&id, second, Some(1), None).is_err());
    }

    #[test]
    fn a_segment_may_be_zero_length_but_never_negative() {
        let mut store = store("segment-validation");
        let id = store.create_at("Strict", 0).unwrap()[0].id.clone();

        store.add_segment(&id, 5_000, 5_000).unwrap();
        let timer = store.snapshot().unwrap().remove(0);
        assert_eq!(timer.accumulated_ms, 0);
        assert_eq!(timer.segments.len(), 1);

        let refused = store.add_segment(&id, 5_001, 5_000).unwrap_err();
        assert!(refused.contains("cannot end before it starts"), "{refused}");
        assert_eq!(store.snapshot().unwrap()[0].segments.len(), 1);

        // An edit is checked as the pair it leaves behind, so moving only the
        // start past the end, or only the end before the start, is refused.
        let segment = timer.segments[0].id;
        assert!(store
            .update_segment(&id, segment, Some(5_001), None)
            .is_err());
        assert!(store
            .update_segment(&id, segment, None, Some(4_999))
            .is_err());
        assert!(store.update_segment(&id, segment, None, None).is_err());
        assert_eq!(
            store.snapshot().unwrap()[0].segments[0].started_at,
            5_000,
            "a refused edit leaves the segment alone"
        );
        assert!(store.add_segment("nope", 1, 2).is_err());
    }

    #[test]
    fn pausing_the_same_instant_it_started_records_a_zero_length_segment() {
        let mut store = store("zero-run");
        let id = store.create_at("Blink", 0).unwrap()[0].id.clone();
        store.start_at(&id, 9_000).unwrap();
        // A clock that stepped back mid-run clamps to zero, not a refusal.
        store.pause_at(&id, 8_000).unwrap();
        let timer = store.snapshot().unwrap().remove(0);
        assert_eq!(timer.accumulated_ms, 0);
        assert_eq!(
            (timer.segments[0].started_at, timer.segments[0].ended_at),
            (9_000, 9_000)
        );
        assert!(!timer.is_running());
    }

    #[test]
    fn the_live_reading_the_pause_and_the_save_agree() {
        let mut store = store("one-computation");
        let id = store.create_at("Agree", 0).unwrap()[0].id.clone();
        store.add_segment(&id, 0, 10_000).unwrap();
        store.start_at(&id, 20_000).unwrap();

        let now = 27_345;
        let live = store.snapshot().unwrap()[0].elapsed_at(now);
        let saved = store.save_state_at(&id, now).unwrap().elapsed_ms;
        store.pause_at(&id, now).unwrap();
        let stored = store.snapshot().unwrap()[0].accumulated_ms;
        assert_eq!((live, saved, stored), (17_345, 17_345, 17_345));
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

    #[test]
    fn upgrade_from_0_9_19_preserves_every_timer_segment_and_saved_state() {
        let dir = temp_dir("0-9-19-upgrade");
        {
            // Exactly the 0.9.19 database shape: user_version at 10,
            // timers table predates ordinal column, timer_segments and
            // timer_saved_states both present with rows across running and paused timers.
            let mut conn = Connection::open(dir.join(DB_FILE)).unwrap();
            crate::migrations::run_migrations(&mut conn).unwrap();
            conn.execute_batch(
                "DROP TABLE timer_saved_states;
                 DROP TABLE timer_segments;
                 DROP TABLE timers;
                 CREATE TABLE timers (
                   id             TEXT PRIMARY KEY,
                   label          TEXT NOT NULL,
                   accumulated_ms INTEGER NOT NULL,
                   started_at     INTEGER,
                   created_at     INTEGER NOT NULL
                 );
                 CREATE TABLE timer_segments (
                   id          INTEGER PRIMARY KEY AUTOINCREMENT,
                   timer_id    TEXT NOT NULL,
                   duration_ms INTEGER NOT NULL,
                   recorded_at INTEGER NOT NULL
                 );
                 CREATE TABLE timer_saved_states (
                   id         INTEGER PRIMARY KEY AUTOINCREMENT,
                   timer_id   TEXT NOT NULL,
                   label      TEXT NOT NULL,
                   elapsed_ms INTEGER NOT NULL,
                   saved_at   INTEGER NOT NULL
                 );
                 -- Running timer: has started_at and 2 segments and 1 saved state
                 INSERT INTO timers VALUES ('t1', 'Running Project', 0, 1700000050000, 1700000000000);
                 INSERT INTO timer_segments (id, timer_id, duration_ms, recorded_at)
                   VALUES (1, 't1', 60000, 1700000010000), (2, 't1', 120000, 1700000020000);
                 INSERT INTO timer_saved_states (id, timer_id, label, elapsed_ms, saved_at)
                   VALUES (1, 't1', 'Checkpoint 1', 180000, 1700000030000);

                 -- Paused timer: started_at is NULL, 1 segment and 1 saved state, created after t3
                 INSERT INTO timers VALUES ('t2', 'Paused Project', 0, NULL, 1700000020000);
                 INSERT INTO timer_segments (id, timer_id, duration_ms, recorded_at)
                   VALUES (3, 't2', 300000, 1700000025000);
                 INSERT INTO timer_saved_states (id, timer_id, label, elapsed_ms, saved_at)
                   VALUES (2, 't2', 'Saved state for t2', 300000, 1700000035000);

                 -- Third timer created after t1 but before t2
                 INSERT INTO timers VALUES ('t3', 'Third Project', 0, NULL, 1700000010000);
                 INSERT INTO timer_segments (id, timer_id, duration_ms, recorded_at)
                   VALUES (4, 't3', 45000, 1700000022000);
                 INSERT INTO timer_saved_states (id, timer_id, label, elapsed_ms, saved_at)
                   VALUES (3, 't3', 'Saved state for t3', 45000, 1700000033000);
                 PRAGMA user_version = 10;",
            )
            .unwrap();
        }

        // Open with the updated TimerStore (which runs migration 11)
        let store = TimerStore::load(&dir).expect("timer store load");
        let timers = store.snapshot().expect("snapshot");
        assert_eq!(timers.len(), 3);

        // Order is stable by created_at: t1 (0000), t3 (0010), t2 (0020)
        assert_eq!(timers[0].id, "t1");
        assert_eq!(timers[0].label, "Running Project");
        assert_eq!(timers[0].ordinal, 0);
        assert_eq!(timers[0].accumulated_ms, 180000);
        assert_eq!(timers[0].started_at, Some(1700000050000));
        assert!(timers[0].is_running());
        assert_eq!(timers[0].segments.len(), 2);
        let segment = |id: i64| timers[0].segments.iter().find(|s| s.id == id).unwrap();
        assert_eq!(segment(1).duration_ms(), 60000);
        assert_eq!(segment(2).duration_ms(), 120000);
        let s1 = store.saved_states("t1").expect("saved states t1");
        assert_eq!(s1.len(), 1);
        assert_eq!(s1[0].label, "Checkpoint 1");
        assert_eq!(s1[0].elapsed_ms, 180000);

        assert_eq!(timers[1].id, "t3");
        assert_eq!(timers[1].label, "Third Project");
        assert_eq!(timers[1].ordinal, 1);
        assert_eq!(timers[1].accumulated_ms, 45000);
        assert_eq!(timers[1].started_at, None);
        assert!(!timers[1].is_running());
        assert_eq!(timers[1].segments.len(), 1);

        assert_eq!(timers[2].id, "t2");
        assert_eq!(timers[2].label, "Paused Project");
        assert_eq!(timers[2].ordinal, 2);
        assert_eq!(timers[2].accumulated_ms, 300000);
        assert_eq!(timers[2].started_at, None);
        assert!(!timers[2].is_running());
        assert_eq!(timers[2].segments.len(), 1);

        // Every segment now ends when it was recorded and starts its old
        // length before that.
        assert_eq!(
            (segment(1).started_at, segment(1).ended_at),
            (1700000010000 - 60000, 1700000010000)
        );
        assert_eq!(
            (segment(2).started_at, segment(2).ended_at),
            (1700000020000 - 120000, 1700000020000)
        );

        let conn = Connection::open(dir.join(DB_FILE)).unwrap();
        let version: i32 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, crate::migrations::SCHEMA_VERSION);
    }

    /// A database in the shape before segments were a start and an end:
    /// `duration_ms` and `recorded_at`, user_version 11.
    fn seed_length_based_segments(dir: &Path) -> Vec<(String, i64)> {
        let mut conn = Connection::open(dir.join(DB_FILE)).unwrap();
        crate::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch(
            "DROP TABLE timer_segments;
             CREATE TABLE timer_segments (
               id          INTEGER PRIMARY KEY AUTOINCREMENT,
               timer_id    TEXT NOT NULL,
               duration_ms INTEGER NOT NULL,
               recorded_at INTEGER NOT NULL
             );
             INSERT INTO timers (id, label, accumulated_ms, started_at, created_at, ordinal) VALUES ('t1', 'Paused', 0, NULL, 1700000000000, 0);
             INSERT INTO timers (id, label, accumulated_ms, started_at, created_at, ordinal) VALUES ('t2', 'Running', 0, 1700000500000, 1700000000000, 1);
             INSERT INTO timers (id, label, accumulated_ms, started_at, created_at, ordinal) VALUES ('t3', 'Empty', 0, NULL, 1700000000000, 2);
             INSERT INTO timer_segments (id, timer_id, duration_ms, recorded_at) VALUES
               (1, 't1', 3600000, 1700003600000),
               (2, 't1', 0, 1700003700000),
               (3, 't1', 1234567, 1700010000000),
               (4, 't2', 90000, 1700000100000);
             PRAGMA user_version = 11;",
        )
        .unwrap();
        totals_by_timer(&conn, "SUM(duration_ms)")
    }

    fn totals_by_timer(conn: &Connection, sum: &str) -> Vec<(String, i64)> {
        conn.prepare(&format!(
            "SELECT timer_id, {sum} FROM timer_segments GROUP BY timer_id ORDER BY timer_id"
        ))
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    #[test]
    fn segments_migrate_to_start_and_end_without_changing_any_total() {
        let dir = temp_dir("start-end-migration");
        let before = seed_length_based_segments(&dir);
        assert_eq!(
            before,
            [("t1".into(), 4_834_567), ("t2".into(), 90_000)],
            "the seed is what the old code stored"
        );

        let store = TimerStore::load(&dir).expect("timer store");
        let timers = store.snapshot().unwrap();
        let after: Vec<(String, i64)> = timers
            .iter()
            .filter(|timer| !timer.segments.is_empty())
            .map(|timer| (timer.id.clone(), timer.accumulated_ms as i64))
            .collect();
        assert_eq!(after, before);
        assert_eq!(timers[2].accumulated_ms, 0);

        // The end is when the stretch was recorded; the start is its old
        // length before that. Ids and the running timer's start survive.
        let spans: Vec<(i64, u64, u64)> = timers[0]
            .segments
            .iter()
            .map(|segment| (segment.id, segment.started_at, segment.ended_at))
            .collect();
        assert_eq!(
            spans,
            [
                (1, 1700000000000, 1700003600000),
                (2, 1700003700000, 1700003700000),
                (3, 1700008765433, 1700010000000),
            ]
        );
        assert_eq!(timers[1].started_at, Some(1700000500000));
        assert_eq!(timers[1].elapsed_at(1700000502000), 92_000);

        let conn = Connection::open(dir.join(DB_FILE)).unwrap();
        assert_eq!(totals_by_timer(&conn, "SUM(ended_at - started_at)"), before);
    }

    #[test]
    fn the_start_and_end_migration_is_idempotent_and_safe_on_the_new_shape() {
        let dir = temp_dir("start-end-idempotent");
        let before = seed_length_based_segments(&dir);

        // Opening twice runs the migration once, then not at all.
        for _ in 0..2 {
            let store = TimerStore::load(&dir).expect("timer store");
            assert_eq!(store.snapshot().unwrap()[0].accumulated_ms, 4_834_567);
        }

        // A database that already has the new columns but still reads as the
        // older version (restored mid-upgrade, say) keeps its rows untouched.
        let mut conn = Connection::open(dir.join(DB_FILE)).unwrap();
        conn.execute_batch("PRAGMA user_version = 11;").unwrap();
        crate::migrations::run_migrations(&mut conn).unwrap();
        assert_eq!(totals_by_timer(&conn, "SUM(ended_at - started_at)"), before);
        let version: i32 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, crate::migrations::SCHEMA_VERSION);
    }

    #[test]
    fn upgrade_from_database_predating_timer_segments_preserves_accumulated_time() {
        let dir = temp_dir("pre-segments-upgrade");
        {
            let mut conn = Connection::open(dir.join(DB_FILE)).unwrap();
            crate::migrations::run_migrations(&mut conn).unwrap();
            conn.execute_batch(
                "DROP TABLE timer_saved_states;
                 DROP TABLE timer_segments;
                 DROP TABLE timers;
                 CREATE TABLE timers (
                   id             TEXT PRIMARY KEY,
                   label          TEXT NOT NULL,
                   accumulated_ms INTEGER NOT NULL,
                   started_at     INTEGER,
                   created_at     INTEGER NOT NULL
                 );
                 INSERT INTO timers VALUES ('t1', 'Pre-segment timer', 3600000, NULL, 1000);
                 PRAGMA user_version = 10;",
            )
            .unwrap();
        }

        let store = TimerStore::load(&dir).expect("timer store load");
        let timers = store.snapshot().expect("snapshot");
        assert_eq!(timers.len(), 1);
        assert_eq!(timers[0].id, "t1");
        assert_eq!(timers[0].label, "Pre-segment timer");
        assert_eq!(timers[0].accumulated_ms, 3600000);
        assert_eq!(timers[0].segments.len(), 1);
        assert_eq!(timers[0].segments[0].duration_ms(), 3600000);
        // It predates segments, so it ends when the timer was created.
        assert_eq!(timers[0].segments[0].ended_at, 3600000);
    }
}
