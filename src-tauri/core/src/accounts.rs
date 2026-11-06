//! Local accounts: a name and a data folder (not a security boundary: no password or sync). The
//! data dir itself is "No account"; others live in `accounts/<id>/` beside `accounts.json` and
//! `switch.json`. Switching restarts the app, so this module is only paths and files.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::activity::ActivityStore;
use crate::paths::{ACCOUNTS_DIR, ACCOUNTS_FILE, DATABASE_FILE, SWITCH_FILE};
use crate::state::lock_stores;
use crate::timers::{now_epoch_ms, TimerStore};

pub const REGISTRY_VERSION: u32 = 1;

pub const MAX_NAME_CHARS: usize = 40;

const RECOVERED_NAME: &str = "Recovered account";

const ML_DIR: &str = "ml";

/// Serializes registry read-modify-write within one process so concurrent edits don't lose changes.
static REGISTRY_EDIT: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// A uuid v7 without dashes; it names the folder, so renames never touch the disk.
    pub id: String,
    pub name: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    pub version: u32,
    pub current: Option<String>,
    pub accounts: Vec<Account>,
    /// Written by a newer release: shown as it is and never rewritten.
    #[serde(skip)]
    newer: bool,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            current: None,
            accounts: Vec::new(),
            newer: false,
        }
    }
}

enum Read {
    Missing,
    Found(Registry),
    Corrupt,
}

impl Registry {
    /// Reads without touching the disk: a missing, unreadable or too-new file is No account.
    pub fn peek(root: &Path) -> Self {
        match read_file(root) {
            Read::Found(registry) => registry,
            Read::Missing | Read::Corrupt => Self::default(),
        }
    }

    /// Like `peek`, but repairs what an edit or launch must not build on: an
    /// unreadable file is moved aside as `accounts.corrupt-<ms>.json`, and the
    /// account folders found on disk are listed again.
    pub fn load(root: &Path) -> Self {
        match read_file(root) {
            Read::Found(registry) => registry,
            Read::Missing => {
                let recovered = recover(root);
                if recovered.is_empty() {
                    return Self::default();
                }
                Self::from_recovered(root, recovered)
            }
            Read::Corrupt => {
                let path = root.join(ACCOUNTS_FILE);
                let quarantine = root.join(format!("accounts.corrupt-{}.json", now_epoch_ms()));
                if fs::rename(&path, &quarantine).is_ok() {
                    eprintln!(
                        "accounts.json was unreadable; moved to {}",
                        quarantine.display()
                    );
                }
                Self::from_recovered(root, recover(root))
            }
        }
    }

    fn from_recovered(root: &Path, accounts: Vec<Account>) -> Self {
        let registry = Self {
            accounts,
            ..Self::default()
        };
        if let Err(error) = registry.save(root) {
            eprintln!("could not save the recovered accounts: {error}");
        }
        registry
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        if self.newer {
            return Err(
                "These accounts were made by a newer version of OpenRize. Update OpenRize to change them."
                    .to_string(),
            );
        }
        fs::create_dir_all(root).map_err(|error| format!("could not create data dir: {error}"))?;
        let mut document = self.clone();
        document.version = REGISTRY_VERSION;
        let json = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
        let path = root.join(ACCOUNTS_FILE);
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, json).map_err(|error| format!("could not write accounts: {error}"))?;
        fs::rename(&temp, &path).map_err(|error| format!("could not replace accounts: {error}"))
    }

    pub fn is_read_only(&self) -> bool {
        self.newer
    }

    pub fn get(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|account| account.id == id)
    }

    fn add(&mut self, name: &str) -> Result<Account, String> {
        let name = validate_name(name, &self.accounts, None)?;
        let account = Account {
            id: uuid::Uuid::now_v7().simple().to_string(),
            name,
            created_at: now_epoch_ms(),
        };
        self.accounts.push(account.clone());
        Ok(account)
    }

    fn rename(&mut self, id: &str, name: &str) -> Result<Account, String> {
        let name = validate_name(name, &self.accounts, Some(id))?;
        let account = self
            .accounts
            .iter_mut()
            .find(|account| account.id == id)
            .ok_or_else(|| unknown(id))?;
        account.name = name;
        Ok(account.clone())
    }

    fn remove(&mut self, id: &str) -> Result<Account, String> {
        let index = self
            .accounts
            .iter()
            .position(|account| account.id == id)
            .ok_or_else(|| unknown(id))?;
        if self.current.as_deref() == Some(id) {
            self.current = None;
        }
        Ok(self.accounts.remove(index))
    }

    fn set_current(&mut self, id: Option<&str>) -> Result<(), String> {
        if let Some(id) = id {
            if self.get(id).is_none() {
                return Err(unknown(id));
            }
        }
        self.current = id.map(str::to_string);
        Ok(())
    }
}

fn unknown(id: &str) -> String {
    format!("There is no account {id}.")
}

pub fn is_valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn read_file(root: &Path) -> Read {
    let Ok(bytes) = fs::read(root.join(ACCOUNTS_FILE)) else {
        return if root.join(ACCOUNTS_FILE).exists() {
            Read::Corrupt
        } else {
            Read::Missing
        };
    };
    let Ok(mut registry) = serde_json::from_slice::<Registry>(&bytes) else {
        // A newer release may have changed the shape; its version still says so.
        let newer = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| value.get("version")?.as_u64())
            .is_some_and(|version| version > u64::from(REGISTRY_VERSION));
        return if newer {
            Read::Found(Registry {
                newer: true,
                ..Registry::default()
            })
        } else {
            Read::Corrupt
        };
    };
    // The file is user-editable, and an id becomes a folder name.
    registry.accounts.retain(|account| is_valid_id(&account.id));
    if registry
        .current
        .as_deref()
        .is_some_and(|id| registry.get(id).is_none())
    {
        registry.current = None;
    }
    if registry.version > REGISTRY_VERSION {
        registry.newer = true;
        registry.current = None;
    }
    Read::Found(registry)
}

fn recover(root: &Path) -> Vec<Account> {
    let Ok(entries) = fs::read_dir(root.join(ACCOUNTS_DIR)) else {
        return Vec::new();
    };
    let mut folders: Vec<(String, u64)> = entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().into_string().ok()?;
            if !is_valid_id(&id) || !entry.path().join(DATABASE_FILE).is_file() {
                return None;
            }
            let created = entry
                .metadata()
                .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()))
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or_else(now_epoch_ms, |since| since.as_millis() as u64);
            Some((id, created))
        })
        .collect();
    folders.sort();
    let mut accounts: Vec<Account> = Vec::new();
    for (id, created_at) in folders {
        let name = if accounts.is_empty() {
            RECOVERED_NAME.to_string()
        } else {
            format!("{RECOVERED_NAME} {}", accounts.len() + 1)
        };
        accounts.push(Account {
            id,
            name,
            created_at,
        });
    }
    accounts
}

pub fn validate_name(
    name: &str,
    accounts: &[Account],
    except: Option<&str>,
) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the account a name.".to_string());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "An account name can be at most {MAX_NAME_CHARS} characters."
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("An account name cannot contain control characters.".to_string());
    }
    let lowered = name.to_lowercase();
    if accounts.iter().any(|account| {
        except != Some(account.id.as_str()) && account.name.to_lowercase() == lowered
    }) {
        return Err(format!("There is already an account called {name}."));
    }
    Ok(name.to_string())
}

pub fn accounts_dir(root: &Path) -> PathBuf {
    root.join(ACCOUNTS_DIR)
}

/// The folder an account keeps its files in, or `root` for No account (also for an id
/// `Registry::add` could not have made).
pub fn data_dir(root: &Path, current: Option<&str>) -> PathBuf {
    match current {
        Some(id) if is_valid_id(id) => accounts_dir(root).join(id),
        _ => root.to_path_buf(),
    }
}

pub fn account_for_dir(data_dir: &Path) -> Option<Account> {
    let id = data_dir.file_name()?.to_str()?;
    let accounts = data_dir.parent()?;
    if accounts.file_name()? != ACCOUNTS_DIR {
        return None;
    }
    Registry::peek(accounts.parent()?).get(id).cloned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub dir: PathBuf,
    pub account: Option<Account>,
    /// The registry named an account whose data is gone, so No account opened instead.
    pub missing: Option<Account>,
}

pub fn resolve(root: &Path) -> Resolved {
    resolve_in(root, &Registry::peek(root))
}

fn resolve_in(root: &Path, registry: &Registry) -> Resolved {
    let none = |missing| Resolved {
        dir: root.to_path_buf(),
        account: None,
        missing,
    };
    let Some(account) = registry.current.as_deref().and_then(|id| registry.get(id)) else {
        return none(None);
    };
    if is_healthy(root, &account.id) {
        Resolved {
            dir: data_dir(root, Some(&account.id)),
            account: Some(account.clone()),
            missing: None,
        }
    } else {
        none(Some(account.clone()))
    }
}

pub fn is_healthy(root: &Path, id: &str) -> bool {
    is_valid_id(id) && data_dir(root, Some(id)).join(DATABASE_FILE).is_file()
}

fn edit<T>(
    root: &Path,
    change: impl FnOnce(&mut Registry) -> Result<T, String>,
) -> Result<T, String> {
    let _editing = REGISTRY_EDIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut registry = Registry::load(root);
    let result = change(&mut registry)?;
    registry.save(root)?;
    Ok(result)
}

pub fn create(root: &Path, name: &str) -> Result<Account, String> {
    edit(root, |registry| {
        let account = registry.add(name)?;
        let dir = data_dir(root, Some(&account.id));
        ActivityStore::load(&dir)?;
        TimerStore::load(&dir)?;
        Ok(account)
    })
}

pub fn register(root: &Path, name: &str) -> Result<Account, String> {
    edit(root, |registry| registry.add(name))
}

pub fn rename(root: &Path, id: &str, name: &str) -> Result<Account, String> {
    edit(root, |registry| registry.rename(id, name))
}

pub fn remove_entry(root: &Path, id: &str) -> Result<Account, String> {
    edit(root, |registry| registry.remove(id))
}

pub fn set_current(root: &Path, id: Option<&str>) -> Result<(), String> {
    edit(root, |registry| registry.set_current(id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handoff {
    pub to: Option<String>,
    pub adopt: bool,
    /// Open the window even if this process was started with `--background`:
    /// the restart repeats the original arguments.
    pub show: bool,
    pub window: Option<WindowBounds>,
}

impl Handoff {
    pub fn write(&self, root: &Path) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        let path = root.join(SWITCH_FILE);
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, json).map_err(|error| format!("could not write the handoff: {error}"))?;
        fs::rename(&temp, &path).map_err(|error| format!("could not replace the handoff: {error}"))
    }

    /// Reads the handoff without removing it. An unreadable file is deleted
    /// and ignored: startup then follows the registry.
    fn read(root: &Path) -> Option<Self> {
        let path = root.join(SWITCH_FILE);
        let bytes = fs::read(&path).ok()?;
        match serde_json::from_slice(&bytes) {
            Ok(handoff) => Some(handoff),
            Err(_) => {
                let _ = fs::remove_file(&path);
                None
            }
        }
    }

    fn clear(root: &Path) {
        let _ = fs::remove_file(root.join(SWITCH_FILE));
    }
}

#[derive(Debug)]
pub struct Launch {
    pub resolved: Resolved,
    pub handoff: Option<Handoff>,
    pub adopted: Option<Adopted>,
}

/// Runs first at every launch, before any store opens: repairs the registry, finishes a requested
/// move, and says which folder to open. A move's handoff stays until `adopt` finishes, so a crash
/// resumes it.
pub fn launch(root: &Path) -> Launch {
    let registry = Registry::load(root);
    let handoff = Handoff::read(root);
    let mut adopted = None;
    if let Some(handoff) = &handoff {
        if !handoff.adopt {
            Handoff::clear(root);
        } else {
            match handoff
                .to
                .as_deref()
                .filter(|id| registry.get(id).is_some())
            {
                Some(id) => match adopt(root, id) {
                    Ok(outcome) => adopted = Some(outcome),
                    Err(error) => eprintln!("could not move the data into the account: {error}"),
                },
                None => Handoff::clear(root),
            }
        }
    }
    let registry = if adopted.is_some() {
        Registry::peek(root)
    } else {
        registry
    };
    let resolved = resolve_in(root, &registry);
    if resolved.missing.is_some() {
        if let Err(error) = set_current(root, None) {
            eprintln!("could not forget the missing account: {error}");
        }
    }
    Launch {
        resolved,
        handoff,
        adopted,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adopted {
    Moved,
    Nothing,
    Kept,
}

/// Moves No account's database and `ml/` into account `id` to open next. Renames only, so safe to
/// re-run after a crash (`switch.json` stays until the last step); never splits a database from its
/// WAL. The old process must have exited and `id` must be registered.
pub fn adopt(root: &Path, id: &str) -> Result<Adopted, String> {
    if !is_valid_id(id) || Registry::peek(root).get(id).is_none() {
        return Err(unknown(id));
    }
    let target = data_dir(root, Some(id));
    fs::create_dir_all(&target)
        .map_err(|error| format!("could not create the account: {error}"))?;
    // No account's lock, then the account's: the data is owned by neither
    // process while it moves.
    let _source_lock = lock(root)?;
    let _target_lock = lock(&target)?;

    let outcome = move_data(root, &target)?;
    set_current(root, Some(id))?;
    Handoff::clear(root);
    Ok(outcome)
}

fn lock(dir: &Path) -> Result<File, String> {
    lock_stores(dir, true)
        .map_err(|error| format!("could not lock {}: {error}", dir.display()))?
        .ok_or_else(|| format!("could not lock {}", dir.display()))
}

fn move_data(root: &Path, target: &Path) -> Result<Adopted, String> {
    let fail = |what: &str, error: io::Error| format!("could not {what}: {error}");
    let source_db = root.join(DATABASE_FILE);
    let target_db = target.join(DATABASE_FILE);
    let mut moved = false;

    if source_db.is_file() {
        checkpoint(&source_db)?;
        let wal = wal_path(&source_db);
        let shm = sibling(&source_db, "-shm");
        if fs::metadata(&wal).is_ok_and(|metadata| metadata.len() > 0) {
            return Err("the database could not be closed cleanly".to_string());
        }
        if target_db.is_file() {
            if holds_data(&target_db) {
                return Ok(Adopted::Kept);
            }
            fs::remove_file(&target_db).map_err(|e| fail("replace the empty database", e))?;
        }
        for stale in [wal_path(&target_db), sibling(&target_db, "-shm")] {
            let _ = fs::remove_file(stale);
        }
        let _ = fs::remove_file(&wal);
        let _ = fs::remove_file(&shm);
        rename_when_free(&source_db, &target_db).map_err(|e| fail("move the database", e))?;
        moved = true;
    }

    let source_ml = root.join(ML_DIR);
    let target_ml = target.join(ML_DIR);
    if source_ml.is_dir() {
        if target_ml.is_dir() {
            let _ = fs::remove_dir(&target_ml);
        }
        if !target_ml.exists() {
            rename_when_free(&source_ml, &target_ml).map_err(|e| fail("move the models", e))?;
            moved = true;
        }
    }
    Ok(if moved {
        Adopted::Moved
    } else {
        Adopted::Nothing
    })
}

const RENAME_ATTEMPTS: u32 = 20;
const RENAME_PAUSE: std::time::Duration = std::time::Duration::from_millis(100);

/// `fs::rename`, retried for a couple of seconds when Windows refuses because something has the
/// file open (a dying previous process or a scanner); other platforms never retry. A failure leaves
/// everything in place.
fn rename_when_free(from: &Path, to: &Path) -> io::Result<()> {
    rename_retrying(from, to, RENAME_ATTEMPTS, RENAME_PAUSE)
}

fn rename_retrying(
    from: &Path,
    to: &Path,
    attempts: u32,
    pause: std::time::Duration,
) -> io::Result<()> {
    // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION.
    let in_use =
        |error: &io::Error| cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33));
    let mut tries = 1;
    loop {
        match fs::rename(from, to) {
            Err(error) if in_use(&error) && tries < attempts => {
                tries += 1;
                std::thread::sleep(pause);
            }
            result => return result,
        }
    }
}

fn sibling(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn wal_path(db: &Path) -> PathBuf {
    sibling(db, "-wal")
}

fn checkpoint(db: &Path) -> Result<(), String> {
    let conn = Connection::open(db).map_err(|error| error.to_string())?;
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if busy != 0 {
        return Err("the database is still in use".to_string());
    }
    Ok(())
}

fn holds_data(path: &Path) -> bool {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map(|conn| !summarize(&conn).is_empty())
        .unwrap_or(true)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSummary {
    pub entries: u64,
    pub projects: u64,
    pub clients: u64,
    pub invoices: u64,
    pub stopwatches: u64,
    pub segments: u64,
    pub since: Option<u64>,
}

impl DataSummary {
    pub fn is_empty(&self) -> bool {
        self.entries
            + self.projects
            + self.clients
            + self.invoices
            + self.stopwatches
            + self.segments
            == 0
    }
}

pub fn summarize(conn: &Connection) -> DataSummary {
    let count = |sql: &str| -> u64 {
        conn.query_row(sql, [], |row| row.get::<_, i64>(0))
            .map_or(0, |count| count.max(0) as u64)
    };
    let earliest = |sql: &str| -> Option<u64> {
        conn.query_row(sql, [], |row| row.get::<_, Option<i64>>(0))
            .ok()
            .flatten()
            .map(|at| at.max(0) as u64)
    };
    let since = [
        earliest("SELECT MIN(started_at) FROM time_entries WHERE deleted_at IS NULL"),
        earliest("SELECT MIN(started_at) FROM segments"),
    ]
    .into_iter()
    .flatten()
    .min();
    DataSummary {
        entries: count("SELECT COUNT(*) FROM time_entries WHERE deleted_at IS NULL"),
        projects: count("SELECT COUNT(*) FROM projects WHERE deleted_at IS NULL"),
        clients: count("SELECT COUNT(*) FROM clients WHERE deleted_at IS NULL"),
        invoices: count("SELECT COUNT(*) FROM invoices"),
        stopwatches: count("SELECT COUNT(*) FROM timers"),
        segments: count("SELECT COUNT(*) FROM segments"),
        since,
    }
}

pub fn folder_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(metadata) if metadata.is_dir() => folder_size(&entry.path()),
            Ok(metadata) => metadata.len(),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openrize-accounts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn with_data(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        ActivityStore::load(dir).unwrap();
        Connection::open(dir.join(DATABASE_FILE))
            .unwrap()
            .execute(
                "INSERT INTO clients (id, name, created_at, updated_at) VALUES ('c1', 'Acme', 1, 1)",
                [],
            )
            .unwrap();
    }

    fn clients(db: &Path) -> i64 {
        Connection::open(db)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM clients", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn a_missing_file_is_no_account_and_creates_nothing() {
        let root = root("missing");
        assert_eq!(Registry::load(&root), Registry::default());
        assert_eq!(resolve(&root).dir, root);
        assert!(!root.join(ACCOUNTS_FILE).exists());
        assert!(!root.join(ACCOUNTS_DIR).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_corrupt_file_is_quarantined_and_the_folders_are_recovered() {
        let root = root("corrupt");
        let account = create(&root, "Kareem").unwrap();
        fs::write(root.join(ACCOUNTS_FILE), b"{ not json").unwrap();

        let registry = Registry::load(&root);
        assert_eq!(registry.accounts.len(), 1);
        assert_eq!(registry.accounts[0].id, account.id);
        assert_eq!(registry.accounts[0].name, RECOVERED_NAME);
        assert_eq!(registry.current, None);
        let quarantined = fs::read_dir(&root).unwrap().flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("accounts.corrupt-")
        });
        assert!(quarantined);
        assert_eq!(Registry::peek(&root).accounts.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_newer_registry_opens_no_account_and_is_never_rewritten() {
        let root = root("newer");
        let text = r#"{"version": 9, "current": "0199f2a4c1d87e3b9a0f6d2e7c1b4a55", "accounts": [], "extra": true}"#;
        fs::write(root.join(ACCOUNTS_FILE), text).unwrap();

        let registry = Registry::load(&root);
        assert!(registry.is_read_only());
        assert_eq!(registry.current, None);
        assert_eq!(resolve(&root).dir, root);
        assert!(registry.save(&root).is_err());
        assert!(create(&root, "Kareem").is_err());
        assert_eq!(fs::read_to_string(root.join(ACCOUNTS_FILE)).unwrap(), text);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn names_are_trimmed_bounded_and_unique_ignoring_case() {
        let existing = [Account {
            id: "a".repeat(32),
            name: "Kareem".to_string(),
            created_at: 1,
        }];
        assert_eq!(validate_name("  Side  ", &existing, None).unwrap(), "Side");
        assert!(validate_name("   ", &existing, None).is_err());
        assert!(validate_name(&"x".repeat(41), &existing, None).is_err());
        assert!(validate_name(&"x".repeat(40), &existing, None).is_ok());
        assert!(validate_name("bad\nname", &existing, None).is_err());
        let error = validate_name("kAREEM", &existing, None).unwrap_err();
        assert_eq!(error, "There is already an account called kAREEM.");
        assert!(validate_name("kareem", &existing, Some(&"a".repeat(32))).is_ok());
    }

    #[test]
    fn ids_that_could_leave_the_accounts_folder_are_never_followed() {
        let root = root("ids");
        assert!(is_valid_id("0199f2a4c1d87e3b9a0f6d2e7c1b4a55"));
        assert!(!is_valid_id("../etc"));
        assert!(!is_valid_id("0199F2A4C1D87E3B9A0F6D2E7C1B4A55"));
        assert_eq!(data_dir(&root, Some("../etc")), root);
        let text = r#"{"version":1,"current":"../x","accounts":[{"id":"../x","name":"Bad","createdAt":1}]}"#;
        fs::write(root.join(ACCOUNTS_FILE), text).unwrap();
        assert!(Registry::peek(&root).accounts.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn create_makes_the_database_and_the_file_holds_only_id_name_and_time() {
        let root = root("create");
        let account = create(&root, "Kareem").unwrap();
        assert!(is_healthy(&root, &account.id));
        assert!(data_dir(&root, Some(&account.id))
            .join(DATABASE_FILE)
            .is_file());
        assert_eq!(
            create(&root, "kareem").unwrap_err(),
            "There is already an account called kareem."
        );

        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(ACCOUNTS_FILE)).unwrap()).unwrap();
        let mut keys: Vec<&String> = document.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["accounts", "current", "version"]);
        let mut keys: Vec<&String> = document["accounts"][0]
            .as_object()
            .unwrap()
            .keys()
            .collect();
        keys.sort();
        assert_eq!(keys, ["createdAt", "id", "name"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_falls_back_to_no_account_when_the_folder_is_gone() {
        let root = root("resolve");
        let account = create(&root, "Kareem").unwrap();
        set_current(&root, Some(&account.id)).unwrap();
        let open = resolve(&root);
        assert_eq!(open.account.as_ref(), Some(&account));
        assert_eq!(open.dir, data_dir(&root, Some(&account.id)));

        fs::remove_dir_all(&open.dir).unwrap();
        let gone = resolve(&root);
        assert_eq!(gone.dir, root);
        assert_eq!(gone.account, None);
        assert_eq!(gone.missing.as_ref(), Some(&account));
        assert!(!open.dir.exists());

        let launched = launch(&root);
        assert_eq!(launched.resolved.missing.as_ref(), Some(&account));
        assert_eq!(Registry::peek(&root).current, None);
        assert_eq!(Registry::peek(&root).accounts.len(), 1);
        assert_eq!(launch(&root).resolved.missing, None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn adopt_moves_the_database_and_the_models() {
        let root = root("adopt");
        with_data(&root);
        fs::create_dir_all(root.join(ML_DIR)).unwrap();
        fs::write(root.join(ML_DIR).join("model.bin"), b"weights").unwrap();
        let account = register(&root, "Kareem").unwrap();

        assert_eq!(adopt(&root, &account.id).unwrap(), Adopted::Moved);
        let dir = data_dir(&root, Some(&account.id));
        assert_eq!(clients(&dir.join(DATABASE_FILE)), 1);
        assert_eq!(
            fs::read(dir.join(ML_DIR).join("model.bin")).unwrap(),
            b"weights"
        );
        assert!(!root.join(DATABASE_FILE).exists());
        assert!(!root.join(ML_DIR).exists());
        assert_eq!(
            Registry::peek(&root).current.as_deref(),
            Some(account.id.as_str())
        );
        assert_eq!(resolve(&root).dir, dir);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn adopt_resumes_after_a_crash_between_any_two_steps() {
        for crash_after in ["checkpoint", "database", "models"] {
            let root = root(&format!("resume-{crash_after}"));
            with_data(&root);
            fs::create_dir_all(root.join(ML_DIR)).unwrap();
            fs::write(root.join(ML_DIR).join("model.bin"), b"weights").unwrap();
            let account = register(&root, "Kareem").unwrap();
            let dir = data_dir(&root, Some(&account.id));
            fs::create_dir_all(&dir).unwrap();
            Handoff {
                to: Some(account.id.clone()),
                adopt: true,
                show: true,
                window: None,
            }
            .write(&root)
            .unwrap();

            checkpoint(&root.join(DATABASE_FILE)).unwrap();
            if crash_after != "checkpoint" {
                fs::rename(root.join(DATABASE_FILE), dir.join(DATABASE_FILE)).unwrap();
            }
            if crash_after == "models" {
                fs::rename(root.join(ML_DIR), dir.join(ML_DIR)).unwrap();
            }

            let launched = launch(&root);
            assert_eq!(launched.resolved.dir, dir, "{crash_after}");
            assert_eq!(clients(&dir.join(DATABASE_FILE)), 1, "{crash_after}");
            assert!(
                dir.join(ML_DIR).join("model.bin").is_file(),
                "{crash_after}"
            );
            assert!(!root.join(DATABASE_FILE).exists(), "{crash_after}");
            assert!(!root.join(SWITCH_FILE).exists(), "{crash_after}");

            assert_eq!(adopt(&root, &account.id).unwrap(), Adopted::Nothing);
            assert_eq!(clients(&dir.join(DATABASE_FILE)), 1, "{crash_after}");
            let _ = fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn adopt_refuses_to_clobber_an_account_that_has_data() {
        let root = root("clobber");
        with_data(&root);
        let account = create(&root, "Kareem").unwrap();
        let dir = data_dir(&root, Some(&account.id));
        with_data(&dir);

        assert_eq!(adopt(&root, &account.id).unwrap(), Adopted::Kept);
        assert_eq!(clients(&root.join(DATABASE_FILE)), 1);
        assert_eq!(clients(&dir.join(DATABASE_FILE)), 1);
        assert_eq!(
            Registry::peek(&root).current.as_deref(),
            Some(account.id.as_str())
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn adopt_replaces_the_empty_database_account_creation_made() {
        let root = root("replace-empty");
        with_data(&root);
        let account = create(&root, "Kareem").unwrap();
        assert_eq!(adopt(&root, &account.id).unwrap(), Adopted::Moved);
        let dir = data_dir(&root, Some(&account.id));
        assert_eq!(clients(&dir.join(DATABASE_FILE)), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn adopt_folds_the_write_ahead_log_into_the_database_before_moving() {
        let root = root("wal");
        let live = root.join("live");
        with_data(&live);
        // Keep a writer open with auto-checkpointing off so its rows are only in the log, then copy
        // the files while it is open: that is what a crashed process leaves behind, and Windows
        // cannot rename an open database.
        let writer = Connection::open(live.join(DATABASE_FILE)).unwrap();
        writer
            .execute_batch(
                "PRAGMA wal_autocheckpoint = 0;
                 INSERT INTO clients (id, name, created_at, updated_at) VALUES ('c2', 'Beta', 1, 1);",
            )
            .unwrap();
        let db = root.join(DATABASE_FILE);
        fs::copy(live.join(DATABASE_FILE), &db).unwrap();
        fs::copy(wal_path(&live.join(DATABASE_FILE)), wal_path(&db)).unwrap();
        drop(writer);
        assert!(fs::metadata(wal_path(&db)).unwrap().len() > 0);
        let account = register(&root, "Kareem").unwrap();

        assert_eq!(adopt(&root, &account.id).unwrap(), Adopted::Moved);
        let dir = data_dir(&root, Some(&account.id));
        assert_eq!(clients(&dir.join(DATABASE_FILE)), 2);
        assert!(!wal_path(&db).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_another_program_holds_is_waited_for_and_then_reported() {
        use std::time::Duration;
        let root = root("busy");
        let from = root.join("from");
        let to = root.join("to");
        fs::write(&from, b"data").unwrap();

        // Windows refuses to rename a file opened without delete sharing.
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let hold = || {
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1) // FILE_SHARE_READ only
                    .open(&from)
                    .unwrap()
            };
            let held = hold();
            let error = rename_retrying(&from, &to, 3, Duration::from_millis(5)).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(32));
            assert!(from.is_file() && !to.exists());

            let releaser = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                drop(held);
            });
            rename_retrying(&from, &to, 50, Duration::from_millis(20)).unwrap();
            releaser.join().unwrap();
        }
        #[cfg(not(windows))]
        rename_retrying(&from, &to, 3, Duration::from_millis(5)).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"data");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unusable_handoff_is_ignored_and_a_plain_one_is_consumed() {
        let root = root("handoff");
        fs::write(root.join(SWITCH_FILE), b"nonsense").unwrap();
        assert!(launch(&root).handoff.is_none());
        assert!(!root.join(SWITCH_FILE).exists());

        let handoff = Handoff {
            to: None,
            adopt: false,
            show: true,
            window: Some(WindowBounds {
                x: 120,
                y: 80,
                width: 1160,
                height: 780,
            }),
        };
        handoff.write(&root).unwrap();
        assert_eq!(launch(&root).handoff, Some(handoff));
        assert!(!root.join(SWITCH_FILE).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_and_remove_edit_only_the_registry() {
        let root = root("edit");
        let account = create(&root, "Kareem").unwrap();
        let other = create(&root, "Side").unwrap();
        assert!(rename(&root, &account.id, "side").is_err());
        assert_eq!(rename(&root, &account.id, "Work").unwrap().name, "Work");
        set_current(&root, Some(&account.id)).unwrap();

        remove_entry(&root, &account.id).unwrap();
        let registry = Registry::peek(&root);
        assert_eq!(registry.current, None);
        assert_eq!(registry.accounts, [other]);
        assert!(data_dir(&root, Some(&account.id))
            .join(DATABASE_FILE)
            .is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_summary_counts_what_the_user_made_and_not_what_was_deleted() {
        let root = root("summary");
        ActivityStore::load(&root).unwrap();
        let conn = Connection::open(root.join(DATABASE_FILE)).unwrap();
        assert!(summarize(&conn).is_empty());
        conn.execute_batch(
            "INSERT INTO clients (id, name, created_at, updated_at) VALUES ('c1', 'Acme', 1, 1);
             INSERT INTO clients (id, name, created_at, updated_at, deleted_at) VALUES ('c2', 'Gone', 1, 1, 5);
             INSERT INTO time_entries (id, started_at, ended_at, description, created_at, updated_at)
               VALUES ('e1', 5000, 6000, 'Work', 1, 1);",
        )
        .unwrap();
        let summary = summarize(&conn);
        assert_eq!((summary.clients, summary.entries), (1, 1));
        assert_eq!(summary.since, Some(5000));
        assert!(!summary.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_storage_paths_name_the_open_account() {
        let root = root("paths");
        let account = create(&root, "Kareem").unwrap();
        let dir = data_dir(&root, Some(&account.id));
        assert_eq!(account_for_dir(&dir), Some(account));
        assert_eq!(account_for_dir(&root), None);
        let _ = fs::remove_dir_all(&root);
    }
}
