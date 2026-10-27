//! Crash diagnostics model and local file persistence.
//!
//! Stores sanitized crash reports inside `<app_data_dir>/crashes/`.
//! All paths and personal identifiers are sanitized locally before saving,
//! preserving OpenRize's privacy guarantees.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

pub const CRASHES_DIR_NAME: &str = "crashes";
pub const MAX_RETAINED_CRASH_REPORTS: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    pub id: String,
    pub timestamp_ms: u64,
    pub app_version: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub source: String,
    pub message: String,
    pub backtrace: Option<String>,
    pub component_stack: Option<String>,
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub sent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReportSummary {
    pub id: String,
    pub timestamp_ms: u64,
    pub app_version: String,
    pub os: String,
    pub source: String,
    pub message: String,
}

impl From<&CrashReport> for CrashReportSummary {
    fn from(report: &CrashReport) -> Self {
        Self {
            id: report.id.clone(),
            timestamp_ms: report.timestamp_ms,
            app_version: report.app_version.clone(),
            os: report.os.clone(),
            source: report.source.clone(),
            message: report.message.clone(),
        }
    }
}

/// Sanitizes a string by replacing the user's home directory path with `~`.
pub fn sanitize_text(input: &str) -> String {
    let mut text = input.to_string();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if !home_str.is_empty() {
            text = text.replace(&*home_str, "~");
        }
    }
    text
}

/// Directory where crash reports are stored.
pub fn crashes_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(CRASHES_DIR_NAME)
}

/// Saves a crash report to disk, pruning older reports if the count exceeds
/// [`MAX_RETAINED_CRASH_REPORTS`].
pub fn save_report(app_data_dir: &Path, report: &CrashReport) -> io::Result<PathBuf> {
    let dir = crashes_dir(app_data_dir);
    fs::create_dir_all(&dir)?;

    let file_path = dir.join(format!("{}.json", report.id));
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    fs::write(&file_path, json)?;
    prune_old_reports(&dir, MAX_RETAINED_CRASH_REPORTS)?;

    Ok(file_path)
}

/// Lists all crash report summaries in descending order of timestamp.
pub fn list_reports(app_data_dir: &Path) -> io::Result<Vec<CrashReportSummary>> {
    let dir = crashes_dir(app_data_dir);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(report) = serde_json::from_str::<CrashReport>(&content) {
                    summaries.push(CrashReportSummary::from(&report));
                }
            }
        }
    }

    summaries.sort_by_key(|a| std::cmp::Reverse(a.timestamp_ms));
    Ok(summaries)
}

/// Reads a specific crash report by id.
pub fn get_report(app_data_dir: &Path, id: &str) -> io::Result<Option<CrashReport>> {
    let file_path = crashes_dir(app_data_dir).join(format!("{id}.json"));
    if !file_path.is_file() {
        return Ok(None);
    }

    let content = fs::read_to_string(file_path)?;
    let report = serde_json::from_str::<CrashReport>(&content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(report))
}

/// Lists all crash reports that have not yet been successfully dispatched.
pub fn list_unsent_reports(app_data_dir: &Path) -> io::Result<Vec<CrashReport>> {
    let dir = crashes_dir(app_data_dir);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut unsent = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(report) = serde_json::from_str::<CrashReport>(&content) {
                    if !report.sent {
                        unsent.push(report);
                    }
                }
            }
        }
    }

    unsent.sort_by_key(|a| a.timestamp_ms);
    Ok(unsent)
}

/// Marks a crash report as sent and updates it on disk.
pub fn mark_report_sent(app_data_dir: &Path, id: &str) -> io::Result<()> {
    if let Some(mut report) = get_report(app_data_dir, id)? {
        report.sent = true;
        let file_path = crashes_dir(app_data_dir).join(format!("{}.json", report.id));
        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&file_path, json)?;
    }
    Ok(())
}

/// Deletes a crash report by id.
pub fn delete_report(app_data_dir: &Path, id: &str) -> io::Result<bool> {
    let file_path = crashes_dir(app_data_dir).join(format!("{id}.json"));
    if file_path.is_file() {
        fs::remove_file(file_path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Deletes all crash reports in the crashes directory.
pub fn clear_reports(app_data_dir: &Path) -> io::Result<usize> {
    let dir = crashes_dir(app_data_dir);
    if !dir.is_dir() {
        return Ok(0);
    }

    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let _ = fs::remove_file(path);
            count += 1;
        }
    }
    Ok(count)
}

/// Removes the oldest crash reports so no more than `max_keep` files remain.
fn prune_old_reports(crashes_dir: &Path, max_keep: usize) -> io::Result<()> {
    let mut files = Vec::new();
    for entry in fs::read_dir(crashes_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    files.push((path, modified));
                }
            }
        }
    }

    if files.len() <= max_keep {
        return Ok(());
    }

    files.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (path, _) in files.into_iter().skip(max_keep) {
        let _ = fs::remove_file(path);
    }

    Ok(())
}

/// Current time in milliseconds since Unix epoch.
pub fn now_epoch_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_read_report() {
        let temp_dir =
            std::env::temp_dir().join(format!("openrize-crash-test-{}", uuid::Uuid::now_v7()));
        let _ = fs::create_dir_all(&temp_dir);
        let report = CrashReport {
            id: "test-crash-1".to_string(),
            timestamp_ms: 1000,
            app_version: "0.9.10".to_string(),
            os: "macos".to_string(),
            os_version: "15.0".to_string(),
            arch: "aarch64".to_string(),
            source: "backend_panic".to_string(),
            message: "panic message".to_string(),
            backtrace: Some("stack trace".to_string()),
            component_stack: None,
            metadata: None,
            sent: false,
        };

        save_report(&temp_dir, &report).unwrap();
        let list = list_reports(&temp_dir).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "test-crash-1");

        let unsent = list_unsent_reports(&temp_dir).unwrap();
        assert_eq!(unsent.len(), 1);
        assert_eq!(unsent[0].id, "test-crash-1");

        mark_report_sent(&temp_dir, "test-crash-1").unwrap();
        let unsent_after = list_unsent_reports(&temp_dir).unwrap();
        assert_eq!(unsent_after.len(), 0);

        let read = get_report(&temp_dir, "test-crash-1").unwrap();
        assert!(read.is_some());
        assert!(read.unwrap().sent);

        delete_report(&temp_dir, "test-crash-1").unwrap();
        assert_eq!(list_reports(&temp_dir).unwrap().len(), 0);
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
