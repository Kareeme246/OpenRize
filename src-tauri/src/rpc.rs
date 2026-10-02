//! Serves `rize` while the app runs. The answers come from
//! `openrize_core::rpc`, the same code `rize` runs itself when the app is
//! closed; the app adds what only it can do (`Live`) and emits the events a
//! window command would, so open windows and the tray update at once.

use std::path::PathBuf;
use std::time::Duration;

use openrize_core::ipc::{self, Endpoint};
use openrize_core::protocol::Operation;
use openrize_core::rpc::{self, Live};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::settings::Settings;
use crate::{activity, commands, AppState};

/// Keeps the stores' lock (`openrize_core::state::lock_stores`) for the app's
/// lifetime.
pub struct StoresLock(#[allow(dead_code)] pub std::fs::File);

/// Serves `rize` on this data directory's endpoint until the app exits.
pub fn serve(app: AppHandle, data_dir: PathBuf) {
    let service = match Endpoint::new(&data_dir).and_then(|endpoint| endpoint.listen()) {
        Ok(service) => service,
        Err(error) => {
            eprintln!("rize endpoint unavailable: {error}");
            return;
        }
    };
    std::thread::spawn(move || loop {
        match service.accept() {
            Ok(stream) => {
                let app = app.clone();
                let data_dir = data_dir.clone();
                std::thread::spawn(move || {
                    ipc::respond(stream, |request| {
                        let state = app.state::<AppState>();
                        rpc::answer(&state, Some(&App(&app)), &data_dir, request)
                    });
                });
            }
            Err(error) => {
                eprintln!("rize endpoint: {error}");
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    });
}

/// The running app, as `openrize_core::rpc` sees it.
struct App<'a>(&'a AppHandle);

impl Live for App<'_> {
    fn open_window(&self, review: bool) {
        commands::open_main_window(self.0.clone(), review);
    }

    fn quit(&self) {
        let app = self.0.clone();
        // Answer first; `exit` runs the same shutdown as the tray's Quit.
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            app.exit(0);
        });
    }

    fn start_focus(&self, label: Option<String>) -> Result<(), String> {
        commands::start_session(self.0.clone(), "focus".into(), label)
    }

    fn stop_focus(&self) -> Result<(), String> {
        commands::stop_session(self.0.clone())
    }

    fn breaks(&self) -> Option<Value> {
        commands::break_state(self.0.clone())
            .ok()
            .and_then(|state| serde_json::to_value(state).ok())
    }

    fn settings_changed(&self, previous: &Settings, next: &Settings) {
        commands::apply_settings_change(self.0, previous, next);
    }

    fn changed(&self, state: &AppState, operation: &Operation) {
        use Operation as O;
        match operation {
            O::TrackSet { .. } | O::TrackIdle { .. } => activity::emit_full(self.0),
            O::TimerCreate { .. }
            | O::TimerStart { .. }
            | O::TimerPause { .. }
            | O::TimerReset { .. }
            | O::TimerRename { .. }
            | O::TimerDelete { .. } => {
                let timers = state
                    .store
                    .lock()
                    .ok()
                    .and_then(|store| store.snapshot().ok());
                if let Some(timers) = timers {
                    commands::refresh_tray(self.0, &timers);
                }
            }
            O::EntryCreate { .. }
            | O::EntriesEdit { .. }
            | O::EntriesApprove { .. }
            | O::EntriesUnapprove { .. }
            | O::EntryReject { .. }
            | O::EntrySplit { .. }
            | O::EntriesDelete { .. }
            | O::EntriesRebuild { .. } => commands::entries_changed(self.0),
            _ => {}
        }
    }
}
