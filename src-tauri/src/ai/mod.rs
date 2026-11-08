//! On-device AI categorization. A worker (`worker.rs`) runs each closed entry through T0 rules, T1
//! personal (kNN + Create ML via the sidecar) and T2 language model behind one `provider::Provider`
//! (Apple via the sidecar today, local llama-server models later); an arbiter blends them.

pub mod apple;
#[cfg(test)]
mod bench;
pub mod features;
pub mod metrics;
// Only the golden-set benchmark starts a local model until the runtime PR (row 3 of the delivery
// plan in docs/design/local-models.md) wires it into the worker.
#[cfg_attr(not(test), allow(dead_code))]
pub mod openai;
pub mod pipeline;
pub mod power;
pub mod provider;
#[cfg_attr(not(test), allow(dead_code))]
pub mod runtime;
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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StatusProvider {
    Apple,
    /// No model provider is installed (Windows, or a build without the sidecar).
    None,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LlmState {
    Ready,
    Unavailable,
}

/// Why the language model cannot run; the UI maps each to a sentence.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LlmReason {
    AppleIntelligenceNotEnabled,
    ModelNotReady,
    DeviceNotEligible,
    #[serde(rename = "unsupportedOS")]
    UnsupportedOs,
    Unknown,
}

impl LlmReason {
    fn from_code(code: &str) -> Self {
        match code {
            "appleIntelligenceNotEnabled" => Self::AppleIntelligenceNotEnabled,
            "modelNotReady" => Self::ModelNotReady,
            "deviceNotEligible" => Self::DeviceNotEligible,
            "unsupportedOS" => Self::UnsupportedOs,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EmbedState {
    Ready,
    None,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeState {
    Running,
    /// Idle-killed; restarts on demand.
    Stopped,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    /// `full` (T0+T1+T2), `fallback` (T0+T1), `rules` (no provider, T0
    /// only), or `starting` before the first probe.
    pub engine: String,
    pub provider: StatusProvider,
    pub llm: LlmState,
    /// Present only while `llm` is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_reason: Option<LlmReason>,
    pub embed: EmbedState,
    pub personal: bool,
    pub runtime: RuntimeState,
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

impl AiStatus {
    /// Sets the provider, LLM and embedder fields from what the provider reports; `None` is no
    /// provider at all.
    pub fn set_capabilities(&mut self, caps: Option<&provider::Capabilities>) {
        let Some(caps) = caps else {
            self.provider = StatusProvider::None;
            self.llm = LlmState::Unavailable;
            self.llm_reason = Some(LlmReason::UnsupportedOs);
            self.embed = EmbedState::None;
            return;
        };
        self.provider = StatusProvider::Apple;
        match &caps.llm {
            provider::Availability::Ready => {
                self.llm = LlmState::Ready;
                self.llm_reason = None;
            }
            provider::Availability::Unavailable(code) => {
                self.llm = LlmState::Unavailable;
                self.llm_reason = Some(LlmReason::from_code(code));
            }
        }
        self.embed = if caps.embed.is_some() {
            EmbedState::Ready
        } else {
            EmbedState::None
        };
    }
}

impl Default for AiStatus {
    fn default() -> Self {
        Self {
            engine: "starting".to_string(),
            provider: if cfg!(target_os = "macos") {
                StatusProvider::Apple
            } else {
                StatusProvider::None
            },
            llm: LlmState::Unavailable,
            llm_reason: Some(LlmReason::Unknown),
            embed: EmbedState::None,
            personal: false,
            runtime: RuntimeState::Stopped,
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

#[cfg(test)]
mod tests {
    use super::provider::{Availability, Capabilities, Egress, EmbedModel};
    use super::*;

    fn caps(llm: Availability, embed: bool) -> Capabilities {
        Capabilities {
            llm,
            embed: embed.then(|| EmbedModel {
                dimension: 512,
                model: "nlembedding-en-r2".to_string(),
            }),
            personal: true,
            egress: Egress::OnDevice,
        }
    }

    fn status_of(caps: Option<&Capabilities>) -> AiStatus {
        let mut status = AiStatus::default();
        status.set_capabilities(caps);
        status
    }

    #[test]
    fn a_ready_provider_has_no_reason() {
        let status = status_of(Some(&caps(Availability::Ready, true)));
        assert_eq!(status.provider, StatusProvider::Apple);
        assert_eq!(status.llm, LlmState::Ready);
        assert_eq!(status.llm_reason, None);
        assert_eq!(status.embed, EmbedState::Ready);
    }

    #[test]
    fn each_unavailable_code_maps_to_its_reason() {
        for (code, reason) in [
            (
                "appleIntelligenceNotEnabled",
                LlmReason::AppleIntelligenceNotEnabled,
            ),
            ("modelNotReady", LlmReason::ModelNotReady),
            ("deviceNotEligible", LlmReason::DeviceNotEligible),
            ("unsupportedOS", LlmReason::UnsupportedOs),
            ("somethingNew", LlmReason::Unknown),
        ] {
            let status = status_of(Some(&caps(
                Availability::Unavailable(code.to_string()),
                true,
            )));
            assert_eq!(status.llm, LlmState::Unavailable);
            assert_eq!(status.llm_reason, Some(reason), "{code}");
        }
    }

    #[test]
    fn no_embed_model_reports_no_embedder() {
        let status = status_of(Some(&caps(Availability::Ready, false)));
        assert_eq!(status.embed, EmbedState::None);
    }

    #[test]
    fn no_provider_reports_none_and_unsupported() {
        let status = status_of(None);
        assert_eq!(status.provider, StatusProvider::None);
        assert_eq!(status.llm_reason, Some(LlmReason::UnsupportedOs));
        assert_eq!(status.embed, EmbedState::None);
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let json = serde_json::to_value(status_of(None)).unwrap();
        assert_eq!(json["provider"], "none");
        assert_eq!(json["llm"], "unavailable");
        assert_eq!(json["llmReason"], "unsupportedOS");
        assert_eq!(json["embed"], "none");
        assert_eq!(json["runtime"], "stopped");
    }
}
