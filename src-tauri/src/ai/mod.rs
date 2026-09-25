//! On-device AI categorization (P2).
//!
//! Closed time entries are queued in `classify_jobs` and drained one at a
//! time by a background worker (`worker.rs`). Each entry runs through three
//! tiers, and an arbiter blends them into a calibrated confidence per field
//! (category and project):
//!
//! - **T0 rules** (`rules.rs`): user and app rules. A hit is p = 1.0.
//! - **T1 personal** (`knn.rs` + the sidecar): kNN over NLEmbedding vectors
//!   of approved entries, plus a Create ML text classifier trained on them.
//! - **T2 Foundation Model** (the sidecar): guided generation constrained to
//!   the live category and project lists. Absent without Apple Intelligence,
//!   in which case the engine runs in fallback (T0 + T1).
//!
//! Everything except the ML calls runs in Rust. The ML calls go to the Swift
//! sidecar `openrize-ml` over JSON lines on stdio (`sidecar.rs`). Nothing
//! here makes a network call.

pub mod arbiter;
pub mod features;
pub mod knn;
pub mod rules;
pub mod sidecar;
pub mod store;
pub mod worker;

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Emitted when an entry gets a fresh suggestion. Payload: `{ entryId }`.
pub const EVENT_SUGGESTION_READY: &str = "suggestion-ready";
/// Emitted whenever the engine status changes. Payload: `AiStatus`.
pub const EVENT_AI_STATUS_CHANGED: &str = "ai-status-changed";

pub const FIELD_CATEGORY: &str = "category";
pub const FIELD_PROJECT: &str = "project";

/// Suggestions at or above this confidence pre-fill the entry's field. Below
/// it the panel shows "Needs you" with nothing pre-selected.
pub const PREFILL_THRESHOLD: f64 = 0.60;
/// Until this many suggestions have a user outcome, displayed confidence is
/// capped at `COLD_START_CAP`, so nothing auto-approves during the first
/// week except deterministic rule hits.
pub const COLD_START_OUTCOMES: u32 = 50;
pub const COLD_START_CAP: f64 = 0.90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Category,
    Project,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Field::Category => FIELD_CATEGORY,
            Field::Project => FIELD_PROJECT,
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            FIELD_CATEGORY => Ok(Field::Category),
            FIELD_PROJECT => Ok(Field::Project),
            other => Err(format!("unknown suggestion field {other}")),
        }
    }
}

/// Engine health as the UI shows it (fallback banner, Settings).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    /// `full` (T0+T1+T2), `fallback` (T0+T1), `rules` (sidecar missing, T0
    /// only), or `starting` before the first probe.
    pub engine: String,
    /// Foundation Model availability as the sidecar reports it: `available`,
    /// `appleIntelligenceNotEnabled`, `modelNotReady`, `deviceNotEligible`,
    /// `unsupportedOS`, or `unknown`.
    pub llm: String,
    pub embed: bool,
    pub personal_model: bool,
    /// `running`, `stopped` (idle-killed, restarts on demand), or
    /// `unavailable`.
    pub sidecar: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    pub queued: u32,
    /// Suggestions with a user outcome so far; see `COLD_START_OUTCOMES`.
    pub outcomes: u32,
    pub calibrated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Default for AiStatus {
    fn default() -> Self {
        Self {
            engine: "starting".to_string(),
            llm: "unknown".to_string(),
            embed: false,
            personal_model: false,
            sidecar: "stopped".to_string(),
            os: None,
            queued: 0,
            outcomes: 0,
            calibrated: false,
            last_error: None,
        }
    }
}

/// Tauri-managed state shared by the worker thread and the commands.
#[derive(Default)]
pub struct AiRuntime {
    status: Mutex<AiStatus>,
    wake: Mutex<bool>,
    wake_signal: Condvar,
}

impl AiRuntime {
    pub fn status(&self) -> AiStatus {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    /// Wakes the worker so a freshly queued job starts now instead of at the
    /// next poll.
    pub fn nudge(&self) {
        if let Ok(mut woken) = self.wake.lock() {
            *woken = true;
            self.wake_signal.notify_one();
        }
    }

    fn wait(&self, timeout: Duration) {
        let Ok(woken) = self.wake.lock() else {
            std::thread::sleep(timeout);
            return;
        };
        if let Ok((mut woken, _)) = self
            .wake_signal
            .wait_timeout_while(woken, timeout, |woken| !*woken)
        {
            *woken = false;
        }
    }
}

/// Applies `change` to the status and emits `ai-status-changed` when it
/// actually changed.
pub fn update_status(app: &AppHandle, change: impl FnOnce(&mut AiStatus)) {
    let runtime = app.state::<AiRuntime>();
    let next = {
        let Ok(mut status) = runtime.status.lock() else {
            return;
        };
        let before = status.clone();
        change(&mut status);
        if *status == before {
            return;
        }
        status.clone()
    };
    let _ = app.emit(EVENT_AI_STATUS_CHANGED, &next);
}

/// Nudges the worker, if it is running. Safe to call from any command.
pub fn nudge(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<AiRuntime>() {
        runtime.nudge();
    }
}
