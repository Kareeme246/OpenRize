//! The local endpoint the running app serves and `rize` connects to: a
//! Unix-domain socket in a private directory, or a Windows named pipe.
//! `interprocess` provides the transport; each platform file adds the access
//! checks its transport needs and exposes the same items.
#[cfg_attr(unix, path = "ipc/unix.rs")]
#[cfg_attr(windows, path = "ipc/windows.rs")]
mod platform;

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use interprocess::local_socket::prelude::*;
pub use interprocess::local_socket::Stream;

use crate::protocol::{self, Request, Response};
pub use platform::Service;

/// Bounds each read and write, so a stalled peer cannot hang either side.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Where the app serving one data directory listens. Every data directory
/// (the installed app, a dev instance) gets its own endpoint.
pub struct Endpoint {
    address: platform::Address,
}

impl Endpoint {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        Ok(Self {
            address: platform::Address::new(&canonical(data_dir))?,
        })
    }

    /// Connects to the app, or fails when nothing is serving.
    pub fn connect(&self) -> Result<Stream, String> {
        self.address.connect()
    }

    /// Takes the endpoint for this process. Only the process holding the
    /// stores' lock may call this, so a leftover endpoint is always stale.
    pub fn listen(&self) -> Result<Service, String> {
        self.address.listen()
    }

    /// One request and its response over a fresh connection.
    pub fn call(&self, request: &Request) -> Result<Response, String> {
        exchange(self.connect()?, request)
    }
}

/// Sends one request on a connected stream and reads the answer.
pub fn exchange(mut stream: Stream, request: &Request) -> Result<Response, String> {
    bound(&stream).map_err(|e| e.to_string())?;
    protocol::write_frame(&mut stream, request, protocol::MAX_REQUEST)?;
    protocol::read_frame(&mut stream, protocol::MAX_RESPONSE)
}

/// Reads one request from a client and writes back what `handle` answers.
pub fn respond(mut stream: Stream, handle: impl FnOnce(Request) -> Response) {
    // A client that disconnects mid-setup must not stop the server.
    if bound(&stream).is_err() {
        return;
    }
    let response = match protocol::read_frame::<Request>(&mut stream, protocol::MAX_REQUEST) {
        Ok(request) => handle(request),
        Err(_) => Response::error("INVALID_REQUEST", "invalid or oversized request"),
    };
    let _ = protocol::write_frame(&mut stream, &response, protocol::MAX_RESPONSE);
}

/// Named pipes have no I/O timeouts; there the pipe's current-user-only access
/// is what keeps a stalled peer out.
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

/// The same directory always names the same endpoint, however it was spelled.
/// Windows canonical paths are verbatim (`\\?\`), so there the absolute path is
/// the key.
fn canonical(data_dir: &Path) -> PathBuf {
    let absolute = std::path::absolute(data_dir).unwrap_or_else(|_| data_dir.into());
    if cfg!(unix) {
        absolute.canonicalize().unwrap_or(absolute)
    } else {
        absolute
    }
}

/// A stable, non-cryptographic endpoint key, short enough for socket paths.
fn key(path: &Path) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in path.as_os_str().as_encoded_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Operation;

    #[test]
    fn a_request_round_trips_and_a_missing_server_is_an_error() {
        let data_dir = std::env::temp_dir().join(format!("rize-ipc-test-{}", std::process::id()));
        let endpoint = Endpoint::new(&data_dir).unwrap();
        assert!(endpoint.connect().is_err());

        let service = endpoint.listen().unwrap();
        let server = std::thread::spawn(move || {
            let stream = service.accept().unwrap();
            respond(stream, |request| {
                Response::success(serde_json::json!({ "echo": request.operation }))
            });
        });
        let response = endpoint
            .call(&Request::new(Operation::TimersList {}))
            .unwrap();
        server.join().unwrap();
        assert!(response.ok);
        assert_eq!(response.data.unwrap()["echo"]["kind"], "timersList");
    }
}
