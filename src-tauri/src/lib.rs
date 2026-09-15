mod commands;
mod timers;
mod tray;

use std::sync::Mutex;

use tauri::{Manager, RunEvent, WindowEvent};

use timers::TimerStore;

/// Emitted after every timer mutation so the frontend can adopt the snapshot
/// Rust already has — the tray can change state without the window asking.
pub const EVENT_TIMERS_CHANGED: &str = "timers-changed";

/// Shared application state. `Mutex` rather than `RwLock`: mutations are the
/// common case and the critical sections are microseconds long.
pub struct AppState {
    pub store: Mutex<TimerStore>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;

            let store = TimerStore::load(&dir);
            let timers = store.snapshot();
            app.manage(AppState {
                store: Mutex::new(store),
            });

            tray::init(app.handle(), &timers)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window must not end a background tracker. Hide it;
            // the tray's "Quit OpenRize" is the deliberate exit.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_timers,
            commands::create_timer,
            commands::start_timer,
            commands::pause_timer,
            commands::reset_timer,
            commands::rename_timer,
            commands::delete_timer,
        ])
        .build(tauri::generate_context!())
        .expect("error while building OpenRize")
        .run(|app, event| {
            // macOS: clicking the dock icon with no visible window reopens it.
            // If this proves unreliable on some macOS versions, the tray's
            // "Open OpenRize" item is the guaranteed path — do not rabbit-hole.
            if let RunEvent::Reopen { .. } = event {
                tray::show_main_window(app);
            }
        });
}
