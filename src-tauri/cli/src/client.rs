//! Reaching OpenRize's data: through the running app over its local
//! endpoint, or, with the app closed, straight from the stores the app made
//! (`openrize_core::rpc`, the same code the app answers with). rize never
//! creates the data: without the app's database there is nothing to work on.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use openrize_core::ipc::{self, Endpoint};
use openrize_core::paths;
use openrize_core::protocol::{code, Hello, Operation, Request, Response};
use openrize_core::rpc;
use openrize_core::state::{self, AppState, NO_DATA};

/// How long a freshly launched app may take to answer.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a quitting app may take to close its stores.
const QUIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Lets a dev or test build point rize at a specific app binary.
const APP_ENV: &str = "RIZE_APP";

pub struct Client {
    data_dir: PathBuf,
    endpoint: Endpoint,
}

impl Client {
    pub fn new(data_dir: Option<&Path>) -> Result<Self, String> {
        let data_dir = match data_dir {
            Some(dir) => std::path::absolute(dir).map_err(|e| e.to_string())?,
            None => paths::app_data_dir().ok_or("no home directory; pass --data-dir")?,
        };
        let endpoint = Endpoint::new(&data_dir)?;
        Ok(Self { data_dir, endpoint })
    }

    pub fn running(&self) -> bool {
        self.endpoint.connect().is_ok()
    }

    /// Asks the running app, or answers from the stored data when it is
    /// closed. An operation that needs the app fails with `APP_NOT_RUNNING`.
    pub fn request(&self, operation: Operation) -> Response {
        let request = Request::new(operation);
        if let Ok(stream) = self.endpoint.connect() {
            return ipc::exchange(stream, &request)
                .unwrap_or_else(|error| Response::error(code::FAILED, error));
        }
        if request.operation.needs_app() {
            return Response::error(
                code::APP_NOT_RUNNING,
                "OpenRize is not running. Start it with `rize app start`.",
            );
        }
        if !self.data_dir.join(paths::DATABASE_FILE).is_file() {
            return Response::error(code::NO_DATA, NO_DATA);
        }
        match state::lock_stores(&self.data_dir, false) {
            Ok(Some(lock)) => {
                let response = match AppState::open_existing(&self.data_dir) {
                    Ok(state) => rpc::answer(&state, None, &self.data_dir, request),
                    Err(error) => Response::failure(error),
                };
                drop(lock);
                response
            }
            // The app was starting while we looked; it has the stores now.
            Ok(None) => match self.wait_for_app(LAUNCH_TIMEOUT) {
                Ok(()) => self
                    .endpoint
                    .call(&request)
                    .unwrap_or_else(|error| Response::error(code::FAILED, error)),
                Err(error) => Response::error(code::FAILED, error),
            },
            Err(error) => Response::error(code::FAILED, error.to_string()),
        }
    }

    /// Starts the app in the background (or with its window, `show`) unless
    /// it is already running, and waits until it answers. Returns whether
    /// this call launched it.
    pub fn start_app(&self, show: bool) -> Result<bool, String> {
        if self.running() {
            return Ok(false);
        }
        let executable = app_executable().map_err(|error| {
            if self.data_dir.join(paths::DATABASE_FILE).is_file() {
                error
            } else {
                NO_DATA.into()
            }
        })?;
        check_version(&executable)?;
        launch(&executable, show)?;
        self.wait_for_app(LAUNCH_TIMEOUT)?;
        Ok(true)
    }

    /// Asks the running app to quit, and returns once it has let go of the
    /// data, so the next command already finds it closed.
    pub fn quit_app(&self) -> Response {
        let response = self.request(Operation::AppQuit {});
        if !response.ok {
            return response;
        }
        let deadline = Instant::now() + QUIT_TIMEOUT;
        while Instant::now() < deadline {
            if !self.running() && matches!(state::lock_stores(&self.data_dir, false), Ok(Some(_))) {
                return response;
            }
            thread::sleep(Duration::from_millis(100));
        }
        Response::error(code::FAILED, "OpenRize did not quit in time")
    }

    pub fn hello(&self) -> Result<Hello, String> {
        let response = self.request(Operation::Hello {});
        match (response.data, response.error) {
            (Some(data), _) => serde_json::from_value(data).map_err(|e| e.to_string()),
            (_, Some(error)) => Err(error.message),
            _ => Err("empty answer".into()),
        }
    }

    fn wait_for_app(&self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.running() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err("OpenRize did not start answering in time".into())
    }
}

/// The OpenRize binary to run: `RIZE_APP`, else the `openrize` next to rize
/// (rize bundled in the app, or a build directory), else the installed app,
/// for a rize installed on its own.
pub fn app_executable() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(APP_ENV) {
        return Ok(PathBuf::from(path));
    }
    let name = format!("openrize{}", std::env::consts::EXE_SUFFIX);
    let sibling = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map(|rize| rize.with_file_name(&name));
    sibling
        .into_iter()
        .chain(installed_apps())
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            "Could not find the OpenRize app. Install it, or set RIZE_APP to its executable."
                .to_string()
        })
}

/// Where an installed OpenRize usually lives.
#[cfg(target_os = "macos")]
fn installed_apps() -> Vec<PathBuf> {
    let executable = |app: PathBuf| app.join("Contents/MacOS/openrize");
    let mut apps = vec![executable(PathBuf::from("/Applications/openrize.app"))];
    if let Some(home) = std::env::var_os("HOME") {
        apps.push(executable(
            PathBuf::from(home).join("Applications/openrize.app"),
        ));
    }
    // Anywhere else Spotlight knows the app to be.
    if let Ok(found) = Command::new("/usr/bin/mdfind")
        .arg(format!(
            "kMDItemCFBundleIdentifier == '{}'",
            openrize_core::APP_IDENTIFIER
        ))
        .output()
    {
        apps.extend(
            String::from_utf8_lossy(&found.stdout)
                .lines()
                .map(|line| executable(PathBuf::from(line))),
        );
    }
    apps
}

/// The per-user install folder the Windows installer uses, then a machine-wide one.
#[cfg(windows)]
fn installed_apps() -> Vec<PathBuf> {
    ["LOCALAPPDATA", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join("openrize").join("openrize.exe"))
        .collect()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn installed_apps() -> Vec<PathBuf> {
    Vec::new()
}

/// The first app version that serves rize and starts in the background. An
/// older app would only open its window, so rize never launches one.
const FIRST_RIZE_VERSION: (u64, u64, u64) = (0, 8, 10);

/// Refuses an installed app too old for rize to launch. Only a macOS bundle
/// records its version; elsewhere rize is built with its app.
fn check_version(executable: &Path) -> Result<(), String> {
    let Some(bundle) = bundle(executable).filter(|_| cfg!(target_os = "macos")) else {
        return Ok(());
    };
    let output = Command::new("/usr/bin/defaults")
        .arg("read")
        .arg(bundle.join("Contents/Info"))
        .arg("CFBundleShortVersionString")
        .output()
        .map_err(|e| e.to_string())?;
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parts: Vec<u64> = version
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect();
    match parts.as_slice() {
        [major, minor, patch] if (*major, *minor, *patch) >= FIRST_RIZE_VERSION => Ok(()),
        _ => Err(format!(
            "The OpenRize app at {} is version {version}, too old for rize. Update the app to v{}.{}.{} or later.",
            bundle.display(),
            FIRST_RIZE_VERSION.0,
            FIRST_RIZE_VERSION.1,
            FIRST_RIZE_VERSION.2
        )),
    }
}

/// The `.app` bundle an executable sits in, if any.
fn bundle(executable: &Path) -> Option<&Path> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.ends_with("Contents/MacOS")
        && bundle
            .extension()
            .is_some_and(|extension| extension == "app"))
    .then_some(bundle)
}

fn launch(executable: &Path, show: bool) -> Result<(), String> {
    let mut command = match bundle(executable).filter(|_| cfg!(target_os = "macos")) {
        // Through LaunchServices, as a Finder launch would be; `-g` keeps the
        // terminal in front.
        Some(bundle) => {
            let mut command = Command::new("/usr/bin/open");
            if !show {
                command.arg("-g");
            }
            command.arg("-a").arg(bundle);
            if !show {
                command.args(["--args", "--background"]);
            }
            command
        }
        None => {
            let mut command = Command::new(executable);
            if !show {
                command.arg("--background");
            }
            detach(&mut command);
            command
        }
    };
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", executable.display()))?;
    drop(status);
    Ok(())
}

/// Runs the app outside this terminal's session, so closing the terminal
/// does not end it.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is async-signal-safe and touches no Rust state.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    /// No console, and outside the caller's Ctrl+C group.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}
