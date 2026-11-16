//! The open stores one process works on, and the lock that makes it the only one: the app holds it
//! for its life, `rize` for one request while the app is closed.

use std::fs::{File, TryLockError};
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::accounts;
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
    /// Read-only-by-convention connection to the same database, off the writer's mutex so a query
    /// never blocks the sampler's tick (activity.rs, decision A6).
    pub activity_reader: Mutex<Connection>,
    /// Whether an OpenRize window (main or the Pulse panel) is focused; drives the push cadence to
    /// the frontend, not the sampling rate.
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

    /// Opens the stores the app already made, for `rize` with the app closed. Creates and migrates
    /// nothing: a missing database or another schema version means this `rize` does not match the
    /// app. The caller must hold the stores' lock.
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
}

/// Holds the stores for this process. `wait` blocks until a `rize` edit finishes (the app at
/// launch); otherwise `None` means another process has them. Only the lock file is created.
pub fn lock_stores(data_dir: &Path, wait: bool) -> io::Result<Option<File>> {
    let file = crate::private_fs::open(&data_dir.join(paths::LOCK_FILE), false)?;
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoragePaths {
    pub config_file: String,
    pub data_dir: String,
    pub database_file: String,
    pub account: Option<String>,
}

impl StoragePaths {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            account: accounts::account_for_dir(data_dir).map(|account| account.name),
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

    #[cfg(unix)]
    #[test]
    fn a_fresh_store_is_private_whatever_the_umask() {
        use crate::private_fs::tests::{mode, permissive_umask, temp};

        let _umask = permissive_umask();
        let root = temp("state-modes");
        let dir = root.join(paths::ACCOUNTS_DIR).join("a1");
        let config = root.join("config-home").join("openrize");

        crate::private_fs::create_dir_all(&dir).unwrap();
        let _lock = lock_stores(&dir, false).unwrap().unwrap();
        let activity = ActivityStore::load(&dir).unwrap();
        let timers = TimerStore::load(&dir).unwrap();
        timers.snapshot().unwrap();
        accounts::Registry::default().save(&root).unwrap();
        accounts::Handoff {
            to: None,
            adopt: false,
            show: false,
            window: None,
        }
        .write(&root)
        .unwrap();
        let mut settings = SettingsStore::load(&config).unwrap();
        settings.set(Settings::default()).unwrap();

        for folder in [&root.join(paths::ACCOUNTS_DIR), &dir, &config] {
            assert_eq!(mode(folder), 0o700, "{folder:?}");
        }
        for file in [
            dir.join(DB_FILE),
            dir.join(format!("{DB_FILE}-wal")),
            dir.join(format!("{DB_FILE}-shm")),
            dir.join(paths::LOCK_FILE),
            root.join(paths::ACCOUNTS_FILE),
            root.join(paths::SWITCH_FILE),
            config.join("settings.json"),
        ] {
            assert_eq!(mode(&file), 0o600, "{file:?}");
        }
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .chain(std::fs::read_dir(&config).unwrap())
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        drop((activity, timers));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn opening_an_older_readable_database_makes_it_and_its_log_private() {
        use crate::private_fs::tests::{mode, permissive_umask, temp};
        use std::os::unix::fs::PermissionsExt;

        let _umask = permissive_umask();
        let dir = temp("state-older");
        let db = dir.join(DB_FILE);
        let wal = dir.join(format!("{DB_FILE}-wal"));
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE older (x);")
                .unwrap();
            assert_eq!((mode(&db), mode(&wal)), (0o644, 0o644));
            std::fs::copy(&wal, dir.join("kept-wal")).unwrap();
        }
        // The log a crashed older build left behind, still readable by others.
        std::fs::copy(dir.join("kept-wal"), &wal).unwrap();
        std::fs::set_permissions(&wal, std::fs::Permissions::from_mode(0o644)).unwrap();

        let store = ActivityStore::load(&dir).unwrap();
        assert_eq!(mode(&db), 0o600);
        assert_eq!(mode(&wal), 0o600);
        assert_eq!(mode(&dir.join(format!("{DB_FILE}-shm"))), 0o600);
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
