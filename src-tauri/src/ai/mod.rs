//! On-device AI categorization: the worker runs each closed entry through T0 rules, T1 personal
//! (kNN + Create ML via the sidecar) and the one T2 language model `Settings::ai_llm` picks (Apple's
//! via the sidecar, or a GGUF on our loopback `llama-server`); an arbiter blends them.

pub mod apple;
#[cfg(test)]
mod bench;
pub mod features;
pub mod guard;
pub mod metrics;
pub mod openai;
pub mod pipeline;
pub mod power;
pub mod provider;
pub mod runtime;
pub mod sidecar;
pub mod worker;

pub use openrize_core::ai::*;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use self::runtime::{Device, DeviceInfo, DeviceReason, RuntimeCheck};

/// Emitted when an entry gets a fresh suggestion. Payload: `{ entryId }`.
pub const EVENT_SUGGESTION_READY: &str = "suggestion-ready";
/// Emitted whenever the engine status changes. Payload: `AiStatus`.
pub const EVENT_AI_STATUS_CHANGED: &str = "ai-status-changed";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StatusProvider {
    Apple,
    /// A GGUF model run by the bundled `llama-server`.
    Local,
    /// No language model: none chosen, or no provider is installed (Windows, or a build without the sidecar).
    None,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LlmState {
    Ready,
    /// The runtime is starting or the model is loading.
    Loading,
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
    /// No local model is selected, or its file is not in the models folder.
    ModelMissing,
    /// The bundled `llama-server` is not next to the app.
    RuntimeMissing,
    /// The server failed to start on the GPU and on the CPU.
    RuntimeFailed,
    Unknown,
}

impl LlmReason {
    fn from_code(code: &str) -> Self {
        match code {
            "appleIntelligenceNotEnabled" => Self::AppleIntelligenceNotEnabled,
            "modelNotReady" => Self::ModelNotReady,
            "deviceNotEligible" => Self::DeviceNotEligible,
            "unsupportedOS" => Self::UnsupportedOs,
            "modelMissing" => Self::ModelMissing,
            "runtimeMissing" => Self::RuntimeMissing,
            "runtimeFailed" => Self::RuntimeFailed,
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

/// The language model half of the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlmView {
    pub provider: StatusProvider,
    pub state: LlmState,
    pub reason: Option<LlmReason>,
}

impl LlmView {
    pub fn apple(caps: &provider::Capabilities) -> Self {
        match &caps.llm {
            provider::Availability::Ready => Self {
                provider: StatusProvider::Apple,
                state: LlmState::Ready,
                reason: None,
            },
            provider::Availability::Unavailable(code) => Self {
                provider: StatusProvider::Apple,
                state: LlmState::Unavailable,
                reason: Some(LlmReason::from_code(code)),
            },
        }
    }

    /// Apple's model was chosen but this build has no sidecar, or nothing runs on this platform.
    pub fn unsupported() -> Self {
        Self {
            provider: StatusProvider::None,
            state: LlmState::Unavailable,
            reason: Some(LlmReason::UnsupportedOs),
        }
    }

    /// `ai_llm = none`. Windows has no other choice yet, so it keeps saying "unsupported".
    pub fn chosen_none() -> Self {
        Self {
            reason: if cfg!(target_os = "macos") {
                None
            } else {
                Some(LlmReason::UnsupportedOs)
            },
            ..Self::unsupported()
        }
    }

    /// `check` is `None` when no usable local model is selected.
    pub fn local(check: Option<RuntimeCheck>, loading: bool) -> Self {
        let unavailable = |reason| Self {
            provider: StatusProvider::Local,
            state: LlmState::Unavailable,
            reason: Some(reason),
        };
        match check {
            None | Some(RuntimeCheck::ModelMissing) => unavailable(LlmReason::ModelMissing),
            Some(RuntimeCheck::RuntimeMissing) => unavailable(LlmReason::RuntimeMissing),
            Some(RuntimeCheck::Failed { .. }) => unavailable(LlmReason::RuntimeFailed),
            Some(RuntimeCheck::Stopped | RuntimeCheck::Running) => Self {
                provider: StatusProvider::Local,
                state: if loading {
                    LlmState::Loading
                } else {
                    LlmState::Ready
                },
                reason: None,
            },
        }
    }
}

/// What the local runtime row of the status says.
pub fn local_runtime_state(check: Option<RuntimeCheck>) -> RuntimeState {
    match check {
        Some(RuntimeCheck::Running) => RuntimeState::Running,
        Some(RuntimeCheck::RuntimeMissing | RuntimeCheck::Failed { .. }) => {
            RuntimeState::Unavailable
        }
        Some(RuntimeCheck::Stopped | RuntimeCheck::ModelMissing) | None => RuntimeState::Stopped,
    }
}

/// `full` (T0+T1+T2) when the language model is ready, `fallback` (T0+T1) when the personal tier
/// runs without it, else `rules`.
pub fn engine_for(llm: LlmState, embed: EmbedState) -> &'static str {
    if llm == LlmState::Ready {
        "full"
    } else if embed == EmbedState::Ready {
        "fallback"
    } else {
        "rules"
    }
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
    /// Where the local model runs; absent while no local runtime runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
    /// Present when `device` is the CPU only because the GPU failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_reason: Option<DeviceReason>,
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
    pub fn set_llm(&mut self, view: LlmView) {
        self.provider = view.provider;
        self.llm = view.state;
        self.llm_reason = view.reason;
    }

    pub fn set_device(&mut self, info: Option<DeviceInfo>) {
        self.device = info.map(|info| info.device);
        self.device_reason = info.and_then(|info| info.reason);
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
            device: None,
            device_reason: None,
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
    local_pid: std::sync::atomic::AtomicU32,
    shutdown: AtomicBool,
    worker_stopped: Mutex<bool>,
    worker_stopped_signal: Condvar,
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

    pub fn local_pid(&self) -> Option<u32> {
        let pid = self.local_pid.load(Ordering::Relaxed);
        (pid > 0).then_some(pid)
    }

    pub fn set_local_pid(&self, pid: Option<u32>) {
        self.local_pid.store(pid.unwrap_or(0), Ordering::Relaxed);
    }

    /// Processes the AI engine runs, for energy attribution.
    pub fn helper_pids(&self) -> impl Iterator<Item = u32> {
        [self.sidecar_pid(), self.local_pid()].into_iter().flatten()
    }

    pub fn shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// The worker stops its model processes and confirms.
    pub fn mark_worker_stopped(&self) {
        if let Ok(mut stopped) = self.worker_stopped.lock() {
            *stopped = true;
            self.worker_stopped_signal.notify_all();
        }
    }

    /// Asks the worker to stop its model processes and waits briefly for it. A worker in the
    /// middle of a job may not answer in time; the process guards in `guard.rs` cover that.
    pub fn shutdown(&self, timeout: Duration) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.nudge();
        if let Ok(stopped) = self.worker_stopped.lock() {
            let _ = self
                .worker_stopped_signal
                .wait_timeout_while(stopped, timeout, |stopped| !*stopped);
        }
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

    /// What the worker publishes for Apple's model: the sidecar's view of both halves.
    fn status_of(caps: Option<&Capabilities>) -> AiStatus {
        let mut status = AiStatus::default();
        match caps {
            Some(caps) => {
                status.set_llm(LlmView::apple(caps));
                status.embed = if caps.embed.is_some() {
                    EmbedState::Ready
                } else {
                    EmbedState::None
                };
            }
            None => {
                status.set_llm(LlmView::unsupported());
                status.embed = EmbedState::None;
            }
        }
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
    fn a_local_model_maps_every_runtime_state() {
        let failed = RuntimeCheck::Failed { retry_due: true };
        for (check, loading, state, reason, runtime) in [
            (
                Some(RuntimeCheck::Stopped),
                false,
                LlmState::Ready,
                None,
                RuntimeState::Stopped,
            ),
            (
                Some(RuntimeCheck::Running),
                false,
                LlmState::Ready,
                None,
                RuntimeState::Running,
            ),
            (
                Some(RuntimeCheck::Stopped),
                true,
                LlmState::Loading,
                None,
                RuntimeState::Stopped,
            ),
            (
                Some(RuntimeCheck::ModelMissing),
                false,
                LlmState::Unavailable,
                Some(LlmReason::ModelMissing),
                RuntimeState::Stopped,
            ),
            (
                None,
                false,
                LlmState::Unavailable,
                Some(LlmReason::ModelMissing),
                RuntimeState::Stopped,
            ),
            (
                Some(RuntimeCheck::RuntimeMissing),
                false,
                LlmState::Unavailable,
                Some(LlmReason::RuntimeMissing),
                RuntimeState::Unavailable,
            ),
            (
                Some(failed),
                false,
                LlmState::Unavailable,
                Some(LlmReason::RuntimeFailed),
                RuntimeState::Unavailable,
            ),
        ] {
            let view = LlmView::local(check, loading);
            assert_eq!(view.provider, StatusProvider::Local, "{check:?}");
            assert_eq!(view.state, state, "{check:?} loading={loading}");
            assert_eq!(view.reason, reason, "{check:?}");
            assert_eq!(local_runtime_state(check), runtime, "{check:?}");
        }
    }

    #[test]
    fn the_engine_follows_the_language_model_and_the_embedder() {
        assert_eq!(engine_for(LlmState::Ready, EmbedState::Ready), "full");
        assert_eq!(engine_for(LlmState::Ready, EmbedState::None), "full");
        assert_eq!(engine_for(LlmState::Loading, EmbedState::Ready), "fallback");
        assert_eq!(
            engine_for(LlmState::Unavailable, EmbedState::Ready),
            "fallback"
        );
        assert_eq!(engine_for(LlmState::Unavailable, EmbedState::None), "rules");
        assert_eq!(engine_for(LlmState::Loading, EmbedState::None), "rules");
    }

    #[test]
    fn choosing_none_is_a_provider_none_without_a_platform_reason_on_a_mac() {
        let view = LlmView::chosen_none();
        assert_eq!(view.provider, StatusProvider::None);
        assert_eq!(view.state, LlmState::Unavailable);
        assert_eq!(
            view.reason,
            (!cfg!(target_os = "macos")).then_some(LlmReason::UnsupportedOs)
        );
    }

    #[test]
    fn the_device_is_reported_only_while_a_local_runtime_runs() {
        let mut status = AiStatus::default();
        let json = serde_json::to_value(&status).unwrap();
        assert!(json.get("device").is_none() && json.get("deviceReason").is_none());

        status.set_device(Some(DeviceInfo {
            device: Device::Gpu,
            reason: None,
        }));
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["device"], "gpu");
        assert!(json.get("deviceReason").is_none());

        status.set_device(Some(DeviceInfo {
            device: Device::Cpu,
            reason: Some(DeviceReason::GpuFailed),
        }));
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["device"], "cpu");
        assert_eq!(json["deviceReason"], "gpuFailed");

        status.set_device(None);
        assert_eq!(status.device, None);
        assert_eq!(status.device_reason, None);
    }

    #[test]
    fn the_new_wire_names_are_camel_case() {
        let mut status = AiStatus::default();
        status.set_llm(LlmView::local(Some(RuntimeCheck::RuntimeMissing), false));
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["provider"], "local");
        assert_eq!(json["llmReason"], "runtimeMissing");
        status.set_llm(LlmView::local(Some(RuntimeCheck::Stopped), true));
        assert_eq!(serde_json::to_value(&status).unwrap()["llm"], "loading");
        status.set_llm(LlmView::local(None, false));
        assert_eq!(
            serde_json::to_value(&status).unwrap()["llmReason"],
            "modelMissing"
        );
        status.set_llm(LlmView::local(
            Some(RuntimeCheck::Failed { retry_due: false }),
            false,
        ));
        assert_eq!(
            serde_json::to_value(&status).unwrap()["llmReason"],
            "runtimeFailed"
        );
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
