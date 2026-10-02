//! A local named pipe that only its owner can open. `interprocess` creates the
//! first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE` and rejects remote
//! clients. Pipe names are global, so both ends also check that the process
//! on the other end runs as the same user: a pipe created first by someone
//! else is never trusted.
use std::io;
use std::path::Path;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, Stream};
use interprocess::os::windows::local_socket::ListenerOptionsExt;
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
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
        let peer = token_user(process);
        let _ = CloseHandle(process);
        match (peer, token_user(GetCurrentProcess())) {
            (Some(peer), Some(own)) => {
                let peer = &*(peer.as_ptr() as *const TOKEN_USER);
                let own = &*(own.as_ptr() as *const TOKEN_USER);
                EqualSid(peer.User.Sid, own.User.Sid).is_ok()
            }
            _ => false,
        }
    }
}

/// The `TOKEN_USER` of a process, in a buffer that also holds its SID.
unsafe fn token_user(process: HANDLE) -> Option<Vec<u64>> {
    let mut token = HANDLE::default();
    OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
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
