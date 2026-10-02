//! The open stores one process works on, and the lock that makes it the only
//! one. The app holds the lock for its whole life; `rize` takes it for one
//! request while the app is closed.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::activity::{ActivityStore, DB_FILE};
use crate::paths;
use crate::protocol::{code, ApiError};
use crate::settings::{self, Settings, SettingsStore};
use crate::timers::TimerStore;

pub const NO_DATA: &str =
    "This laptop doesn't have any rize data. Are you sure you've installed the app before?";

pub struct AppState {
    pub store: Mutex<TimerStore>,
    pub activity: Mutex<ActivityStore>,
    /// Read-only-by-convention connection to the same database, kept off the
    /// writer's mutex so a query never blocks behind (or blocks) the
    /// sampler's tick - see activity.rs's module doc, decision A6.
    pub activity_reader: Mutex<Connection>,
    /// Whether an OpenRize window (main or the Pulse panel) is focused.
    /// Drives the push cadence to the frontend; the underlying sampling rate
    /// is unaffected.
    pub foreground: AtomicBool,
    pub settings: Mutex<SettingsStore>,
}

impl AppState {
    /// Opens every store in `dir`, creating and migrating the database as
    /// needed. Only the app does this. The caller must hold the stores' lock
    /// (`lock_stores`): loading closes any segment left open.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let activity = ActivityStore::load(dir)?;
        Self::with(dir, activity)
    }

    /// Opens the stores the app already made, for `rize` while the app is
    /// closed. Creates nothing and migrates nothing: a missing database means
    /// the app never ran here, and one at another schema version belongs to a
    /// different release than this `rize`. The caller must hold the stores'
    /// lock.
    pub fn open_existing(dir: &Path) -> Result<Self, ApiError> {
        let path = dir.join(DB_FILE);
        if !path.is_file() {
            return Err(ApiError::new(code::NO_DATA, NO_DATA));
        }
        let failed = |error: String| ApiError::new(code::FAILED, error);
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| failed(error.to_string()))?;
        let version: i32 = conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .map_err(|error| failed(error.to_string()))?;
        match version.cmp(&crate::migrations::SCHEMA_VERSION) {
            std::cmp::Ordering::Equal => {}
            std::cmp::Ordering::Less => {
                return Err(ApiError::new(
                    code::INCOMPATIBLE,
                    "Your OpenRize data is from an older version of the app. Update OpenRize and open it once, then try again.",
                ))
            }
            std::cmp::Ordering::Greater => {
                return Err(ApiError::new(
                    code::INCOMPATIBLE,
                    "Your OpenRize data is from a newer version of the app than this rize. Update rize to match the app.",
                ))
            }
        }
        let activity = ActivityStore::from_conn(conn).map_err(failed)?;
        Self::with(dir, activity).map_err(failed)
    }

    fn with(dir: &Path, mut activity: ActivityStore) -> Result<Self, String> {
        let store = TimerStore::load(dir)?;
        let activity_reader = ActivityStore::open_reader(dir)?;
        let settings = SettingsStore::load(&settings::config_dir())?;
        activity.set_tracking_hours(settings.snapshot().tracking_hours.clone());
        activity.set_agents_enabled(settings.snapshot().advanced_workflow_tracking);
        Ok(Self {
            store: Mutex::new(store),
            activity: Mutex::new(activity),
            activity_reader: Mutex::new(activity_reader),
            foreground: AtomicBool::new(true),
            settings: Mutex::new(settings),
        })
    }

    pub fn settings_snapshot(&self) -> Settings {
        self.settings
            .lock()
            .map(|store| store.snapshot())
            .unwrap_or_default()
    }

    /// Recomputes the agent ledger of every calendar day in `[start, end]`.
    /// Does nothing while advanced workflow tracking is off.
    pub fn refresh_agent_days(&self, start_ms: u64, end_ms: u64, now: u64) {
        let settings = self.settings_snapshot();
        if !settings.advanced_workflow_tracking {
            return;
        }
        let auto_accept = settings.auto_accept;
        let Ok(mut store) = self.activity.lock() else {
            return;
        };
        let mut cursor = start_ms;
        // At most two weeks, so a stray wide range cannot stall the store.
        for _ in 0..16 {
            let (day_start, day_end) = crate::agents::ledger::calendar_day(cursor);
            if let Err(error) = crate::agents::ledger::refresh(
                store.conn_mut(),
                day_start,
                day_end,
                now,
                auto_accept,
            ) {
                eprintln!("agent ledger: {error}");
            }
            if day_end > end_ms {
                break;
            }
            cursor = day_end;
        }
    }
}

/// Holds the stores for this process. `wait` blocks until a `rize` edit
/// finishes (the app at launch); otherwise `None` means another process has
/// them (`rize` finding the app open). Only the lock file is created: the
/// caller decides whether the data directory may be.
pub fn lock_stores(data_dir: &Path, wait: bool) -> io::Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(data_dir.join(paths::LOCK_FILE))?;
    if wait {
        file.lock()?;
        return Ok(Some(file));
    }
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// Where OpenRize keeps its files, as Settings and `rize paths` show them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoragePaths {
    pub config_file: String,
    pub data_dir: String,
    pub database_file: String,
}

impl StoragePaths {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            config_file: settings::config_dir()
                .join("settings.json")
                .display()
                .to_string(),
            data_dir: data_dir.display().to_string(),
            database_file: data_dir.join(DB_FILE).display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openrize-state-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn open_existing_never_creates_the_database() {
        let dir = dir("missing");
        let error = AppState::open_existing(&dir).err().unwrap();
        assert_eq!(error.code, code::NO_DATA);
        assert!(!dir.join(DB_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_existing_refuses_another_schema_version() {
        let dir = dir("version");
        Connection::open(dir.join(DB_FILE))
            .unwrap()
            .execute_batch("PRAGMA user_version = 1;")
            .unwrap();
        let error = AppState::open_existing(&dir).err().unwrap();
        assert_eq!(error.code, code::INCOMPATIBLE);
        assert!(error.message.contains("older"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
