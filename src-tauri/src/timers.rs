//! Timer store — the single source of truth for stopwatch state.
//!
//! Clock policy (decision D2): the *persisted* anchor is a wall-clock epoch in
//! milliseconds, because an `Instant` cannot survive a restart. While the
//! process is alive, `now` comes from a monotonic `Instant` anchored to a
//! startup epoch, so a system-clock change mid-session cannot corrupt a running
//! stopwatch.
//!
//! Persistence policy (D1): the whole store is written synchronously on every
//! mutation.
// ponytail: synchronous write per mutation — the file is a few KB and mutations
// arrive at human rate. Add a debounce thread only if something ever mutates at
// machine rate (iteration 2's activity samples, which will move to SQLite).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "timers.json";

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

#[derive(Debug, Serialize, Deserialize)]
struct TimersFile {
    version: u32,
    next_id: u64,
    timers: Vec<Timer>,
}

pub struct TimerStore {
    clock: Clock,
    path: PathBuf,
    timers: Vec<Timer>,
    next_id: u64,
}

impl TimerStore {
    /// Loads `timers.json` from `dir`. A missing file is an empty store; a
    /// corrupt one is moved aside rather than panicked over, so a bad write can
    /// never cost the user every timer silently.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        let loaded = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<TimersFile>(&bytes).ok());

        let (timers, next_id) = match loaded {
            Some(file) => (file.timers, file.next_id),
            None => {
                if path.exists() {
                    let quarantine = path.with_extension(format!("corrupt-{}", now_epoch_ms()));
                    let _ = fs::rename(&path, &quarantine);
                    eprintln!(
                        "timers.json was unreadable; moved to {}",
                        quarantine.display()
                    );
                }
                (Vec::new(), 1)
            }
        };

        Self {
            clock: Clock::new(),
            path,
            timers,
            next_id,
        }
    }

    pub fn snapshot(&self) -> Vec<Timer> {
        self.timers.clone()
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
        let id = format!("t{}", self.next_id);
        self.next_id += 1;
        // Created idle: naming a tracker is not the same as deciding to bill
        // time to it, so the card waits for an explicit Start.
        self.timers.push(Timer {
            id,
            label: label.to_string(),
            accumulated_ms: 0,
            started_at: None,
            created_at: now,
        });
        self.persist()?;
        Ok(self.snapshot())
    }

    pub fn start(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        let now = self.clock.now_ms();
        self.start_at(id, now)
    }

    pub fn start_at(&mut self, id: &str, now: u64) -> Result<Vec<Timer>, String> {
        let timer = self.find_mut(id)?;
        if timer.started_at.is_none() {
            timer.started_at = Some(now);
        }
        self.persist()?;
        Ok(self.snapshot())
    }

    pub fn pause(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        let now = self.clock.now_ms();
        self.pause_at(id, now)
    }

    pub fn pause_at(&mut self, id: &str, now: u64) -> Result<Vec<Timer>, String> {
        let timer = self.find_mut(id)?;
        if let Some(started) = timer.started_at.take() {
            timer.accumulated_ms += now.saturating_sub(started);
        }
        self.persist()?;
        Ok(self.snapshot())
    }

    pub fn reset(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        let timer = self.find_mut(id)?;
        timer.accumulated_ms = 0;
        timer.started_at = None;
        self.persist()?;
        Ok(self.snapshot())
    }

    pub fn rename(&mut self, id: &str, label: &str) -> Result<Vec<Timer>, String> {
        let label = label.trim();
        if label.is_empty() {
            return Err("a tracker needs a name".to_string());
        }
        let timer = self.find_mut(id)?;
        timer.label = label.to_string();
        self.persist()?;
        Ok(self.snapshot())
    }

    pub fn delete(&mut self, id: &str) -> Result<Vec<Timer>, String> {
        let index = self
            .timers
            .iter()
            .position(|timer| timer.id == id)
            .ok_or_else(|| unknown(id))?;
        self.timers.remove(index);
        self.persist()?;
        Ok(self.snapshot())
    }

    fn find_mut(&mut self, id: &str) -> Result<&mut Timer, String> {
        self.timers
            .iter_mut()
            .find(|timer| timer.id == id)
            .ok_or_else(|| unknown(id))
    }

    /// Atomic write: a crash mid-write leaves the previous file intact.
    fn persist(&self) -> Result<(), String> {
        let payload = TimersFile {
            version: SCHEMA_VERSION,
            next_id: self.next_id,
            timers: self.timers.clone(),
        };
        let json = serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?;
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, json).map_err(|error| format!("could not write timers: {error}"))?;
        fs::rename(&temp, &self.path).map_err(|error| format!("could not replace timers: {error}"))
    }
}

fn unknown(id: &str) -> String {
    format!("no tracker with id {id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openrize-{name}-{}", now_epoch_ms()));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn store(name: &str) -> TimerStore {
        TimerStore::load(&temp_dir(name))
    }

    #[test]
    fn pause_accumulates_only_the_time_that_ran() {
        let mut store = store("pause");
        let id = store.create_at("Write the handoff", 1_000).unwrap()[0]
            .id
            .clone();
        store.start_at(&id, 1_000).unwrap();

        store.pause_at(&id, 4_500).unwrap();
        let paused = &store.snapshot()[0];
        assert_eq!(paused.accumulated_ms, 3_500);
        assert!(!paused.is_running());

        // Pausing again must not double-count.
        store.pause_at(&id, 9_000).unwrap();
        assert_eq!(store.snapshot()[0].accumulated_ms, 3_500);
    }

    #[test]
    fn reset_zeroes_and_stops_the_timer() {
        let mut store = store("reset");
        let id = store.create_at("Reset me", 1_000).unwrap()[0].id.clone();
        store.start_at(&id, 1_000).unwrap();

        store.reset(&id).unwrap();
        let timer = &store.snapshot()[0];
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
        let mut first = TimerStore::load(&dir);
        let id = first.create_at("Survive a restart", 1_000).unwrap()[0]
            .id
            .clone();
        first.start_at(&id, 1_000).unwrap();
        first.pause_at(&id, 4_000).unwrap();

        let second = TimerStore::load(&dir);
        let timer = &second.snapshot()[0];
        assert_eq!(timer.label, "Survive a restart");
        assert_eq!(timer.accumulated_ms, 3_000);
        assert_eq!(second.snapshot().len(), 1);
    }
}
