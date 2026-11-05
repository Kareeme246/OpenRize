//! The local endpoint the running app serves and `rize` connects to: a Unix socket in a private
//! directory, or a Windows named pipe. Each platform file exposes the same items.
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

    pub fn connect(&self) -> Result<Stream, String> {
        self.address.connect()
    }

    /// Takes the endpoint for this process. Only the process holding the
    /// stores' lock may call this, so a leftover endpoint is always stale.
    pub fn listen(&self) -> Result<Service, String> {
        self.address.listen()
    }

    pub fn call(&self, request: &Request) -> Result<Response, String> {
        exchange(self.connect()?, request)
    }
}

pub fn exchange(mut stream: Stream, request: &Request) -> Result<Response, String> {
    bound(&stream).map_err(|e| e.to_string())?;
    // A server that turns the connection away answers and hangs up, which can
    // fail this write; its answer is still waiting to be read.
    let sent = protocol::write_frame(&mut stream, request, protocol::MAX_REQUEST);
    match protocol::read_frame(&mut stream, protocol::MAX_RESPONSE) {
        Ok(response) => Ok(response),
        Err(error) => Err(match sent {
            Err(write_error) => format!("could not reach OpenRize: {write_error}"),
            Ok(()) => format!(
                "OpenRize hung up without answering ({error}). It may have refused this connection."
            ),
        }),
    }
}

pub fn respond(mut stream: Stream, handle: impl FnOnce(Request) -> Response) {
    // A client that disconnects mid-setup must not stop the server.
    if bound(&stream).is_err() {
        return;
    }
    let response = match protocol::read_frame::<Request>(&mut stream, protocol::MAX_REQUEST) {
        // Answered, not dropped, so `rize` can say why it got nothing.
        Ok(_) if !platform::client_allowed(&stream) => Response::error(
            protocol::code::FAILED,
            "OpenRize refused the connection: this program does not run as the user that started the app.",
        ),
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
fn canonical(data_dir: &Path) -> PathBuf {
    let absolute = std::path::absolute(data_dir).unwrap_or_else(|_| data_dir.into());
    if cfg!(unix) {
        absolute.canonicalize().unwrap_or(absolute)
    } else {
        on_disk_form(&absolute)
    }
}

/// Windows paths are case-insensitive, so the key uses the spelling the file system reports
/// (casing, no trailing separator, `.`/`..` or 8.3 names); a missing tail keeps its own spelling.
/// Folders already in that form hash as before, so an older `rize` still finds a newer app.
fn on_disk_form(absolute: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut base = absolute;
    loop {
        // Canonical Windows paths are verbatim (`\\?\`), which the key must not be.
        if let Ok(real) = base.canonicalize() {
            let mut form = strip_verbatim(&real);
            form.extend(tail.iter().rev());
            return form;
        }
        match (base.parent(), base.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name);
                base = parent;
            }
            _ => return absolute.components().collect(),
        }
    }
}

/// `\\?\C:\x` is `C:\x` and `\\?\UNC\server\share` is `\\server\share`.
fn strip_verbatim(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(local) if local.as_bytes().get(1) == Some(&b':') => PathBuf::from(local),
        _ => path.to_path_buf(),
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
    fn verbatim_prefixes_come_off_local_and_network_paths_only() {
        let strip = |text: &str| strip_verbatim(Path::new(text));
        assert_eq!(strip(r"\\?\C:\Users\k"), Path::new(r"C:\Users\k"));
        assert_eq!(
            strip(r"\\?\UNC\server\share\d"),
            Path::new(r"\\server\share\d")
        );
        assert_eq!(
            strip(r"\\?\Volume{1234}\d"),
            Path::new(r"\\?\Volume{1234}\d")
        );
        assert_eq!(strip(r"C:\plain"), Path::new(r"C:\plain"));
    }

    #[cfg(windows)]
    #[test]
    fn every_spelling_of_a_windows_folder_names_the_same_endpoint() {
        let dir = std::env::temp_dir().join(format!("Rize-Ipc-Key-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Data")).unwrap();
        let reference = canonical(&dir.join("Data"));
        assert_eq!(canonical(&reference), reference);
        assert_eq!(strip_verbatim(&reference), reference);

        let text = reference.to_string_lossy().into_owned();
        for spelling in [
            text.to_uppercase(),
            text.to_lowercase(),
            format!("{text}\\"),
            text.replace('\\', "/"),
            format!("{text}\\.\\"),
            format!("{text}\\..\\Data"),
        ] {
            assert_eq!(
                key(&canonical(Path::new(&spelling))),
                key(&reference),
                "{spelling}"
            );
        }

        // A folder that is not there yet: the existing part is corrected, the
        // rest is as typed and loses its trailing separator.
        let missing = canonical(Path::new(&format!("{}\\New\\", text.to_uppercase())));
        assert_eq!(missing, reference.join("New"));
        let _ = std::fs::remove_dir_all(&dir);
    }

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
