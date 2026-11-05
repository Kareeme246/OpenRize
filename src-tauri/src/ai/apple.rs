//! Apple's on-device model as a `Provider`: it owns the `openrize-ml` sidecar
//! and speaks its unchanged JSON protocol. The sidecar also hosts the Create
//! ML personal classifier, which stays Apple-only until that moves behind the
//! trait, so the worker reaches it through `sidecar_mut`.

use std::collections::HashMap;
use std::path::PathBuf;

use openrize_core::ai::arbiter::Votes;
use openrize_core::ai::prompt::LlmRequest;
use openrize_core::ai::store;
use serde::Deserialize;
use serde_json::json;

use super::provider::{
    Availability, Capabilities, Classified, Egress, EmbedModel, Provider, ProviderId, ProviderKind,
};
use super::sidecar::{self, Sidecar};

pub struct AppleProvider {
    sidecar: Sidecar,
}

impl AppleProvider {
    pub fn new() -> Self {
        Self {
            sidecar: Sidecar::new(),
        }
    }

    /// Points at a built `openrize-ml` binary, for the benchmark.
    #[cfg_attr(not(test), allow(dead_code))] // only the golden-set benchmark uses it
    pub fn at(path: PathBuf) -> Self {
        Self {
            sidecar: Sidecar::at(path),
        }
    }

    /// Whether the binary ships with this build (macOS only).
    pub fn is_installed(&self) -> bool {
        self.sidecar.is_installed()
    }

    /// The last probe as the sidecar reported it, for `AiStatus` (`os` and
    /// the raw LLM availability code).
    pub fn sidecar_capabilities(&self) -> Option<&sidecar::Capabilities> {
        self.sidecar.capabilities()
    }

    /// The sidecar itself, for the personal-model calls that are not part of
    /// `Provider`.
    pub fn sidecar_mut(&mut self) -> &mut Sidecar {
        &mut self.sidecar
    }
}

/// The provider id for a capabilities probe. Before any probe nothing reads
/// the tag, so the placeholder never reaches `model_version`.
pub fn id_for(caps: Option<&sidecar::Capabilities>) -> ProviderId {
    let model = match caps {
        Some(caps) => format!("{}:{}", caps.os, caps.llm),
        None => "unknown:unknown".to_string(),
    };
    ProviderId {
        kind: ProviderKind::Apple,
        model,
    }
}

pub fn capabilities_of(caps: &sidecar::Capabilities) -> Capabilities {
    Capabilities {
        llm: if caps.llm == "available" {
            Availability::Ready
        } else {
            Availability::Unavailable(caps.llm.clone())
        },
        embed: caps.embed.then(|| EmbedModel {
            dimension: caps.embed_dimension,
            model: format!("nlembedding-en-r{}", caps.embed_revision),
        }),
        personal: caps.personal,
        egress: Egress::OnDevice,
    }
}

impl Provider for AppleProvider {
    fn id(&self) -> ProviderId {
        id_for(self.sidecar.capabilities())
    }

    fn capabilities(&mut self) -> Result<Capabilities, String> {
        self.sidecar.probe().map(|caps| capabilities_of(&caps))
    }

    fn classify(
        &mut self,
        request: &LlmRequest,
        samples: u32,
        temperature: Option<f64>,
    ) -> Result<Classified, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Reply {
            category_votes: HashMap<String, u32>,
            project_votes: Option<HashMap<String, u32>>,
            description: Option<String>,
            samples: u32,
            #[serde(default)]
            latency_ms: u64,
        }
        let mut params = json!({
            "features": request.features,
            "categories": request.categories,
            "examples": request.examples,
            "customInstructions": request.custom,
            "samples": samples,
        });
        if let Some(projects) = &request.projects {
            params["projects"] = json!(projects);
        }
        if let Some(temperature) = temperature {
            params["temperature"] = json!(temperature);
        }
        let reply: Reply = self
            .sidecar
            .call("classify", params, sidecar::TIMEOUT_CLASSIFY)?;
        let votes = |counts: HashMap<String, u32>| Votes {
            counts: counts
                .into_iter()
                .map(|(id, n)| (store::label_from_model(&id), n))
                .collect(),
            samples: reply.samples,
        };
        Ok(Classified {
            category: votes(reply.category_votes),
            project: request
                .projects
                .is_some()
                .then(|| votes(reply.project_votes.unwrap_or_default())),
            description: reply.description.filter(|d| !d.is_empty()),
            samples: reply.samples,
            latency_ms: reply.latency_ms,
        })
    }

    fn embed(&mut self, texts: &[String]) -> Result<Vec<Option<Vec<f32>>>, String> {
        #[derive(Deserialize)]
        struct Embedded {
            vectors: Vec<Option<Vec<f32>>>,
        }
        let reply: Embedded =
            self.sidecar
                .call("embed", json!({ "texts": texts }), sidecar::TIMEOUT_QUICK)?;
        Ok(reply.vectors)
    }

    fn pid(&self) -> Option<u32> {
        self.sidecar.pid()
    }

    fn is_running(&self) -> bool {
        self.sidecar.is_running()
    }

    fn stop_if_idle(&mut self) -> bool {
        self.sidecar.stop_if_idle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(llm: &str) -> sidecar::Capabilities {
        sidecar::Capabilities {
            llm: llm.to_string(),
            embed: true,
            embed_dimension: 512,
            embed_revision: 2,
            personal: true,
            os: "26.1".to_string(),
            version: "1".to_string(),
        }
    }

    #[test]
    fn the_tag_keeps_todays_model_version_form() {
        assert_eq!(id_for(Some(&caps("available"))).tag(), "fm:26.1:available");
    }

    #[test]
    fn capabilities_map_from_the_sidecar() {
        let ready = capabilities_of(&caps("available"));
        assert!(ready.llm.is_ready());
        assert_eq!(
            ready.embed,
            Some(EmbedModel {
                dimension: 512,
                model: "nlembedding-en-r2".to_string()
            })
        );
        assert_eq!(
            capabilities_of(&caps("modelNotReady")).llm,
            Availability::Unavailable("modelNotReady".to_string())
        );
    }
}
