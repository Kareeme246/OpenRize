//! The bundled embedder: a second, small `llama-server` running the GGUF `build.rs` ships as a
//! resource, asked through `/v1/embeddings`. It is what T1's kNN has without Apple's NLEmbedding;
//! a Mac keeps NLEmbedding until the golden set says otherwise (docs/design/local-models.md).

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::provider::EmbedModel;
use super::runtime::{Connection, RuntimeCheck, RuntimeManager};

/// The identity the vectors carry, from `build.rs` so the file, its hash and the tag can never
/// drift.
pub const IDENTITY: &str = env!("OPENRIZE_EMBEDDER_TAG");
/// Where the platform configs ship the model inside the bundle's resources.
pub const RESOURCE: &str = concat!("models/", env!("OPENRIZE_EMBEDDER_FILE"));
/// `bert.embedding_length` in that GGUF; kNN only compares vectors of this width.
pub const DIMENSION: usize = 384;
/// The model's own window (`bert.context_length`): a longer text would be truncated, not rejected.
const CTX_SIZE: u32 = 512;
/// An embedding is one short forward pass per text; 64 of them still finish well inside this.
const TIMEOUT: Duration = Duration::from_secs(60);

pub struct Embedder {
    model: EmbedModel,
    runtime: RuntimeManager,
    client: reqwest::blocking::Client,
}

impl Embedder {
    /// `binary` is the bundled `llama-server`; `None` (not staged in this build) shows up as
    /// `RuntimeMissing` rather than as a panic.
    pub fn new(binary: Option<PathBuf>, model: PathBuf) -> Result<Self, String> {
        crate::net::ensure_crypto_provider();
        // Loopback only: no proxies, and never follow a redirect off the host.
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            model: EmbedModel {
                dimension: DIMENSION,
                model: IDENTITY.to_string(),
            },
            runtime: RuntimeManager::embedder(binary, model, CTX_SIZE),
            client,
        })
    }

    /// The vectors' tag and width once the model and the runtime are both on disk. A start that
    /// failed is still available: the next request retries it.
    pub fn model(&mut self) -> Option<EmbedModel> {
        match self.runtime.check() {
            RuntimeCheck::ModelMissing | RuntimeCheck::RuntimeMissing => None,
            RuntimeCheck::Stopped | RuntimeCheck::Running | RuntimeCheck::Failed { .. } => {
                Some(self.model.clone())
            }
        }
    }

    /// Starts the server if it is stopped; the first load is what takes the time.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn start(&mut self) -> Result<(), String> {
        self.runtime.ensure_running().map(|_: Connection| ())
    }

    /// One request for every text. `None` for any text the server left out, so a caller writes no
    /// vector rather than a wrong one.
    pub fn embed(&mut self, texts: &[String]) -> Result<Vec<Option<Vec<f32>>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct Reply {
            data: Vec<Vector>,
        }
        #[derive(Deserialize)]
        struct Vector {
            index: usize,
            embedding: Vec<f32>,
        }

        let connection = self.runtime.ensure_running()?;
        let sent = self
            .client
            .post(format!("{}/v1/embeddings", connection.base_url))
            .bearer_auth(&connection.api_key)
            .timeout(TIMEOUT)
            .json(&json!({ "input": texts }))
            .send();
        self.runtime.touch();
        let response = match sent {
            Ok(response) => response,
            Err(error) => {
                // A server that stopped answering is replaced by the next request.
                self.runtime.crashed();
                return Err(format!("the embedder did not answer: {error}"));
            }
        };
        let status = response.status();
        if !status.is_success() {
            return Err(format!("the embedder answered HTTP {status}"));
        }
        let reply: Reply = response
            .json()
            .map_err(|error| format!("the embedder sent unreadable JSON: {error}"))?;
        let mut vectors = vec![None; texts.len()];
        for vector in reply.data {
            let Some(slot) = vectors.get_mut(vector.index) else {
                continue;
            };
            // A width the tag does not name would make kNN skip every vector silently.
            if vector.embedding.len() != DIMENSION {
                return Err(format!(
                    "the embedder returned {} values, expected {DIMENSION}",
                    vector.embedding.len()
                ));
            }
            *slot = Some(vector.embedding);
        }
        Ok(vectors)
    }

    pub fn pid(&self) -> Option<u32> {
        self.runtime.pid()
    }

    pub fn stop_if_idle(&mut self) -> bool {
        self.runtime.stop_if_idle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedder(runtime: Option<PathBuf>) -> (Embedder, PathBuf) {
        let dir = std::env::temp_dir().join(format!("openrize-embedder-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join(RESOURCE.rsplit('/').next().unwrap());
        let embedder = Embedder::new(runtime, model.clone()).unwrap();
        (embedder, model)
    }

    #[test]
    fn no_model_file_means_no_embedder() {
        let (mut embedder, model) = embedder(None);
        assert_eq!(embedder.model(), None);
        assert!(embedder.embed(&["x".to_string()]).is_err());
        // The model without the runtime is not an embedder either.
        std::fs::write(&model, b"gguf").unwrap();
        assert_eq!(embedder.model(), None);
    }

    #[test]
    fn the_tag_is_the_pinned_identity_and_the_width_the_model_has() {
        let binary = std::env::temp_dir().join(format!("llama-server-{}", uuid::Uuid::new_v4()));
        std::fs::write(&binary, b"").unwrap();
        let (mut embedder, model) = embedder(Some(binary));
        assert!(embedder.model().is_none());
        std::fs::write(&model, b"gguf").unwrap();
        assert_eq!(
            embedder.model(),
            Some(EmbedModel {
                dimension: DIMENSION,
                model: IDENTITY.to_string()
            })
        );
        assert_eq!(DIMENSION, 384);
        assert!(IDENTITY.starts_with("bge-small-en-v1.5@r1-"), "{IDENTITY}");
        let _ = std::fs::remove_dir_all(model.parent().unwrap());
    }

    /// The real GGUF through the real bundled runtime: the path Windows depends on. `cargo test
    /// --lib ai::embedder -- --ignored --nocapture` after a build staged the model and the server
    /// next to the test binary.
    #[test]
    #[ignore = "runs the bundled llama-server and the 37 MB embedder"]
    fn the_bundled_embedder_answers_with_vectors_that_cluster() {
        let profile = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let binary = profile.join(format!("llama-server{}", std::env::consts::EXE_SUFFIX));
        if !binary.is_file() || !profile.join(RESOURCE).is_file() {
            panic!("no staged embedder at {profile:?}: build the app first");
        }
        let mut embedder = Embedder::new(Some(binary), profile.join(RESOURCE)).unwrap();
        assert_eq!(
            embedder.model(),
            Some(EmbedModel {
                dimension: DIMENSION,
                model: IDENTITY.to_string()
            })
        );
        let texts: Vec<String> = [
            "Xcode, main.swift, editing code",
            "Xcode, ContentView.swift, editing code",
            "Zoom, weekly sync meeting",
        ]
        .iter()
        .map(|text| text.to_string())
        .collect();
        let vectors: Vec<Vec<f32>> = embedder
            .embed(&texts)
            .unwrap()
            .into_iter()
            .map(|vector| vector.expect("a vector for every text"))
            .collect();
        for vector in &vectors {
            assert_eq!(vector.len(), DIMENSION);
        }
        let cosine = |a: &[f32], b: &[f32]| {
            let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let (na, nb): (f32, f32) = (
                a.iter().map(|x| x * x).sum::<f32>().sqrt(),
                b.iter().map(|x| x * x).sum::<f32>().sqrt(),
            );
            dot / (na * nb)
        };
        let same_app = cosine(&vectors[0], &vectors[1]);
        let other_app = cosine(&vectors[0], &vectors[2]);
        println!("same app {same_app:.3}, other app {other_app:.3}");
        assert!(
            same_app > other_app,
            "the embedder does not separate apps: {same_app:.3} vs {other_app:.3}"
        );
    }
}
