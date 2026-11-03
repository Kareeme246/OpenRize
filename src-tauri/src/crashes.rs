//! Recording crashes and, if the user has asked for it, sending them.
//!
//! What a report may contain, how it is scrubbed and where it is stored is in
//! `openrize_core::crashes`; read that first. This module is the part that
//! needs the running app: the panic hook, the UI's error command, and the one
//! outbound request.
//!
//! # Recording
//!
//! Reports are always written to `<install root>/crashes` on this machine
//! (`CRASH_ROOT`, set at startup to the folder `data_location::adopt` chose),
//! whether or not sending is on, so a user can see what would be sent and a
//! later opt-in has something to send. Before the root is known (a panic in
//! the first lines of `run`) nothing is written: guessing a folder here could
//! create one in the old Roaming location or in another build's data.
//!
//! The panic hook runs in a process that is about to end (`panic = "abort"`
//! in release), so it writes synchronously and never waits, never sends and
//! must never panic itself: it skips recording if it is already running, and
//! ignores every error. The next launch sends what it left, if sending is on.
//!
//! # Sending
//!
//! Off by default (`Settings::crash_reporting`). Nothing leaves the machine
//! while it is off, and every send re-reads the setting so turning it off
//! stops a send already queued. The address is a constant of this build
//! ([`CRASH_ENDPOINT`], https only, checked at compile time); there is no
//! setting, environment variable or report field that can change where a
//! report goes. The request has timeouts, follows no redirects, and carries
//! only the report, which holds no user content. Turning sending on also
//! sends reports already saved on this machine; they were scrubbed when they
//! were written.

use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use openrize_core::crashes::{self, Admission, CrashReport, Environment};
use tauri::{AppHandle, Manager};

use crate::AppState;

/// The folder holding `crashes/`, set once the app has resolved its data root.
static CRASH_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);
/// Which UI failures were already recorded in this run.
static ADMISSION: Mutex<Admission> = Mutex::new(Admission::new());
/// Set while the panic hook records, so a panic inside it cannot recurse.
static IN_PANIC_HOOK: AtomicBool = AtomicBool::new(false);
/// Set while the pending-report sweep runs, so toggling does not start two.
static SWEEPING: AtomicBool = AtomicBool::new(false);

/// Where reports are sent: fixed in the build, https only. A build can point
/// it elsewhere with `OPENRIZE_CRASH_ENDPOINT` at compile time, for a fork.
const CRASH_ENDPOINT: &str = match option_env!("OPENRIZE_CRASH_ENDPOINT") {
    Some(url) => url,
    None => "https://openrize-crashes.kareeme246.workers.dev/report",
};

const fn is_https(url: &str) -> bool {
    let (url, prefix) = (url.as_bytes(), b"https://");
    if url.len() <= prefix.len() {
        return false;
    }
    let mut i = 0;
    while i < prefix.len() {
        if url[i] != prefix[i] {
            return false;
        }
        i += 1;
    }
    true
}

const _: () = assert!(is_https(CRASH_ENDPOINT), "the crash endpoint must be https");

/// Records the folder reports are written under. Call it as soon as the data
/// root is resolved, before accounts or stores are opened.
pub fn set_crash_root(root: &Path) {
    if let Ok(mut lock) = CRASH_ROOT.write() {
        *lock = Some(root.to_path_buf());
    }
}

fn crash_root() -> Option<PathBuf> {
    CRASH_ROOT.read().ok().and_then(|lock| lock.clone())
}

/// The OS version, in the form the OS itself uses.
fn os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        let version = objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersion();
        format!(
            "{}.{}.{}",
            version.majorVersion, version.minorVersion, version.patchVersion
        )
    }
    #[cfg(windows)]
    {
        windows_registry::LOCAL_MACHINE
            .open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .and_then(|key| key.get_string("CurrentBuildNumber"))
            .map_or_else(|_| "unknown".to_string(), |build| format!("10.0.{build}"))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        "unknown".to_string()
    }
}

fn environment() -> Environment {
    Environment {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        os_version: os_version(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

/// Clears [`IN_PANIC_HOOK`] when the hook is done, however it ends.
struct HookGuard;

impl Drop for HookGuard {
    fn drop(&mut self) {
        IN_PANIC_HOOK.store(false, Ordering::SeqCst);
    }
}

/// Installs a panic hook that writes a scrubbed report before the default
/// hook runs. See the module docs for what it must never do.
pub fn init_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !IN_PANIC_HOOK.swap(true, Ordering::SeqCst) {
            let _guard = HookGuard;
            record_panic(info);
        }
        default_hook(info);
    }));
}

/// Writes the report for a panic. Only the source location and the symbol
/// names of the stack are kept; the payload (the panic's message) is dropped
/// because it can quote user data.
fn record_panic(info: &PanicHookInfo<'_>) {
    let Some(root) = crash_root() else {
        return;
    };
    let message = match info.location() {
        Some(location) => format!(
            "Rust panic at {}",
            crashes::panic_location(location.file(), location.line(), location.column())
        ),
        None => "Rust panic".to_string(),
    };
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    let environment = environment();
    let report = CrashReport {
        id: uuid::Uuid::now_v7().to_string(),
        timestamp_ms: crashes::now_epoch_ms(),
        app_version: environment.app_version,
        os: environment.os,
        os_version: environment.os_version,
        arch: environment.arch,
        source: crashes::SOURCE_BACKEND_PANIC.to_string(),
        message,
        backtrace: crashes::scrub_native_backtrace(&backtrace),
        component_stack: None,
        sent: false,
    };
    let _ = crashes::save_report(&root, &report);
}

/// Records a failure the UI caught. The UI sends what it has; everything is
/// reduced to the fields a report may hold here, never trusted as sent.
/// Returns without recording when this failure was already recorded in this
/// run or the run's budget is spent.
pub fn record_client_crash(
    app: &AppHandle,
    source: &str,
    name: Option<&str>,
    message: Option<&str>,
    stack: Option<&str>,
    component_stack: Option<&str>,
) -> Result<(), String> {
    let report = crashes::client_report(
        source,
        name,
        message,
        stack,
        component_stack,
        &environment(),
    );
    let admitted = ADMISSION
        .lock()
        .map_err(|error| error.to_string())?
        .admit(&report);
    if !admitted {
        return Ok(());
    }
    let root = crash_root().ok_or_else(|| "the data folder is not known yet".to_string())?;
    crashes::save_report(&root, &report).map_err(|error| error.to_string())?;
    spawn_dispatch(app, root, report);
    Ok(())
}

fn sending_enabled(app: &AppHandle) -> bool {
    app.state::<AppState>().settings_snapshot().crash_reporting
}

/// Sends one report, if sending is on, and marks it sent.
fn spawn_dispatch(app: &AppHandle, root: PathBuf, report: CrashReport) {
    if !sending_enabled(app) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        match dispatch(&report).await {
            Ok(()) => {
                let _ = crashes::mark_report_sent(&root, &report.id);
            }
            Err(error) => eprintln!("crash report not sent: {error}"),
        }
    });
}

/// Sends every report not yet sent, oldest first, if sending is on. Run at
/// startup and when the user turns sending on. It stops as soon as sending is
/// turned off again or a send fails (the rest wait for the next launch).
pub fn dispatch_pending_reports(app: AppHandle) {
    let Some(root) = crash_root() else {
        return;
    };
    if !sending_enabled(&app) || SWEEPING.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Ok(unsent) = crashes::list_unsent_reports(&root) {
            for report in unsent {
                if !sending_enabled(&app) {
                    break;
                }
                if let Err(error) = dispatch(&report).await {
                    eprintln!("crash report not sent: {error}");
                    break;
                }
                let _ = crashes::mark_report_sent(&root, &report.id);
            }
        }
        SWEEPING.store(false, Ordering::SeqCst);
    });
}

/// Posts one report to [`CRASH_ENDPOINT`].
async fn dispatch(report: &CrashReport) -> Result<(), String> {
    // reqwest is built without a bundled crypto provider, like the updater's.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("OpenRize/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())?;
    client
        .post(CRASH_ENDPOINT)
        .json(report)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map(|_| ())
        .map_err(|error| error.without_url().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_is_saved_with_its_location_and_without_its_message() {
        let root = std::env::temp_dir().join(format!("openrize-panic-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        set_crash_root(&root);
        init_panic_hook();

        let result = std::panic::catch_unwind(|| {
            panic!("could not save 'Client Acme' at https://bank.example/secret");
        });
        assert!(result.is_err());

        let reports = crashes::list_unsent_reports(&root).unwrap();
        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert_eq!(report.source, crashes::SOURCE_BACKEND_PANIC);
        assert!(report.message.starts_with("Rust panic at src/crashes.rs:"));
        let saved = std::fs::read_to_string(
            crashes::crashes_dir(&root).join(format!("{}.json", report.id)),
        )
        .unwrap();
        for leaked in ["Acme", "bank.example", "Captain", "Users"] {
            assert!(!saved.contains(leaked), "{leaked} in {saved}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_endpoint_is_https() {
        assert!(is_https(CRASH_ENDPOINT));
        assert!(!is_https("http://example.com"));
        assert!(!is_https("https://"));
    }
}
