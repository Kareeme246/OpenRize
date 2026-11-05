//! The seam beneath the worker: every model the engine can call sits behind
//! one `Provider`. Today that is Apple's on-device model through the Swift
//! sidecar (`AppleProvider`) and any OpenAI-compatible server
//! (`OpenAiProvider`), which in this pass only ever means our own
//! `llama-server` on loopback. See `docs/design/local-models.md`.
//!
//! **One LLM is active at a time.** The worker holds at most one provider in
//! its LLM slot and never asks two for the same block. The embedder behind
//! kNN is held separately, so switching LLMs never touches stored vectors.

use openrize_core::ai::arbiter::Votes;
use openrize_core::ai::prompt::{LlmRequest, PROMPT_VERSION};

/// Which family a provider belongs to. A later bring-your-own endpoint is one
/// more variant here, not a second interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Apple's Foundation Model through the `openrize-ml` sidecar.
    Apple,
    /// A model served by our own `llama-server`.
    Local,
}

/// Who a provider is, as `model_version` records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderId {
    pub kind: ProviderKind,
    /// Apple: `<os>:<llm availability>`, exactly what `model_version` has
    /// always carried. Local: the served model's identity.
    pub model: String,
}

impl ProviderId {
    /// The provider's part of `model_version`. Apple's keeps today's `fm:`
    /// form so `metrics.rs` groups old and new suggestions together.
    pub fn tag(&self) -> String {
        match self.kind {
            ProviderKind::Apple => format!("fm:{}", self.model),
            ProviderKind::Local => format!("local:{} p{PROMPT_VERSION}", self.model),
        }
    }
}

/// Where a provider sends what it is given. Everything in this pass runs on
/// this machine; a later endpoint adds `Endpoint`, whose prompts are stripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Egress {
    OnDevice,
}

/// Whether the provider's language model can classify right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Ready,
    /// A reason code the UI maps to a sentence, such as Apple's
    /// `appleIntelligenceNotEnabled` or `modelNotReady`.
    Unavailable(String),
}

impl Availability {
    pub fn is_ready(&self) -> bool {
        matches!(self, Availability::Ready)
    }
}

/// The embedding model a provider serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedModel {
    pub dimension: usize,
    /// Stored in `entry_embeddings.model`; kNN only compares vectors that
    /// share it.
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub llm: Availability,
    /// `None` when the provider cannot embed.
    pub embed: Option<EmbedModel>,
    /// Whether it can train and run the personal (T1) classifier.
    pub personal: bool,
    pub egress: Egress,
}

/// One classification: votes over `samples` independent generations,
/// counted the way the Swift sidecar counts them (an unknown category is no
/// vote, an unknown project is "No project").
#[derive(Debug, Clone)]
pub struct Classified {
    pub category: Votes,
    /// `None` when the request asked for no project.
    pub project: Option<Votes>,
    /// The first usable description any sample wrote.
    pub description: Option<String>,
    pub samples: u32,
    pub latency_ms: u64,
}

/// A model the engine can call. Errors are plain sentences, like the rest of
/// the worker's; a failed call makes the job fall back to T0 + T1 and the
/// existing job backoff applies.
pub trait Provider: Send {
    fn id(&self) -> ProviderId;

    /// Starts the provider if needed and reports what it can do now.
    fn capabilities(&mut self) -> Result<Capabilities, String>;

    /// Runs `samples` generations of the shared prompt. `temperature` `None`
    /// is greedy decoding; the disagreement re-ask passes `Some(0.9)`.
    fn classify(
        &mut self,
        request: &LlmRequest,
        samples: u32,
        temperature: Option<f64>,
    ) -> Result<Classified, String>;

    /// One vector per text, `None` where the model had none.
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Option<Vec<f32>>>, String>;

    /// The process doing the work, for energy attribution.
    fn pid(&self) -> Option<u32>;

    fn is_running(&self) -> bool;

    /// Stops a provider process that has sat idle long enough. Returns
    /// whether it stopped one.
    fn stop_if_idle(&mut self) -> bool;
}
