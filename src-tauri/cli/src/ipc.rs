//! Local IPC between the client and its read service. The same executable
//! hosts the service on demand (`rize __serve`); it neither launches the GUI
//! nor captures activity. `interprocess` provides the transport: a Unix-domain
//! socket in a private directory, or a Windows named pipe. Each platform file
//! adds the access checks its transport needs and exposes the same items.
#[cfg_attr(unix, path = "ipc/unix.rs")]
#[cfg_attr(windows, path = "ipc/windows.rs")]
mod platform;

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::Stream;
use openrize_core::{paths, readonly};

use crate::protocol::{self, Data, Operation, Request, Response};

const TIMEOUT: Duration = Duration::from_secs(5);
const IDLE: Duration = Duration::from_secs(30);

pub struct Endpoint {
    data_dir: PathBuf,
    runtime_dir: Option<PathBuf>,
    address: platform::Address,
}

impl Endpoint {
    /// `runtime_dir` overrides where the endpoint lives: the socket directory
    /// on Unix, a pipe-name namespace on Windows.
    pub fn new(data_dir: &Path, runtime_dir: Option<&Path>) -> Result<Self, String> {
        Ok(Self {
            data_dir: data_dir.into(),
            runtime_dir: runtime_dir.map(Into::into),
            address: platform::Address::new(data_dir, runtime_dir)?,
        })
    }

    pub fn call(&self, request: &Request) -> Result<Response, String> {
        self.address.prepare()?;
        let response = match self.address.connect() {
            // An existing service may stop between our connect and its
            // accept, so a failed exchange retries once on a fresh service.
            Ok(stream) => match exchange(stream, request) {
                Ok(response) => response,
                Err(_) => exchange(self.start_service()?, request)?,
            },
            Err(_) => exchange(self.start_service()?, request)?,
        };
        if response.schema_version != protocol::VERSION {
            return Err("incompatible read service response version".into());
        }
        Ok(response)
    }

    fn start_service(&self) -> Result<Stream, String> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let spawn = || {
            let mut command = Command::new(&executable);
            command.arg("--data-dir").arg(&self.data_dir);
            if let Some(runtime_dir) = &self.runtime_dir {
                command.arg("--runtime-dir").arg(runtime_dir);
            }
            command
                .arg("__serve")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            platform::detach(&mut command);
            command
                .spawn()
                .map_err(|e| format!("could not start read service: {e}"))
        };
        let mut child = spawn()?;
        let deadline = Instant::now() + TIMEOUT;
        let connected: Result<Stream, String> = loop {
            if let Ok(stream) = self.address.connect() {
                break Ok(stream);
            }
            // A clean exit means another instance holds the endpoint, possibly
            // while shutting down; start again once it is free.
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
        connected
    }

    pub fn serve(&self) -> Result<(), String> {
        let Some(mut service) = self.address.listen()? else {
            // Another instance owns this endpoint.
            return Ok(());
        };
        let mut last_request = Instant::now();
        while last_request.elapsed() < IDLE {
            match service.listener.accept() {
                Ok(stream) => {
                    last_request = Instant::now();
                    self.respond(stream);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50))
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        // Stop new connections where the platform can, then answer every
        // client that already connected before the listener closes.
        service.close_name();
        while let Ok(stream) = service.listener.accept() {
            self.respond(stream);
        }
        Ok(())
    }

    fn respond(&self, mut stream: Stream) {
        // A disconnected probe may make socket setup fail; a bad client must
        // not stop the service.
        if stream.set_nonblocking(false).is_err() || bound(&stream).is_err() {
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
        let database = self.data_dir.join(paths::DATABASE_FILE);
        if !database.is_file() {
            return Response::error(
                "NO_DATA",
                "no existing OpenRize activity database; select an existing store with --data-dir",
            );
        }
        let conn = match readonly::open_database(&database) {
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

fn exchange(mut stream: Stream, request: &Request) -> Result<Response, String> {
    bound(&stream).map_err(|e| e.to_string())?;
    protocol::write_frame(&mut stream, request, protocol::MAX_REQUEST)?;
    protocol::read_frame(&mut stream, protocol::MAX_RESPONSE)
}

/// Bounds each read and write by `TIMEOUT`. Named pipes have no I/O timeouts;
/// there the pipe's current-user-only access is what keeps a stalled peer out.
fn bound(stream: &Stream) -> io::Result<()> {
    let unsupported = |e: io::Error| match e.kind() {
        io::ErrorKind::Unsupported => Ok(()),
        _ => Err(e),
    };
    stream
        .set_recv_timeout(Some(TIMEOUT))
        .or_else(unsupported)?;
    stream.set_send_timeout(Some(TIMEOUT)).or_else(unsupported)
}

/// A stable, non-cryptographic endpoint key. Separate data directories get
/// separate services, and the key stays short enough for socket paths.
fn key(parts: &[&Path]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for byte in part.as_os_str().as_encoded_bytes() {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    hash
}
