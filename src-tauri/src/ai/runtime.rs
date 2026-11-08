//! A `llama-server` child on loopback, guarded by a random per-launch API key so no other
//! local program can use the model. The key goes in a 0600 `--api-key-file`, deleted once the
//! server is healthy, keeping it out of the process list.

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_POLL: Duration = Duration::from_millis(200);
const STDERR_TAIL: usize = 2000;

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub binary: PathBuf,
    /// For a split model, its first part.
    pub model: PathBuf,
    pub ctx_size: u32,
}

impl LaunchSpec {
    pub fn new(binary: impl Into<PathBuf>, model: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            model: model.into(),
            ctx_size: 4096,
        }
    }
}

pub struct LlamaServer {
    /// Behind a mutex because `try_wait` needs `&mut Child`.
    child: Mutex<Child>,
    pid: u32,
    base_url: String,
    api_key: String,
    stderr: Arc<Mutex<Vec<u8>>>,
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
        let key_file = write_key_file(&key_dir, &api_key)?;

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
            .arg("--no-webui")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let spawned = command.spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&key_dir);
                return Err(format!(
                    "could not start {}: {error}",
                    spec.binary.display()
                ));
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

fn write_key_file(dir: &Path, key: &str) -> Result<PathBuf, String> {
    use std::io::Write;
    std::fs::create_dir_all(dir).map_err(|error| format!("could not create {dir:?}: {error}"))?;
    let path = dir.join("key");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        options.mode(0o600);
        let _ = std::fs::DirBuilder::new().mode(0o700).create(dir);
    }
    options
        .open(&path)
        .and_then(|mut file| file.write_all(key.as_bytes()))
        .map_err(|error| format!("could not write the API key file: {error}"))?;
    Ok(path)
}
