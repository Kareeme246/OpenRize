//! Every model the engine can call sits behind one `Provider` (Apple's sidecar, or our own
//! `llama-server` on loopback; see `docs/design/local-models.md`). One LLM is active at a
//! time; the kNN embedder is held separately so switching LLMs never touches stored vectors.

use openrize_core::ai::arbiter::Votes;
use openrize_core::ai::prompt::{LlmRequest, PROMPT_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Apple,
    Local,
    /// No language model is active (rules and the personal model only).
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderId {
    pub kind: ProviderKind,
    /// Apple: `<os>:<llm availability>`, as `model_version` has always carried. Local: the model.
    pub model: String,
}

impl ProviderId {
    /// Apple keeps the `fm:` form so `metrics.rs` groups old and new suggestions together.
    pub fn tag(&self) -> String {
        match self.kind {
            ProviderKind::Apple => format!("fm:{}", self.model),
            ProviderKind::Local => format!("local:{} p{PROMPT_VERSION}", self.model),
            ProviderKind::None => "none".to_string(),
        }
    }
}

/// Everything runs on this machine; a later remote endpoint adds `Endpoint`, whose prompts are stripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Egress {
    OnDevice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Ready,
    /// A reason code the UI maps to a sentence, e.g. `appleIntelligenceNotEnabled`.
    Unavailable(String),
}

impl Availability {
    pub fn is_ready(&self) -> bool {
        matches!(self, Availability::Ready)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedModel {
    pub dimension: usize,
    /// Stored in `entry_embeddings.model`; kNN only compares vectors that share it.
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub llm: Availability,
    pub embed: Option<EmbedModel>,
    pub personal: bool,
    pub egress: Egress,
}

/// Votes over `samples` generations, counted as the Swift sidecar does (an unknown category
/// is no vote, an unknown project is "No project").
#[derive(Debug, Clone)]
pub struct Classified {
    pub category: Votes,
    pub project: Option<Votes>,
    pub description: Option<String>,
    pub samples: u32,
    pub latency_ms: u64,
}

/// A failed call makes the job fall back to T0 + T1 and the existing job backoff applies.
pub trait Provider: Send {
    fn id(&self) -> ProviderId;

    fn capabilities(&mut self) -> Result<Capabilities, String>;

    /// `temperature` `None` is greedy decoding; the disagreement re-ask passes `Some(0.9)`.
    fn classify(
        &mut self,
        request: &LlmRequest,
        samples: u32,
        temperature: Option<f64>,
    ) -> Result<Classified, String>;

    fn embed(&mut self, texts: &[String]) -> Result<Vec<Option<Vec<f32>>>, String>;

    /// Brings the backing process up ahead of a request so the caller can show it loading.
    /// Providers that start on their first call have nothing to do.
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn pid(&self) -> Option<u32>;

    fn is_running(&self) -> bool;

    fn stop_if_idle(&mut self) -> bool;
}
