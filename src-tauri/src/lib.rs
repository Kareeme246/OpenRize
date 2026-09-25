mod activity;
mod ai;
mod capture;
mod commands;
mod entry_builder;
mod migrations;
mod models;
mod settings;
mod timers;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;
use tauri::{Manager, RunEvent, WindowEvent};

use activity::ActivityStore;
use settings::{CloseBehavior, Settings, SettingsStore};
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

/// Emitted whenever preferences change, so any open view (and the tray) can
/// re-read them without polling.
pub const EVENT_SETTINGS_CHANGED: &str = "settings-changed";

/// Emitted whenever time entries are modified or rebuilt.
pub const EVENT_ENTRIES_CHANGED: &str = "entries-changed";

/// Shared application state.
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
    pub settings: Mutex<SettingsStore>,
}

impl AppState {
    pub fn settings_snapshot(&self) -> Settings {
        self.settings
            .lock()
            .map(|store| store.snapshot())
            .unwrap_or_default()
    }
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
            let settings = SettingsStore::load(&settings::config_dir())
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let preferences = settings.snapshot();

            app.manage(AppState {
                store: Mutex::new(store),
                activity: Mutex::new(activity),
                activity_reader: Mutex::new(activity_reader),
                foreground: AtomicBool::new(true),
                settings: Mutex::new(settings),
            });
            app.manage(ai::AiRuntime::default());

            if preferences.tray_enabled {
                tray::init(app.handle(), &timers)?;
            }
            activity::spawn_sampler(app.handle().clone());
            ai::worker::spawn(
                app.handle().clone(),
                dir.join(activity::DB_FILE),
                dir.join("ml"),
            );
            capture::register_sleep_listeners(app.handle().clone());
            spawn_retention_sweeper(app.handle().clone());
            // One sweep on startup, so a long-dormant install is cleaned before
            // the first hour-long wait elapses.
            if let Err(error) = commands::sweep_retention(app.handle()) {
                eprintln!("retention sweep failed: {error}");
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing is a preference: either end the app, or hide to the menu
            // bar so a background tracker keeps running. With the tray off a
            // hidden window would be unreachable, so that combination quits.
            WindowEvent::CloseRequested { api, .. } => {
                let app = window.app_handle();
                let preferences = app.state::<AppState>().settings_snapshot();
                api.prevent_close();
                if preferences.tray_enabled && preferences.close_behavior == CloseBehavior::Hide {
                    let _ = window.hide();
                } else {
                    app.exit(0);
                }
            }
            // Drives the activity push cadence (1Hz focused / 30s
            // backgrounded — see activity.rs)
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
            commands::get_settings,
            commands::update_settings,
            commands::storage_paths,
            commands::list_categories,
            commands::create_category,
            commands::update_category,
            commands::delete_category,
            commands::list_projects,
            commands::create_project,
            commands::update_project,
            commands::delete_project,
            commands::list_clients,
            commands::create_client,
            commands::update_client,
            commands::delete_client,
            commands::list_time_entries,
            commands::get_entry_detail,
            commands::update_time_entry,
            commands::approve_time_entries,
            commands::reject_time_entry,
            commands::split_time_entry,
            commands::delete_time_entry,
            commands::create_time_entry,
            commands::rebuild_time_entries,
            commands::list_apps,
            commands::update_app,
            commands::ai_status,
            commands::retry_classification,
            commands::resolve_rule_suggestion,
            commands::ai_metrics,
            commands::ai_retrain,
            commands::ai_reset_learned,
        ])
        .build(tauri::generate_context!())
        .expect("error while building OpenRize")
        .run(|app, event| {
            // macOS: clicking the dock icon with no visible window reopens it.
            if let RunEvent::Reopen { .. } = event {
                tray::show_main_window(app);
            }
        });
}

/// Deletes activity history past the retention window once an hour. Startup
/// does the first pass; this keeps a process open.
fn spawn_retention_sweeper(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3600));
        if let Err(error) = commands::sweep_retention(&app) {
            eprintln!("retention sweep failed: {error}");
        }
    });
}
