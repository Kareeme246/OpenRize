//! A local named pipe that only its owner can open. `interprocess` creates the
//! first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE`, which elects one
//! service per pipe name, and rejects remote clients.
use std::io;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    GenericNamespaced, Listener, ListenerNonblockingMode, ListenerOptions, Stream,
};
use interprocess::os::windows::local_socket::ListenerOptionsExt;
use interprocess::os::windows::security_descriptor::SecurityDescriptor;

/// Protected DACL with one ACE: generic-all for the pipe's owner, the user
/// that started the service. Everyone else cannot open it, even to read.
const OWNER_ONLY: &widestring::U16CStr = widestring::u16cstr!("D:P(A;;GA;;;OW)");
/// `CreateProcess` flags: no console, and outside the caller's Ctrl+C group.
const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

pub struct Address {
    pipe: String,
}

pub struct Service {
    pub listener: Listener,
}

impl Service {
    /// A pipe name lives as long as its last instance, so it closes when the
    /// listener drops.
    pub fn close_name(&mut self) {}
}

impl Address {
    pub fn new(data_dir: &Path, runtime_dir: Option<&Path>) -> Result<Self, String> {
        let key = match runtime_dir {
            Some(runtime_dir) => super::key(&[runtime_dir, data_dir]),
            None => super::key(&[data_dir]),
        };
        Ok(Self {
            pipe: format!("rize-v1-{key:016x}"),
        })
    }

    /// Named pipes need no directory.
    pub fn prepare(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn connect(&self) -> Result<Stream, String> {
        Stream::connect(self.name()?).map_err(|e| e.to_string())
    }

    /// Creates the pipe, or `None` when another service already owns it.
    pub fn listen(&self) -> Result<Option<Service>, String> {
        let security = SecurityDescriptor::deserialize(OWNER_ONLY).map_err(|e| e.to_string())?;
        match ListenerOptions::new()
            .name(self.name()?)
            .nonblocking(ListenerNonblockingMode::Accept)
            .security_descriptor(security)
            .create_sync()
        {
            Ok(listener) => Ok(Some(Service { listener })),
            // FILE_FLAG_FIRST_PIPE_INSTANCE fails with ERROR_ACCESS_DENIED.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn name(&self) -> Result<interprocess::local_socket::Name<'_>, String> {
        self.pipe
            .as_str()
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| e.to_string())
    }
}

/// Runs the service without a console window that would close with the terminal.
pub fn detach(command: &mut Command) {
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}
