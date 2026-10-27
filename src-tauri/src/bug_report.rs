//! In-app bug report collector and dispatcher.
//!
//! Gathers sanitized diagnostics (environment info, non-sensitive settings,
//! and recent local crash reports) and submits user-initiated bug reports to
//! the Cloudflare Worker endpoint, which creates an issue on GitHub.
//!
//! Privacy & Security guarantees:
//! - Window titles, URLs, file paths outside app_data_dir, and time entries are NEVER collected.
//! - NO client secrets or tokens are compiled into the binary.
//! - Reports only submit on explicit user action ("Submit"), never automatically.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::AppState;

pub const DEFAULT_BUG_ENDPOINT: &str = match option_env!("OPENRIZE_BUG_ENDPOINT") {
    Some(val) => val,
    None => "https://openrize-bug-report.kareeme246.workers.dev/report",
};

pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024; // 64 KB cap

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSettings {
    pub theme: String,
    pub accent: String,
    pub shape: String,
    pub size_mode: String,
    pub close_behavior: String,
    pub tray_enabled: bool,
    pub retention_days: u32,
    pub ai_suggest: String,
    pub auto_accept: bool,
    pub auto_accept_percent: u8,
    pub weekly_target_hours: u16,
    pub tracking_hours_enabled: bool,
    pub breaks_enabled: bool,
    pub stopwatch_reminder_enabled: bool,
    pub notification_placement: String,
    pub crash_reporting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugDiagnostics {
    pub app_version: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub locale: String,
    pub app_data_dir: String,
    pub settings: DiagnosticSettings,
    pub recent_crashes: Vec<openrize_core::crashes::CrashReportSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportPayload {
    pub install_id: String,
    pub what_happened: String,
    pub what_expected: String,
    pub steps_to_reproduce: String,
    #[serde(default)]
    pub additional_notes: Option<String>,
    #[serde(default)]
    pub diagnostics: Option<BugDiagnostics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportResponse {
    pub status: String,
    pub issue_number: u64,
    pub issue_url: String,
}

/// Collects diagnostic state from the running app, sanitizing all paths.
pub fn collect_diagnostics(
    app: &AppHandle,
    locale: Option<String>,
) -> Result<BugDiagnostics, String> {
    let state = app.state::<AppState>();
    let settings = state.settings_snapshot();

    let app_data_dir = crate::crashes::get_crash_data_dir()
        .or_else(|| app.path().app_data_dir().ok())
        .map(|p| openrize_core::crashes::sanitize_text(&p.to_string_lossy()))
        .unwrap_or_default();

    let recent_crashes = if let Some(dir) =
        crate::crashes::get_crash_data_dir().or_else(|| app.path().app_data_dir().ok())
    {
        openrize_core::crashes::list_reports(&dir)
            .unwrap_or_default()
            .into_iter()
            .take(5)
            .collect()
    } else {
        Vec::new()
    };

    Ok(BugDiagnostics {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        os_version: crate::crashes::os_version_string(),
        arch: std::env::consts::ARCH.to_string(),
        locale: locale.unwrap_or_else(|| "en-US".to_string()),
        app_data_dir,
        settings: DiagnosticSettings {
            theme: format!("{:?}", settings.theme).to_lowercase(),
            accent: format!("{:?}", settings.accent).to_lowercase(),
            shape: format!("{:?}", settings.shape).to_lowercase(),
            size_mode: format!("{:?}", settings.size_mode).to_lowercase(),
            close_behavior: format!("{:?}", settings.close_behavior).to_lowercase(),
            tray_enabled: settings.tray_enabled,
            retention_days: settings.retention_days,
            ai_suggest: format!("{:?}", settings.ai_suggest).to_lowercase(),
            auto_accept: settings.auto_accept,
            auto_accept_percent: settings.auto_accept_percent,
            weekly_target_hours: settings.weekly_target_hours,
            tracking_hours_enabled: settings.tracking_hours.enabled,
            breaks_enabled: settings.breaks.enabled,
            stopwatch_reminder_enabled: settings.stopwatch_reminder.enabled,
            notification_placement: format!("{:?}", settings.notification_placement).to_lowercase(),
            crash_reporting: settings.crash_reporting,
        },
        recent_crashes,
    })
}

/// Validates payload limits and required content.
pub fn validate_payload(payload: &BugReportPayload) -> Result<String, String> {
    if payload.what_happened.trim().is_empty() {
        return Err("Please describe what happened.".to_string());
    }
    if payload.what_expected.trim().is_empty() {
        return Err("Please describe what you expected to happen.".to_string());
    }
    if payload.steps_to_reproduce.trim().is_empty() {
        return Err("Please provide steps to reproduce the issue.".to_string());
    }

    let serialized = serde_json::to_string(payload)
        .map_err(|e| format!("Failed to serialize bug report: {e}"))?;

    if serialized.len() > MAX_PAYLOAD_BYTES {
        return Err(format!(
            "Bug report payload ({} KB) exceeds 64 KB limit. Try shortening notes or excluding diagnostics.",
            serialized.len() / 1024
        ));
    }

    Ok(serialized)
}

/// Dispatches a bug report to the Cloudflare Worker endpoint.
pub async fn submit_bug_report(payload: &BugReportPayload) -> Result<BugReportResponse, String> {
    let serialized = validate_payload(payload)?;

    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("OpenRize/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let response = client
        .post(DEFAULT_BUG_ENDPOINT)
        .header("Content-Type", "application/json")
        .body(serialized)
        .send()
        .await
        .map_err(|e| format!("Network error submitting report: {e}"))?;

    let status = response.status();
    if status.is_success() {
        let parsed: BugReportResponse = response
            .json()
            .await
            .map_err(|e| format!("Invalid response format from bug report worker: {e}"))?;
        Ok(parsed)
    } else {
        let error_body = response.text().await.unwrap_or_default();
        let error_msg = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&error_body) {
            json.get("error")
                .and_then(|v| v.as_str())
                .unwrap_or(&error_body)
                .to_string()
        } else if !error_body.trim().is_empty() {
            error_body
        } else {
            format!("Endpoint returned HTTP {}", status)
        };
        Err(error_msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_payload_success() {
        let payload = BugReportPayload {
            install_id: "test-install-id".to_string(),
            what_happened: "Glitch on invoice".to_string(),
            what_expected: "Invoice renders clean".to_string(),
            steps_to_reproduce: "1. Open invoices\n2. Click preview".to_string(),
            additional_notes: Some("macOS 15".to_string()),
            diagnostics: None,
        };

        assert!(validate_payload(&payload).is_ok());
    }

    #[test]
    fn test_validate_payload_missing_fields() {
        let payload = BugReportPayload {
            install_id: "test-install-id".to_string(),
            what_happened: "".to_string(),
            what_expected: "something".to_string(),
            steps_to_reproduce: "step".to_string(),
            additional_notes: None,
            diagnostics: None,
        };

        assert!(validate_payload(&payload).is_err());
    }

    #[test]
    fn test_validate_payload_size_limit() {
        let payload = BugReportPayload {
            install_id: "test-install-id".to_string(),
            what_happened: "Crash".to_string(),
            what_expected: "No crash".to_string(),
            steps_to_reproduce: "Do something".to_string(),
            additional_notes: Some("x".repeat(70 * 1024)),
            diagnostics: None,
        };

        let res = validate_payload(&payload);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("exceeds 64 KB limit"));
    }
}
