//! In-app updates.
//!
//! `tauri-plugin-updater` reads `latest.json` from the newest GitHub Release
//! (written by `.github/workflows/release.yml`), verifies the downloaded
//! `openrize.app.tar.gz` against the minisign public key in `tauri.conf.json`,
//! and swaps the bundle in place. This module owns *when* that happens and
//! what the UI sees:
//!
//! - **Schedule.** A check 10s after launch, then hourly, plus one whenever a
//!   window regains focus if the last check is at least 5 minutes old.
//! - **Status.** One [`UpdateStatus`] snapshot, returned by `update_status`
//!   and pushed on every change as [`EVENT_UPDATE_STATUS`].
//! - **Changelog.** `latest.json` only carries the newest release's notes, so
//!   the notes of every release between the installed and the offered version
//!   come from the GitHub Releases API, falling back to `latest.json`'s notes.
//! - **Install.** Only on request. The restart goes through
//!   [`AppHandle::request_restart`], so `RunEvent::Exit` still closes the open
//!   activity segment and the relaunched app starts a fresh one.
//!
//! Debug builds never check: installing would overwrite `target/debug`. Set
//! `OPENRIZE_SIMULATE_UPDATE=1` there to exercise the UI with a fake update.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::timers::now_epoch_ms;

/// Emitted with the full [`UpdateStatus`] whenever it changes.
pub const EVENT_UPDATE_STATUS: &str = "update-status";

const REPOSITORY: &str = "Kareeme246/OpenRize";
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
const FOCUS_CHECK_MIN_GAP: Duration = Duration::from_secs(5 * 60);
/// Download progress is pushed at most this often.
const PROGRESS_EMIT_GAP: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    /// When the last check finished, successfully or not.
    pub last_checked_ms: Option<u64>,
    pub phase: UpdatePhase,
    /// A newer version that can be installed, if the last successful check
    /// found one.
    pub available: Option<AvailableUpdate>,
    /// Why the last manual check or install failed. Automatic checks only log,
    /// so an offline laptop doesn't show a stale error.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdatePhase {
    Idle,
    Checking,
    Downloading { downloaded: u64, total: Option<u64> },
    Installing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    pub version: String,
    /// Every release newer than the installed version, newest first.
    pub releases: Vec<ReleaseNotes>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotes {
    pub version: String,
    /// RFC 3339 publish time, when known.
    pub published_at: Option<String>,
    pub notes: String,
}

pub struct UpdaterState {
    status: Mutex<UpdateStatus>,
    /// The plugin's handle for `available`, kept to install it later.
    pending: Mutex<Option<Update>>,
    /// When the last check started. Starts at launch, so the focus a fresh
    /// launch gets doesn't jump ahead of the delayed first check.
    last_attempt: Mutex<Instant>,
}

impl UpdaterState {
    pub fn new(current_version: String) -> Self {
        Self {
            status: Mutex::new(UpdateStatus {
                current_version,
                last_checked_ms: None,
                phase: UpdatePhase::Idle,
                available: None,
                error: None,
            }),
            pending: Mutex::new(None),
            last_attempt: Mutex::new(Instant::now()),
        }
    }

    pub fn snapshot(&self) -> Result<UpdateStatus, String> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|error| error.to_string())
    }
}

/// Starts the launch + hourly schedule.
pub fn spawn_scheduler(app: AppHandle) {
    if cfg!(debug_assertions) {
        if std::env::var_os("OPENRIZE_SIMULATE_UPDATE").is_some() {
            simulate(&app);
        }
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_DELAY);
        loop {
            tauri::async_runtime::block_on(check(&app, false));
            std::thread::sleep(CHECK_INTERVAL);
        }
    });
}

/// Called when an OpenRize window regains focus.
pub fn on_focus(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let state = app.state::<UpdaterState>();
    let Ok(last_attempt) = state.last_attempt.lock() else {
        return;
    };
    if last_attempt.elapsed() < FOCUS_CHECK_MIN_GAP {
        return;
    }
    drop(last_attempt);
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check(&app, false).await });
}

/// Checks for a newer release. `manual` checks surface their errors in the
/// status; automatic ones only log them.
pub async fn check(app: &AppHandle, manual: bool) {
    let state = app.state::<UpdaterState>();
    if cfg!(debug_assertions) {
        if let (true, Ok(mut status)) = (manual, state.status.lock()) {
            status.error = Some("Development builds don't check for updates.".to_string());
            emit(app, &status);
        }
        return;
    }
    if !begin(app, UpdatePhase::Checking) {
        return;
    }
    if let Ok(mut last_attempt) = state.last_attempt.lock() {
        *last_attempt = Instant::now();
    }

    let result = find_update(app).await;

    let Ok(mut pending) = state.pending.lock() else {
        return;
    };
    let Ok(mut status) = state.status.lock() else {
        return;
    };
    status.phase = UpdatePhase::Idle;
    status.last_checked_ms = Some(now_epoch_ms());
    match result {
        Ok(found) => {
            status.error = None;
            match found {
                Some((update, releases)) => {
                    status.available = Some(AvailableUpdate {
                        version: update.version.clone(),
                        releases,
                    });
                    *pending = Some(update);
                }
                None => {
                    status.available = None;
                    *pending = None;
                }
            }
        }
        Err(error) => {
            eprintln!("update check failed: {error}");
            if manual {
                status.error = Some(format!("Couldn't check for updates: {error}"));
            }
        }
    }
    emit(app, &status);
}

/// Downloads and installs the pending update, then restarts into it. Only
/// returns on failure; the error is also left in the status.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<UpdaterState>();
    let update = state
        .pending
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or_else(|| "There's no update to install.".to_string())?;
    if !begin(
        app,
        UpdatePhase::Downloading {
            downloaded: 0,
            total: None,
        },
    ) {
        return Err("An update check or install is already running.".to_string());
    }

    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();
    let result = update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let done = total.is_some_and(|total| downloaded >= total);
                if done || last_emit.elapsed() >= PROGRESS_EMIT_GAP {
                    last_emit = Instant::now();
                    set_phase(app, UpdatePhase::Downloading { downloaded, total });
                }
            },
            || set_phase(app, UpdatePhase::Installing),
        )
        .await;

    match result {
        Ok(()) => {
            app.request_restart();
            Ok(())
        }
        Err(error) => {
            let message = format!("Couldn't install the update: {error}");
            eprintln!("{message}");
            if let Ok(mut status) = state.status.lock() {
                status.phase = UpdatePhase::Idle;
                status.error = Some(message.clone());
                emit(app, &status);
            }
            Err(message)
        }
    }
}

/// Moves from `Idle` into `phase`; false when something else is running.
fn begin(app: &AppHandle, phase: UpdatePhase) -> bool {
    let state = app.state::<UpdaterState>();
    let Ok(mut status) = state.status.lock() else {
        return false;
    };
    if status.phase != UpdatePhase::Idle {
        return false;
    }
    status.phase = phase;
    status.error = None;
    emit(app, &status);
    true
}

fn set_phase(app: &AppHandle, phase: UpdatePhase) {
    let state = app.state::<UpdaterState>();
    if let Ok(mut status) = state.status.lock() {
        status.phase = phase;
        emit(app, &status);
    };
}

fn emit(app: &AppHandle, status: &UpdateStatus) {
    let _ = app.emit(EVENT_UPDATE_STATUS, status);
}

async fn find_update(app: &AppHandle) -> Result<Option<(Update, Vec<ReleaseNotes>)>, String> {
    let updater = app.updater().map_err(|error| error.to_string())?;
    let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
        return Ok(None);
    };

    // Reuse the notes already fetched for this version: the hourly check
    // shouldn't spend the GitHub API's unauthenticated rate limit.
    let known = app
        .state::<UpdaterState>()
        .snapshot()
        .ok()
        .and_then(|status| status.available)
        .filter(|available| available.version == update.version)
        .map(|available| available.releases);
    let releases = match known {
        Some(releases) => releases,
        None => match release_notes_between(&update.current_version, &update.version).await {
            Ok(releases) if !releases.is_empty() => releases,
            result => {
                if let Err(error) = result {
                    eprintln!("fetching release notes failed: {error}");
                }
                vec![ReleaseNotes {
                    version: update.version.clone(),
                    published_at: None,
                    notes: update.body.clone().unwrap_or_default(),
                }]
            }
        },
    };
    Ok(Some((update, releases)))
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    body: Option<String>,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
}

/// Notes of every published release in `(current, latest]`, newest first.
async fn release_notes_between(current: &str, latest: &str) -> Result<Vec<ReleaseNotes>, String> {
    let current = semver::Version::parse(current).map_err(|error| error.to_string())?;
    let latest = semver::Version::parse(latest).map_err(|error| error.to_string())?;

    // reqwest is built without a bundled crypto provider, like the updater's.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("OpenRize/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let releases: Vec<GithubRelease> = client
        .get(format!(
            "https://api.github.com/repos/{REPOSITORY}/releases?per_page=100"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;

    let mut notes: Vec<(semver::Version, ReleaseNotes)> = releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let version = semver::Version::parse(release.tag_name.trim_start_matches('v')).ok()?;
            (version > current && version <= latest).then(|| {
                let notes = ReleaseNotes {
                    version: version.to_string(),
                    published_at: release.published_at,
                    notes: release.body.unwrap_or_default().trim().to_string(),
                };
                (version, notes)
            })
        })
        .collect();
    notes.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(notes.into_iter().map(|(_, notes)| notes).collect())
}

/// Debug builds only: pretend an update is available so the UI can be seen.
fn simulate(app: &AppHandle) {
    let state = app.state::<UpdaterState>();
    let Ok(mut status) = state.status.lock() else {
        return;
    };
    status.last_checked_ms = Some(now_epoch_ms());
    status.available = Some(AvailableUpdate {
        version: "9.9.9".to_string(),
        releases: vec![
            ReleaseNotes {
                version: "9.9.9".to_string(),
                published_at: Some("2026-09-28T12:00:00Z".to_string()),
                notes: "### Features\n- **updates:** Simulated release notes for the \
                        in-app updater\n\n### Bug Fixes\n- **capture:** A second \
                        simulated line that is long enough to wrap onto another line \
                        in the Settings changelog"
                    .to_string(),
            },
            ReleaseNotes {
                version: "9.9.8".to_string(),
                published_at: Some("2026-09-20T12:00:00Z".to_string()),
                notes: "### Bug Fixes\n- **calendar:** An older simulated release".to_string(),
            },
        ],
    });
}
