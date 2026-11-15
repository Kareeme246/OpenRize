//! Crash reports: what is recorded when OpenRize fails, where it is kept, and
//! how it is cleaned before it is ever written down.
//!
//! # What a report may contain
//!
//! A report is **technical data about the program, never about the person
//! using it**. Everything in it is one of:
//!
//! - the app version, the OS name, its version and the CPU architecture;
//! - where the code failed: a source location (`src/activity/redact.rs:12:3`),
//!   a stack of function names, and a stack of UI component names;
//! - the *kind* of failure (`TypeError`, "Cannot read properties of
//!   undefined"), taken from a fixed list;
//! - an id, a timestamp and whether the report was sent.
//!
//! It never contains window titles, URLs, domains, descriptions, project or
//! client names, amounts, or any other text the user typed or the machine
//! captured, **including anything a private browsing window showed**
//! (`activity::redact`). A failure's own message is free text that can quote
//! such data (a panic that formats a title, a `JSON.parse` error that quotes
//! its input, a command error naming a client), so it is *not* recorded:
//!
//! - A Rust panic keeps only its source location. The payload is dropped.
//! - A UI error keeps its name and, only when it is one of the fixed
//!   messages in [`classify_js_message`], a generic description of it. Property
//!   and identifier names are dropped from that too. A rejection that is not
//!   an `Error` (the app's commands reject with plain strings that can name
//!   user data) is recorded as nothing but that fact.
//! - Stacks are rebuilt from scratch ([`scrub_js_stack`],
//!   [`scrub_component_stack`], [`scrub_native_backtrace`]). An error stack
//!   keeps only frames with a real `file:line:column`, and the file is cut
//!   down to its repository-relative or last path components, so a user name
//!   in a path cannot survive. A function or component name is kept only when
//!   it is made of ASCII identifier characters. Anything else is discarded
//!   rather than redacted. V8 repeats the error's message at the top of its
//!   stack; those lines are dropped because they are not frames, whatever the
//!   message passed along with the stack says. The one thing this cannot tell
//!   apart is a message line that is itself a complete frame, location and
//!   all, so the message is also cut out of the stack when it is found there.
//!
//! Rules like these cannot be bolted on at the point of sending, so they are
//! applied where a report is *built* ([`client_report`] for the UI, the panic
//! hook for Rust), and a file this app wrote is already clean. A file in the
//! folder is still not trusted: every report read back goes through
//! [`normalize`], which applies the same rules again and skips a file that
//! does not fit them, so a planted or hand-edited file cannot be sent or
//! offered to the bug report form as written.
//!
//! There is deliberately no free-form `metadata` field: a field that accepts
//! arbitrary JSON from the UI is a leak waiting for a caller.
//!
//! # Where reports live
//!
//! `<install root>/crashes/<id>.json`, where the install root is the folder the
//! app resolved at startup (`data_location::adopt`; Local AppData on Windows),
//! the parent of `accounts/`. Reports are **per install, not per local
//! account**: a crash is about the program, carries nothing from an account,
//! and happens before an account is chosen as often as after. Nothing here is
//! touched by account switching, which moves named files only
//! (`accounts.rs`), and the data-location migration treats `crashes` as one
//! more entry of the old folder.
//!
//! At most [`MAX_RETAINED_CRASH_REPORTS`] are kept. Nothing leaves the machine
//! from this module; sending is `src-tauri/src/crashes.rs` and only when the
//! user has turned `crashReporting` on.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

pub const CRASHES_DIR_NAME: &str = "crashes";
pub const MAX_RETAINED_CRASH_REPORTS: usize = 20;

/// Where a report came from. Anything else a caller claims becomes
/// [`SOURCE_FRONTEND_ERROR`], so the field can never carry free text.
pub const SOURCE_BACKEND_PANIC: &str = "backend_panic";
pub const SOURCE_FRONTEND_ERROR: &str = "frontend_error";
pub const SOURCE_UNHANDLED_REJECTION: &str = "unhandled_rejection";

const MAX_STACK_FRAMES: usize = 60;
const MAX_NAME_CHARS: usize = 80;
const MAX_FILE_CHARS: usize = 120;
/// Longest app version, OS name, OS version or architecture a report holds.
const MAX_ENVIRONMENT_CHARS: usize = 40;

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
    /// Function and file:line frames of the failing stack (Rust or UI).
    pub backtrace: Option<String>,
    /// The UI component names between the error and the root.
    pub component_stack: Option<String>,
    /// No longer waiting to be sent: delivered, or refused for good by the
    /// endpoint.
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

// --- Scrubbing ----------------------------------------------------------

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A source path as a stable, anonymous `crate/src/file.rs`: the part after
/// `registry/src/<index>/` for a dependency, after `src-tauri/` or `library/`
/// for ours and the standard library, and otherwise just the file name. A
/// build machine's user name, a checkout folder or `C:\Users\...` never
/// survives.
pub fn source_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches("//?/");
    let relative = if let Some(i) = path.find("/registry/src/") {
        let rest = &path[i + "/registry/src/".len()..];
        rest.split_once('/')
            .map_or(rest, |(_, crate_path)| crate_path)
    } else if let Some(i) = path.rfind("/src-tauri/") {
        &path[i + "/src-tauri/".len()..]
    } else if let Some(i) = path.rfind("/library/") {
        &path[i + 1..]
    } else if path.starts_with("library/") || path.starts_with("src/") {
        path
    } else {
        path.rsplit('/').next().unwrap_or(path)
    };
    let cleaned: String = relative.chars().filter(|c| is_path_char(*c)).collect();
    truncate_chars(&cleaned, MAX_FILE_CHARS)
}

fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '+')
}

/// A path read back from a stored report. What [`source_path`] wrote is kept
/// as it is: running it twice would cut `core/src/crashes.rs` down to its file
/// name. Anything else is a path this app did not write and is reduced.
fn stored_path(path: &str) -> String {
    let scrubbed = !path.starts_with('/')
        && !path.contains("..")
        && path.len() <= MAX_FILE_CHARS
        && path.chars().all(is_path_char);
    if scrubbed {
        path.to_string()
    } else {
        source_path(path)
    }
}

/// `path:line:column` from a Rust panic location, as [`source_path`] plus the
/// numbers.
pub fn panic_location(file: &str, line: u32, column: u32) -> String {
    format!("{}:{line}:{column}", source_path(file))
}

/// A Rust backtrace (`std::backtrace::Backtrace`'s text) reduced to numbered
/// symbol names and `at file:line:col` lines. Symbols are compiled-in names;
/// the file is passed through [`source_path`]. Any other line is dropped.
pub fn scrub_native_backtrace(backtrace: &str) -> Option<String> {
    native_frames(backtrace, source_path)
}

fn native_frames(backtrace: &str, path: fn(&str) -> String) -> Option<String> {
    let mut lines = Vec::new();
    for raw in backtrace.lines() {
        let line = raw.trim();
        if let Some(location) = line.strip_prefix("at ") {
            if let Some((file, line_no, column)) = split_location(location) {
                lines.push(format!("      at {}:{line_no}:{column}", path(file)));
            }
            continue;
        }
        let Some((number, symbol)) = line.split_once(": ") else {
            continue;
        };
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let symbol: String = symbol
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "_:<>{}()&*[]; ,$'#@.".contains(*c))
            .collect();
        lines.push(format!("{number}: {}", truncate_chars(&symbol, 200)));
        if lines.len() >= MAX_STACK_FRAMES * 2 {
            break;
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// `(path, line, column)` from `path:line:column`.
fn split_location(location: &str) -> Option<(&str, u32, u32)> {
    let (rest, column) = location.rsplit_once(':')?;
    let (file, line) = rest.rsplit_once(':')?;
    Some((file, line.parse().ok()?, column.parse().ok()?))
}

/// A frame's function or component name as an engine prints it, or `None`:
/// ASCII identifier characters and dots (`Object.<anonymous>`), after V8's
/// `async ` and `new ` and before its `[as alias]`, or one of WebKit's names
/// for code outside a function. Free text has spaces or other characters and
/// never passes.
fn js_frame_name(name: &str) -> Option<&str> {
    const WEBKIT_CODE: [&str; 3] = ["global code", "module code", "eval code"];
    let name = name.trim();
    if name.len() > MAX_NAME_CHARS {
        return None;
    }
    if WEBKIT_CODE.contains(&name) {
        return Some(name);
    }
    let bare = name.strip_prefix("async ").unwrap_or(name);
    let bare = bare.strip_prefix("new ").unwrap_or(bare);
    let (bare, alias) = bare
        .strip_suffix(']')
        .and_then(|bare| bare.split_once(" [as "))
        .unwrap_or((bare, bare));
    let identifier = |text: &str| {
        !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_$.<>".contains(&b))
    };
    (identifier(bare) && identifier(alias)).then_some(name)
}

/// One stack line as a clean frame, from V8's `at name (url:l:c)` or WebKit's
/// `name@url:l:c`. A frame with a location is kept, under `<anonymous>` when
/// its name does not pass [`js_frame_name`]. A frame without one is kept only
/// when `name_only` is set and its name passes.
fn js_frame(line: &str, name_only: bool) -> Option<String> {
    let line = line.trim();
    let (name, location) = if let Some(rest) = line.strip_prefix("at ") {
        match rest.strip_suffix(')').and_then(|r| r.rsplit_once(" (")) {
            Some((name, location)) => (name, js_location(location)),
            None => match js_location(rest.strip_prefix("async ").unwrap_or(rest)) {
                Some(location) => ("", Some(location)),
                None => (rest, None),
            },
        }
    } else if let Some((name, location)) = line.split_once('@') {
        (name, js_location(location))
    } else {
        return None;
    };
    match (js_frame_name(name), location) {
        (name, Some(location)) => {
            Some(format!("at {} ({location})", name.unwrap_or("<anonymous>")))
        }
        (Some(name), None) if name_only => Some(format!("at {name}")),
        _ => None,
    }
}

/// `file.js:line:col` from a URL or path with the same suffix; the folder,
/// origin and query are dropped.
fn js_location(location: &str) -> Option<String> {
    let (rest, column) = location.rsplit_once(':')?;
    let (file, line) = rest.rsplit_once(':')?;
    let (line, column): (u32, u32) = (line.parse().ok()?, column.parse().ok()?);
    let file = file.split(['?', '#']).next()?;
    let file = file.rsplit(['/', '\\']).next()?;
    let ok = !file.is_empty()
        && file.chars().count() <= MAX_FILE_CHARS
        && file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    ok.then(|| format!("{file}:{line}:{column}"))
}

fn js_frames(stack: &str, name_only: bool) -> Option<String> {
    let frames: Vec<String> = stack
        .lines()
        .filter_map(|line| js_frame(line, name_only))
        .take(MAX_STACK_FRAMES)
        .collect();
    (!frames.is_empty()).then(|| frames.join("\n"))
}

/// A JavaScript `Error.stack` reduced to the frames that have a real
/// `file:line:column`. V8 puts the error's message in the first lines of the
/// stack; a line of it is not a frame with a location and is dropped like any
/// other text. `message` is cut out first as well when the stack holds it, for
/// the one case that rule cannot see: a message line that is a complete frame.
pub fn scrub_js_stack(stack: &str, message: &str) -> Option<String> {
    let stack = if message.is_empty() {
        stack
    } else {
        stack
            .find(message)
            .map_or(stack, |i| &stack[i + message.len()..])
    };
    js_frames(stack, false)
}

/// A React component stack reduced to frames. It holds no message, and a host
/// component has a name and no location (`at div`), so a frame that is only a
/// name is kept here.
pub fn scrub_component_stack(stack: &str) -> Option<String> {
    js_frames(stack, true)
}

/// A fragment of a browser's message and the fixed description recorded for it.
const KNOWN_JS_MESSAGES: [(&str, &str); 14] = [
    (
        "Cannot read properties of undefined",
        "Cannot read properties of undefined",
    ),
    (
        "Cannot read properties of null",
        "Cannot read properties of null",
    ),
    (
        "Cannot set properties of undefined",
        "Cannot set properties of undefined",
    ),
    (
        "Cannot set properties of null",
        "Cannot set properties of null",
    ),
    ("is not a function", "A value is not a function"),
    ("is not defined", "A name is not defined"),
    ("is not iterable", "A value is not iterable"),
    ("is not a constructor", "A value is not a constructor"),
    (
        "Maximum call stack size exceeded",
        "Maximum call stack size exceeded",
    ),
    ("Invalid time value", "Invalid time value"),
    ("Too many re-renders", "Too many re-renders"),
    ("Minified React error", "Minified React error"),
    (
        "Rendered more hooks",
        "Rendered more hooks than previous render",
    ),
    ("Script error.", "Script error"),
];

/// A UI error's message turned into a fixed description, or `None`. Browser
/// messages are free text that quotes data (`JSON.parse` shows its input,
/// `Invalid regular expression` the pattern), so only these known shapes pass,
/// and the property or identifier they mention is dropped.
pub fn classify_js_message(message: &str) -> Option<&'static str> {
    KNOWN_JS_MESSAGES
        .iter()
        .find(|(needle, _)| message.contains(needle))
        .map(|(_, description)| *description)
}

/// An error's class name when it is a plain identifier, else `Error`.
fn error_name(name: Option<&str>) -> &str {
    match name {
        Some(name)
            if (1..=40).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphabetic()) =>
        {
            name
        }
        _ => "Error",
    }
}

/// Which build and machine a report is about.
#[derive(Debug, Clone)]
pub struct Environment {
    pub app_version: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
}

const NOT_AN_ERROR: &str = "Unhandled rejection of a value that is not an Error";

/// A report for a failure in the UI, built only from what
/// [`classify_js_message`], [`scrub_js_stack`] and the fixed sources allow. See
/// the module docs. `name` is `None` when what was thrown was not an `Error`.
pub fn client_report(
    source: &str,
    name: Option<&str>,
    message: Option<&str>,
    stack: Option<&str>,
    component_stack: Option<&str>,
    environment: &Environment,
) -> CrashReport {
    let source = match source {
        SOURCE_UNHANDLED_REJECTION => SOURCE_UNHANDLED_REJECTION,
        _ => SOURCE_FRONTEND_ERROR,
    };
    let message_text = message.unwrap_or("");
    let summary = match (name, classify_js_message(message_text)) {
        (None, _) if source == SOURCE_UNHANDLED_REJECTION => NOT_AN_ERROR.to_string(),
        (name, Some(description)) => format!("{}: {description}", error_name(name)),
        (name, None) => error_name(name).to_string(),
    };
    CrashReport {
        id: uuid::Uuid::now_v7().to_string(),
        timestamp_ms: now_epoch_ms(),
        app_version: environment.app_version.clone(),
        os: environment.os.clone(),
        os_version: environment.os_version.clone(),
        arch: environment.arch.clone(),
        source: source.to_string(),
        message: summary,
        backtrace: stack.and_then(|s| scrub_js_stack(s, message_text)),
        component_stack: component_stack.and_then(scrub_component_stack),
        sent: false,
    }
}

// --- Reading back ---------------------------------------------------------

/// An app version, OS name, OS version or architecture as this app writes
/// them (`0.9.10`, `macos`, `15.0.1`, `aarch64`): short, and nothing a
/// sentence or a forged log line could be made of.
fn is_environment_value(text: &str) -> bool {
    (1..=MAX_ENVIRONMENT_CHARS).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
}

/// A stored message rebuilt from the only shapes this module writes, so it
/// cannot carry text even when the file was edited.
fn normalize_message(source: &str, message: &str) -> String {
    if source == SOURCE_BACKEND_PANIC {
        return message
            .strip_prefix("Rust panic at ")
            .and_then(split_location)
            .map_or_else(
                || "Rust panic".to_string(),
                |(file, line, column)| {
                    format!("Rust panic at {}:{line}:{column}", stored_path(file))
                },
            );
    }
    if message == NOT_AN_ERROR && source == SOURCE_UNHANDLED_REJECTION {
        return NOT_AN_ERROR.to_string();
    }
    let (name, description) = message.split_once(": ").unwrap_or((message, ""));
    let name = error_name(Some(name));
    match KNOWN_JS_MESSAGES.iter().find(|(_, d)| *d == description) {
        Some((_, description)) => format!("{name}: {description}"),
        None => name.to_string(),
    }
}

/// A report read from `<file_id>.json`, held to the rules a report is built
/// under, or `None` when it is not one this app could have written: its id is
/// not the file's name, its source is not a known one, or its version, OS or
/// architecture is not a short plain value. The message and the stacks are
/// rebuilt, which leaves what this app wrote unchanged.
fn normalize(report: CrashReport, file_id: &str) -> Option<CrashReport> {
    let source = [
        SOURCE_BACKEND_PANIC,
        SOURCE_FRONTEND_ERROR,
        SOURCE_UNHANDLED_REJECTION,
    ]
    .into_iter()
    .find(|known| *known == report.source)?;
    let environment = [
        &report.app_version,
        &report.os,
        &report.os_version,
        &report.arch,
    ];
    if report.id != file_id
        || !is_report_id(file_id)
        || !environment.into_iter().all(|v| is_environment_value(v))
    {
        return None;
    }
    let backtrace = report.backtrace.as_deref().and_then(|stack| {
        if source == SOURCE_BACKEND_PANIC {
            native_frames(stack, stored_path)
        } else {
            scrub_js_stack(stack, "")
        }
    });
    Some(CrashReport {
        source: source.to_string(),
        message: normalize_message(source, &report.message),
        backtrace,
        component_stack: report
            .component_stack
            .as_deref()
            .and_then(scrub_component_stack),
        ..report
    })
}

// --- Admission ------------------------------------------------------------

/// Distinct failures recorded per run of the app.
pub const MAX_REPORTS_PER_SESSION: usize = 10;

/// Decides whether a report is worth recording this session: the same failure
/// twice, or a render loop throwing every frame, must not write a file and
/// make a request each time.
#[derive(Debug, Default)]
pub struct Admission {
    seen: Vec<u64>,
}

impl Admission {
    pub const fn new() -> Self {
        Self { seen: Vec::new() }
    }

    /// Whether `report` is new this session and the session's budget is left.
    pub fn admit(&mut self, report: &CrashReport) -> bool {
        let mut hasher = DefaultHasher::new();
        report.source.hash(&mut hasher);
        report.message.hash(&mut hasher);
        report.backtrace.hash(&mut hasher);
        report.component_stack.hash(&mut hasher);
        let signature = hasher.finish();
        if self.seen.contains(&signature) || self.seen.len() >= MAX_REPORTS_PER_SESSION {
            return false;
        }
        self.seen.push(signature);
        true
    }
}

/// Replaces this user's home folder with `~` wherever it appears in `text`,
/// for anything shown to or sent by the user (the bug report form).
pub fn sanitize_text(text: &str) -> String {
    match dirs::home_dir() {
        Some(home) if !home.as_os_str().is_empty() => text.replace(&*home.to_string_lossy(), "~"),
        _ => text.to_string(),
    }
}

// --- Files --------------------------------------------------------------

/// Directory where crash reports are stored under the install root.
pub fn crashes_dir(root: &Path) -> PathBuf {
    root.join(CRASHES_DIR_NAME)
}

/// Whether `id` is shaped like one of our ids (a uuid).
fn is_report_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A report's file, or `None` when `id` is not shaped like one of our ids, so
/// an id can never name a path outside the folder.
fn report_path(root: &Path, id: &str) -> Option<PathBuf> {
    is_report_id(id).then(|| crashes_dir(root).join(format!("{id}.json")))
}

/// The report in `path`, through [`normalize`]: `None` for a file that is not
/// a report this app could have written.
fn read_report(path: &Path) -> io::Result<Option<CrashReport>> {
    let report = serde_json::from_str::<CrashReport>(&fs::read_to_string(path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let file_id = path.file_stem().and_then(|stem| stem.to_str());
    Ok(file_id.and_then(|file_id| normalize(report, file_id)))
}

fn write_report(path: &Path, report: &CrashReport) -> io::Result<()> {
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    // Written whole, then renamed, so a process that dies mid-write (a panic
    // aborting the app) never leaves a half-file that hides the report.
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)?;
    fs::rename(&temp, path)
}

/// Saves a crash report to disk, pruning older reports if the count exceeds
/// [`MAX_RETAINED_CRASH_REPORTS`].
pub fn save_report(root: &Path, report: &CrashReport) -> io::Result<PathBuf> {
    let dir = crashes_dir(root);
    let path = report_path(root, &report.id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid report id"))?;
    fs::create_dir_all(&dir)?;
    write_report(&path, report)?;
    prune_old_reports(&dir, MAX_RETAINED_CRASH_REPORTS)?;
    Ok(path)
}

fn read_all(root: &Path) -> io::Result<Vec<CrashReport>> {
    let dir = crashes_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut reports = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(Some(report)) = read_report(&path) {
            reports.push(report);
        }
    }
    Ok(reports)
}

/// Lists all crash report summaries in descending order of timestamp.
pub fn list_reports(root: &Path) -> io::Result<Vec<CrashReportSummary>> {
    let mut summaries: Vec<CrashReportSummary> = read_all(root)?
        .iter()
        .map(CrashReportSummary::from)
        .collect();
    summaries.sort_by_key(|a| std::cmp::Reverse(a.timestamp_ms));
    Ok(summaries)
}

/// Reads a specific crash report by id.
pub fn get_report(root: &Path, id: &str) -> io::Result<Option<CrashReport>> {
    let Some(path) = report_path(root, id) else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    read_report(&path)
}

/// Lists all crash reports that have not yet been successfully dispatched,
/// oldest first.
pub fn list_unsent_reports(root: &Path) -> io::Result<Vec<CrashReport>> {
    let mut unsent: Vec<CrashReport> = read_all(root)?.into_iter().filter(|r| !r.sent).collect();
    unsent.sort_by_key(|a| a.timestamp_ms);
    Ok(unsent)
}

/// Takes a crash report out of the queue (see [`CrashReport::sent`]) and
/// updates it on disk.
pub fn mark_report_sent(root: &Path, id: &str) -> io::Result<()> {
    if let Some(mut report) = get_report(root, id)? {
        report.sent = true;
        if let Some(path) = report_path(root, &report.id) {
            write_report(&path, &report)?;
        }
    }
    Ok(())
}

/// Deletes a crash report by id.
pub fn delete_report(root: &Path, id: &str) -> io::Result<bool> {
    match report_path(root, id) {
        Some(path) if path.is_file() => {
            fs::remove_file(path)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Deletes all crash reports in the crashes directory.
pub fn clear_reports(root: &Path) -> io::Result<usize> {
    let dir = crashes_dir(root);
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json")
            && fs::remove_file(path).is_ok()
        {
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
            if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                files.push((path, modified));
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
    fn the_home_folder_is_replaced_with_a_tilde() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let text = format!("failed to open {}\\notes.txt", home.display());
        assert_eq!(sanitize_text(&text), "failed to open ~\\notes.txt");
        assert_eq!(sanitize_text("no paths here"), "no paths here");
    }

    fn temp_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openrize-crash-{name}-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn environment() -> Environment {
        Environment {
            app_version: "1.0.0".to_string(),
            os: "windows".to_string(),
            os_version: "10".to_string(),
            arch: "x86_64".to_string(),
        }
    }

    fn report(id: &str, timestamp_ms: u64) -> CrashReport {
        CrashReport {
            id: id.to_string(),
            timestamp_ms,
            app_version: "0.9.10".to_string(),
            os: "macos".to_string(),
            os_version: "15.0".to_string(),
            arch: "aarch64".to_string(),
            source: SOURCE_BACKEND_PANIC.to_string(),
            message: "Rust panic at src/lib.rs:1:1".to_string(),
            backtrace: Some("0: openrize::run".to_string()),
            component_stack: None,
            sent: false,
        }
    }

    fn ui_report(name: Option<&str>, message: &str, stack: &str) -> CrashReport {
        client_report(
            SOURCE_FRONTEND_ERROR,
            name,
            Some(message),
            Some(stack),
            None,
            &environment(),
        )
    }

    #[test]
    fn test_save_and_read_report() {
        let root = temp_root("roundtrip");
        save_report(&root, &report("test-crash-1", 1000)).unwrap();
        assert!(crashes_dir(&root).join("test-crash-1.json").is_file());
        let list = list_reports(&root).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "test-crash-1");

        assert_eq!(list_unsent_reports(&root).unwrap().len(), 1);
        mark_report_sent(&root, "test-crash-1").unwrap();
        assert!(list_unsent_reports(&root).unwrap().is_empty());
        assert!(get_report(&root, "test-crash-1").unwrap().unwrap().sent);

        assert!(delete_report(&root, "test-crash-1").unwrap());
        assert!(list_reports(&root).unwrap().is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn only_the_newest_twenty_reports_are_kept_and_no_temporary_file_stays() {
        let root = temp_root("prune");
        for i in 0..(MAX_RETAINED_CRASH_REPORTS + 5) {
            save_report(&root, &report(&format!("r{i}"), i as u64)).unwrap();
            // File times have coarse resolution on some file systems.
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let names: Vec<String> = fs::read_dir(crashes_dir(&root))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), MAX_RETAINED_CRASH_REPORTS);
        assert!(names.iter().all(|n| n.ends_with(".json")));
        assert!(get_report(&root, "r24").unwrap().is_some());
        assert!(get_report(&root, "r0").unwrap().is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_id_can_never_name_a_path_outside_the_folder() {
        let root = temp_root("ids");
        for id in ["../x", "..\\x", "a/b", "", "a.json"] {
            assert!(report_path(&root, id).is_none(), "{id}");
            assert!(get_report(&root, id).unwrap().is_none());
            assert!(!delete_report(&root, id).unwrap());
        }
        assert!(save_report(&root, &report("../escape", 1)).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_paths_lose_the_build_machines_user_and_checkout() {
        assert_eq!(
            source_path(
                r"C:\Users\Captain\Documents\Code\OpenRize\src-tauri\src\activity\redact.rs"
            ),
            "src/activity/redact.rs"
        );
        assert_eq!(
            source_path("/Users/jane/work/OpenRize/src-tauri/core/src/crashes.rs"),
            "core/src/crashes.rs"
        );
        assert_eq!(
            source_path(
                r"C:\Users\Captain\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\rusqlite-0.37.0\src\lib.rs"
            ),
            "rusqlite-0.37.0/src/lib.rs"
        );
        assert_eq!(
            source_path("/rustc/abc123/library/core/src/option.rs"),
            "library/core/src/option.rs"
        );
        assert_eq!(source_path("/home/jane/odd/place.rs"), "place.rs");
        assert_eq!(source_path(r"\\?\C:\Users\Captain\x\y.rs"), "y.rs");
        assert_eq!(
            panic_location(r"C:\Users\Captain\p\src-tauri\src\a.rs", 3, 9),
            "src/a.rs:3:9"
        );
    }

    #[test]
    fn a_native_backtrace_keeps_symbols_and_anonymous_locations_only() {
        let text = "   0: openrize_lib::activity::redact::sweep\n             at C:\\Users\\Captain\\Code\\OpenRize\\src-tauri\\src\\activity\\mod.rs:10:5\n   1: <unknown>\n   2: std::rt::lang_start\n             at /rustc/abc/library/std/src/rt.rs:199:18\nstray line with My Client Name\n";
        let scrubbed = scrub_native_backtrace(text).unwrap();
        assert!(scrubbed.contains("0: openrize_lib::activity::redact::sweep"));
        assert!(scrubbed.contains("at src/activity/mod.rs:10:5"));
        assert!(scrubbed.contains("at library/std/src/rt.rs:199:18"));
        assert!(!scrubbed.contains("Captain"));
        assert!(!scrubbed.contains("Client"));
        assert!(scrub_native_backtrace("nothing useful here").is_none());
    }

    #[test]
    fn a_ui_message_is_replaced_by_a_fixed_description_and_never_quoted() {
        let quoted = ui_report(
            Some("SyntaxError"),
            "Unexpected token 'A', \"Acme Corp - https://bank.example/secret\" is not valid JSON",
            "SyntaxError: Unexpected token\n    at parse (http://tauri.localhost/assets/index-AbC123.js:1:100)",
        );
        assert_eq!(quoted.message, "SyntaxError");

        let property = ui_report(
            Some("TypeError"),
            "Cannot read properties of undefined (reading 'Acme Corp')",
            "",
        );
        assert_eq!(
            property.message,
            "TypeError: Cannot read properties of undefined"
        );

        let named = ui_report(Some("TypeError"), "Project Falcon is not a function", "");
        assert_eq!(named.message, "TypeError: A value is not a function");
        assert!(!named.message.contains("Falcon"));
    }

    #[test]
    fn an_error_name_that_is_not_a_plain_identifier_is_dropped() {
        assert_eq!(
            ui_report(Some("Error: Client Acme"), "boom", "").message,
            "Error"
        );
        assert_eq!(
            ui_report(Some("QuotaExceededError"), "boom", "").message,
            "QuotaExceededError"
        );
    }

    #[test]
    fn a_rejection_that_is_not_an_error_records_only_that_fact() {
        let report = client_report(
            SOURCE_UNHANDLED_REJECTION,
            None,
            Some("Client \"Acme\" already exists"),
            None,
            None,
            &environment(),
        );
        assert_eq!(
            report.message,
            "Unhandled rejection of a value that is not an Error"
        );
        assert!(!serde_json::to_string(&report).unwrap().contains("Acme"));
    }

    #[test]
    fn the_source_is_one_of_the_known_values() {
        let report = client_report(
            "Acme Corp quarterly",
            None,
            None,
            None,
            None,
            &environment(),
        );
        assert_eq!(report.source, SOURCE_FRONTEND_ERROR);
    }

    #[test]
    fn a_ui_stack_keeps_frames_and_drops_the_message_and_origin() {
        let message = "Failed for client\n    at secret (https://evil.example/Acme.js:1:1)";
        let stack = format!(
            "Error: {message}\n    at renderRow (http://tauri.localhost/assets/index-AbC123.js:12:345)\n    at http://localhost:1420/src/components/Foo.tsx?t=17:45:6\n    at async load (C:\\Users\\Captain\\app\\x.js:1:2)\n    at weird name! (file.js:1:1)\n    at Object.<anonymous> (<anonymous>)"
        );
        let scrubbed = scrub_js_stack(&stack, message).unwrap();
        assert!(scrubbed.contains("at renderRow (index-AbC123.js:12:345)"));
        assert!(scrubbed.contains("at <anonymous> (Foo.tsx:45:6)"));
        assert!(scrubbed.contains("(x.js:1:2)"));
        assert!(scrubbed.contains("at <anonymous> (file.js:1:1)"));
        assert!(!scrubbed.contains("secret"));
        assert!(!scrubbed.contains("Captain"));
        assert!(!scrubbed.contains("tauri.localhost"));
        assert!(!scrubbed.contains("Acme"));
    }

    const SAVE_FRAME: &str = "    at save (http://tauri.localhost/assets/index-abc.js:10:5)";

    #[test]
    fn message_lines_in_a_v8_stack_are_dropped_whatever_message_is_passed() {
        let stack = format!(
            "Error: could not save entry\nat Acme Corp quarterly review\njane.doe@bank.example\n{SAVE_FRAME}"
        );
        // Chromium's `ErrorEvent.message` has a prefix the stack does not, so
        // the message is not found in the stack and cannot be cut out of it.
        let message =
            "Uncaught Error: could not save entry\nat Acme Corp quarterly review\njane.doe@bank.example";
        for message in [message, &message["Uncaught Error: ".len()..], ""] {
            assert_eq!(
                scrub_js_stack(&stack, message).unwrap(),
                "at save (index-abc.js:10:5)",
                "{message}"
            );
        }
    }

    #[test]
    fn a_single_line_message_leaves_no_frame_behind() {
        let stack = format!("Error: mail jane.doe@bank.example failed\n{SAVE_FRAME}");
        assert_eq!(
            scrub_js_stack(&stack, "Uncaught Error: mail jane.doe@bank.example failed").unwrap(),
            "at save (index-abc.js:10:5)"
        );
        assert!(scrub_js_stack("Error: nothing but a message", "").is_none());
    }

    #[test]
    fn a_frame_name_is_an_identifier_in_one_of_the_engines_forms() {
        let stack = "at async load (a.js:1:2)\nat new Store (a.js:3:4)\nat Object.<anonymous> (a.js:5:6)\nat Proxy.save [as run] (a.js:7:8)\nat async a.js:9:10\nglobal code@a.js:11:12\nat Acme Corp (a.js:13:14)\nat Zo\u{eb} (a.js:15:16)\nat forEach (native)\nat Object.<anonymous> (<anonymous>)";
        assert_eq!(
            scrub_js_stack(stack, "").unwrap(),
            "at async load (a.js:1:2)\nat new Store (a.js:3:4)\nat Object.<anonymous> (a.js:5:6)\nat Proxy.save [as run] (a.js:7:8)\nat <anonymous> (a.js:9:10)\nat global code (a.js:11:12)\nat <anonymous> (a.js:13:14)\nat <anonymous> (a.js:15:16)"
        );
    }

    #[test]
    fn a_webkit_stack_and_a_react_component_stack_parse_too() {
        let webkit = "renderRow@http://tauri.localhost/assets/index-X.js:3:4\n@http://tauri.localhost/assets/index-X.js:5:6\nforEach@[native code]";
        assert_eq!(
            scrub_js_stack(webkit, "").unwrap(),
            "at renderRow (index-X.js:3:4)\nat <anonymous> (index-X.js:5:6)"
        );
        let component = "\n    at Row (http://localhost:1420/src/Row.tsx:9:1)\n    at div\n    at App\n    at span (<anonymous>)\n    at Acme Corp\n    at Zo\u{eb}";
        assert_eq!(
            scrub_component_stack(component).unwrap(),
            "at Row (Row.tsx:9:1)\nat div\nat App\nat span"
        );
        // The same name-only frames are not frames of an error stack.
        assert_eq!(
            scrub_js_stack(component, "").unwrap(),
            "at Row (Row.tsx:9:1)"
        );
    }

    /// Writes `json` as `<name>.json` in the crash folder, as another program
    /// or a hand edit would.
    fn plant(root: &Path, name: &str, json: &serde_json::Value) {
        fs::create_dir_all(crashes_dir(root)).unwrap();
        fs::write(
            crashes_dir(root).join(format!("{name}.json")),
            json.to_string(),
        )
        .unwrap();
    }

    fn stored(id: &str) -> serde_json::Value {
        serde_json::to_value(report(id, 1)).unwrap()
    }

    #[test]
    fn a_report_this_app_wrote_reads_back_unchanged() {
        let root = temp_root("unchanged");
        let mut panic = report("panic", 1);
        panic.message = "Rust panic at core/src/crashes.rs:12:3".to_string();
        panic.os_version = "15.0.1".to_string();
        panic.backtrace = scrub_native_backtrace(
            "   0: openrize_core::crashes::save_report\n             at /Users/jane/OpenRize/src-tauri/core/src/crashes.rs:10:5\n   1: rusqlite::open\n             at /Users/jane/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rusqlite-0.37.0/src/lib.rs:1:2",
        );
        let mut ui = client_report(
            SOURCE_UNHANDLED_REJECTION,
            Some("TypeError"),
            Some("x is not a function"),
            Some("renderRow@http://tauri.localhost/assets/index-X.js:3:4"),
            Some("\n    at Row (http://localhost:1420/src/Row.tsx:9:1)\n    at div"),
            &environment(),
        );
        ui.os_version = "10.0.26300".to_string();
        let script_error = ui_report(None, "Script error.", "");
        let not_an_error = client_report(
            SOURCE_UNHANDLED_REJECTION,
            None,
            None,
            None,
            None,
            &environment(),
        );
        for report in [panic, ui, script_error, not_an_error] {
            save_report(&root, &report).unwrap();
            let read = get_report(&root, &report.id).unwrap().unwrap();
            assert_eq!(
                serde_json::to_value(&read).unwrap(),
                serde_json::to_value(&report).unwrap()
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_planted_file_is_skipped_when_it_is_not_a_report_this_app_wrote() {
        let root = temp_root("planted");
        plant(&root, "other-name", &stored("wrong-id"));
        let mut unknown_source = stored("unknown-source");
        unknown_source["source"] = "Acme Corp quarterly".into();
        plant(&root, "unknown-source", &unknown_source);
        let mut forged_version = stored("forged-version");
        forged_version["appVersion"] = "1.0.0\nreport stored: forged".into();
        plant(&root, "forged-version", &forged_version);
        let mut long_os = stored("long-os");
        long_os["osVersion"] = "1".repeat(MAX_ENVIRONMENT_CHARS + 1).into();
        plant(&root, "long-os", &long_os);
        plant(&root, "bad id", &stored("bad id"));
        plant(&root, "genuine", &stored("genuine"));

        let listed: Vec<String> = list_reports(&root)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(listed, ["genuine"]);
        let unsent: Vec<String> = list_unsent_reports(&root)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(unsent, ["genuine"]);
        for id in ["other-name", "wrong-id", "unknown-source", "forged-version"] {
            assert!(get_report(&root, id).unwrap().is_none(), "{id}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn free_text_in_a_planted_file_is_scrubbed_when_it_is_read() {
        let root = temp_root("free-text");
        let mut ui = stored("ui");
        ui["source"] = SOURCE_FRONTEND_ERROR.into();
        ui["message"] = "TypeError: Acme Corp owes 4000\nsecond line".into();
        ui["backtrace"] =
            "at Acme Corp quarterly review\njane.doe@bank.example\nat save (index-abc.js:10:5)"
                .into();
        ui["componentStack"] = "at Row (Row.tsx:9:1)\nat Acme Corp\nat div".into();
        plant(&root, "ui", &ui);
        let mut panic = stored("panic");
        panic["message"] = "Rust panic at /Users/jane/secret plan.rs:1:2".into();
        panic["backtrace"] = "0: openrize::run\n      at /Users/jane/OpenRize/src-tauri/src/lib.rs:1:2\nAcme Corp quarterly review".into();
        plant(&root, "panic", &panic);

        let ui = get_report(&root, "ui").unwrap().unwrap();
        assert_eq!(ui.message, "TypeError");
        assert_eq!(ui.backtrace.as_deref(), Some("at save (index-abc.js:10:5)"));
        assert_eq!(
            ui.component_stack.as_deref(),
            Some("at Row (Row.tsx:9:1)\nat div")
        );
        let panic = get_report(&root, "panic").unwrap().unwrap();
        assert_eq!(panic.message, "Rust panic at secretplan.rs:1:2");
        assert_eq!(
            panic.backtrace.as_deref(),
            Some("0: openrize::run\n      at src/lib.rs:1:2")
        );
        for report in list_unsent_reports(&root).unwrap() {
            let json = serde_json::to_string(&report).unwrap();
            for leaked in ["Acme", "jane", "bank", "4000"] {
                assert!(!json.contains(leaked), "{leaked} in {json}");
            }
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_session_admits_a_failure_once_and_only_so_many() {
        let mut admission = Admission::new();
        let first = report("a", 1);
        assert!(admission.admit(&first));
        assert!(!admission.admit(&first));
        for i in 0..20 {
            let mut other = report("x", 1);
            other.message = format!("different {i}");
            admission.admit(&other);
        }
        let mut late = report("late", 1);
        late.message = "never admitted".to_string();
        assert!(!admission.admit(&late));
    }

    #[test]
    fn a_report_serializes_without_any_free_form_field() {
        let json = serde_json::to_value(report("id-1", 5)).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "appVersion",
                "arch",
                "backtrace",
                "componentStack",
                "id",
                "message",
                "os",
                "osVersion",
                "sent",
                "source",
                "timestampMs"
            ]
        );
    }
}
