//! What `rize` asks the app, and the envelope every answer comes back in.
//! The same request runs in the app over IPC or, with the app closed, in
//! `rize` itself (`crate::rpc`), so both sides share these types.
//!
//! References to projects, clients, categories, timers and entries are what
//! the person typed (an id, a name or a short id); the app resolves them.
use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::{
    NewCategory, NewClient, NewProject, NewTimeEntry, UpdateCategory, UpdateClient, UpdateProject,
    UpdateTimeEntry,
};
use crate::EntryFilter;

/// Bumped whenever an operation or its answer changes incompatibly. 3: every
/// answer carrying a timer now describes its segments by `startedAt` and
/// `endedAt` (no `recordedAt`), so an app and a `rize` that disagree about
/// segments stop at the version check instead of misreading each other.
pub const PROTOCOL_VERSION: u32 = 3;
/// The `--json` envelope's version. Adding a field keeps it; removing or
/// renaming one bumps it. 3: a timer segment's `durationMs` and `recordedAt`
/// became `startedAt` and `endedAt` (`durationMs` stays, derived from them).
pub const SCHEMA_VERSION: u32 = 3;
/// Why a command that changes nothing destructive cannot take `--dry-run`;
/// the core and `rize` give the same words.
pub const DRY_RUN_UNSUPPORTED: &str = "--dry-run only previews commands that delete, reset or rebuild; this one would run for real. Run it without --dry-run.";
pub const MAX_REQUEST: usize = 1024 * 1024;
pub const MAX_RESPONSE: usize = 64 * 1024 * 1024;

/// Error codes, which also pick the exit code.
pub mod code {
    pub const INVALID_ARGUMENT: &str = "INVALID_ARGUMENT";
    pub const NOT_FOUND: &str = "NOT_FOUND";
    pub const AMBIGUOUS: &str = "AMBIGUOUS";
    pub const NO_DATA: &str = "NO_DATA";
    pub const APP_NOT_RUNNING: &str = "APP_NOT_RUNNING";
    pub const INCOMPATIBLE: &str = "INCOMPATIBLE";
    pub const FAILED: &str = "FAILED";
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub protocol_version: u32,
    pub operation: Operation,
}

impl Request {
    pub fn new(operation: Operation) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            operation,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Operation {
    /// The app's version and whether it is the running app.
    Hello {},
    Status {
        day_start: u64,
    },
    /// Shows the main window, on the review queue when `review` is set.
    AppOpen {
        review: bool,
    },
    AppQuit {},
    TrackSet {
        enabled: bool,
    },
    TrackIdle {
        minutes: u64,
    },
    FocusStart {
        label: Option<String>,
    },
    FocusStop {},
    TimersList {},
    TimerCreate {
        label: String,
    },
    TimerStart {
        timer: String,
    },
    TimerPause {
        timer: String,
    },
    TimerReset {
        timer: String,
    },
    TimerRename {
        timer: String,
        label: String,
    },
    TimerDelete {
        timer: String,
    },
    /// Puts a timer in the group `group`, or takes it out of its group when
    /// `group` is `None`.
    TimerGroup {
        timer: String,
        group: Option<String>,
    },
    /// Starts every paused timer in the group.
    TimerGroupStart {
        group: String,
    },
    /// Pauses every running timer in the group.
    TimerGroupPause {
        group: String,
    },
    /// One timer with its segments, each a start and an end.
    TimerSegments {
        timer: String,
    },
    /// Adds a segment from `started_at` to `ended_at` (epoch milliseconds,
    /// `ended_at >= started_at`).
    TimerSegmentAdd {
        timer: String,
        started_at: u64,
        ended_at: u64,
    },
    /// Changes a segment's start and/or end; the one left out stays.
    TimerSegmentEdit {
        timer: String,
        segment: i64,
        started_at: Option<u64>,
        ended_at: Option<u64>,
    },
    EntriesList {
        filter: EntryFilter,
        limit: u32,
    },
    EntryShow {
        entry: String,
    },
    EntryCreate {
        entry: NewTimeEntry,
    },
    EntriesEdit {
        entries: Vec<String>,
        patch: UpdateTimeEntry,
    },
    EntriesApprove {
        entries: Vec<String>,
    },
    EntriesUnapprove {
        entries: Vec<String>,
    },
    EntryReject {
        entry: String,
    },
    EntrySplit {
        entry: String,
        at: u64,
    },
    EntryMerge {
        entry: String,
        other: String,
    },
    EntriesDelete {
        entries: Vec<String>,
    },
    EntriesRebuild {
        from: u64,
        to: u64,
    },
    EntriesExport {
        filter: EntryFilter,
        format: String,
    },
    Report {
        filter: EntryFilter,
        boundaries: Vec<u64>,
        group_by: String,
    },
    ProjectsList {},
    ProjectShow {
        project: String,
        range_start: u64,
        range_end: u64,
        month_start: u64,
    },
    ProjectCreate {
        project: NewProject,
    },
    ProjectEdit {
        project: String,
        patch: UpdateProject,
    },
    ProjectDelete {
        project: String,
    },
    ClientsList {},
    ClientShow {
        client: String,
    },
    ClientCreate {
        client: NewClient,
    },
    ClientEdit {
        client: String,
        patch: UpdateClient,
    },
    ClientDelete {
        client: String,
    },
    CategoriesList {},
    CategoryCreate {
        category: NewCategory,
    },
    CategoryEdit {
        category: String,
        patch: UpdateCategory,
    },
    CategoryDelete {
        category: String,
    },
    SettingsGet {},
    /// One field by its camelCase path, such as `breaks.enabled`.
    SettingsSet {
        key: String,
        value: Value,
    },
    Paths {},
    /// What `operation` would change, answered without changing anything.
    /// Only the commands `rize` confirms (`TimerReset`, `TimerDelete`,
    /// `EntriesDelete`, `EntriesRebuild`, `ProjectDelete`, `ClientDelete`,
    /// `CategoryDelete`) can be previewed; anything else, or a nested
    /// preview, is `INVALID_ARGUMENT`. This adds an operation and changes no
    /// existing request or answer. (The protocol version moved to 3 for the
    /// segment change, not for this.)
    Preview {
        operation: Box<Operation>,
    },
}

impl Operation {
    /// Needs the live app: there is nothing to do with only the stored data.
    pub fn needs_app(&self) -> bool {
        matches!(
            self,
            Self::AppOpen { .. } | Self::AppQuit {} | Self::FocusStart { .. } | Self::FocusStop {}
        )
    }
}

/// The answer to `Hello`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub app_version: String,
    pub protocol_version: u32,
    /// The answer came from the running app, not from rize on its own.
    pub running: bool,
    pub data_dir: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub schema_version: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: String,
    pub message: String,
    /// What an ambiguous reference could have meant.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
}

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            candidates: Vec::new(),
        }
    }
}

impl Response {
    pub fn success(data: impl Serialize) -> Self {
        match serde_json::to_value(data) {
            Ok(data) => Self {
                schema_version: SCHEMA_VERSION,
                ok: true,
                data: Some(data),
                error: None,
            },
            Err(error) => Self::error(code::FAILED, error.to_string()),
        }
    }

    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self::failure(ApiError::new(code, message))
    }

    pub fn failure(error: ApiError) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ok: false,
            data: None,
            error: Some(error),
        }
    }

    pub fn code(&self) -> Option<&str> {
        self.error.as_ref().map(|error| error.code.as_str())
    }
}

pub fn write_frame<T: Serialize>(
    writer: &mut impl Write,
    value: &T,
    max: usize,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err("IPC payload exceeds limit".into());
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .map_err(|e| e.to_string())?;
    writer.write_all(&bytes).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

pub fn read_frame<T: for<'de> Deserialize<'de>>(
    reader: &mut impl Read,
    max: usize,
) -> Result<T, String> {
    let mut header = [0; 4];
    reader.read_exact(&mut header).map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 || len > max {
        return Err("IPC payload exceeds limit".into());
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
