//! User-local Unix IPC. The same independently installed executable hosts the
//! read-only service on demand; it neither launches the GUI nor captures activity.
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use openrize_core::readonly;

use crate::protocol::{self, Data, Operation, Request, Response};

const TIMEOUT: Duration = Duration::from_secs(5);
const IDLE: Duration = Duration::from_secs(30);

pub struct Endpoint {
    pub directory: PathBuf,
    pub socket: PathBuf,
    pub database: PathBuf,
}

impl Endpoint {
    pub fn new(data_dir: &Path, runtime_dir: &Path) -> Result<Self, String> {
        // A stable, non-cryptographic path key keeps separate data directories
        // isolated without overflowing sockaddr_un's small path limit.
        let mut hash = 0xcbf29ce484222325_u64;
        for byte in data_dir.as_os_str().as_encoded_bytes() {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
        let socket = runtime_dir.join(format!("v1-{hash:016x}.sock"));
        if socket.as_os_str().as_encoded_bytes().len() >= 104 {
            return Err(
                "runtime directory is too long for a Unix socket; pass --runtime-dir".into(),
            );
        }
        Ok(Self {
            directory: runtime_dir.into(),
            socket,
            database: data_dir.join("activity.db"),
        })
    }

    fn secure_directory(&self) -> Result<(), String> {
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

    fn connect(&self) -> Result<UnixStream, String> {
        let meta = fs::symlink_metadata(&self.socket).map_err(|e| e.to_string())?;
        if !meta.file_type().is_socket() || meta.uid() != current_uid() || meta.mode() & 0o077 != 0
        {
            return Err("unsafe IPC socket".into());
        }
        let stream = UnixStream::connect(&self.socket).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(TIMEOUT))
            .map_err(|e| e.to_string())?;
        Ok(stream)
    }

    pub fn call(&self, request: &Request) -> Result<Response, String> {
        self.secure_directory()?;
        let mut stream = match self.connect() {
            Ok(stream) => stream,
            Err(_) => {
                let executable = std::env::current_exe().map_err(|e| e.to_string())?;
                let data_dir = self.database.parent().ok_or("missing data directory")?;
                let spawn = || {
                    Command::new(&executable)
                        .arg("--data-dir")
                        .arg(data_dir)
                        .arg("--runtime-dir")
                        .arg(&self.directory)
                        .arg("__serve")
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(|e| format!("could not start read service: {e}"))
                };
                let mut child = spawn()?;
                let deadline = Instant::now() + TIMEOUT;
                let connected: Result<UnixStream, String> = loop {
                    if let Ok(stream) = self.connect() {
                        break Ok(stream);
                    }
                    // A clean exit means another instance held the service
                    // lock, possibly while shutting down; start again once free.
                    match child.try_wait().map_err(|e| e.to_string())? {
                        Some(status) if !status.success() => {
                            break Err("read service could not start".into());
                        }
                        Some(_) => child = spawn()?,
                        None => {}
                    }
                    if Instant::now() >= deadline {
                        break Err("read service startup timed out".into());
                    }
                    thread::sleep(Duration::from_millis(50));
                };
                // Successful service is intentionally detached and self-expires.
                // Failed startups must not leave our own orphan process behind.
                if connected.is_err() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                connected?
            }
        };
        protocol::write_frame(&mut stream, request, protocol::MAX_REQUEST)?;
        let response: Response = protocol::read_frame(&mut stream, protocol::MAX_RESPONSE)?;
        if response.schema_version != protocol::VERSION {
            return Err("incompatible read service response version".into());
        }
        Ok(response)
    }

    pub fn serve(&self) -> Result<(), String> {
        self.secure_directory()?;
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
            return Ok(());
        }
        match fs::symlink_metadata(&self.socket) {
            Ok(meta) if meta.file_type().is_socket() && meta.uid() == current_uid() => {
                fs::remove_file(&self.socket).map_err(|e| e.to_string())?;
            }
            Ok(_) => return Err("refusing to replace a non-socket IPC path".into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        let listener = UnixListener::bind(&self.socket).map_err(|e| e.to_string())?;
        fs::set_permissions(&self.socket, fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let cleanup = SocketCleanup(&self.socket);
        let mut last_request = Instant::now();
        while last_request.elapsed() < IDLE {
            match listener.accept() {
                Ok((stream, _)) => {
                    last_request = Instant::now();
                    self.respond(stream);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50))
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        // Unlink first so no new client can connect, then answer every client
        // that already connected before the listener closes.
        drop(cleanup);
        while let Ok((stream, _)) = listener.accept() {
            self.respond(stream);
        }
        Ok(())
    }

    fn respond(&self, mut stream: UnixStream) {
        // Accepted socket mode varies across Unix platforms. A disconnected
        // probe may also make timeout setup return EINVAL on macOS; a bad
        // client must not stop the service.
        if stream
            .set_nonblocking(false)
            .and_then(|()| stream.set_read_timeout(Some(TIMEOUT)))
            .and_then(|()| stream.set_write_timeout(Some(TIMEOUT)))
            .is_err()
        {
            return;
        }
        let response = match protocol::read_frame::<Request>(&mut stream, protocol::MAX_REQUEST) {
            Ok(request) => self.dispatch(request),
            Err(_) => Response::error("INVALID_REQUEST", "invalid or oversized IPC request"),
        };
        let _ = protocol::write_frame(&mut stream, &response, protocol::MAX_RESPONSE);
    }

    fn dispatch(&self, request: Request) -> Response {
        if request.protocol_version != protocol::VERSION {
            return Response::error(
                "PROTOCOL_MISMATCH",
                "unsupported IPC protocol version; update the CLI",
            );
        }
        if let Operation::Entries {
            from,
            to,
            limit,
            ref status,
            ..
        } = request.operation
        {
            if from > to
                || to >= i64::MAX as u64
                || !(1..=readonly::MAX_LIST).contains(&limit)
                || status
                    .as_deref()
                    .is_some_and(|s| !["pending", "approved"].contains(&s))
            {
                return Response::error(
                    "INVALID_ARGUMENT",
                    "invalid entries range, status, or limit",
                );
            }
        }
        if !self.database.is_file() {
            return Response::error(
                "NO_DATA",
                "no existing OpenRize activity database; select an existing store with --data-dir",
            );
        }
        let conn = match readonly::open_database(&self.database) {
            Ok(conn) => conn,
            Err(_) => {
                return Response::error(
                    "DATABASE_UNAVAILABLE",
                    "could not open the existing database read-only",
                )
            }
        };
        let result = match request.operation {
            Operation::Status {} => readonly::status(&conn).map(Data::Status),
            Operation::Entries {
                from,
                to,
                status,
                limit,
                full,
            } => readonly::entries(&conn, from, to, status, limit, full).map(Data::Entries),
        };
        match result {
            Ok(data) => Response::success(data),
            Err(_) => Response::error(
                "QUERY_FAILED",
                "read query failed; the database may require a compatible OpenRize schema",
            ),
        }
    }
}

struct SocketCleanup<'a>(&'a Path);
impl Drop for SocketCleanup<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0);
    }
}

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
