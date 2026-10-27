//! A local named pipe that only its owner can open. `interprocess` creates the
//! first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE` and rejects remote
//! clients. Pipe names are global, so both ends also check that the other end
//! runs as the same user: a pipe created first by someone else is never
//! trusted.
use std::io;
use std::path::Path;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, Stream};
use interprocess::os::windows::local_socket::ListenerOptionsExt;
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcess, OpenProcessToken, OpenThreadToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Protected DACL with one ACE: generic-all for the pipe's owner, the user
/// that started the app. Everyone else cannot open it, even to read.
const OWNER_ONLY: &widestring::U16CStr = widestring::u16cstr!("D:P(A;;GA;;;OW)");

pub struct Address {
    pipe: String,
}

pub struct Service {
    listener: Listener,
}

impl Service {
    /// Waits for the next client. Who it is can only be asked once it has
    /// sent something (see `client_allowed`), so `respond` checks.
    pub fn accept(&self) -> io::Result<Stream> {
        self.listener.accept()
    }
}

impl Address {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        Ok(Self {
            pipe: format!("rize-v2-{:016x}", super::key(data_dir)),
        })
    }

    pub fn connect(&self) -> Result<Stream, String> {
        let stream = Stream::connect(self.name()?).map_err(|e| e.to_string())?;
        if !same_user(&stream) {
            return Err("the IPC pipe is served by another user".into());
        }
        Ok(stream)
    }

    pub fn listen(&self) -> Result<Service, String> {
        let security = SecurityDescriptor::deserialize(OWNER_ONLY).map_err(|e| e.to_string())?;
        let listener = ListenerOptions::new()
            .name(self.name()?)
            .security_descriptor(security)
            .create_sync()
            .map_err(|e| e.to_string())?;
        Ok(Service { listener })
    }

    fn name(&self) -> Result<interprocess::local_socket::Name<'_>, String> {
        self.pipe
            .as_str()
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| e.to_string())
    }
}

fn same_user(stream: &Stream) -> bool {
    let Some(pid) = stream.peer_creds().ok().and_then(|creds| creds.pid()) else {
        return false;
    };
    // SAFETY: every handle opened here is checked and closed before return,
    // and the SID buffers outlive the comparison.
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        let _ = CloseHandle(process);
        let peer = opened.ok().and_then(|()| token_user(token));
        same_sid(peer, own_user())
    }
}

/// Whether the client of a connection accepted here runs as this user, at any
/// elevation. The client's process token is the wrong thing to ask for: an
/// elevated process's token cannot be opened from a non-elevated one, so an
/// administrator's `rize` would be refused by the app. Impersonating the pipe
/// client instead yields a token for the connection itself, which this process
/// may always query; the guard puts the thread back as it was on every path.
/// Windows refuses to impersonate until the server has read from the pipe, so
/// this only works after the request has started to arrive.
pub fn client_allowed(stream: &Stream) -> bool {
    let Stream::NamedPipe(pipe) = stream;
    let impersonation = pipe.inner().impersonate_client();
    let Ok(impersonation) = impersonation else {
        return false;
    };
    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle needs no closing; `token` is closed by
    // `token_user` when it is opened. OpenAsSelf checks access against this
    // process rather than the client being impersonated.
    let opened = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token) };
    drop(impersonation);
    let peer = opened.ok().and_then(|()| unsafe { token_user(token) });
    same_sid(peer, own_user())
}

fn own_user() -> Option<Vec<u64>> {
    // SAFETY: the process token is opened and closed here.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        token_user(token)
    }
}

fn same_sid(peer: Option<Vec<u64>>, own: Option<Vec<u64>>) -> bool {
    match (peer, own) {
        // SAFETY: both buffers hold a TOKEN_USER and its SID, filled by
        // `token_user`, and live through the comparison.
        (Some(peer), Some(own)) => unsafe {
            let peer = &*(peer.as_ptr() as *const TOKEN_USER);
            let own = &*(own.as_ptr() as *const TOKEN_USER);
            EqualSid(peer.User.Sid, own.User.Sid).is_ok()
        },
        _ => false,
    }
}

/// The `TOKEN_USER` of a token, in a buffer that also holds its SID. Closes
/// the token.
unsafe fn token_user(token: HANDLE) -> Option<Vec<u64>> {
    let mut needed = 0;
    let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
    // u64 elements keep the buffer aligned for TOKEN_USER's pointer field.
    let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
    let filled = GetTokenInformation(
        token,
        TokenUser,
        Some(buffer.as_mut_ptr().cast()),
        needed,
        &mut needed,
    );
    let _ = CloseHandle(token);
    filled.ok().map(|()| buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Whether this thread is running as someone else right now.
    fn impersonating() -> bool {
        let mut token = HANDLE::default();
        // SAFETY: the token, when there is one, is closed straight away.
        unsafe {
            let opened = OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token);
            if opened.is_ok() {
                let _ = CloseHandle(token);
            }
            opened.is_ok()
        }
    }

    #[test]
    fn the_client_is_this_user_once_it_has_written_and_the_thread_is_restored() {
        let dir = std::env::temp_dir().join(format!("rize-ipc-win-{}", std::process::id()));
        let address = Address::new(&dir).unwrap();
        let service = address.listen().unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = service.accept().unwrap();
            // Nothing read yet, so Windows cannot say who the client is.
            assert!(!client_allowed(&stream));
            assert!(!impersonating());
            let mut byte = [0_u8; 1];
            stream.read_exact(&mut byte).unwrap();
            assert!(client_allowed(&stream));
            assert!(!impersonating());
            stream.write_all(&byte).unwrap();
        });
        let mut client = address.connect().unwrap();
        client.write_all(b"x").unwrap();
        let mut echoed = [0_u8; 1];
        client.read_exact(&mut echoed).unwrap();
        server.join().unwrap();
        assert_eq!(&echoed, b"x");
    }

    #[test]
    fn a_missing_user_is_never_the_same_user() {
        assert!(!same_sid(None, own_user()));
        assert!(!same_sid(own_user(), None));
        assert!(same_sid(own_user(), own_user()));
    }
}
