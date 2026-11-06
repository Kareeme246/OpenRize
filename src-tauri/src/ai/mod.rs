//! On-device AI categorization. A worker (`worker.rs`) runs each closed entry through T0 rules, T1
//! personal (kNN + Create ML via the sidecar) and T2 Foundation Model (absent without Apple
//! Intelligence; fallback T0+T1); an arbiter blends them. No network. Data: `openrize_core::ai`.

pub mod features;
pub mod metrics;
pub mod power;
pub mod sidecar;
pub mod worker;

pub use openrize_core::ai::*;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Emitted when an entry gets a fresh suggestion. Payload: `{ entryId }`.
pub const EVENT_SUGGESTION_READY: &str = "suggestion-ready";
/// Emitted whenever the engine status changes. Payload: `AiStatus`.
pub const EVENT_AI_STATUS_CHANGED: &str = "ai-status-changed";

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
    /// Verdicts calibration can learn from so far; see `COLD_START_OUTCOMES`.
    pub outcomes: u32,
    pub calibrated: bool,
    /// Personal-model retraining: `idle`, `queued` (Retrain now pressed), or
    /// `running`.
    pub retrain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrain_note: Option<String>,
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
            retrain: "idle".to_string(),
            retrain_note: None,
            last_error: None,
        }
    }
}

#[derive(Default)]
pub struct AiRuntime {
    status: Mutex<AiStatus>,
    wake: Mutex<bool>,
    wake_signal: Condvar,
    retrain_requested: AtomicBool,
    is_busy: AtomicBool,
    sidecar_pid: std::sync::atomic::AtomicU32,
}

impl AiRuntime {
    pub fn status(&self) -> AiStatus {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    pub fn is_busy(&self) -> bool {
        self.is_busy.load(Ordering::Relaxed)
    }

    pub fn set_busy(&self, busy: bool) {
        self.is_busy.store(busy, Ordering::Relaxed);
    }

    pub fn sidecar_pid(&self) -> Option<u32> {
        let pid = self.sidecar_pid.load(Ordering::Relaxed);
        if pid > 0 {
            Some(pid)
        } else {
            None
        }
    }

    pub fn set_sidecar_pid(&self, pid: Option<u32>) {
        self.sidecar_pid.store(pid.unwrap_or(0), Ordering::Relaxed);
    }

    pub fn nudge(&self) {
        if let Ok(mut woken) = self.wake.lock() {
            *woken = true;
            self.wake_signal.notify_one();
        }
    }

    /// Skips the idle/power gate and the new-label threshold.
    pub fn request_retrain(&self) {
        self.retrain_requested.store(true, Ordering::SeqCst);
        self.nudge();
    }

    fn take_retrain_request(&self) -> bool {
        self.retrain_requested.swap(false, Ordering::SeqCst)
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

pub fn nudge(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<AiRuntime>() {
        runtime.nudge();
    }
}
