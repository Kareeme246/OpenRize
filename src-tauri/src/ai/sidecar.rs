//! The `openrize-ml` Swift sidecar: spawned lazily, spoken to over JSON
//! lines on stdio, restarted with backoff after a crash, and stopped after
//! ten idle minutes. The protocol is documented in
//! `swift/Sources/openrize-ml/Protocol.swift`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

/// Must match `protocolVersion` in the sidecar's Entry.swift.
const PROTOCOL_VERSION: &str = "1";
const BINARY_NAME: &str = "openrize-ml";
pub const IDLE_STOP_AFTER: Duration = Duration::from_secs(10 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(10 * 60);

pub const TIMEOUT_QUICK: Duration = Duration::from_secs(15);
pub const TIMEOUT_CLASSIFY: Duration = Duration::from_secs(90);
pub const TIMEOUT_TRAIN: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub llm: String,
    pub embed: bool,
    pub embed_dimension: usize,
    pub embed_revision: i64,
    pub personal: bool,
    pub os: String,
    pub version: String,
}

struct Process {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

pub struct Sidecar {
    path: Option<PathBuf>,
    process: Option<Process>,
    next_id: u64,
    last_used: Instant,
    failures: u32,
    retry_at: Option<Instant>,
    capabilities: Option<Capabilities>,
}

impl Sidecar {
    pub fn new() -> Self {
        Self {
            path: locate(),
            process: None,
            next_id: 1,
            last_used: Instant::now(),
            failures: 0,
            retry_at: None,
            capabilities: None,
        }
    }

    /// Whether the binary ships with this build (macOS only).
    pub fn is_installed(&self) -> bool {
        self.path.is_some()
    }

    pub fn is_running(&self) -> bool {
        self.process.is_some()
    }

    /// The PID of the running child process, if any.
    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().map(|p| p.child.id())
    }

    /// The last capabilities probe, refreshed on every (re)start.
    pub fn capabilities(&self) -> Option<&Capabilities> {
        self.capabilities.as_ref()
    }

    /// Starts the sidecar (if needed) and returns its capabilities.
    pub fn probe(&mut self) -> Result<Capabilities, String> {
        self.ensure_running()?;
        self.capabilities
            .clone()
            .ok_or_else(|| "sidecar reported no capabilities".to_string())
    }

    pub fn call<R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<R, String> {
        self.ensure_running()?;
        let result = self.request(method, params, timeout)?;
        serde_json::from_value(result).map_err(|error| format!("bad {method} reply: {error}"))
    }

    /// Stops a sidecar that has sat unused for `IDLE_STOP_AFTER`. Returns
    /// whether it stopped one.
    pub fn stop_if_idle(&mut self) -> bool {
        if self.process.is_some() && self.last_used.elapsed() >= IDLE_STOP_AFTER {
            self.stop();
            return true;
        }
        false
    }

    pub fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
    }

    fn ensure_running(&mut self) -> Result<(), String> {
        if let Some(process) = &mut self.process {
            match process.child.try_wait() {
                Ok(None) => return Ok(()),
                _ => {
                    self.process = None;
                    self.record_failure();
                }
            }
        }
        let Some(path) = self.path.clone() else {
            return Err("the ML sidecar is not bundled with this build".to_string());
        };
        if let Some(retry_at) = self.retry_at {
            if Instant::now() < retry_at {
                return Err("the ML sidecar is restarting after a failure".to_string());
            }
        }

        let mut child = Command::new(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                self.record_failure();
                format!("could not start {}: {error}", path.display())
            })?;
        let stdin = child.stdin.take().ok_or("sidecar stdin missing")?;
        let stdout = child.stdout.take().ok_or("sidecar stdout missing")?;
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        self.process = Some(Process {
            child,
            stdin,
            lines,
        });

        let capabilities: Capabilities = self
            .request("capabilities", Value::Null, TIMEOUT_QUICK)
            .and_then(|value| {
            serde_json::from_value(value).map_err(|error| error.to_string())
        })?;
        if capabilities.version != PROTOCOL_VERSION {
            self.stop();
            self.record_failure();
            return Err(format!(
                "sidecar speaks protocol {}, expected {PROTOCOL_VERSION}",
                capabilities.version
            ));
        }
        self.capabilities = Some(capabilities);
        self.failures = 0;
        self.retry_at = None;
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.last_used = Instant::now();

        let process = self.process.as_mut().ok_or("sidecar is not running")?;
        let mut line = json!({ "id": id, "m": method, "p": params }).to_string();
        line.push('\n');
        if let Err(error) = process
            .stdin
            .write_all(line.as_bytes())
            .and_then(|_| process.stdin.flush())
        {
            self.crashed();
            return Err(format!("could not write to the sidecar: {error}"));
        }

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let received = match &self.process {
                Some(process) => process.lines.recv_timeout(remaining),
                None => Err(RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(reply) => {
                    let Ok(reply) = serde_json::from_str::<Value>(&reply) else {
                        continue;
                    };
                    if reply.get("id").and_then(Value::as_u64) != Some(id) {
                        continue;
                    }
                    self.last_used = Instant::now();
                    if let Some(error) = reply.get("e").and_then(Value::as_str) {
                        return Err(format!("{method}: {error}"));
                    }
                    return Ok(reply.get("r").cloned().unwrap_or(Value::Null));
                }
                Err(RecvTimeoutError::Timeout) => {
                    // A stuck generation holds the process; start over.
                    self.crashed();
                    return Err(format!("{method} timed out after {}s", timeout.as_secs()));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.crashed();
                    return Err(format!("the sidecar exited during {method}"));
                }
            }
        }
    }

    fn crashed(&mut self) {
        self.stop();
        self.record_failure();
    }

    fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
        let backoff = Duration::from_secs(5u64.saturating_mul(1 << self.failures.min(8)));
        self.retry_at = Some(Instant::now() + backoff.min(MAX_BACKOFF));
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Tauri places `externalBin` binaries next to the app executable, both in
/// `tauri dev` (target/debug) and in the bundle (Contents/MacOS).
fn locate() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join(BINARY_NAME);
    path.is_file().then_some(path)
}
