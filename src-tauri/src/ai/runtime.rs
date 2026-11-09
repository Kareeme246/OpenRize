//! A loopback `llama-server` behind a random per-launch API key, passed in a 0600 file deleted once
//! healthy so it never shows in the process list. `RuntimeManager` starts it on demand, retries
//! once on the CPU when the GPU backend fails, and stops it when idle.

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::guard::{self, LifetimeGuard};
use super::sidecar::IDLE_STOP_AFTER;

const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_POLL: Duration = Duration::from_millis(200);
const STDERR_TAIL: usize = 2000;
const MAX_BACKOFF: Duration = Duration::from_secs(10 * 60);
const SERVER_NAME: &str = "llama-server";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Device {
    Gpu,
    Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceReason {
    /// The GPU backend would not start or load the model, so this run is on the CPU.
    GpuFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceInfo {
    pub device: Device,
    pub reason: Option<DeviceReason>,
}

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub binary: PathBuf,
    /// For a split model, its first part.
    pub model: PathBuf,
    pub ctx_size: u32,
    pub device: Device,
}

impl LaunchSpec {
    pub fn new(binary: impl Into<PathBuf>, model: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            model: model.into(),
            ctx_size: 4096,
            device: Device::Gpu,
        }
    }
}

/// `llama-server` is staged next to the app executable, in `tauri dev` and in the bundle.
pub fn locate() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = format!("{SERVER_NAME}{}", std::env::consts::EXE_SUFFIX);
    let path = exe.parent()?.join(name);
    path.is_file().then_some(path)
}

pub struct LlamaServer {
    /// Behind a mutex because `try_wait` needs `&mut Child`.
    child: Mutex<Child>,
    pid: u32,
    base_url: String,
    api_key: String,
    stderr: Arc<Mutex<Vec<u8>>>,
    /// Dropped after the child is killed.
    _guard: LifetimeGuard,
}

impl LlamaServer {
    pub fn start(spec: &LaunchSpec) -> Result<LlamaServer, String> {
        // Another process could grab the port between release and the child's bind; the
        // early-exit check then reports it.
        let port = TcpListener::bind(("127.0.0.1", 0))
            .and_then(|listener| listener.local_addr())
            .map_err(|error| format!("could not find a free port: {error}"))?
            .port();
        let api_key = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let key_dir = std::env::temp_dir().join(format!("openrize-llama-{}", uuid::Uuid::new_v4()));
        let key_file = write_key_file(&key_dir, &api_key).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&key_dir);
        })?;

        let threads = std::thread::available_parallelism()
            .map(|n| n.get() / 2)
            .unwrap_or(1)
            .max(1);
        let mut command = Command::new(&spec.binary);
        command
            .arg("--model")
            .arg(&spec.model)
            .args(["--host", "127.0.0.1", "--port"])
            .arg(port.to_string())
            .arg("--api-key-file")
            .arg(&key_file)
            .args(["--parallel", "1", "--ctx-size"])
            .arg(spec.ctx_size.to_string())
            .args(["--threads", &threads.to_string()])
            .arg("--no-webui");
        match spec.device {
            Device::Gpu => command.args(["--n-gpu-layers", "all"]),
            Device::Cpu => command.args(["--device", "none", "--n-gpu-layers", "0"]),
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        guard::prepare(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&key_dir);
                return Err(format!(
                    "could not start {}: {error}",
                    spec.binary.display()
                ));
            }
        };
        let lifetime = match guard::attach(&child) {
            Ok(lifetime) => lifetime,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&key_dir);
                return Err(error);
            }
        };

        // Drain on a thread so the pipe never fills.
        let stderr = Arc::new(Mutex::new(Vec::new()));
        if let Some(mut pipe) = child.stderr.take() {
            let sink = Arc::clone(&stderr);
            std::thread::spawn(move || {
                let mut chunk = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    let mut tail = sink.lock().unwrap_or_else(|e| e.into_inner());
                    tail.extend_from_slice(&chunk[..n]);
                    let excess = tail.len().saturating_sub(STDERR_TAIL);
                    tail.drain(..excess);
                }
            });
        }

        let mut server = LlamaServer {
            pid: child.id(),
            child: Mutex::new(child),
            base_url: format!("http://127.0.0.1:{port}"),
            api_key,
            stderr,
            _guard: lifetime,
        };
        let healthy = server.wait_healthy();
        let _ = std::fs::remove_dir_all(&key_dir);
        healthy?;
        Ok(server)
    }

    fn wait_healthy(&mut self) -> Result<(), String> {
        crate::net::ensure_crypto_provider();
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + HEALTH_TIMEOUT;
        loop {
            let exited = self.child_mut().try_wait();
            if let Ok(Some(status)) = exited {
                return Err(format!(
                    "llama-server exited ({status}): {}",
                    self.stderr_tail()
                ));
            }
            let up = client
                .get(format!("{}/health", self.base_url))
                .bearer_auth(&self.api_key)
                .send()
                .is_ok_and(|response| response.status().is_success());
            if up {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let tail = self.stderr_tail();
                self.stop();
                return Err(format!(
                    "llama-server was not healthy after {}s: {tail}",
                    HEALTH_TIMEOUT.as_secs()
                ));
            }
            std::thread::sleep(HEALTH_POLL);
        }
    }

    fn stderr_tail(&self) -> String {
        let bytes = self.stderr.lock().unwrap_or_else(|e| e.into_inner());
        String::from_utf8_lossy(&bytes).trim().to_string()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn is_running(&self) -> bool {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        matches!(child.try_wait(), Ok(None))
    }

    pub fn stop(&mut self) {
        let child = self.child_mut();
        let _ = child.kill();
        let _ = child.wait();
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.get_mut().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Creates the directory itself at 0700: `create_dir_all` first would leave it at the default mode.
fn write_key_file(dir: &Path, key: &str) -> Result<PathBuf, String> {
    use std::io::Write;
    let created = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
        }
        #[cfg(not(unix))]
        {
            std::fs::create_dir_all(dir)
        }
    };
    created.map_err(|error| format!("could not create {dir:?}: {error}"))?;
    let path = dir.join("key");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&path)
        .and_then(|mut file| file.write_all(key.as_bytes()))
        .map_err(|error| format!("could not write the API key file: {error}"))?;
    Ok(path)
}

/// What the status shows about the local runtime before any request is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCheck {
    ModelMissing,
    RuntimeMissing,
    /// The last start failed on every device or the server died. Starting is allowed again
    /// once the backoff is over.
    Failed {
        retry_due: bool,
    },
    Stopped,
    Running,
}

#[derive(Debug)]
pub struct Connection {
    pub base_url: String,
    pub api_key: String,
    /// Changes with every start, so the caller knows the first request after one is slow.
    pub generation: u64,
}

/// The server the active local model needs: started on the first request, stopped when idle.
pub struct RuntimeManager {
    binary: Option<PathBuf>,
    model: PathBuf,
    server: Option<LlamaServer>,
    device: Option<DeviceInfo>,
    /// After the GPU failed once for this model, later starts go straight to the CPU.
    prefer_cpu: bool,
    generation: u64,
    last_used: Instant,
    failures: u32,
    retry_at: Option<Instant>,
}

impl RuntimeManager {
    pub fn new(binary: Option<PathBuf>, model: PathBuf) -> Self {
        Self {
            binary,
            model,
            server: None,
            device: None,
            prefer_cpu: false,
            generation: 0,
            last_used: Instant::now(),
            failures: 0,
            retry_at: None,
        }
    }

    pub fn check(&mut self) -> RuntimeCheck {
        self.reap_dead_server();
        if !self.model.is_file() {
            RuntimeCheck::ModelMissing
        } else if !self.binary.as_deref().is_some_and(Path::is_file) {
            RuntimeCheck::RuntimeMissing
        } else if self.server.is_some() {
            RuntimeCheck::Running
        } else if self.failures > 0 {
            RuntimeCheck::Failed {
                retry_due: self.retry_at.is_none_or(|at| Instant::now() >= at),
            }
        } else {
            RuntimeCheck::Stopped
        }
    }

    /// Starts the server if it is not up. A start that fails on every device backs off.
    pub fn ensure_running(&mut self) -> Result<Connection, String> {
        self.reap_dead_server();
        if self.server.is_none() {
            if self.retry_at.is_some_and(|at| Instant::now() < at) {
                return Err("the local model is restarting after a failure".to_string());
            }
            let binary = self.binary.clone().ok_or("llama-server is not bundled")?;
            let spec = LaunchSpec::new(binary, &self.model);
            match start_with_fallback(self.prefer_cpu, |device| {
                LlamaServer::start(&LaunchSpec {
                    device,
                    ..spec.clone()
                })
            }) {
                Ok((server, info)) => {
                    self.prefer_cpu = info.device == Device::Cpu;
                    self.server = Some(server);
                    self.device = Some(info);
                    self.generation += 1;
                    self.failures = 0;
                    self.retry_at = None;
                }
                Err(error) => {
                    self.record_failure();
                    return Err(error);
                }
            }
        }
        self.last_used = Instant::now();
        let server = self
            .server
            .as_ref()
            .ok_or("the local model is not running")?;
        Ok(Connection {
            base_url: server.base_url().to_string(),
            api_key: server.api_key().to_string(),
            generation: self.generation,
        })
    }

    pub fn touch(&mut self) {
        self.last_used = Instant::now();
    }

    /// The server stopped answering: stop it so the next request starts a fresh one.
    pub fn crashed(&mut self) {
        self.stop();
        self.record_failure();
    }

    pub fn stop_if_idle(&mut self) -> bool {
        if self.server.is_some() && self.last_used.elapsed() >= IDLE_STOP_AFTER {
            self.stop();
            return true;
        }
        false
    }

    pub fn stop(&mut self) {
        self.server = None;
        self.device = None;
    }

    pub fn pid(&self) -> Option<u32> {
        self.server.as_ref().map(LlamaServer::pid)
    }

    pub fn is_running(&self) -> bool {
        self.server.as_ref().is_some_and(LlamaServer::is_running)
    }

    pub fn device(&self) -> Option<DeviceInfo> {
        self.server.as_ref().and(self.device)
    }

    fn reap_dead_server(&mut self) {
        if self.server.as_ref().is_some_and(|s| !s.is_running()) {
            self.crashed();
        }
    }

    fn record_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
        let backoff = Duration::from_secs(5u64.saturating_mul(1 << self.failures.min(8)));
        self.retry_at = Some(Instant::now() + backoff.min(MAX_BACKOFF));
    }
}

/// Full GPU offload first; when that fails, one retry on the CPU, which is reported rather than
/// silent. `prefer_cpu` skips the GPU attempt that already failed for this model.
fn start_with_fallback<S>(
    prefer_cpu: bool,
    mut launch: impl FnMut(Device) -> Result<S, String>,
) -> Result<(S, DeviceInfo), String> {
    let gpu_error = if prefer_cpu {
        None
    } else {
        match launch(Device::Gpu) {
            Ok(server) => {
                let info = DeviceInfo {
                    device: Device::Gpu,
                    reason: None,
                };
                return Ok((server, info));
            }
            Err(error) => Some(error),
        }
    };
    match launch(Device::Cpu) {
        Ok(server) => {
            let info = DeviceInfo {
                device: Device::Cpu,
                reason: Some(DeviceReason::GpuFailed),
            };
            Ok((server, info))
        }
        Err(cpu_error) => Err(match gpu_error {
            Some(gpu_error) => format!("on the GPU: {gpu_error}; on the CPU: {cpu_error}"),
            None => cpu_error,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_working_gpu_is_used_and_nothing_is_retried() {
        let mut tried = Vec::new();
        let (server, info) = start_with_fallback(false, |device| {
            tried.push(device);
            Ok("server")
        })
        .unwrap();
        assert_eq!(server, "server");
        assert_eq!(tried, [Device::Gpu]);
        assert_eq!(
            info,
            DeviceInfo {
                device: Device::Gpu,
                reason: None
            }
        );
    }

    #[test]
    fn a_failed_gpu_retries_once_on_the_cpu_and_says_so() {
        let mut tried = Vec::new();
        let (_, info) = start_with_fallback(false, |device| {
            tried.push(device);
            match device {
                Device::Gpu => Err("vulkan init failed".to_string()),
                Device::Cpu => Ok(()),
            }
        })
        .unwrap();
        assert_eq!(tried, [Device::Gpu, Device::Cpu]);
        assert_eq!(
            info,
            DeviceInfo {
                device: Device::Cpu,
                reason: Some(DeviceReason::GpuFailed)
            }
        );
    }

    #[test]
    fn failing_on_both_devices_reports_both_errors_after_two_tries() {
        let mut tries = 0;
        let error = start_with_fallback::<()>(false, |device| {
            tries += 1;
            Err(format!("{device:?} broke"))
        })
        .err()
        .unwrap();
        assert_eq!(tries, 2);
        assert!(
            error.contains("Gpu broke") && error.contains("Cpu broke"),
            "{error}"
        );
    }

    #[test]
    fn a_gpu_that_already_failed_is_not_tried_again() {
        let mut tried = Vec::new();
        let (_, info) = start_with_fallback(true, |device| {
            tried.push(device);
            Ok(())
        })
        .unwrap();
        assert_eq!(tried, [Device::Cpu]);
        assert_eq!(info.reason, Some(DeviceReason::GpuFailed));
    }

    #[test]
    fn the_manager_reports_what_is_missing_before_it_starts_anything() {
        let dir = std::env::temp_dir().join(format!("openrize-runtime-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("tiny.gguf");
        let binary = dir.join("llama-server");

        let mut manager = RuntimeManager::new(Some(binary.clone()), model.clone());
        assert_eq!(manager.check(), RuntimeCheck::ModelMissing);
        std::fs::write(&model, "gguf").unwrap();
        assert_eq!(manager.check(), RuntimeCheck::RuntimeMissing);
        std::fs::write(&binary, "").unwrap();
        assert_eq!(manager.check(), RuntimeCheck::Stopped);
        assert_eq!(manager.device(), None);

        // A binary that cannot run is a failed start, then a backoff.
        assert!(manager.ensure_running().is_err());
        assert_eq!(manager.check(), RuntimeCheck::Failed { retry_due: false });
        assert!(manager.ensure_running().unwrap_err().contains("restarting"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_key_directory_is_private_from_the_start() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("openrize-key-{}", uuid::Uuid::new_v4()));
        let key = write_key_file(&dir, "secret").unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&key), 0o600);
        let _ = std::fs::remove_dir_all(dir);
    }
}
