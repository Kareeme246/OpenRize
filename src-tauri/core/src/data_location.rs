//! Data folder choice: older Windows builds used Roaming AppData, current ones Local. The app calls
//! [`adopt`] at startup to move old contents; `rize` calls [`choose`], the same rule read-only. A
//! marker file in the new folder marks a move under way; "has data" means a db or account list.
use std::collections::hash_map::DefaultHasher;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::hash::Hasher;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::paths::{ACCOUNTS_FILE, CONFIG_DIR, DATABASE_FILE, LOCK_FILE, MODELS_DIR};
use crate::state::lock_stores;

const STAGING_PREFIX: &str = ".migrating-";

/// Holds the old folder's path in `local` while a move is under way; present means it was
/// interrupted, so the account list can already be in `local` while the database is still in
/// `legacy`. Not data. (Not under `STAGING_PREFIX`, which is swept as leftover copies.)
const MARKER_FILE: &str = ".moving-from";

/// The two file system operations a move uses, replaceable so tests can make a
/// rename cross volumes or a copy fail part-way.
struct Io<'a> {
    rename: &'a dyn Fn(&Path, &Path) -> io::Result<()>,
    copy: &'a dyn Fn(&Path, &Path) -> io::Result<u64>,
}

fn real_rename(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

fn real_copy(from: &Path, to: &Path) -> io::Result<u64> {
    fs::copy(from, to)
}

const REAL_IO: Io<'static> = Io {
    rename: &real_rename,
    copy: &real_copy,
};

pub fn has_data(dir: &Path) -> bool {
    dir.join(DATABASE_FILE).is_file() || dir.join(ACCOUNTS_FILE).is_file()
}

fn marker_path(local: &Path) -> PathBuf {
    local.join(MARKER_FILE)
}

/// Whether a move into `local` was started and not finished. The data may be split between the
/// folders, so `rize` refuses to edit; the app finishes the move at its next start.
pub fn move_unfinished(local: &Path) -> bool {
    marker_path(local).is_file()
}

/// The folder holding the data, read-only: `local` once it has data (or neither does), else
/// `legacy`. While a move is unfinished, `legacy` as long as it still has the database.
pub fn choose(legacy: &Path, local: &Path) -> PathBuf {
    if legacy == local {
        return local.to_path_buf();
    }
    if move_unfinished(local) {
        return unfinished_dir(legacy, local);
    }
    if has_data(local) || !has_data(legacy) {
        local.to_path_buf()
    } else {
        legacy.to_path_buf()
    }
}

/// Where the data is when a move did not complete. The database decides,
/// since the account list may have been moved before it.
fn unfinished_dir(legacy: &Path, local: &Path) -> PathBuf {
    if legacy.join(DATABASE_FILE).is_file() || (!has_data(local) && has_data(legacy)) {
        legacy.to_path_buf()
    } else {
        local.to_path_buf()
    }
}

/// The folder to open at startup, moving data from `legacy` into `local` when only `legacy` has
/// any. Nothing is merged; on failure everything is put back and `legacy` serves this launch. Call
/// before any store is open: it takes the old folder's lock itself, waiting out `rize`.
pub fn adopt(legacy: &Path, local: &Path) -> PathBuf {
    adopt_with(legacy, local, &REAL_IO)
}

fn adopt_with(legacy: &Path, local: &Path, io: &Io) -> PathBuf {
    if legacy == local {
        return local.to_path_buf();
    }
    let resuming = move_unfinished(local);
    if !resuming {
        if has_data(local) {
            if has_data(legacy) {
                eprintln!(
                    "data exists in both {} and {}; using the second and leaving the first untouched",
                    legacy.display(),
                    local.display()
                );
            }
            return local.to_path_buf();
        }
        if !has_data(legacy) {
            return local.to_path_buf();
        }
    }
    match migrate(legacy, local, io, resuming) {
        Ok(()) => local.to_path_buf(),
        Err(error) => {
            eprintln!(
                "could not move the data from {} to {}: {error}; using {} for this launch",
                legacy.display(),
                local.display(),
                legacy.display()
            );
            unfinished_dir(legacy, local)
        }
    }
}

struct MoveError {
    message: String,
    clean: bool,
}

impl MoveError {
    fn clean(message: String) -> Self {
        Self {
            message,
            clean: true,
        }
    }
}

fn migrate(legacy: &Path, local: &Path, io: &Io, resuming: bool) -> Result<(), String> {
    fs::create_dir_all(local).map_err(|error| format!("could not create the folder: {error}"))?;
    if resuming && !legacy.is_dir() {
        // Everything arrived; only the marker was left.
        return finish(local);
    }
    let lock = match lock_stores(legacy, true) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Err("the old folder is in use".into()),
        Err(error) => return Err(format!("could not lock the old folder: {error}")),
    };
    remove_stale_staging(local);
    if !resuming {
        if let Err(error) = write_marker(local, legacy) {
            remove_entry(&marker_path(local));
            return Err(format!("could not record the move: {error}"));
        }
    }
    let result = move_entries(legacy, local, io, resuming);
    drop(lock);
    match result {
        Ok(()) => {
            // The lock file was not moved (it is only a lock, and a stray one
            // in the new folder would not mean anything); tidy up what is left.
            let _ = fs::remove_file(legacy.join(LOCK_FILE));
            let _ = fs::remove_dir(legacy);
            finish(local)
        }
        Err(error) => {
            // Back where it started, so the next launch begins afresh; an
            // interrupted or part-moved state keeps the marker.
            if error.clean && !resuming {
                remove_entry(&marker_path(local));
            }
            Err(error.message)
        }
    }
}

/// The last step: with the marker gone the new folder is the whole truth.
fn finish(local: &Path) -> Result<(), String> {
    fs::remove_file(marker_path(local))
        .or_else(|error| match error.kind() {
            io::ErrorKind::NotFound => Ok(()),
            _ => Err(error),
        })
        .map_err(|error| format!("could not clear the move marker: {error}"))
}

fn write_marker(local: &Path, legacy: &Path) -> io::Result<()> {
    let mut file = File::create(marker_path(local))?;
    file.write_all(legacy.to_string_lossy().as_bytes())?;
    file.sync_all()
}

/// Moves last, so an interruption leaves the old folder looking like the one with data.
fn rank(name: &OsStr) -> u8 {
    let name = name.to_string_lossy();
    if name == DATABASE_FILE {
        3
    } else if name.starts_with(&format!("{DATABASE_FILE}-")) {
        2
    } else if name == ACCOUNTS_FILE {
        1
    } else {
        0
    }
}

fn crosses_devices(error: &io::Error) -> bool {
    // ERROR_NOT_SAME_DEVICE on Windows, EXDEV elsewhere.
    const NOT_SAME_DEVICE: i32 = if cfg!(windows) { 17 } else { 18 };
    error.kind() == io::ErrorKind::CrossesDevices || error.raw_os_error() == Some(NOT_SAME_DEVICE)
}

fn move_entries(legacy: &Path, local: &Path, io: &Io, resuming: bool) -> Result<(), MoveError> {
    let mut names: Vec<OsString> = fs::read_dir(legacy)
        .map_err(|error| MoveError::clean(format!("could not list the old folder: {error}")))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        // The lock is only a lock, settings and model files are not data, and the marker is ours.
        .filter(|name| {
            name != LOCK_FILE && name != CONFIG_DIR && name != MODELS_DIR && name != MARKER_FILE
        })
        .collect();
    names.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    // Nothing already in the new folder is replaced: refuse before moving any, except when
    // resuming, where an entry on both sides that verifies identical was already published and the
    // old one goes.
    let mut pending: Vec<OsString> = Vec::new();
    for name in names {
        let theirs = local.join(&name);
        if theirs.symlink_metadata().is_err() {
            pending.push(name);
            continue;
        }
        let ours = legacy.join(&name);
        if !(resuming && trees_equal(&ours, &theirs)) {
            return Err(MoveError {
                message: format!(
                    "{} already exists in the new folder",
                    name.to_string_lossy()
                ),
                clean: !resuming,
            });
        }
        if let Err(error) = remove_entry_checked(&ours) {
            eprintln!(
                "could not remove {} after finding it in the new folder: {error}",
                ours.display()
            );
        }
    }
    let names = pending;
    let mut moved: Vec<&OsString> = Vec::new();
    for name in &names {
        match (io.rename)(&legacy.join(name), &local.join(name)) {
            Ok(()) => moved.push(name),
            Err(error) => {
                let stuck: Vec<String> = moved
                    .iter()
                    .rev()
                    .filter(|name| (io.rename)(&local.join(name), &legacy.join(name)).is_err())
                    .map(|name| name.to_string_lossy().into_owned())
                    .collect();
                // Everything is back where it was: copy instead.
                if crosses_devices(&error) && stuck.is_empty() {
                    return copy_entries(legacy, local, &names, io);
                }
                let mut message = format!("moving {} failed: {error}", name.to_string_lossy());
                if !stuck.is_empty() {
                    message.push_str(&format!(
                        "; could not put back {} (they are in the new folder)",
                        stuck.join(", ")
                    ));
                }
                return Err(MoveError {
                    message,
                    clean: stuck.is_empty(),
                });
            }
        }
    }
    Ok(())
}

/// Copies every entry into a staging folder in `local`, verifies it, renames it into place, then
/// removes the old ones. Holding the old lock keeps the `.db`/`-wal`/`-shm` set consistent; a raw
/// copy, not SQLite's backup API, since the folder holds one database per account.
fn copy_entries(legacy: &Path, local: &Path, names: &[OsString], io: &Io) -> Result<(), MoveError> {
    let staging = local.join(format!("{STAGING_PREFIX}{}", uuid::Uuid::now_v7().simple()));
    fs::create_dir(&staging)
        .map_err(|error| MoveError::clean(format!("could not create a copy folder: {error}")))?;
    let result = copy_and_publish(legacy, local, &staging, names, io);
    // Holds only the unpublished remainder of a failed copy; empty otherwise.
    let _ = fs::remove_dir_all(&staging);
    result
}

fn copy_and_publish(
    legacy: &Path,
    local: &Path,
    staging: &Path,
    names: &[OsString],
    io: &Io,
) -> Result<(), MoveError> {
    for name in names {
        copy_tree(&legacy.join(name), &staging.join(name), io).map_err(MoveError::clean)?;
    }
    let mut published: Vec<&OsString> = Vec::new();
    for name in names {
        if let Err(error) = (io.rename)(&staging.join(name), &local.join(name)) {
            let mut clean = true;
            for done in published.iter().rev() {
                // The originals are intact, so a copy that will not go back
                // to staging is simply deleted.
                if (io.rename)(&local.join(done), &staging.join(done)).is_err()
                    && !remove_entry(&local.join(done))
                {
                    clean = false;
                }
            }
            return Err(MoveError {
                message: format!(
                    "publishing the copy of {} failed: {error}",
                    name.to_string_lossy()
                ),
                clean,
            });
        }
        published.push(name);
    }
    // The new folder is complete and verified: the old entries can go. A
    // failure here costs only tidiness, since the new folder wins from now on.
    for name in names.iter().rev() {
        let path = legacy.join(name);
        if let Err(error) = remove_entry_checked(&path) {
            eprintln!(
                "could not remove {} after copying it: {error}",
                path.display()
            );
        }
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path, io: &Io) -> Result<(), String> {
    let fail = |action: &str, error: &dyn std::fmt::Display| {
        format!("could not {action} {}: {error}", from.display())
    };
    let metadata = fs::symlink_metadata(from).map_err(|e| fail("read", &e))?;
    if metadata.is_dir() {
        fs::create_dir(to).map_err(|e| fail("copy", &e))?;
        for entry in fs::read_dir(from).map_err(|e| fail("list", &e))? {
            let entry = entry.map_err(|e| fail("list", &e))?;
            copy_tree(&entry.path(), &to.join(entry.file_name()), io)?;
        }
        Ok(())
    } else if metadata.is_file() {
        (io.copy)(from, to).map_err(|e| fail("copy", &e))?;
        // Flushed to disk before the original is ever removed.
        OpenOptions::new()
            .write(true)
            .open(to)
            .and_then(|file| file.sync_all())
            .map_err(|e| fail("flush the copy of", &e))?;
        let copied = fs::metadata(to).map_err(|e| fail("verify the copy of", &e))?;
        if copied.len() != metadata.len() {
            return Err(fail("verify the copy of", &"its size differs"));
        }
        match (hash_file(from), hash_file(to)) {
            (Ok(a), Ok(b)) if a == b => Ok(()),
            (Ok(_), Ok(_)) => Err(fail("verify the copy of", &"its contents differ")),
            (Err(e), _) | (_, Err(e)) => Err(fail("verify the copy of", &e)),
        }
    } else {
        Err(fail("copy", &"it is not a plain file or folder"))
    }
}

/// A fingerprint to compare a copy with its source (not a security hash).
fn hash_file(path: &Path) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let mut hasher = DefaultHasher::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hasher.finish());
        }
        hasher.write(&buffer[..read]);
    }
}

fn remove_entry_checked(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Removes `path`; whether it is gone (it may never have been there).
fn remove_entry(path: &Path) -> bool {
    match remove_entry_checked(path) {
        Ok(()) => true,
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

fn trees_equal(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (fs::symlink_metadata(a), fs::symlink_metadata(b)) else {
        return false;
    };
    if ma.is_dir() && mb.is_dir() {
        let names = |dir: &Path| -> Option<Vec<OsString>> {
            let mut names = fs::read_dir(dir)
                .ok()?
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            names.sort();
            Some(names)
        };
        match (names(a), names(b)) {
            (Some(x), Some(y)) if x == y => x.iter().all(|n| trees_equal(&a.join(n), &b.join(n))),
            _ => false,
        }
    } else if ma.is_file() && mb.is_file() {
        ma.len() == mb.len() && matches!((hash_file(a), hash_file(b)), (Ok(x), Ok(y)) if x == y)
    } else {
        false
    }
}

/// A crash mid-copy leaves a staging folder behind: ours, and only a copy.
fn remove_stale_staging(local: &Path) {
    let Ok(entries) = fs::read_dir(local) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            remove_entry(&entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openrize-data-location-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A legacy folder shaped like an install: WAL database, accounts, PDFs.
    fn legacy_install(legacy: &Path) {
        fs::create_dir_all(legacy.join("accounts").join("a1")).unwrap();
        fs::create_dir_all(legacy.join("invoices")).unwrap();
        fs::write(legacy.join("activity.db"), "db").unwrap();
        fs::write(legacy.join("activity.db-wal"), "wal").unwrap();
        fs::write(legacy.join("activity.db-shm"), "shm").unwrap();
        fs::write(legacy.join("accounts.json"), "{}").unwrap();
        fs::write(legacy.join("switch.json"), "{}").unwrap();
        fs::write(legacy.join("app.lock"), "").unwrap();
        fs::write(legacy.join("accounts/a1/activity.db"), "acct").unwrap();
        fs::write(legacy.join("invoices/1.pdf"), "pdf").unwrap();
    }

    /// Real file system, except for the given rename.
    fn with_rename<'a>(rename: &'a dyn Fn(&Path, &Path) -> io::Result<()>) -> Io<'a> {
        Io { rename, ..REAL_IO }
    }

    /// Renames as if the old folder were on another volume: only moves out of
    /// it fail, the way a rename across volumes does.
    fn crossing(legacy: &Path) -> impl Fn(&Path, &Path) -> io::Result<()> + '_ {
        move |from, to| {
            if from.starts_with(legacy) {
                return Err(io::Error::from(io::ErrorKind::CrossesDevices));
            }
            real_rename(from, to)
        }
    }

    fn contents(dir: &Path, file: &str) -> String {
        fs::read_to_string(dir.join(file)).unwrap()
    }

    #[test]
    fn a_fresh_install_uses_the_new_folder_and_creates_nothing() {
        let root = temp_dir("fresh");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        assert_eq!(choose(&legacy, &local), local);
        assert_eq!(adopt(&legacy, &local), local);
        assert!(!legacy.exists() && !local.exists());

        // A legacy folder with no data (only a stray lock) is not data either.
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("app.lock"), "").unwrap();
        assert_eq!(adopt(&legacy, &local), local);
        assert!(legacy.join("app.lock").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn choose_follows_where_the_data_is_without_touching_anything() {
        let root = temp_dir("choose");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        // Local exists (WebView2) but holds no data yet: the old folder.
        fs::create_dir_all(local.join("EBWebView")).unwrap();
        assert_eq!(choose(&legacy, &local), legacy);
        assert!(legacy.join("activity.db").is_file());
        assert!(!local.join("activity.db").exists());

        fs::write(local.join("accounts.json"), "{}").unwrap();
        assert_eq!(choose(&legacy, &local), local);
        assert_eq!(choose(&local, &local), local);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn existing_data_moves_whole_into_the_folder_webview_already_made() {
        let root = temp_dir("migrate");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        fs::create_dir_all(local.join("EBWebView")).unwrap();
        fs::write(local.join("EBWebView/profile"), "p").unwrap();

        assert_eq!(adopt(&legacy, &local), local);

        for (file, body) in [
            ("activity.db", "db"),
            ("activity.db-wal", "wal"),
            ("activity.db-shm", "shm"),
            ("accounts.json", "{}"),
            ("switch.json", "{}"),
            ("accounts/a1/activity.db", "acct"),
            ("invoices/1.pdf", "pdf"),
            ("EBWebView/profile", "p"),
        ] {
            assert_eq!(
                fs::read_to_string(local.join(file)).unwrap(),
                body,
                "{file}"
            );
        }
        assert!(!local.join("app.lock").exists(), "the lock is not data");
        assert!(!legacy.exists(), "an emptied old folder is removed");
        assert_eq!(choose(&legacy, &local), local);

        assert_eq!(adopt(&legacy, &local), local);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn data_already_in_the_new_folder_is_left_alone() {
        let root = temp_dir("done");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        fs::create_dir_all(&local).unwrap();
        fs::write(local.join("activity.db"), "new").unwrap();
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("settings-like.txt"), "x").unwrap();

        assert_eq!(adopt(&legacy, &local), local);
        assert_eq!(
            fs::read_to_string(local.join("activity.db")).unwrap(),
            "new"
        );
        assert!(legacy.join("settings-like.txt").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn data_in_both_keeps_the_new_folder_and_leaves_the_old_untouched() {
        let root = temp_dir("both");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        fs::create_dir_all(&local).unwrap();
        fs::write(local.join("activity.db"), "new").unwrap();

        assert_eq!(adopt(&legacy, &local), local);
        assert_eq!(
            fs::read_to_string(local.join("activity.db")).unwrap(),
            "new"
        );
        assert!(!local.join("accounts.json").exists());
        assert_eq!(
            fs::read_to_string(legacy.join("activity.db")).unwrap(),
            "db"
        );
        assert!(legacy.join("accounts.json").is_file() && legacy.join("app.lock").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_same_folder_on_both_sides_is_never_moved() {
        let root = temp_dir("same");
        let dir = root.join("support");
        legacy_install(&dir);
        assert_eq!(adopt(&dir, &dir), dir);
        assert!(dir.join("activity.db").is_file() && dir.join("app.lock").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_failed_move_puts_everything_back_and_the_next_launch_succeeds() {
        let root = temp_dir("rollback");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);

        // The database is the last entry to move, so everything else has gone.
        let fail_on_database = |from: &Path, to: &Path| {
            if from.file_name() == Some(OsStr::new(DATABASE_FILE)) && to.starts_with(&local) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            real_rename(from, to)
        };
        assert_eq!(
            adopt_with(&legacy, &local, &with_rename(&fail_on_database)),
            legacy
        );
        for file in [
            "activity.db",
            "activity.db-wal",
            "activity.db-shm",
            "accounts.json",
            "switch.json",
            "accounts/a1/activity.db",
            "invoices/1.pdf",
        ] {
            assert!(legacy.join(file).is_file(), "{file} is back");
        }
        assert!(!has_data(&local));
        assert_eq!(choose(&legacy, &local), legacy, "rize agrees");
        assert!(!move_unfinished(&local), "a clean failure leaves no marker");

        assert_eq!(adopt(&legacy, &local), local);
        assert_eq!(fs::read_to_string(local.join("activity.db")).unwrap(), "db");
        assert!(!legacy.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_failed_put_back_never_loses_a_file_and_the_database_decides() {
        let root = temp_dir("split");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);

        let broken = |from: &Path, to: &Path| {
            let name = from.file_name();
            let to_database = to.file_name() == Some(OsStr::new(DATABASE_FILE));
            let back_accounts = name == Some(OsStr::new(ACCOUNTS_FILE)) && to.starts_with(&legacy);
            if to_database || back_accounts {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            real_rename(from, to)
        };
        // accounts.json is stranded in the new folder, but the database is
        // still in the old one, so that is where the app opens.
        assert_eq!(adopt_with(&legacy, &local, &with_rename(&broken)), legacy);
        assert_eq!(
            fs::read_to_string(local.join("accounts.json")).unwrap(),
            "{}"
        );
        assert!(legacy.join("activity.db").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_name_already_in_the_new_folder_blocks_the_move_before_anything_moves() {
        let root = temp_dir("collision");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        fs::create_dir_all(&local).unwrap();
        fs::write(local.join("switch.json"), "theirs").unwrap();

        assert_eq!(adopt(&legacy, &local), legacy);
        assert_eq!(
            fs::read_to_string(local.join("switch.json")).unwrap(),
            "theirs"
        );
        assert!(!local.join("invoices").exists());
        assert!(legacy.join("activity.db").is_file() && legacy.join("switch.json").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_move_interrupted_before_the_database_resumes_on_the_next_launch() {
        let root = temp_dir("resume");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        // What a crash after the first entries leaves behind.
        fs::create_dir_all(&local).unwrap();
        fs::rename(legacy.join("invoices"), local.join("invoices")).unwrap();
        assert_eq!(choose(&legacy, &local), legacy);

        assert_eq!(adopt(&legacy, &local), local);
        assert!(local.join("invoices/1.pdf").is_file());
        assert_eq!(
            fs::read_to_string(local.join("activity.db-wal")).unwrap(),
            "wal"
        );
        assert!(!legacy.exists());
        let _ = fs::remove_dir_all(root);
    }

    fn staging_entries(local: &Path) -> Vec<String> {
        fs::read_dir(local)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(STAGING_PREFIX))
            .collect()
    }

    #[test]
    fn settings_in_the_config_folder_are_not_data_and_are_never_moved() {
        let root = temp_dir("config");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        fs::create_dir_all(local.join(CONFIG_DIR)).unwrap();
        fs::write(local.join("config/settings.json"), "mine").unwrap();
        // Only settings in local: no data there, so the old folder still wins.
        assert!(!has_data(&local));
        legacy_install(&legacy);
        assert_eq!(choose(&legacy, &local), legacy);

        // A config folder in the old one (never written by a build, but it
        // would not be ours to move) stays, and keeps the folder alive.
        fs::create_dir_all(legacy.join(CONFIG_DIR)).unwrap();
        fs::write(legacy.join("config/settings.json"), "theirs").unwrap();

        assert_eq!(adopt(&legacy, &local), local);
        assert_eq!(contents(&local, "activity.db"), "db");
        assert_eq!(contents(&local, "config/settings.json"), "mine");
        assert_eq!(contents(&legacy, "config/settings.json"), "theirs");
        assert!(!legacy.join("activity.db").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_models_folder_is_not_data_and_is_never_moved() {
        let root = temp_dir("models");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        fs::create_dir_all(local.join(MODELS_DIR)).unwrap();
        fs::write(local.join("models/small.gguf"), "mine").unwrap();
        // Models alone are not data, so the old folder still wins.
        assert!(!has_data(&local));
        legacy_install(&legacy);
        assert_eq!(choose(&legacy, &local), legacy);
        fs::create_dir_all(legacy.join(MODELS_DIR)).unwrap();
        fs::write(legacy.join("models/theirs.gguf"), "theirs").unwrap();

        assert_eq!(adopt(&legacy, &local), local);
        assert_eq!(contents(&local, "activity.db"), "db");
        assert_eq!(contents(&local, "models/small.gguf"), "mine");
        assert_eq!(contents(&legacy, "models/theirs.gguf"), "theirs");
        assert!(!local.join("models/theirs.gguf").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn across_volumes_everything_is_copied_verified_and_then_removed() {
        let root = temp_dir("copy");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        fs::create_dir_all(local.join("EBWebView")).unwrap();
        fs::create_dir_all(local.join(CONFIG_DIR)).unwrap();
        fs::write(local.join("config/settings.json"), "mine").unwrap();
        let renames = crossing(&legacy);

        assert_eq!(adopt_with(&legacy, &local, &with_rename(&renames)), local);
        for (file, body) in [
            ("activity.db", "db"),
            ("activity.db-wal", "wal"),
            ("activity.db-shm", "shm"),
            ("accounts.json", "{}"),
            ("switch.json", "{}"),
            ("accounts/a1/activity.db", "acct"),
            ("invoices/1.pdf", "pdf"),
            ("config/settings.json", "mine"),
        ] {
            assert_eq!(contents(&local, file), body, "{file}");
        }
        assert!(local.join("EBWebView").is_dir());
        assert!(!local.join("app.lock").exists());
        assert!(staging_entries(&local).is_empty(), "no copy folder is left");
        assert!(!legacy.exists(), "the verified originals are gone");
        assert_eq!(choose(&legacy, &local), local);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_copy_that_fails_leaves_the_old_folder_whole_and_the_new_one_clean() {
        let root = temp_dir("copy-fails");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        let renames = crossing(&legacy);
        let copy_fails_on_a_pdf = |from: &Path, to: &Path| {
            if from.extension() == Some(OsStr::new("pdf")) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            fs::copy(from, to)
        };
        let io = Io {
            rename: &renames,
            copy: &copy_fails_on_a_pdf,
        };

        assert_eq!(adopt_with(&legacy, &local, &io), legacy);
        assert_eq!(contents(&legacy, "activity.db"), "db");
        assert_eq!(contents(&legacy, "invoices/1.pdf"), "pdf");
        assert!(!has_data(&local) && !local.join("invoices").exists());
        assert!(staging_entries(&local).is_empty());
        assert_eq!(choose(&legacy, &local), legacy, "rize agrees");
        assert!(!move_unfinished(&local), "a clean failure leaves no marker");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_copy_that_differs_from_its_source_is_caught_before_anything_is_removed() {
        let root = temp_dir("copy-corrupt");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        let renames = crossing(&legacy);
        // Same size, different bytes: only the fingerprint can tell.
        let flips_the_database = |from: &Path, to: &Path| {
            if from.file_name() == Some(OsStr::new(DATABASE_FILE))
                && from.parent() == Some(legacy.as_path())
            {
                fs::write(to, "XX")?;
                return Ok(2);
            }
            fs::copy(from, to)
        };
        let io = Io {
            rename: &renames,
            copy: &flips_the_database,
        };

        assert_eq!(adopt_with(&legacy, &local, &io), legacy);
        assert_eq!(contents(&legacy, "activity.db"), "db");
        assert!(legacy.join("activity.db-wal").is_file());
        assert!(!has_data(&local) && staging_entries(&local).is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_failed_publish_after_the_copy_deletes_the_copies_and_keeps_the_originals() {
        let root = temp_dir("copy-publish");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        let crosses = crossing(&legacy);
        // Moving the copied database into place fails, after the rest went.
        let publish_fails = |from: &Path, to: &Path| {
            let from_staging = from
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name.to_string_lossy().starts_with(STAGING_PREFIX));
            if from_staging && from.file_name() == Some(OsStr::new(DATABASE_FILE)) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            crosses(from, to)
        };

        assert_eq!(
            adopt_with(&legacy, &local, &with_rename(&publish_fails)),
            legacy
        );
        assert_eq!(contents(&legacy, "activity.db"), "db");
        assert_eq!(contents(&legacy, "accounts.json"), "{}");
        assert!(!has_data(&local) && !local.join("invoices").exists());
        assert!(staging_entries(&local).is_empty());

        let renames = crossing(&legacy);
        assert_eq!(adopt_with(&legacy, &local, &with_rename(&renames)), local);
        assert_eq!(contents(&local, "activity.db"), "db");
        let _ = fs::remove_dir_all(root);
    }

    /// Runs `adopt_with` and lets it die when `io` panics, as a power cut does:
    /// nothing after the panicking operation happens, no cleanup runs.
    fn crash_during(legacy: &Path, local: &Path, io: &Io) {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            adopt_with(legacy, local, io)
        }));
        assert!(outcome.is_err(), "the move was meant to be cut short");
    }

    fn assert_whole_in(local: &Path, legacy: &Path) {
        for (file, body) in [
            ("activity.db", "db"),
            ("activity.db-wal", "wal"),
            ("activity.db-shm", "shm"),
            ("accounts.json", "{}"),
            ("switch.json", "{}"),
            ("accounts/a1/activity.db", "acct"),
            ("invoices/1.pdf", "pdf"),
        ] {
            assert_eq!(contents(local, file), body, "{file}");
        }
        assert!(!move_unfinished(local), "the marker is cleared");
        assert!(staging_entries(local).is_empty());
        assert!(!legacy.exists(), "nothing is left behind");
        assert_eq!(choose(legacy, local), local);
    }

    #[test]
    fn a_cut_between_the_account_list_and_the_database_resumes_and_ends_whole() {
        let root = temp_dir("crash-rename");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        let cut = |from: &Path, to: &Path| {
            if from.file_name() == Some(OsStr::new(DATABASE_FILE)) && from.starts_with(&legacy) {
                panic!("power cut");
            }
            real_rename(from, to)
        };
        crash_during(&legacy, &local, &with_rename(&cut));

        // The account list is in the new folder, the database is not.
        assert!(move_unfinished(&local));
        assert!(local.join("accounts.json").is_file() && has_data(&local));
        assert!(legacy.join("activity.db").is_file());
        // rize is pointed at the database, and the next launch finishes.
        assert_eq!(choose(&legacy, &local), legacy);
        assert_eq!(adopt(&legacy, &local), local);
        assert_whole_in(&local, &legacy);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_cut_while_publishing_a_copy_resumes_and_ends_whole() {
        let root = temp_dir("crash-copy");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        let crosses = crossing(&legacy);
        let cut = |from: &Path, to: &Path| {
            let from_staging = from
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name.to_string_lossy().starts_with(STAGING_PREFIX));
            if from_staging && from.file_name() == Some(OsStr::new(DATABASE_FILE)) {
                panic!("power cut");
            }
            crosses(from, to)
        };
        crash_during(&legacy, &local, &with_rename(&cut));

        // The account list is published, the originals are all still there.
        assert!(move_unfinished(&local));
        assert!(local.join("accounts.json").is_file() && !local.join("activity.db").exists());
        assert!(legacy.join("activity.db").is_file() && legacy.join("accounts.json").is_file());
        assert_eq!(choose(&legacy, &local), legacy);
        assert_eq!(adopt(&legacy, &local), local);
        assert_whole_in(&local, &legacy);

        // The same cut again, this time resumed across volumes too.
        let (legacy, local) = (root.join("roaming2"), root.join("local2"));
        legacy_install(&legacy);
        let crosses = crossing(&legacy);
        let cut = |from: &Path, to: &Path| {
            let from_staging = from
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name.to_string_lossy().starts_with(STAGING_PREFIX));
            if from_staging && from.file_name() == Some(OsStr::new(DATABASE_FILE)) {
                panic!("power cut");
            }
            crosses(from, to)
        };
        crash_during(&legacy, &local, &with_rename(&cut));
        let renames = crossing(&legacy);
        assert_eq!(adopt_with(&legacy, &local, &with_rename(&renames)), local);
        assert_whole_in(&local, &legacy);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resuming_never_discards_an_old_entry_that_differs_from_the_new_one() {
        let root = temp_dir("resume-differs");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        let cut = |from: &Path, to: &Path| {
            if from.file_name() == Some(OsStr::new(DATABASE_FILE)) && from.starts_with(&legacy) {
                panic!("power cut");
            }
            real_rename(from, to)
        };
        crash_during(&legacy, &local, &with_rename(&cut));
        // Something else put a different file where an old entry would go.
        fs::remove_dir_all(local.join("invoices")).unwrap();
        fs::write(local.join("invoices"), "not the folder").unwrap();
        fs::create_dir_all(legacy.join("invoices")).unwrap();
        fs::write(legacy.join("invoices/1.pdf"), "pdf").unwrap();

        assert_eq!(adopt(&legacy, &local), legacy);
        assert_eq!(contents(&legacy, "invoices/1.pdf"), "pdf");
        assert_eq!(contents(&legacy, "activity.db"), "db");
        assert!(
            move_unfinished(&local),
            "still unfinished, tried again later"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn choose_follows_the_database_while_a_move_is_unfinished() {
        let root = temp_dir("choose-unfinished");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&local).unwrap();
        fs::write(local.join(MARKER_FILE), legacy.to_string_lossy().as_bytes()).unwrap();
        // The marker alone is not data.
        assert!(!has_data(&local));
        assert_eq!(choose(&legacy, &local), local);

        fs::write(local.join("accounts.json"), "{}").unwrap();
        fs::write(legacy.join("activity.db"), "db").unwrap();
        assert_eq!(choose(&legacy, &local), legacy);

        fs::remove_file(legacy.join("activity.db")).unwrap();
        assert_eq!(choose(&legacy, &local), local);

        fs::remove_file(local.join(MARKER_FILE)).unwrap();
        fs::write(legacy.join("activity.db"), "db").unwrap();
        assert_eq!(choose(&legacy, &local), local);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_finished_move_whose_marker_survived_is_cleared_without_touching_data() {
        let root = temp_dir("marker-left");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        fs::create_dir_all(&local).unwrap();
        fs::write(local.join("activity.db"), "db").unwrap();
        fs::write(local.join(MARKER_FILE), "x").unwrap();

        assert_eq!(adopt(&legacy, &local), local);
        assert!(!move_unfinished(&local));
        assert!(!legacy.exists());
        assert_eq!(contents(&local, "activity.db"), "db");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_copy_folder_left_by_a_crash_is_cleared_at_the_next_move() {
        let root = temp_dir("copy-stale");
        let (legacy, local) = (root.join("roaming"), root.join("local"));
        legacy_install(&legacy);
        fs::create_dir_all(local.join(".migrating-old/accounts")).unwrap();
        fs::write(local.join(".migrating-old/accounts/x"), "half").unwrap();

        assert_eq!(adopt(&legacy, &local), local);
        assert!(staging_entries(&local).is_empty());
        assert_eq!(contents(&local, "activity.db"), "db");
        let _ = fs::remove_dir_all(root);
    }
}
