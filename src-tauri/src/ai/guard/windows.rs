//! A Job Object with kill-on-close: the OS ends the server when the app's last handle to the job
//! closes, including when the app is killed or crashes.

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW};

pub struct LifetimeGuard(HANDLE);

// A job handle is valid from any thread.
unsafe impl Send for LifetimeGuard {}

impl Drop for LifetimeGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub fn prepare(command: &mut Command) {
    command.creation_flags((BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW).0);
}

pub fn attach(child: &Child) -> Result<LifetimeGuard, String> {
    let describe = |what: &str, error: windows::core::Error| format!("{what}: {error}");
    unsafe {
        let job = CreateJobObjectW(None, PCWSTR::null())
            .map_err(|error| describe("could not create a job object", error))?;
        let guard = LifetimeGuard(job);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .map_err(|error| describe("could not set the job limits", error))?;
        AssignProcessToJobObject(job, HANDLE(child.as_raw_handle()))
            .map_err(|error| describe("could not put the server in the job", error))?;
        Ok(guard)
    }
}

pub fn run_watchdog(_app: u32, _child: u32) -> i32 {
    0
}
