//! A Unix-domain socket in a private (0700), user-owned directory. A
//! process-scoped `flock` elects one service per socket.
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    GenericFilePath, Listener, ListenerNonblockingMode, ListenerOptions, Stream,
};
use openrize_core::paths;

pub struct Address {
    directory: PathBuf,
    socket: PathBuf,
}

/// A bound listener. It holds the service lock until dropped.
pub struct Service {
    pub listener: Listener,
    socket: PathBuf,
    _lock: File,
}

impl Service {
    /// Unlinks the socket so no new client can connect.
    pub fn close_name(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.close_name();
    }
}

impl Address {
    pub fn new(data_dir: &Path, runtime_dir: Option<&Path>) -> Result<Self, String> {
        let directory = match runtime_dir {
            Some(dir) => dir.into(),
            None => paths::app_cache_dir()
                .ok_or("no home directory; provide --runtime-dir")?
                .join("cli"),
        };
        let socket = directory.join(format!("v1-{:016x}.sock", super::key(&[data_dir])));
        // sockaddr_un holds 104 bytes on macOS (108 on Linux).
        if socket.as_os_str().as_encoded_bytes().len() >= 104 {
            return Err(
                "runtime directory is too long for a Unix socket; pass --runtime-dir".into(),
            );
        }
        Ok(Self { directory, socket })
    }

    /// Creates the private directory, or rejects one anyone else can reach.
    pub fn prepare(&self) -> Result<(), String> {
        if !self.directory.exists() {
            // Parent may already be the system cache directory. The endpoint
            // directory itself must be owned by this user and private.
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.directory)
                .map_err(|e| e.to_string())?;
        }
        let meta = fs::symlink_metadata(&self.directory).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.uid() != current_uid() || meta.mode() & 0o077 != 0 {
            return Err(
                "runtime directory must be a real, user-owned directory with mode 0700".into(),
            );
        }
        Ok(())
    }

    pub fn connect(&self) -> Result<Stream, String> {
        let meta = fs::symlink_metadata(&self.socket).map_err(|e| e.to_string())?;
        if !meta.file_type().is_socket() || meta.uid() != current_uid() || meta.mode() & 0o077 != 0
        {
            return Err("unsafe IPC socket".into());
        }
        Stream::connect(self.name()?).map_err(|e| e.to_string())
    }

    /// Binds the socket, or `None` when another service holds the lock.
    pub fn listen(&self) -> Result<Option<Service>, String> {
        self.prepare()?;
        // flock is process-scoped and released even on a crash. Only the lock
        // holder can remove a stale socket; concurrent autostarts never unlink
        // a live endpoint. O_NOFOLLOW rejects hostile lock-file symlinks.
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.socket.with_extension("lock"))
            .map_err(|e| e.to_string())?;
        let meta = lock.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.uid() != current_uid() || meta.mode() & 0o077 != 0 {
            return Err("unsafe service lock".into());
        }
        if !try_lock(&lock)? {
            return Ok(None);
        }
        match fs::symlink_metadata(&self.socket) {
            Ok(meta) if meta.file_type().is_socket() && meta.uid() == current_uid() => {
                fs::remove_file(&self.socket).map_err(|e| e.to_string())?;
            }
            Ok(_) => return Err("refusing to replace a non-socket IPC path".into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        // The service unlinks the socket itself, before draining, so the
        // listener must not reclaim the name again when it drops.
        let listener = ListenerOptions::new()
            .name(self.name()?)
            .nonblocking(ListenerNonblockingMode::Accept)
            .reclaim_name(false)
            .create_sync()
            .map_err(|e| e.to_string())?;
        let service = Service {
            listener,
            socket: self.socket.clone(),
            _lock: lock,
        };
        fs::set_permissions(&self.socket, fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        Ok(Some(service))
    }

    fn name(&self) -> Result<interprocess::local_socket::Name<'_>, String> {
        self.socket
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .map_err(|e| e.to_string())
    }
}

/// The service needs no detaching on Unix; it ignores the terminal it never reads.
pub fn detach(_command: &mut Command) {}

fn current_uid() -> u32 {
    // SAFETY: geteuid has no arguments and cannot invalidate Rust memory.
    unsafe { libc::geteuid() }
}

fn try_lock(file: &File) -> Result<bool, String> {
    // SAFETY: file owns a valid fd for the duration of this call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        Ok(false)
    } else {
        Err(error.to_string())
    }
}
