//! Any OpenAI-compatible chat server behind `Provider`. The request shape is deliberately the
//! subset every such server accepts, so a later bring-your-own endpoint is one more `Endpoint`
//! variant. The client is blocking because the AI worker is a plain thread.

use std::time::{Duration, Instant};

use openrize_core::ai::prompt::{self, LlmRequest, Sample};
use serde_json::{json, Value};

use super::provider::{
    Availability, Capabilities, Classified, Egress, Provider, ProviderId, ProviderKind,
};
use super::runtime::LlamaServer;
use super::sidecar::TIMEOUT_CLASSIFY;

/// The first request after a start also pays for the model load.
const TIMEOUT_FIRST: Duration = Duration::from_secs(120);
const TIMEOUT_HEALTH: Duration = Duration::from_secs(3);
const MAX_TOKENS: u32 = 200;
const UNAVAILABLE: &str = "runtimeUnavailable";

/// Owning the process here means dropping the provider stops the server.
pub enum Endpoint {
    Local(LlamaServer),
    #[cfg(test)]
    Fake {
        base_url: String,
        api_key: String,
    },
}

impl Endpoint {
    fn base_url(&self) -> &str {
        match self {
            Endpoint::Local(server) => server.base_url(),
            #[cfg(test)]
            Endpoint::Fake { base_url, .. } => base_url,
        }
    }

    fn api_key(&self) -> &str {
        match self {
            Endpoint::Local(server) => server.api_key(),
            #[cfg(test)]
            Endpoint::Fake { api_key, .. } => api_key,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalModel {
    pub identity: String,
    pub model: String,
    /// Sends `chat_template_kwargs: {enable_thinking: false}`; Qwen needs it to skip thinking.
    pub disable_thinking: bool,
}

/// A 400 is the only failure that can change the request shape (the server rejecting `json_schema`).
enum CallError {
    BadRequest(String),
    Failed(String),
}

pub struct OpenAiProvider {
    endpoint: Endpoint,
    model: LocalModel,
    client: reqwest::blocking::Client,
    schema_supported: bool,
    /// Until the first success, requests get the long timeout.
    warm: bool,
}

impl OpenAiProvider {
    pub fn new(endpoint: Endpoint, model: LocalModel) -> Result<Self, String> {
        crate::net::ensure_crypto_provider();
        // Loopback only: no proxies, and never follow a redirect off the host.
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            endpoint,
            model,
            client,
            schema_supported: true,
            warm: false,
        })
    }

    fn body(&self, request: &LlmRequest, temperature: f64, with_schema: bool) -> Value {
        let built = prompt::build(request);
        let (user, response_format) = if with_schema {
            (
                built.user,
                json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": "ActivityLabel",
                        "strict": true,
                        "schema": prompt::json_schema(request),
                    },
                }),
            )
        } else {
            (
                format!("{}\n\n{}", built.user, prompt::json_object_hint(request)),
                json!({ "type": "json_object" }),
            )
        };
        let mut body = json!({
            "model": self.model.model,
            "stream": false,
            "temperature": temperature,
            "max_tokens": MAX_TOKENS,
            "messages": [
                { "role": "system", "content": built.system },
                { "role": "user", "content": user },
            ],
            "response_format": response_format,
        });
        if self.model.disable_thinking {
            body["chat_template_kwargs"] = json!({ "enable_thinking": false });
        }
        body
    }

    /// Empty text if the choice is absent; the validator rejects it like any unusable reply.
    fn complete(&mut self, body: &Value) -> Result<String, CallError> {
        let timeout = if self.warm {
            TIMEOUT_CLASSIFY
        } else {
            TIMEOUT_FIRST
        };
        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.endpoint.base_url()))
            .bearer_auth(self.endpoint.api_key())
            .timeout(timeout)
            .json(body)
            .send()
            .map_err(|error| {
                CallError::Failed(format!("the local model did not answer: {error}"))
            })?;
        let status = response.status();
        if status == reqwest::StatusCode::BAD_REQUEST {
            let detail = response.text().unwrap_or_default();
            return Err(CallError::BadRequest(detail));
        }
        if !status.is_success() {
            return Err(CallError::Failed(format!(
                "the local model answered HTTP {status}"
            )));
        }
        let value: Value = response.json().map_err(|error| {
            CallError::Failed(format!("the local model sent unreadable JSON: {error}"))
        })?;
        self.warm = true;
        Ok(value["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    fn complete_with_fallback(
        &mut self,
        request: &LlmRequest,
        temperature: f64,
    ) -> Result<String, String> {
        if self.schema_supported {
            match self.complete(&self.body(request, temperature, true)) {
                Ok(text) => return Ok(text),
                Err(CallError::Failed(message)) => return Err(message),
                Err(CallError::BadRequest(_)) => self.schema_supported = false,
            }
        }
        match self.complete(&self.body(request, temperature, false)) {
            Ok(text) => Ok(text),
            Err(CallError::Failed(message)) => Err(message),
            Err(CallError::BadRequest(detail)) => Err(format!(
                "the local model rejected the request (HTTP 400): {}",
                detail.trim()
            )),
        }
    }

    fn sample(&mut self, request: &LlmRequest, temperature: f64) -> Result<Sample, String> {
        let mut last = String::new();
        for _ in 0..2 {
            let text = self.complete_with_fallback(request, temperature)?;
            match prompt::parse_reply(&text, request) {
                Ok(sample) => return Ok(sample),
                Err(error) => last = error,
            }
        }
        Err(format!(
            "the local model's reply was unusable twice: {last}"
        ))
    }

    fn healthy(&self) -> bool {
        self.client
            .get(format!("{}/health", self.endpoint.base_url()))
            .timeout(TIMEOUT_HEALTH)
            .send()
            .is_ok_and(|response| response.status().is_success())
    }
}

impl Provider for OpenAiProvider {
    fn id(&self) -> ProviderId {
        ProviderId {
            kind: ProviderKind::Local,
            model: self.model.identity.clone(),
        }
    }

    fn capabilities(&mut self) -> Result<Capabilities, String> {
        let llm = if self.is_running() && self.healthy() {
            Availability::Ready
        } else {
            Availability::Unavailable(UNAVAILABLE.to_string())
        };
        Ok(Capabilities {
            llm,
            embed: None,
            personal: false,
            egress: Egress::OnDevice,
        })
    }

    fn classify(
        &mut self,
        request: &LlmRequest,
        samples: u32,
        temperature: Option<f64>,
    ) -> Result<Classified, String> {
        let started = Instant::now();
        let mut collected = Vec::with_capacity(samples as usize);
        for _ in 0..samples {
            collected.push(self.sample(request, temperature.unwrap_or(0.0))?);
        }
        let (category, project, description) =
            prompt::tally(&collected, request.projects.is_some());
        Ok(Classified {
            category,
            project,
            description,
            samples,
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    fn embed(&mut self, _texts: &[String]) -> Result<Vec<Option<Vec<f32>>>, String> {
        Err("the local provider has no embedder yet".to_string())
    }

    fn pid(&self) -> Option<u32> {
        match &self.endpoint {
            Endpoint::Local(server) => Some(server.pid()),
            #[cfg(test)]
            Endpoint::Fake { .. } => None,
        }
    }

    fn is_running(&self) -> bool {
        match &self.endpoint {
            Endpoint::Local(server) => server.is_running(),
            #[cfg(test)]
            Endpoint::Fake { .. } => true,
        }
    }

    fn stop_if_idle(&mut self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use openrize_core::ai::prompt::Choice;

    use super::*;
    use crate::ai::runtime::LaunchSpec;

    fn choice(id: &str, name: &str) -> Choice {
        Choice {
            id: id.into(),
            name: name.into(),
            description: None,
            hint: None,
        }
    }

    fn request() -> LlmRequest {
        LlmRequest {
            features: "app: Xcode".into(),
            categories: vec![choice("c1", "Coding"), choice("c2", "Meetings")],
            projects: Some(vec![choice("p1", "Alpha")]),
            examples: vec![],
            custom: String::new(),
        }
    }

    const GOOD: &str = r#"{"category":"Coding","project":"Alpha","description":"Editing code"}"#;

    fn chat(content: &str) -> (u16, String) {
        (
            200,
            json!({ "choices": [{ "message": { "content": content } }] }).to_string(),
        )
    }

    /// Extra requests get a 500.
    fn fake_server(replies: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            let mut replies = replies.into_iter();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut raw = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = stream.read(&mut chunk).unwrap_or(0);
                    raw.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    if n == 0 {
                        break;
                    }
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            break;
                        }
                    }
                }
                log.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&raw).to_string());
                let (status, body) = replies.next().unwrap_or((500, "{}".to_string()));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (base_url, seen)
    }

    fn provider(base_url: String, disable_thinking: bool) -> OpenAiProvider {
        OpenAiProvider::new(
            Endpoint::Fake {
                base_url,
                api_key: "sekret".into(),
            },
            LocalModel {
                identity: "test-model".into(),
                model: "test-model".into(),
                disable_thinking,
            },
        )
        .unwrap()
    }

    fn body_of(raw: &str) -> Value {
        serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap()
    }

    #[test]
    fn votes_count_and_think_prefix_is_stripped() {
        let think = format!("<think>hmm</think>\n{GOOD}");
        let (url, seen) = fake_server(vec![chat(GOOD), chat(&think)]);
        let mut p = provider(url, false);
        let out = p.classify(&request(), 2, None).unwrap();
        assert_eq!(out.samples, 2);
        assert_eq!(out.category.counts.get(&Some("c1".to_string())), Some(&2));
        let projects = out.project.unwrap();
        assert_eq!(projects.counts.get(&Some("p1".to_string())), Some(&2));
        assert_eq!(out.description.as_deref(), Some("Editing code"));
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert_eq!(
            p.id().tag(),
            format!("local:test-model p{}", prompt::PROMPT_VERSION)
        );
    }

    #[test]
    fn request_shape_and_bearer() {
        let (url, seen) = fake_server(vec![chat(GOOD)]);
        provider(url, true)
            .classify(&request(), 1, Some(0.9))
            .unwrap();
        let raw = seen.lock().unwrap()[0].clone();
        assert!(raw.starts_with("POST /v1/chat/completions"));
        assert!(raw
            .to_ascii_lowercase()
            .contains("authorization: bearer sekret"));
        let body = body_of(&raw);
        assert_eq!(body["stream"], false);
        assert_eq!(body["max_tokens"], 200);
        assert_eq!(body["temperature"], 0.9);
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            "ActivityLabel"
        );
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn greedy_and_no_thinking_flag_by_default() {
        let (url, seen) = fake_server(vec![chat(GOOD)]);
        provider(url, false).classify(&request(), 1, None).unwrap();
        let body = body_of(&seen.lock().unwrap()[0]);
        assert_eq!(body["temperature"], 0.0);
        assert!(body.get("chat_template_kwargs").is_none());
    }

    #[test]
    fn schema_rejected_falls_back_and_is_remembered() {
        let (url, seen) = fake_server(vec![
            (400, "{\"error\":\"no schema\"}".into()),
            chat(GOOD),
            chat(GOOD),
        ]);
        let mut p = provider(url, false);
        p.classify(&request(), 1, None).unwrap();
        p.classify(&request(), 1, None).unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(body_of(&seen[0])["response_format"]["type"], "json_schema");
        let retry = body_of(&seen[1]);
        assert_eq!(retry["response_format"]["type"], "json_object");
        let user = retry["messages"][1]["content"].as_str().unwrap();
        assert!(user.ends_with(&prompt::json_object_hint(&request())));
        assert_eq!(body_of(&seen[2])["response_format"]["type"], "json_object");
    }

    #[test]
    fn unparseable_twice_errors() {
        let (url, seen) = fake_server(vec![chat("no json"), chat("still none")]);
        let error = provider(url, false)
            .classify(&request(), 1, None)
            .unwrap_err();
        assert!(error.contains("unusable twice"), "{error}");
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn unparseable_once_then_valid_is_ok() {
        let (url, _) = fake_server(vec![chat("no json"), chat(GOOD)]);
        let out = provider(url, false).classify(&request(), 1, None).unwrap();
        assert_eq!(out.category.counts.get(&Some("c1".to_string())), Some(&1));
    }

    #[test]
    fn server_error_and_refused_connection_fail() {
        let (url, _) = fake_server(vec![(503, "{}".into())]);
        let error = provider(url, false)
            .classify(&request(), 1, None)
            .unwrap_err();
        assert!(error.contains("503"), "{error}");

        let free = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", free.local_addr().unwrap());
        drop(free);
        assert!(provider(url, false).classify(&request(), 1, None).is_err());
    }

    /// Needs a real model: `OPENRIZE_TEST_GGUF=<file.gguf>` and, unless
    /// `llama-server` is on PATH, `OPENRIZE_LLAMA_SERVER=<binary>`.
    #[test]
    #[ignore = "starts a real llama-server"]
    fn real_llama_server_returns_a_vote() {
        let Ok(gguf) = std::env::var("OPENRIZE_TEST_GGUF") else {
            panic!("set OPENRIZE_TEST_GGUF to a GGUF file");
        };
        let binary =
            std::env::var("OPENRIZE_LLAMA_SERVER").unwrap_or_else(|_| "llama-server".into());
        let server = LlamaServer::start(&LaunchSpec::new(binary, &gguf)).unwrap();
        let stem = std::path::Path::new(&gguf)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut p = OpenAiProvider::new(
            Endpoint::Local(server),
            LocalModel {
                identity: stem.clone(),
                model: stem,
                disable_thinking: true,
            },
        )
        .unwrap();
        assert!(p.capabilities().unwrap().llm.is_ready());
        let out = p.classify(&request(), 1, None).unwrap();
        println!("{out:?}");
        assert!(!out.category.counts.is_empty(), "no vote came back");
        assert!(p.pid().is_some());
    }
}
