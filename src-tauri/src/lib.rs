mod activity;
mod commands;
mod timers;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{Manager, RunEvent, WindowEvent};

use activity::ActivityStore;
use timers::TimerStore;

/// Emitted after every timer mutation so the frontend can adopt the snapshot
/// Rust already has — the tray can change state without the window asking.
pub const EVENT_TIMERS_CHANGED: &str = "timers-changed";

/// Emitted on every structural activity change (a segment opened or closed)
/// and on focus-regained reconciliation. Carries the full `ActivitySnapshot`
/// — see activity.rs's module doc for the push-model rationale.
pub const EVENT_ACTIVITY_CHANGED: &str = "activity-changed";

/// Emitted at 1Hz while OpenRize is focused, or every 30s while it isn't and
/// nothing structural has changed. Carries the lighter `ActivityTick`.
pub const EVENT_ACTIVITY_TICK: &str = "activity-tick";

/// Shared application state. `Mutex` rather than `RwLock`: mutations are the
/// common case and the critical sections are microseconds long.
pub struct AppState {
    pub store: Mutex<TimerStore>,
    pub activity: Mutex<ActivityStore>,
    /// Read-only-by-convention connection to the same database, kept off the
    /// writer's mutex so a query never blocks behind (or blocks) the
    /// sampler's tick — see activity.rs's module doc, decision A6.
    pub activity_reader: Mutex<Connection>,
    /// Whether the OpenRize window itself is focused. Drives the push
    /// cadence to the frontend; the underlying sampling rate is unaffected.
    pub foreground: AtomicBool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;

            let store = TimerStore::load(&dir)?;
            let timers = store.snapshot()?;
            let activity = ActivityStore::load(&dir)?;
            let activity_reader = ActivityStore::open_reader(&dir)?;
            app.manage(AppState {
                store: Mutex::new(store),
                activity: Mutex::new(activity),
                activity_reader: Mutex::new(activity_reader),
                foreground: AtomicBool::new(true),
            });

            tray::init(app.handle(), &timers)?;
            activity::spawn_sampler(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the window must not end a background tracker. Hide it;
            // the tray's "Quit OpenRize" is the deliberate exit.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            // Drives the activity push cadence (1Hz focused / 30s
            // backgrounded — see activity.rs) and fires an immediate
            // reconciliation the instant OpenRize regains focus.
            WindowEvent::Focused(is_focused) => {
                let state = window.state::<AppState>();
                let was_foreground = state.foreground.swap(*is_focused, Ordering::SeqCst);
                if *is_focused && !was_foreground {
                    activity::emit_full(window.app_handle());
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_timers,
            commands::create_timer,
            commands::start_timer,
            commands::pause_timer,
            commands::reset_timer,
            commands::rename_timer,
            commands::delete_timer,
            commands::activity_snapshot,
            commands::set_capture_enabled,
            commands::set_idle_threshold,
            commands::start_session,
            commands::stop_session,
            commands::mark_segment_reviewed,
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
