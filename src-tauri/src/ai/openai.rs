//! Any OpenAI-compatible chat server behind `Provider`. The request shape is deliberately the
//! subset every such server accepts, so a later bring-your-own endpoint is one more `Endpoint`
//! variant. The client is blocking because the AI worker is a plain thread.

use std::time::{Duration, Instant};

use openrize_core::ai::prompt::{self, LlmRequest, Sample};
use serde_json::{json, Value};

use super::provider::{
    Availability, Capabilities, Classified, Egress, Provider, ProviderId, ProviderKind,
};
use super::runtime::{Connection, DeviceInfo, RuntimeCheck, RuntimeManager};
use super::sidecar::TIMEOUT_CLASSIFY;

/// The first request after a start also pays for the model load.
const TIMEOUT_FIRST: Duration = Duration::from_secs(120);
const MAX_TOKENS: u32 = 200;

/// Owning the runtime here means dropping the provider stops the server.
pub enum Endpoint {
    Local(Box<RuntimeManager>),
    #[cfg(test)]
    Fake {
        base_url: String,
        api_key: String,
    },
}

impl Endpoint {
    /// Starts the local server if it is not up.
    fn connect(&mut self) -> Result<Connection, String> {
        match self {
            Endpoint::Local(runtime) => runtime.ensure_running(),
            #[cfg(test)]
            Endpoint::Fake { base_url, api_key } => Ok(Connection {
                base_url: base_url.clone(),
                api_key: api_key.clone(),
                generation: 0,
            }),
        }
    }

    fn check(&mut self) -> RuntimeCheck {
        match self.runtime_mut() {
            Some(runtime) => runtime.check(),
            None => RuntimeCheck::Running,
        }
    }

    fn runtime_mut(&mut self) -> Option<&mut RuntimeManager> {
        match self {
            Endpoint::Local(runtime) => Some(runtime),
            #[cfg(test)]
            Endpoint::Fake { .. } => None,
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
    /// Nothing answered: the server is down or stuck, so more samples would only wait again.
    Unreachable(String),
}

struct SampleFailure {
    message: String,
    unreachable: bool,
}

impl From<String> for SampleFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            unreachable: false,
        }
    }
}

pub struct OpenAiProvider {
    endpoint: Endpoint,
    model: LocalModel,
    client: reqwest::blocking::Client,
    schema_supported: bool,
    /// The server start that has answered a request. A new start pays for the model load again,
    /// so its first request gets the long timeout.
    warm_generation: Option<u64>,
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
            warm_generation: None,
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
        let connection = self.endpoint.connect().map_err(CallError::Unreachable)?;
        let timeout = if self.warm_generation == Some(connection.generation) {
            TIMEOUT_CLASSIFY
        } else {
            TIMEOUT_FIRST
        };
        let sent = self
            .client
            .post(format!("{}/v1/chat/completions", connection.base_url))
            .bearer_auth(&connection.api_key)
            .timeout(timeout)
            .json(body)
            .send();
        if let Some(runtime) = self.endpoint.runtime_mut() {
            runtime.touch();
        }
        let response = match sent {
            Ok(response) => response,
            Err(error) => {
                // A server that stopped answering is replaced by the next request.
                if let Some(runtime) = self.endpoint.runtime_mut() {
                    runtime.crashed();
                }
                return Err(CallError::Unreachable(format!(
                    "the local model did not answer: {error}"
                )));
            }
        };
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
        self.warm_generation = Some(connection.generation);
        Ok(value["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    fn complete_with_fallback(
        &mut self,
        request: &LlmRequest,
        temperature: f64,
    ) -> Result<String, SampleFailure> {
        let unreachable = |message| SampleFailure {
            message,
            unreachable: true,
        };
        if self.schema_supported {
            match self.complete(&self.body(request, temperature, true)) {
                Ok(text) => return Ok(text),
                Err(CallError::Failed(message)) => return Err(message.into()),
                Err(CallError::Unreachable(message)) => return Err(unreachable(message)),
                Err(CallError::BadRequest(_)) => self.schema_supported = false,
            }
        }
        match self.complete(&self.body(request, temperature, false)) {
            Ok(text) => Ok(text),
            Err(CallError::Failed(message)) => Err(message.into()),
            Err(CallError::Unreachable(message)) => Err(unreachable(message)),
            Err(CallError::BadRequest(detail)) => Err(format!(
                "the local model rejected the request (HTTP 400): {}",
                detail.trim()
            )
            .into()),
        }
    }

    fn sample(&mut self, request: &LlmRequest, temperature: f64) -> Result<Sample, SampleFailure> {
        let mut last = String::new();
        for _ in 0..2 {
            let text = self.complete_with_fallback(request, temperature)?;
            match prompt::parse_reply(&text, request) {
                Ok(sample) => return Ok(sample),
                Err(error) => last = error,
            }
        }
        Err(format!("the local model's reply was unusable twice: {last}").into())
    }

    /// Why the local model cannot answer right now, if it cannot.
    pub fn check(&mut self) -> RuntimeCheck {
        self.endpoint.check()
    }

    /// The device the running local server uses, if one runs.
    pub fn device(&self) -> Option<DeviceInfo> {
        match &self.endpoint {
            Endpoint::Local(runtime) => runtime.device(),
            #[cfg(test)]
            Endpoint::Fake { .. } => None,
        }
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
        // No request: the runtime verified /health when it started, and a server that later stops
        // answering is stopped by `complete`, which shows up here as a failed or stopped runtime.
        let llm = match self.check() {
            RuntimeCheck::ModelMissing => Availability::Unavailable("modelMissing".to_string()),
            RuntimeCheck::RuntimeMissing => Availability::Unavailable("runtimeMissing".to_string()),
            RuntimeCheck::Failed { .. } => Availability::Unavailable("runtimeFailed".to_string()),
            RuntimeCheck::Stopped | RuntimeCheck::Running => Availability::Ready,
        };
        Ok(Capabilities {
            llm,
            embed: None,
            personal: false,
            egress: Egress::OnDevice,
        })
    }

    fn start(&mut self) -> Result<(), String> {
        self.endpoint.connect().map(|_| ())
    }

    fn classify(
        &mut self,
        request: &LlmRequest,
        samples: u32,
        temperature: Option<f64>,
    ) -> Result<Classified, String> {
        let started = Instant::now();
        let mut collected = Vec::with_capacity(samples as usize);
        let mut failure = None;
        for _ in 0..samples {
            match self.sample(request, temperature.unwrap_or(0.0)) {
                Ok(sample) => collected.push(sample),
                Err(error) => {
                    let stop = error.unreachable;
                    failure = Some(error.message);
                    if stop {
                        break;
                    }
                }
            }
        }
        // One bad sample of several costs a vote, not the whole answer.
        if collected.is_empty() {
            return Err(failure.unwrap_or_else(|| "no samples were asked for".to_string()));
        }
        let (category, project, description) =
            prompt::tally(&collected, request.projects.is_some());
        Ok(Classified {
            category,
            project,
            description,
            samples: collected.len() as u32,
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }

    fn embed(&mut self, _texts: &[String]) -> Result<Vec<Option<Vec<f32>>>, String> {
        Err("the local provider has no embedder yet".to_string())
    }

    fn pid(&self) -> Option<u32> {
        match &self.endpoint {
            Endpoint::Local(runtime) => runtime.pid(),
            #[cfg(test)]
            Endpoint::Fake { .. } => None,
        }
    }

    fn is_running(&self) -> bool {
        match &self.endpoint {
            Endpoint::Local(runtime) => runtime.is_running(),
            #[cfg(test)]
            Endpoint::Fake { .. } => true,
        }
    }

    fn stop_if_idle(&mut self) -> bool {
        match &mut self.endpoint {
            Endpoint::Local(runtime) => runtime.stop_if_idle(),
            #[cfg(test)]
            Endpoint::Fake { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use openrize_core::ai::prompt::Choice;

    use super::*;

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

    #[test]
    fn a_failed_sample_among_several_keeps_the_good_ones() {
        let (url, _) = fake_server(vec![chat(GOOD), (500, "{}".into()), chat(GOOD)]);
        let out = provider(url, false)
            .classify(&request(), 3, Some(0.9))
            .unwrap();
        assert_eq!(out.samples, 2);
        assert_eq!(out.category.samples, 2);
        assert_eq!(out.category.counts.get(&Some("c1".to_string())), Some(&2));
    }

    #[test]
    fn classify_fails_only_when_every_sample_fails() {
        let (url, _) = fake_server(vec![(500, "{}".into()), (500, "{}".into())]);
        let error = provider(url, false)
            .classify(&request(), 2, Some(0.9))
            .unwrap_err();
        assert!(error.contains("500"), "{error}");
    }

    #[test]
    fn an_unreachable_server_is_asked_once_not_once_per_sample() {
        let free = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", free.local_addr().unwrap());
        drop(free);
        let mut p = provider(url, false);
        assert!(p.classify(&request(), 3, Some(0.9)).is_err());
    }

    #[test]
    fn a_local_model_is_ready_to_start_until_a_start_fails() {
        let dir = std::env::temp_dir().join(format!("openrize-openai-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("tiny.gguf");
        std::fs::write(&model, "gguf").unwrap();
        let binary = dir.join("llama-server");
        let mut p = OpenAiProvider::new(
            Endpoint::Local(Box::new(RuntimeManager::new(Some(binary.clone()), model))),
            LocalModel {
                identity: "tiny".into(),
                model: "tiny".into(),
                disable_thinking: true,
            },
        )
        .unwrap();
        let code = |p: &mut OpenAiProvider| match p.capabilities().unwrap().llm {
            Availability::Ready => "ready".to_string(),
            Availability::Unavailable(code) => code,
        };
        assert_eq!(code(&mut p), "runtimeMissing");
        std::fs::write(&binary, "").unwrap();
        assert_eq!(code(&mut p), "ready");
        assert!(p.start().is_err());
        assert_eq!(code(&mut p), "runtimeFailed");
        assert!(!p.is_running());
        assert_eq!(p.pid(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `OPENRIZE_TEST_GGUF=<file.gguf> cargo test --lib -- --ignored real_llama_server`, after a
    /// build staged `llama-server` in the target directory (`OPENRIZE_LLAMA_SERVER` overrides it).
    #[test]
    #[ignore = "starts a real llama-server"]
    fn real_llama_server_returns_a_vote_and_is_gone_after_drop() {
        let Ok(gguf) = std::env::var("OPENRIZE_TEST_GGUF") else {
            panic!("set OPENRIZE_TEST_GGUF to a GGUF file");
        };
        let binary = std::env::var("OPENRIZE_LLAMA_SERVER")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| staged_server());
        let stem = std::path::Path::new(&gguf)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut p = OpenAiProvider::new(
            Endpoint::Local(Box::new(RuntimeManager::new(Some(binary), gguf.into()))),
            LocalModel {
                identity: stem.clone(),
                model: stem,
                disable_thinking: true,
            },
        )
        .unwrap();
        assert!(p.capabilities().unwrap().llm.is_ready());
        assert!(!p.is_running(), "nothing starts before the first request");
        let out = p.classify(&request(), 1, None).unwrap();
        println!("{out:?} on {:?}", p.device());
        assert!(!out.category.counts.is_empty(), "no vote came back");
        assert!(p.capabilities().unwrap().llm.is_ready());
        let pid = p.pid().expect("the server runs after a classify");
        assert!(p.device().is_some());
        drop(p);
        assert!(
            !process_alive(pid),
            "llama-server {pid} outlived its provider"
        );
    }

    /// `target/<profile>/llama-server`, where the packaging step stages it.
    fn staged_server() -> std::path::PathBuf {
        let exe = std::env::current_exe().unwrap();
        let profile = exe.parent().and_then(|deps| deps.parent()).unwrap();
        profile.join(format!("llama-server{}", std::env::consts::EXE_SUFFIX))
    }

    fn process_alive(pid: u32) -> bool {
        #[cfg(target_os = "macos")]
        {
            // Signal 0 only checks that the pid exists; a reaped child does not.
            unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
        }
        #[cfg(not(target_os = "macos"))]
        {
            std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}")])
                .output()
                .map(|out| String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()))
                .unwrap_or(false)
        }
    }
}
