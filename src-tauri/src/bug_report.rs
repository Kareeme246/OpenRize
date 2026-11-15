//! In-app bug reports: what a report can contain, and the one place it is sent.
//!
//! A report is what the user typed plus attachments they tick. Nothing is
//! collected or sent until they press Send, and the Rust side builds the
//! payload for both the preview the form shows and the request itself, from the
//! same draft, so the preview is exactly the bytes that go out.
//!
//! What can be attached, each on its own switch:
//! - the environment: app version, OS and its version, architecture, locale and
//!   which kind of folder the data lives in (never its path, which holds the
//!   user's name);
//! - the preferences: enum and number settings only. Nothing free-form (the AI
//!   prompt) and nothing that names a client, project or category;
//! - crash summaries, each chosen individually. A panic message can quote data
//!   it was handling, so they start unticked and the user reads the text first.
//!
//! Every report also carries a random id for this install ([`install_id`]),
//! which the endpoint uses to limit abuse and does not publish. The form says
//! so.
//!
//! Never read here: window titles, URLs, domains, entries, projects, clients,
//! the database or its path. The text the user types is sent as written; the
//! form says the report becomes a public GitHub issue.
//!
//! The only destination is [`ENDPOINT`], a constant fixed at build time, over
//! HTTPS, without following redirects. Its answer is not trusted either: it is
//! read up to a small cap, and the issue link it returns must be the issue of
//! that number in this project's repository ([`checked_response`]).
//!
//! The form lives in the main window, so the commands answer only that window
//! ([`ensure_main_window`]), and one run of the app sends at most
//! [`MAX_SENDS_PER_RUN`] reports, so a script in a webview cannot post in a
//! loop.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::AppState;

pub const ENDPOINT: &str = match option_env!("OPENRIZE_BUG_ENDPOINT") {
    Some(val) => val,
    None => "https://openrize-bug-report.kareeme246.workers.dev/report",
};

/// Same cap as the worker's, so a report is refused here rather than there.
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// The worker's cap on each text box. It counts as JavaScript's `length`
/// does, in UTF-16 code units, so [`validate`] counts those too: counting
/// `char`s would let through text with emoji that the worker then refuses.
pub const MAX_FIELD_CHARS: usize = 8000;

/// The most of the endpoint's answer that is read. A real one is under 200
/// bytes.
const MAX_RESPONSE_BYTES: usize = 4 * 1024;

/// An error message from the endpoint is cut here before it is shown.
const MAX_ERROR_MESSAGE_CHARS: usize = 200;

/// Reports one run of the app will send. The endpoint has its own limit (5 an
/// hour); this one holds whatever the endpoint does.
pub const MAX_SENDS_PER_RUN: u32 = 5;

/// Sends attempted in this run.
static SENDS: AtomicU32 = AtomicU32::new(0);

/// The window that holds the form.
const MAIN_WINDOW: &str = "main";

/// Crash summaries offered for attaching, newest first.
const OFFERED_CRASHES: usize = 5;

/// A crash message is cut here so a long dump cannot carry much user data.
const MAX_CRASH_MESSAGE_CHARS: usize = 300;

/// The persistent anonymous id, in the config folder (not in `settings.json`,
/// which the frontend writes back whole).
const INSTALL_ID_FILE: &str = "install-id";

/// Where the user's data lives, as a kind rather than a path.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DataFolder {
    /// The current default (Local AppData on Windows).
    Local,
    /// Roaming AppData, where earlier Windows builds kept it.
    LegacyRoaming,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub app_version: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub locale: String,
    pub data_folder: DataFolder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
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
pub struct CrashSummary {
    pub id: String,
    pub timestamp_ms: u64,
    pub app_version: String,
    pub source: String,
    /// First line only, with the user's folder names removed and cut short.
    pub message: String,
}

/// Everything that could be attached, as the form offers it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugDiagnostics {
    pub environment: Environment,
    pub preferences: Preferences,
    pub recent_crashes: Vec<CrashSummary>,
}

/// Which attachments the user ticked.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachments {
    #[serde(default)]
    pub environment: bool,
    #[serde(default)]
    pub preferences: bool,
    #[serde(default)]
    pub crash_ids: Vec<String>,
}

/// What the form holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportDraft {
    pub what_happened: String,
    pub what_expected: String,
    pub steps_to_reproduce: String,
    #[serde(default)]
    pub additional_notes: Option<String>,
    #[serde(default)]
    pub attachments: Attachments,
    /// The webview's language, e.g. `en-US`; only sent with the environment.
    #[serde(default)]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentDiagnostics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferences: Option<Preferences>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub recent_crashes: Vec<CrashSummary>,
}

/// The request body, byte for byte.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportPayload {
    pub install_id: String,
    pub what_happened: String,
    pub what_expected: String,
    pub steps_to_reproduce: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<SentDiagnostics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReportResponse {
    pub status: String,
    pub issue_number: u64,
    pub issue_url: String,
}

/// A user's profile folder inside a path or message: `C:\Users\name`,
/// `/Users/name`, `/home/name`. Matched without regard to case or slash
/// direction, so it holds where `dirs::home_dir` would not.
static PROFILE_FOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:[a-z]:)?[\\/](?:users|home)[\\/][^\\/\s"'<>|:]+"#)
        .expect("profile folder pattern is valid")
});

/// Replaces the user's profile folder with `~`, wherever it appears: first
/// their exact home folder (which may hold spaces), then any profile folder.
fn scrub(text: &str) -> String {
    let text = openrize_core::crashes::sanitize_text(text);
    PROFILE_FOLDER.replace_all(&text, "~").into_owned()
}

fn crash_message(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or("").trim();
    let scrubbed = scrub(first);
    if scrubbed.chars().count() > MAX_CRASH_MESSAGE_CHARS {
        let cut: String = scrubbed.chars().take(MAX_CRASH_MESSAGE_CHARS).collect();
        format!("{cut}…")
    } else {
        scrubbed
    }
}

fn os_version() -> String {
    #[cfg(windows)]
    {
        // `CurrentBuildNumber` is the build (26300); `DisplayVersion` the release (25H2).
        let key = windows_registry::LOCAL_MACHINE
            .open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .ok();
        let read = |name: &str| key.as_ref().and_then(|key| key.get_string(name).ok());
        match (read("DisplayVersion"), read("CurrentBuildNumber")) {
            (Some(version), Some(build)) => format!("{version} (build {build})"),
            (None, Some(build)) => format!("build {build}"),
            _ => std::env::consts::OS.to_string(),
        }
    }
    #[cfg(not(windows))]
    {
        crate::crashes::os_version()
    }
}

/// The folder crash reports are kept in: the one the app opened at startup,
/// so it follows the Local AppData move (one per install, not per account).
fn data_dir() -> Option<PathBuf> {
    crate::crashes::crash_root()
}

fn data_folder(dir: Option<&PathBuf>) -> DataFolder {
    let Some(dir) = dir else {
        return DataFolder::Other;
    };
    let under = |root: Option<PathBuf>| root.is_some_and(|root| dir.starts_with(root));
    if under(openrize_core::paths::local_data_dir()) {
        DataFolder::Local
    } else if under(openrize_core::paths::legacy_data_dir()) {
        DataFolder::LegacyRoaming
    } else {
        DataFolder::Other
    }
}

fn enum_name<T: std::fmt::Debug>(value: &T) -> String {
    format!("{value:?}").to_lowercase()
}

/// `text` when it is a version 4 uuid: random bits and nothing else. Earlier
/// builds wrote a version 7 one, which starts with the time it was made.
fn random_id(text: &str) -> Option<uuid::Uuid> {
    uuid::Uuid::parse_str(text)
        .ok()
        .filter(|id| id.get_version() == Some(uuid::Version::Random))
}

/// A random id for this install (a version 4 uuid), kept in the config
/// folder. It exists so the endpoint can limit reports per install, which
/// does not publish it; it names no one, says nothing about when it was made,
/// and is created the first time a report is previewed or sent. An id of any
/// other kind found on disk is replaced, never sent.
fn install_id() -> Result<String, String> {
    let path = openrize_core::settings::config_dir().join(INSTALL_ID_FILE);
    if let Ok(existing) = fs::read_to_string(&path) {
        if let Some(id) = random_id(existing.trim()) {
            return Ok(id.to_string());
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not save the install id: {e}"))?;
    }
    fs::write(&path, &id).map_err(|e| format!("could not save the install id: {e}"))?;
    Ok(id)
}

fn environment(locale: Option<&str>) -> Environment {
    Environment {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        os_version: os_version(),
        arch: std::env::consts::ARCH.to_string(),
        locale: locale
            .map(|l| l.chars().take(35).collect::<String>())
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| "unknown".to_string()),
        data_folder: data_folder(data_dir().as_ref()),
    }
}

fn preferences(app: &AppHandle) -> Preferences {
    let settings = app.state::<AppState>().settings_snapshot();
    Preferences {
        theme: enum_name(&settings.theme),
        accent: enum_name(&settings.accent),
        shape: enum_name(&settings.shape),
        size_mode: enum_name(&settings.size_mode),
        close_behavior: enum_name(&settings.close_behavior),
        tray_enabled: settings.tray_enabled,
        retention_days: settings.retention_days,
        ai_suggest: enum_name(&settings.ai_suggest),
        auto_accept: settings.auto_accept,
        auto_accept_percent: settings.auto_accept_percent,
        weekly_target_hours: settings.weekly_target_hours,
        tracking_hours_enabled: settings.tracking_hours.enabled,
        breaks_enabled: settings.breaks.enabled,
        stopwatch_reminder_enabled: settings.stopwatch_reminder.enabled,
        notification_placement: enum_name(&settings.notification_placement),
        crash_reporting: settings.crash_reporting,
    }
}

fn recent_crashes() -> Vec<CrashSummary> {
    let Some(dir) = data_dir() else {
        return Vec::new();
    };
    openrize_core::crashes::list_reports(&dir)
        .unwrap_or_default()
        .into_iter()
        .take(OFFERED_CRASHES)
        .map(|report| CrashSummary {
            id: report.id,
            timestamp_ms: report.timestamp_ms,
            app_version: report.app_version,
            source: report.source,
            message: crash_message(&report.message),
        })
        .collect()
}

/// Everything the form can offer to attach. Read-only, and nothing leaves the
/// machine.
pub fn collect_diagnostics(
    app: &AppHandle,
    locale: Option<String>,
) -> Result<BugDiagnostics, String> {
    Ok(BugDiagnostics {
        environment: environment(locale.as_deref()),
        preferences: preferences(app),
        recent_crashes: recent_crashes(),
    })
}

fn clean(text: &str) -> String {
    text.trim().to_string()
}

/// Builds the payload from the draft: required text present, only the ticked
/// attachments included, size within the cap. Used by the preview and by
/// [`submit_bug_report`], so they cannot differ.
pub fn build_payload(app: &AppHandle, draft: &BugReportDraft) -> Result<BugReportPayload, String> {
    let attachments = &draft.attachments;
    let diagnostics = SentDiagnostics {
        environment: attachments
            .environment
            .then(|| environment(draft.locale.as_deref())),
        preferences: attachments.preferences.then(|| preferences(app)),
        // Chosen from the list the form was shown, never by joining an id into
        // a path.
        recent_crashes: recent_crashes()
            .into_iter()
            .filter(|crash| attachments.crash_ids.contains(&crash.id))
            .collect(),
    };
    let has_diagnostics = diagnostics.environment.is_some()
        || diagnostics.preferences.is_some()
        || !diagnostics.recent_crashes.is_empty();

    let payload = BugReportPayload {
        install_id: install_id()?,
        what_happened: clean(&draft.what_happened),
        what_expected: clean(&draft.what_expected),
        steps_to_reproduce: clean(&draft.steps_to_reproduce),
        additional_notes: draft
            .additional_notes
            .as_deref()
            .map(clean)
            .filter(|notes| !notes.is_empty()),
        diagnostics: has_diagnostics.then_some(diagnostics),
    };
    validate(&payload)?;
    Ok(payload)
}

/// Required text present, each box within the worker's cap and the
/// serialized report within the size cap.
pub fn validate(payload: &BugReportPayload) -> Result<(), String> {
    if payload.what_happened.is_empty() {
        return Err("Please describe what happened.".to_string());
    }
    if payload.what_expected.is_empty() {
        return Err("Please describe what you expected to happen.".to_string());
    }
    if payload.steps_to_reproduce.is_empty() {
        return Err("Please provide steps to reproduce the issue.".to_string());
    }
    let boxes = [
        ("What happened", payload.what_happened.as_str()),
        ("What you expected", &payload.what_expected),
        ("Steps to reproduce", &payload.steps_to_reproduce),
        (
            "Additional context",
            payload.additional_notes.as_deref().unwrap_or(""),
        ),
    ];
    for (label, text) in boxes {
        let length = text.encode_utf16().count();
        if length > MAX_FIELD_CHARS {
            return Err(format!(
                "\"{label}\" is {length} characters, over the {MAX_FIELD_CHARS} character limit. Shorten it."
            ));
        }
    }
    let size = serde_json::to_vec(payload)
        .map_err(|e| format!("Failed to serialize bug report: {e}"))?
        .len();
    if size > MAX_PAYLOAD_BYTES {
        return Err(format!(
            "The report is {} KB, over the {} KB limit. Shorten the text or untick attachments.",
            size / 1024,
            MAX_PAYLOAD_BYTES / 1024
        ));
    }
    Ok(())
}

/// The destination, checked to be HTTPS before anything is sent. It is a
/// constant, so this guards a bad build-time override, not user input.
fn endpoint() -> Result<url::Url, String> {
    let url = url::Url::parse(ENDPOINT).map_err(|e| format!("Bad bug report endpoint: {e}"))?;
    if url.scheme() != "https" {
        return Err("The bug report endpoint must use https.".to_string());
    }
    Ok(url)
}

/// Refuses a caller that is not the main window. Commands are open to every
/// webview of the app, and the small panels (menu-bar pulse, break reminder)
/// have no business reading diagnostics or posting a report.
pub fn ensure_main_window(label: &str) -> Result<(), String> {
    if label == MAIN_WINDOW {
        Ok(())
    } else {
        Err("Bug reports can only be sent from the main window.".to_string())
    }
}

/// Counts one send against `limit`, or returns `false` when it is spent.
fn charge(sends: &AtomicU32, limit: u32) -> bool {
    sends
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |sent| {
            (sent < limit).then_some(sent + 1)
        })
        .is_ok()
}

/// The address of issue `number` in this project's repository.
fn issue_url(number: u64) -> String {
    format!(
        "https://github.com/{}/issues/{number}",
        crate::updater::REPOSITORY
    )
}

/// The endpoint's answer, if its link is exactly the issue it names. The form
/// opens that link in the browser, so a link to anywhere else is an error, not
/// something to show.
fn checked_response(response: BugReportResponse) -> Result<BugReportResponse, String> {
    if response.issue_url == issue_url(response.issue_number) {
        Ok(response)
    } else {
        Err("The bug report endpoint returned an unexpected issue link.".to_string())
    }
}

/// What to tell the user when the endpoint answers `status` with `body`.
fn error_message(status: reqwest::StatusCode, body: &[u8]) -> String {
    match status {
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            "Too many reports were sent from this computer or network in the last hour.".to_string()
        }
        reqwest::StatusCode::SERVICE_UNAVAILABLE => {
            "The bug report service is unavailable right now.".to_string()
        }
        _ => serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|json| {
                json.get("error")?
                    .as_str()
                    .map(str::trim)
                    .map(str::to_string)
            })
            .filter(|message| !message.is_empty())
            .map_or_else(
                || format!("Endpoint returned HTTP {status}"),
                |message| message.chars().take(MAX_ERROR_MESSAGE_CHARS).collect(),
            ),
    }
}

/// The response body, or `None` when it is longer than `cap` (the rest is not
/// read) or cannot be read.
async fn read_capped(mut response: reqwest::Response, cap: usize) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > cap {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

/// Sends the report to [`ENDPOINT`]: the one outbound request this module
/// makes, counted against [`MAX_SENDS_PER_RUN`] whatever comes of it.
pub async fn send(payload: &BugReportPayload) -> Result<BugReportResponse, String> {
    validate(payload)?;
    let body = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
    let url = endpoint()?;

    // reqwest is built without a bundled crypto provider, like the updater's.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("OpenRize/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    if !charge(&SENDS, MAX_SENDS_PER_RUN) {
        return Err(format!(
            "OpenRize sends at most {MAX_SENDS_PER_RUN} bug reports each time it runs. Restart it to send another."
        ));
    }
    // reqwest adds no `Origin` header, which the worker takes as the mark of a
    // web page and refuses.
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("Network error submitting report: {e}"))?;

    let status = response.status();
    let body = read_capped(response, MAX_RESPONSE_BYTES).await;
    if !status.is_success() {
        return Err(error_message(status, body.as_deref().unwrap_or_default()));
    }
    let body = body.ok_or("The bug report endpoint returned an unreadable response.")?;
    serde_json::from_slice(&body)
        .map_err(|e| format!("Invalid response from the bug report endpoint: {e}"))
        .and_then(checked_response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> BugReportPayload {
        BugReportPayload {
            install_id: "id".to_string(),
            what_happened: "Glitch on invoice".to_string(),
            what_expected: "Invoice renders clean".to_string(),
            steps_to_reproduce: "1. Open invoices".to_string(),
            additional_notes: None,
            diagnostics: None,
        }
    }

    #[test]
    fn a_complete_report_is_valid() {
        assert!(validate(&payload()).is_ok());
    }

    #[test]
    fn each_required_field_is_required() {
        for blank in 0..3 {
            let mut p = payload();
            match blank {
                0 => p.what_happened.clear(),
                1 => p.what_expected.clear(),
                _ => p.steps_to_reproduce.clear(),
            }
            assert!(validate(&p).is_err());
        }
    }

    #[test]
    fn an_oversized_report_is_refused_before_sending() {
        let mut p = payload();
        p.additional_notes = Some("x".repeat(70 * 1024));
        assert!(validate(&p).unwrap_err().contains("limit"));
    }

    #[test]
    fn each_text_box_is_capped_as_the_worker_counts_it() {
        let set: [fn(&mut BugReportPayload, String); 4] = [
            |p, text| p.what_happened = text,
            |p, text| p.what_expected = text,
            |p, text| p.steps_to_reproduce = text,
            |p, text| p.additional_notes = Some(text),
        ];
        for set in set {
            let mut p = payload();
            set(&mut p, "x".repeat(MAX_FIELD_CHARS));
            assert!(validate(&p).is_ok());
            set(&mut p, "x".repeat(MAX_FIELD_CHARS + 1));
            assert!(validate(&p).unwrap_err().contains("character limit"));
            // One emoji is one `char` and two UTF-16 code units, which is how
            // the worker counts: 4001 of them are 8002.
            set(&mut p, "\u{1F600}".repeat(MAX_FIELD_CHARS / 2));
            assert!(validate(&p).is_ok());
            set(&mut p, "\u{1F600}".repeat(MAX_FIELD_CHARS / 2 + 1));
            assert!(validate(&p).unwrap_err().contains("character limit"));
        }
    }

    #[test]
    fn only_the_main_window_may_use_the_form() {
        assert!(ensure_main_window("main").is_ok());
        for label in ["pulse", "reminder", "", "Main", "main2"] {
            assert!(ensure_main_window(label).is_err(), "{label}");
        }
    }

    #[test]
    fn a_run_sends_only_so_many_reports() {
        let sends = AtomicU32::new(0);
        for _ in 0..MAX_SENDS_PER_RUN {
            assert!(charge(&sends, MAX_SENDS_PER_RUN));
        }
        assert!(!charge(&sends, MAX_SENDS_PER_RUN));
        assert!(!charge(&sends, MAX_SENDS_PER_RUN));
        assert_eq!(sends.load(Ordering::SeqCst), MAX_SENDS_PER_RUN);
    }

    #[test]
    fn the_install_id_is_random_and_an_older_timestamped_one_is_not_kept() {
        let random = uuid::Uuid::new_v4().to_string();
        assert_eq!(random_id(&random).unwrap().to_string(), random);
        assert!(random_id(&uuid::Uuid::now_v7().to_string()).is_none());
        assert!(random_id("not an id").is_none());
        assert!(random_id("").is_none());
    }

    fn response(issue_number: u64, issue_url: &str) -> BugReportResponse {
        BugReportResponse {
            status: "created".to_string(),
            issue_number,
            issue_url: issue_url.to_string(),
        }
    }

    #[test]
    fn the_issue_link_must_be_that_issue_in_this_repository() {
        let ok = "https://github.com/Kareeme246/OpenRize/issues/42";
        assert_eq!(checked_response(response(42, ok)).unwrap().issue_url, ok);
        for url in [
            "https://github.com/Kareeme246/OpenRize/issues/43",
            "https://github.com/Kareeme246/OpenRize/issues/42/",
            "https://github.com/Kareeme246/OpenRize/issues/42?x=1",
            "https://github.com/Kareeme246/OpenRize/issues/42#frag",
            "https://github.com/Kareeme246/OpenRize/issues/042",
            "https://github.com/someone/else/issues/42",
            "https://github.com/Kareeme246/OpenRize/releases/download/v1/x.dmg",
            "https://github.com.evil.example/Kareeme246/OpenRize/issues/42",
            "http://github.com/Kareeme246/OpenRize/issues/42",
            " https://github.com/Kareeme246/OpenRize/issues/42",
            "",
        ] {
            assert!(checked_response(response(42, url)).is_err(), "{url}");
        }
    }

    #[test]
    fn an_endpoint_error_becomes_a_short_message() {
        use reqwest::StatusCode;
        let quoted = br#"{"error":"  Missing required fields.  "}"#;
        assert_eq!(
            error_message(StatusCode::BAD_REQUEST, quoted),
            "Missing required fields."
        );
        assert!(error_message(StatusCode::TOO_MANY_REQUESTS, quoted).contains("Too many reports"));
        assert!(error_message(StatusCode::SERVICE_UNAVAILABLE, b"").contains("unavailable"));
        for body in [&b""[..], b"<html>", br#"{"error":" "}"#, br#"{"error":5}"#] {
            assert_eq!(
                error_message(StatusCode::BAD_GATEWAY, body),
                "Endpoint returned HTTP 502 Bad Gateway"
            );
        }
        let long = format!(r#"{{"error":"{}"}}"#, "y".repeat(1000));
        assert_eq!(
            error_message(StatusCode::BAD_REQUEST, long.as_bytes())
                .chars()
                .count(),
            MAX_ERROR_MESSAGE_CHARS
        );
    }

    #[test]
    fn the_wire_format_omits_what_was_not_attached() {
        let json = serde_json::to_value(payload()).unwrap();
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert!(!keys.contains(&"diagnostics"));
        assert!(!keys.contains(&"additionalNotes"));
    }

    #[test]
    fn profile_folders_are_scrubbed_in_any_case_and_slash() {
        for input in [
            r"panic at C:\Users\Jane\proj\a.rs",
            r"panic at c:\users\jane\proj\a.rs",
            "panic at C:/Users/jane/proj/a.rs",
            "panic at /Users/jane/proj/a.rs",
            "panic at /home/jane/proj/a.rs",
        ] {
            let out = scrub(input);
            assert!(!out.to_lowercase().contains("jane"), "{input} -> {out}");
            assert!(out.contains('~'), "{out}");
        }
    }

    #[test]
    fn a_crash_message_is_one_short_line() {
        let long = format!("first line {}\nsecond line with data", "y".repeat(500));
        let message = crash_message(&long);
        assert!(!message.contains("second"));
        assert!(message.chars().count() <= MAX_CRASH_MESSAGE_CHARS + 1);
    }

    #[test]
    fn the_endpoint_is_https() {
        assert_eq!(endpoint().unwrap().scheme(), "https");
    }

    #[test]
    fn data_folder_names_a_kind_not_a_path() {
        assert_eq!(data_folder(None), DataFolder::Other);
        let local = openrize_core::paths::local_data_dir().unwrap();
        assert_eq!(
            data_folder(Some(&local.join("accounts").join("a"))),
            DataFolder::Local
        );
    }
}
