//! Reaching OpenRize's data through the running app's local endpoint, or,
//! with the app closed, straight from its stores. rize never creates the data.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use openrize_core::accounts;
use openrize_core::data_location;
use openrize_core::ipc::{self, Endpoint};
use openrize_core::paths;
use openrize_core::protocol::{code, Hello, Operation, Request, Response};
use openrize_core::rpc;
use openrize_core::state::{self, AppState, NO_DATA};

const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
const QUIT_TIMEOUT: Duration = Duration::from_secs(30);
const APP_ENV: &str = "RIZE_APP";

struct Target {
    data_dir: PathBuf,
    endpoint: Endpoint,
}

pub struct Client {
    target: Mutex<Target>,
    /// Whether the folder was derived (no `--data-dir`); it can change when
    /// `start_app` moves the data on its first run.
    follows_app: bool,
}

impl Client {
    pub fn new(data_dir: Option<&Path>) -> Result<Self, String> {
        Self::in_root(data_dir, paths::app_data_dir())
    }

    fn in_root(data_dir: Option<&Path>, root: Option<PathBuf>) -> Result<Self, String> {
        let follows_app = data_dir.is_none();
        let data_dir = match data_dir {
            Some(dir) => std::path::absolute(dir).map_err(|e| e.to_string())?,
            // The account the app remembers, else the data directory (No account).
            None => accounts::resolve(&root.ok_or("no home directory; pass --data-dir")?).dir,
        };
        let endpoint = Endpoint::new(&data_dir)?;
        Ok(Self {
            target: Mutex::new(Target { data_dir, endpoint }),
            follows_app,
        })
    }

    fn target(&self) -> MutexGuard<'_, Target> {
        self.target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn data_dir(&self) -> PathBuf {
        self.target().data_dir.clone()
    }

    /// An updated Windows app's first run moves the data from the roaming folder
    /// to the local one; looks again so a rize that started the app follows.
    fn follow_app(&self) {
        if !self.follows_app {
            return;
        }
        let Some(root) = paths::app_data_dir() else {
            return;
        };
        let data_dir = accounts::resolve(&root).dir;
        let mut target = self.target();
        if data_dir != target.data_dir {
            if let Ok(endpoint) = Endpoint::new(&data_dir) {
                *target = Target { data_dir, endpoint };
            }
        }
    }

    pub fn running(&self) -> bool {
        self.target().endpoint.connect().is_ok()
    }

    /// An operation that needs the app fails with `APP_NOT_RUNNING` when it is closed.
    pub fn request(&self, operation: Operation) -> Response {
        let request = Request::new(operation);
        let connected = self.target().endpoint.connect();
        if let Ok(stream) = connected {
            return ipc::exchange(stream, &request)
                .unwrap_or_else(|error| Response::error(code::FAILED, error));
        }
        if request.operation.needs_app() {
            return Response::error(
                code::APP_NOT_RUNNING,
                "OpenRize is not running. Start it with `rize app start`.",
            );
        }
        // A move the app did not finish leaves the database possibly apart
        // from its journal; only the app, starting, may touch it then.
        if self.follows_app
            && paths::local_data_dir().is_some_and(|local| data_location::move_unfinished(&local))
        {
            return Response::error(
                code::FAILED,
                "OpenRize was stopped while moving its data to a new folder. \
                 Start the app with `rize app start` to finish the move.",
            );
        }
        let data_dir = self.data_dir();
        if !data_dir.join(paths::DATABASE_FILE).is_file() {
            return Response::error(code::NO_DATA, NO_DATA);
        }
        match state::lock_stores(&data_dir, false) {
            Ok(Some(lock)) => {
                let response = match AppState::open_existing(&data_dir) {
                    Ok(state) => rpc::answer(&state, None, &data_dir, request),
                    Err(error) => Response::failure(error),
                };
                drop(lock);
                response
            }
            Ok(None) => match self.wait_for_app(LAUNCH_TIMEOUT) {
                Ok(()) => self
                    .target()
                    .endpoint
                    .call(&request)
                    .unwrap_or_else(|error| Response::error(code::FAILED, error)),
                Err(error) => Response::error(code::FAILED, error),
            },
            Err(error) => Response::error(code::FAILED, error.to_string()),
        }
    }

    /// Returns whether this call launched the app.
    pub fn start_app(&self, show: bool) -> Result<bool, String> {
        if self.running() {
            return Ok(false);
        }
        let executable = app_executable().map_err(|error| {
            if self.data_dir().join(paths::DATABASE_FILE).is_file() {
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

    /// Returns once the app has let go of the data.
    pub fn quit_app(&self) -> Response {
        let response = self.request(Operation::AppQuit {});
        if !response.ok {
            return response;
        }
        let deadline = Instant::now() + QUIT_TIMEOUT;
        while Instant::now() < deadline {
            if !self.running() && matches!(state::lock_stores(&self.data_dir(), false), Ok(Some(_)))
            {
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
            self.follow_app();
            if self.running() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err("OpenRize did not start answering in time".into())
    }
}

/// `RIZE_APP`, else the `openrize` next to rize, else the installed app.
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

/// NTFS ignores case, so installs from before the folder was `OpenRize` match too.
#[cfg(windows)]
fn installed_apps() -> Vec<PathBuf> {
    ["LOCALAPPDATA", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join("OpenRize").join("openrize.exe"))
        .collect()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn installed_apps() -> Vec<PathBuf> {
    Vec::new()
}

/// The first app version that serves rize and starts in the background. An
/// older app would only open its window, so rize never launches one.
const FIRST_RIZE_VERSION: (u64, u64, u64) = (0, 8, 10);

/// Refuses an installed app too old to launch. Only a macOS bundle records its version.
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

/// Runs the app outside this terminal's session so closing it does not end the app.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_data_dir_rize_reaches_the_account_the_app_remembers() {
        let root = std::env::temp_dir().join(format!("rize-client-account-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let before = Client::in_root(None, Some(root.clone())).unwrap();
        assert_eq!(before.data_dir(), root);

        let account = accounts::create(&root, "Kareem").unwrap();
        accounts::set_current(&root, Some(&account.id)).unwrap();
        let open = Client::in_root(None, Some(root.clone())).unwrap();
        assert_eq!(
            open.data_dir(),
            accounts::data_dir(&root, Some(&account.id))
        );

        accounts::set_current(&root, None).unwrap();
        assert_eq!(
            Client::in_root(None, Some(root.clone()))
                .unwrap()
                .data_dir(),
            root
        );
        let other = root.join("elsewhere");
        let explicit = Client::in_root(Some(&other), Some(root.clone())).unwrap();
        assert_eq!(explicit.data_dir(), other);
        let _ = std::fs::remove_dir_all(&root);
    }
}
