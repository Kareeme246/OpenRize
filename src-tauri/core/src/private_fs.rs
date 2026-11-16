//! Owner-only storage: the data and settings hold window titles, addresses and client details, so
//! their folders are 0700 and their files 0600 instead of whatever the umask leaves. Nothing here
//! changes anything on Windows, where the profile's ACL already keeps other accounts out.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

/// `fs::create_dir_all`, with every folder it makes private. A folder that already exists keeps
/// its mode (`repair` is what tightens an older install).
pub fn create_dir_all(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(dir)
}

/// Opens `path` for reading and writing, creating it private. A file that is already there is
/// made private too: a create mode only applies to a new file.
pub fn open(path: &Path, truncate: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .create(true)
        .read(true)
        .write(true)
        .truncate(truncate);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options.open(path)?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

/// `fs::write`, into a private file.
pub fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    open(path, true)?.write_all(contents)
}

/// Makes the database file private before SQLite opens it: SQLite gives the `-wal` and `-shm`
/// files it creates the mode of the database, so the database has to be private first. An empty
/// file is a new database to SQLite.
pub fn prepare_database(db: &Path) -> io::Result<()> {
    open(db, false)?;
    #[cfg(unix)]
    for suffix in ["-wal", "-shm"] {
        let mut name = db.as_os_str().to_owned();
        name.push(suffix);
        match fs::set_permissions(&name, fs::Permissions::from_mode(0o600)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// Takes group and other access off `path` and everything in it, for installs whose files were
/// made with the default umask. Only bits are removed, so it is safe to run at every launch and
/// costs one `lstat` per file once done. Symlinks are not followed; failures are skipped.
pub fn repair(path: &Path) {
    #[cfg(unix)]
    {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return;
        };
        if meta.file_type().is_symlink() {
            return;
        }
        let mode = meta.permissions().mode();
        if mode & 0o077 != 0 {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o700));
        }
        if meta.is_dir() {
            for entry in fs::read_dir(path).into_iter().flatten().flatten() {
                repair(&entry.path());
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};

    /// The umask belongs to the whole process, so tests that set it take turns.
    static UMASK: Mutex<()> = Mutex::new(());

    pub(crate) struct PermissiveUmask {
        previous: libc::mode_t,
        _turn: MutexGuard<'static, ()>,
    }

    /// A umask that would leave new files world-readable, restored when dropped.
    pub(crate) fn permissive_umask() -> PermissiveUmask {
        let turn = UMASK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // SAFETY: umask only swaps the process file-mode mask and cannot fail.
        let previous = unsafe { libc::umask(0o022) };
        PermissiveUmask {
            previous,
            _turn: turn,
        }
    }

    impl Drop for PermissiveUmask {
        fn drop(&mut self) {
            // SAFETY: as above.
            unsafe { libc::umask(self.previous) };
        }
    }

    pub(crate) fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    pub(crate) fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openrize-{name}-{}", uuid::Uuid::now_v7()));
        fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn new_folders_and_files_are_private_under_a_permissive_umask() {
        let _umask = permissive_umask();
        let root = temp("private-fs-new");
        let nested = root.join("accounts").join("a1");
        create_dir_all(&nested).unwrap();
        write(&nested.join("accounts.json"), b"{}").unwrap();

        assert_eq!(mode(&root.join("accounts")), 0o700);
        assert_eq!(mode(&nested), 0o700);
        assert_eq!(mode(&nested.join("accounts.json")), 0o600);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn writing_over_a_readable_file_makes_it_private_and_keeps_no_old_bytes() {
        let _umask = permissive_umask();
        let root = temp("private-fs-over");
        let file = root.join("settings.json.tmp");
        fs::write(&file, b"a longer, older body").unwrap();
        assert_eq!(mode(&file), 0o644);

        write(&file, b"new").unwrap();
        assert_eq!(mode(&file), 0o600);
        assert_eq!(fs::read(&file).unwrap(), b"new");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repair_tightens_an_older_install_and_only_removes_access() {
        let _umask = permissive_umask();
        let root = temp("private-fs-repair");
        let ml = root.join("accounts").join("a1").join("ml");
        fs::create_dir_all(&ml).unwrap();
        let files = [
            root.join("activity.db"),
            root.join("activity.db-wal"),
            root.join("activity.db-shm"),
            root.join("accounts.json"),
            root.join("switch.json"),
            root.join("app.lock"),
            root.join("accounts").join("a1").join("activity.db"),
            ml.join("category-train.jsonl"),
        ];
        for file in &files {
            fs::write(file, b"x").unwrap();
            assert_eq!(mode(file), 0o644);
        }
        let tool = root.join("tool");
        fs::write(&tool, b"x").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        let outside = temp("private-fs-outside");
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(mode(&ml), 0o755);

        repair(&root);
        repair(&root);

        for dir in [&root, &root.join("accounts"), &ml] {
            assert_eq!(mode(dir), 0o700, "{dir:?}");
        }
        for file in &files {
            assert_eq!(mode(file), 0o600, "{file:?}");
        }
        assert_eq!(mode(&tool), 0o700);
        assert_eq!(mode(&outside), 0o755, "a symlink's target is not ours");
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
