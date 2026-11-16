//! The local endpoint the running app serves and `rize` connects to: a Unix socket in a private
//! directory, or a Windows named pipe. Each platform file exposes the same items.
#[cfg_attr(unix, path = "ipc/unix.rs")]
#[cfg_attr(windows, path = "ipc/windows.rs")]
mod platform;

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
pub use interprocess::local_socket::Stream;

use crate::protocol::{self, Request, Response};
pub use platform::Service;

/// Bounds each read and write, so a stalled peer cannot hang either side. The
/// app also gives a whole request, and a whole answer, no longer than this.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// How often the app tries again for an endpoint it could not take.
const LISTEN_RETRY: Duration = Duration::from_secs(5);

/// Requests the app answers at once. `rize` sends one per run, so more than a
/// few at a time is a flood; the rest wait to be accepted.
const MAX_CONNECTIONS: usize = 8;

/// Every data directory (the installed app, a dev instance) gets its own endpoint.
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

/// Answers every request to `endpoint` with `handle`, each on its own thread.
pub fn serve(endpoint: Endpoint, handle: impl Fn(Request) -> Response + Send + Sync + 'static) {
    serve_retrying(endpoint, LISTEN_RETRY, handle);
}

fn serve_retrying(
    endpoint: Endpoint,
    retry: Duration,
    handle: impl Fn(Request) -> Response + Send + Sync + 'static,
) {
    std::thread::spawn(move || {
        // A Windows pipe name is anyone's to take first; the app gets its
        // endpoint back once whoever holds the name lets go.
        let mut reported = false;
        let service = loop {
            match endpoint.listen() {
                Ok(service) => break service,
                Err(error) => {
                    if !std::mem::replace(&mut reported, true) {
                        eprintln!("rize endpoint unavailable, still trying: {error}");
                    }
                    std::thread::sleep(retry);
                }
            }
        };
        let handle = Arc::new(handle);
        let slots = Slots::new(MAX_CONNECTIONS);
        loop {
            if let Err(error) = admit(&service, &slots, TIMEOUT, &handle) {
                eprintln!("rize endpoint: {error}");
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    });
}

/// Hands the next connection to a thread of its own, once a slot is free.
fn admit(
    service: &Service,
    slots: &Arc<Slots>,
    timeout: Duration,
    handle: &Arc<impl Fn(Request) -> Response + Send + Sync + 'static>,
) -> io::Result<()> {
    let stream = service.accept()?;
    let slot = Slot::take(slots);
    let handle = handle.clone();
    std::thread::spawn(move || {
        respond(stream, timeout, |request| handle(request));
        drop(slot);
    });
    Ok(())
}

struct Slots {
    free: Mutex<usize>,
    freed: Condvar,
}

impl Slots {
    fn new(free: usize) -> Arc<Self> {
        Arc::new(Self {
            free: Mutex::new(free),
            freed: Condvar::new(),
        })
    }
}

/// One of the connections being answered; dropping it, even by a panic in the
/// handler, lets the next one in.
struct Slot(Arc<Slots>);

impl Slot {
    fn take(slots: &Arc<Slots>) -> Self {
        let mut free = slots.free.lock().unwrap_or_else(PoisonError::into_inner);
        while *free == 0 {
            free = slots
                .freed
                .wait(free)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *free -= 1;
        Self(slots.clone())
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        self.0.freed.notify_one();
    }
}

fn respond(mut stream: Stream, timeout: Duration, handle: impl FnOnce(Request) -> Response) {
    let request = protocol::read_frame::<Request>(
        &mut Timed::new(&mut stream, timeout),
        protocol::MAX_REQUEST,
    );
    let response = match request {
        // Answered, not dropped, so `rize` can say why it got nothing.
        Ok(_) if !platform::client_allowed(&stream) => Response::error(
            protocol::code::FAILED,
            "OpenRize refused the connection: this program does not run as the user that started the app.",
        ),
        Ok(request) => handle(request),
        Err(_) => Response::error("INVALID_REQUEST", "invalid or oversized request"),
    };
    let _ = protocol::write_frame(
        &mut Timed::new(&mut stream, timeout),
        &response,
        protocol::MAX_RESPONSE,
    );
}

/// A stream whose reads and writes must all finish by one deadline. A timeout
/// on each read alone lets a peer sending a byte at a time hold its thread
/// for as long as the largest frame takes.
struct Timed<'a> {
    stream: &'a mut Stream,
    until: Instant,
}

impl<'a> Timed<'a> {
    fn new(stream: &'a mut Stream, timeout: Duration) -> Self {
        Self {
            stream,
            until: Instant::now() + timeout,
        }
    }

    fn left(&self) -> io::Result<Option<Duration>> {
        match self.until.saturating_duration_since(Instant::now()) {
            Duration::ZERO => Err(io::ErrorKind::TimedOut.into()),
            left => Ok(Some(left)),
        }
    }
}

impl Read for Timed<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream
            .set_recv_timeout(self.left()?)
            .or_else(unsupported)?;
        self.stream.read(buffer)
    }
}

impl Write for Timed<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream
            .set_send_timeout(self.left()?)
            .or_else(unsupported)?;
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn bound(stream: &Stream) -> io::Result<()> {
    stream
        .set_recv_timeout(Some(TIMEOUT))
        .or_else(unsupported)?;
    stream.set_send_timeout(Some(TIMEOUT)).or_else(unsupported)
}

/// Named pipes have no I/O timeouts; there the pipe's current-user-only access
/// is what keeps a stalled peer out, and `MAX_CONNECTIONS` what bounds one.
fn unsupported(error: io::Error) -> io::Result<()> {
    match error.kind() {
        io::ErrorKind::Unsupported => Ok(()),
        _ => Err(error),
    }
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
            respond(stream, TIMEOUT, |request| {
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

    /// Serves `requests` connections as `serve` does, then gives the endpoint up.
    fn serving(
        endpoint: &Endpoint,
        requests: usize,
        connections: usize,
        timeout: Duration,
        handle: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> std::thread::JoinHandle<()> {
        let service = endpoint.listen().unwrap();
        std::thread::spawn(move || {
            let (slots, handle) = (Slots::new(connections), Arc::new(handle));
            for _ in 0..requests {
                admit(&service, &slots, timeout, &handle).unwrap();
            }
        })
    }

    #[test]
    fn requests_beyond_the_limit_wait_their_turn() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let data_dir = std::env::temp_dir().join(format!("rize-ipc-limit-{}", std::process::id()));
        let endpoint = Arc::new(Endpoint::new(&data_dir).unwrap());
        let (now, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (counted_now, counted_most) = (now.clone(), most.clone());
        let server = serving(&endpoint, 6, 2, TIMEOUT, move |_| {
            counted_most.fetch_max(
                counted_now.fetch_add(1, Ordering::SeqCst) + 1,
                Ordering::SeqCst,
            );
            std::thread::sleep(Duration::from_millis(100));
            counted_now.fetch_sub(1, Ordering::SeqCst);
            Response::success(serde_json::json!({}))
        });
        let clients: Vec<_> = (0..6)
            .map(|_| {
                let endpoint = endpoint.clone();
                std::thread::spawn(move || endpoint.call(&Request::new(Operation::TimersList {})))
            })
            .collect();
        for client in clients {
            assert!(client.join().unwrap().unwrap().ok);
        }
        server.join().unwrap();
        assert_eq!(most.load(Ordering::SeqCst), 2);
    }

    /// A header promising a megabyte and nothing after it: the per-read
    /// timeout is far off, the deadline for the whole request is not.
    #[cfg(unix)]
    #[test]
    fn a_request_that_never_completes_is_cut_off_at_the_deadline() {
        let data_dir = std::env::temp_dir().join(format!("rize-ipc-slow-{}", std::process::id()));
        let endpoint = Endpoint::new(&data_dir).unwrap();
        let server = serving(&endpoint, 2, 1, Duration::from_millis(400), |_| {
            Response::success(serde_json::json!({}))
        });
        let mut slow = endpoint.connect().unwrap();
        slow.write_all(&(protocol::MAX_REQUEST as u32).to_be_bytes())
            .unwrap();
        let started = Instant::now();
        // The only slot is taken, so a second request is answered once it frees.
        let waiting = endpoint.call(&Request::new(Operation::TimersList {}));
        let took = started.elapsed();
        assert!(waiting.unwrap().ok);
        assert!(
            took >= Duration::from_millis(350) && took < Duration::from_secs(5),
            "{took:?}"
        );
        let answer = protocol::read_frame::<Response>(&mut slow, protocol::MAX_RESPONSE);
        assert_eq!(answer.unwrap().code(), Some("INVALID_REQUEST"));
        server.join().unwrap();
    }
}
