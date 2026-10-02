//! A Unix-domain socket in a private (0700), user-owned directory. Both ends
//! also check that the peer runs as the same user.
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericFilePath, Listener, ListenerOptions, Stream};

use crate::paths;

pub struct Address {
    directory: PathBuf,
    socket: PathBuf,
}

/// A bound listener. Dropping it removes the socket.
pub struct Service {
    listener: Listener,
    socket: PathBuf,
}

impl Service {
    /// Waits for the next client running as this user.
    pub fn accept(&self) -> io::Result<Stream> {
        loop {
            let stream = self.listener.accept()?;
            if same_user(&stream) {
                return Ok(stream);
            }
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

impl Address {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let directory = paths::app_cache_dir()
            .ok_or("no home directory for the app's socket")?
            .join("cli");
        let socket = directory.join(format!("v2-{:016x}.sock", super::key(data_dir)));
        // sockaddr_un holds 104 bytes on macOS (108 on Linux).
        if socket.as_os_str().as_encoded_bytes().len() >= 104 {
            return Err(format!(
                "the socket path {} is too long for a Unix socket",
                socket.display()
            ));
        }
        Ok(Self { directory, socket })
    }

    /// Creates the private directory, or rejects one anyone else can reach.
    fn prepare(&self) -> Result<(), String> {
        if !self.directory.exists() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.directory)
                .map_err(|e| e.to_string())?;
        }
        let meta = fs::symlink_metadata(&self.directory).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.uid() != current_uid() || meta.mode() & 0o077 != 0 {
            return Err(format!(
                "{} must be a real, user-owned directory with mode 0700",
                self.directory.display()
            ));
        }
        Ok(())
    }

    pub fn connect(&self) -> Result<Stream, String> {
        let meta = fs::symlink_metadata(&self.socket).map_err(|e| e.to_string())?;
        if !meta.file_type().is_socket() || meta.uid() != current_uid() || meta.mode() & 0o077 != 0
        {
            return Err("unsafe IPC socket".into());
        }
        let stream = Stream::connect(self.name()?).map_err(|e| e.to_string())?;
        if !same_user(&stream) {
            return Err("the IPC socket is served by another user".into());
        }
        Ok(stream)
    }

    pub fn listen(&self) -> Result<Service, String> {
        self.prepare()?;
        // The caller holds the stores' lock, so a socket left here belongs to
        // an app that is gone.
        match fs::symlink_metadata(&self.socket) {
            Ok(meta) if meta.file_type().is_socket() && meta.uid() == current_uid() => {
                fs::remove_file(&self.socket).map_err(|e| e.to_string())?;
            }
            Ok(_) => return Err("refusing to replace a non-socket IPC path".into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        let listener = ListenerOptions::new()
            .name(self.name()?)
            .reclaim_name(false)
            .create_sync()
            .map_err(|e| e.to_string())?;
        let service = Service {
            listener,
            socket: self.socket.clone(),
        };
        fs::set_permissions(&self.socket, fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        Ok(service)
    }

    fn name(&self) -> Result<interprocess::local_socket::Name<'_>, String> {
        self.socket
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .map_err(|e| e.to_string())
    }
}

fn same_user(stream: &Stream) -> bool {
    stream
        .peer_creds()
        .ok()
        .and_then(|creds| creds.euid())
        .is_some_and(|uid| uid == current_uid())
}

fn current_uid() -> u32 {
    // SAFETY: geteuid has no arguments and cannot invalidate Rust memory.
    unsafe { libc::geteuid() }
}
