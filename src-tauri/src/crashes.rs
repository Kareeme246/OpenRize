//! Native and frontend crash diagnostics handler.
//!
//! Installs a panic hook to capture backend panics into local disk storage,
//! and provides functionality to dispatch sanitized crash reports to an opt-in
//! ingestion endpoint (e.g. Cloudflare Worker).

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use openrize_core::crashes::{self, CrashReport};

static CRASH_DATA_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Sets the active data directory where crash logs should be stored.
pub fn set_crash_data_dir(dir: &Path) {
    if let Ok(mut lock) = CRASH_DATA_DIR.write() {
        *lock = Some(dir.to_path_buf());
    }
}

/// Retrieves the active data directory, falling back to the standard app data dir.
pub fn get_crash_data_dir() -> Option<PathBuf> {
    CRASH_DATA_DIR
        .read()
        .ok()
        .and_then(|lock| lock.clone())
        .or_else(openrize_core::paths::app_data_dir)
}

/// Returns a readable OS version string.
fn os_version_string() -> String {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSProcessInfo;
        let info = NSProcessInfo::processInfo();
        let version = info.operatingSystemVersion();
        format!(
            "{}.{}.{}",
            version.majorVersion, version.minorVersion, version.patchVersion
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::consts::OS.to_string()
    }
}

/// Installs a global panic hook that writes a sanitized crash report before
/// chaining to the default hook.
pub fn init_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };

        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()));

        let backtrace = std::backtrace::Backtrace::capture();
        let backtrace_str = match backtrace.status() {
            std::backtrace::BacktraceStatus::Captured => Some(backtrace.to_string()),
            _ => None,
        };

        let sanitized_msg = crashes::sanitize_text(&message);
        let full_message = match location {
            Some(loc) => format!(
                "Panic at {}: {}",
                crashes::sanitize_text(&loc),
                sanitized_msg
            ),
            None => sanitized_msg,
        };

        let sanitized_backtrace = backtrace_str.map(|bt| crashes::sanitize_text(&bt));

        let report = CrashReport {
            id: uuid::Uuid::now_v7().to_string(),
            timestamp_ms: crashes::now_epoch_ms(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            os_version: os_version_string(),
            arch: std::env::consts::ARCH.to_string(),
            source: "backend_panic".to_string(),
            message: full_message,
            backtrace: sanitized_backtrace,
            component_stack: None,
            metadata: None,
            sent: false,
        };

        if let Some(dir) = get_crash_data_dir() {
            let _ = crashes::save_report(&dir, &report);
        }

        default_hook(panic_info);
    }));
}

pub const DEFAULT_CRASH_ENDPOINT: &str = match option_env!("OPENRIZE_CRASH_ENDPOINT") {
    Some(val) => val,
    None => "https://crashes.openrize.io/report",
};

/// Dispatches a sanitized crash report to the built-in ingestion endpoint.
pub async fn dispatch_crash_report(report: &CrashReport) -> Result<(), String> {
    if DEFAULT_CRASH_ENDPOINT.trim().is_empty() {
        return Err("No crash reporting endpoint configured".to_string());
    }

    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("OpenRize/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let response = client
        .post(DEFAULT_CRASH_ENDPOINT)
        .json(report)
        .send()
        .await
        .map_err(|e| format!("Network error sending crash report: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Crash ingestion endpoint returned status {}",
            response.status()
        ));
    }

    Ok(())
}

/// Dispatches a crash report asynchronously in the background, updating its `sent` status on success.
pub fn spawn_dispatch(dir: PathBuf, report: CrashReport) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = dispatch_crash_report(&report).await {
            eprintln!("crash report dispatch failed: {e}");
        } else {
            let _ = crashes::mark_report_sent(&dir, &report.id);
        }
    });
}

/// On app startup, checks for unsent crash reports (e.g. from previous backend panics)
/// and dispatches them in the background.
pub fn dispatch_pending_reports(dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        if let Ok(unsent) = crashes::list_unsent_reports(&dir) {
            for report in unsent {
                if let Err(e) = dispatch_crash_report(&report).await {
                    eprintln!("pending crash report dispatch failed: {e}");
                } else {
                    let _ = crashes::mark_report_sent(&dir, &report.id);
                }
            }
        }
    });
}

/// Creates a new `CrashReport` from client-side errors (React render errors or unhandled rejections).
pub fn create_client_report(
    source: String,
    message: String,
    component_stack: Option<String>,
    metadata: Option<serde_json::Value>,
) -> CrashReport {
    CrashReport {
        id: uuid::Uuid::now_v7().to_string(),
        timestamp_ms: crashes::now_epoch_ms(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        os_version: os_version_string(),
        arch: std::env::consts::ARCH.to_string(),
        source,
        message: crashes::sanitize_text(&message),
        backtrace: None,
        component_stack: component_stack.map(|cs| crashes::sanitize_text(&cs)),
        metadata,
        sent: false,
    }
}
