mod activity;
mod agents;
mod ai;
mod breaks;
mod capture;
mod commands;
mod energy;
mod invoices;
mod login_item;
mod pulse;
mod rpc;
mod threads;
mod tray;
mod updater;

use openrize_core::{entry_builder, models, projects, reports, settings, timers};

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{Manager, RunEvent, WindowEvent};

use settings::CloseBehavior;

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

/// Sent to the main window when the break reminder asks for its settings.
pub const EVENT_OPEN_BREAK_SETTINGS: &str = "open-break-settings";

/// Sent to the main window when the Pulse panel's review button asks it to
/// open today's review queue.
pub const EVENT_OPEN_REVIEW: &str = "open-review";

pub use openrize_core::state::AppState;

/// Keep the standalone command in sync with the app release. `rize` ships
/// beside the app executable and the replacement is atomic for running shells.
#[cfg(target_os = "macos")]
fn install_bundled_cli() {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let bundled = std::env::current_exe()?.with_file_name("rize");
        if !bundled.is_file() {
            return Ok(());
        }
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        let directory = std::path::PathBuf::from(home).join(".local/bin");
        std::fs::create_dir_all(&directory)?;
        let destination = directory.join("rize");
        let temporary = directory.join(format!(".rize-{}", std::process::id()));
        let _ = std::fs::remove_file(&temporary);
        std::os::unix::fs::symlink(&bundled, &temporary)?;
        std::fs::rename(&temporary, &destination)?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("could not update ~/.local/bin/rize: {error}");
    }
}

#[cfg(not(target_os = "macos"))]
fn install_bundled_cli() {}

/// Launched by `rize app start`: run in the menu bar with no window until
/// one is asked for.
const BACKGROUND_ARG: &str = "--background";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let background = std::env::args().any(|arg| arg == BACKGROUND_ARG);
    tauri::Builder::default()
        // Registered first so a second launch exits before it opens the
        // database; the running copy comes forward instead.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|arg| arg == BACKGROUND_ARG) {
                tray::show_main_window(app);
            }
        }))
        .plugin(
            // Seeds the page's theme/shape hint before its first paint; see
            // `settings::appearance_init_script`.
            tauri::plugin::Builder::<tauri::Wry>::new("appearance")
                .js_init_script(settings::appearance_init_script(&settings::config_dir()))
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            install_bundled_cli();
            let dir = app.path().app_data_dir()?;
            // Waits out a `rize` edit made while the app was closed, then
            // keeps the stores for as long as the app runs.
            std::fs::create_dir_all(&dir)?;
            let lock = openrize_core::state::lock_stores(&dir, true)?.expect("a blocking lock");
            app.manage(rpc::StoresLock(lock));

            let state = AppState::load(&dir)
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            let timers = state.store.lock().map_err(|e| e.to_string())?.snapshot()?;
            let preferences = state.settings_snapshot();
            app.manage(state);
            app.manage(ai::AiRuntime::default());
            app.manage(pulse::PulseState::default());
            app.manage(agents::AgentRuntime::default());
            breaks::init(app.handle())?;
            app.manage(updater::UpdaterState::new(
                app.package_info().version.to_string(),
            ));

            if preferences.tray_enabled {
                tray::init(app.handle(), &timers)?;
            }
            activity::spawn_sampler(app.handle().clone());
            ai::worker::spawn(
                app.handle().clone(),
                dir.join(activity::DB_FILE),
                dir.join("ml"),
            );
            energy::spawn_sampler(app.handle().clone(), dir.join(activity::DB_FILE));
            agents::spawn(app.handle().clone(), dir.join(activity::DB_FILE));
            capture::register_sleep_listeners(app.handle().clone());
            spawn_retention_sweeper(app.handle().clone());
            updater::spawn_scheduler(app.handle().clone());
            // One sweep on startup, so a long-dormant install is cleaned before
            // the first hour-long wait elapses.
            if let Err(error) = commands::sweep_retention(app.handle()) {
                eprintln!("retention sweep failed: {error}");
            }
            rpc::serve(app.handle().clone(), dir);
            // The window is created hidden (tauri.conf.json), so a background
            // launch never flashes it.
            if background {
                #[cfg(target_os = "macos")]
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            } else {
                tray::show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // The panel has no close control; anything that asks just hides it.
            WindowEvent::CloseRequested { api, .. } if window.label() == pulse::LABEL => {
                api.prevent_close();
                let _ = window.hide();
            }
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
            // Drives the activity push cadence (1Hz while any OpenRize window
            // is focused / 30s otherwise — see activity.rs). Recomputed from
            // every window, so the order of one window's blur and the next
            // one's focus does not matter.
            WindowEvent::Focused(is_focused) => {
                let app = window.app_handle();
                if !*is_focused && window.label() == pulse::LABEL {
                    pulse::on_blur(app);
                }
                let focused = app
                    .webview_windows()
                    .values()
                    .any(|window| window.is_focused().unwrap_or(false));
                let state = window.state::<AppState>();
                let was_foreground = state.foreground.swap(focused, Ordering::SeqCst);
                if focused && !was_foreground {
                    activity::emit_full(app);
                    updater::on_focus(app);
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
            commands::launch_at_login,
            commands::set_launch_at_login,
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
            // Local invoice lifecycle.
            commands::list_invoices,
            commands::get_invoice,
            commands::list_billable_entries,
            commands::quote_invoice,
            commands::render_invoice_preview,
            commands::get_invoice_pdf,
            commands::save_invoice_draft,
            commands::finalize_invoice,
            commands::set_invoice_paid,
            commands::void_invoice,
            commands::delete_draft_invoice,
            commands::export_invoice_pdf,
            commands::get_invoice_profile,
            commands::update_invoice_profile,
            commands::get_invoice_logo,
            commands::set_invoice_logo,
            commands::clear_invoice_logo,
            commands::list_time_entries,
            commands::get_entry_detail,
            commands::update_time_entry,
            commands::approve_time_entries,
            commands::unapprove_time_entries,
            commands::reject_time_entry,
            commands::split_time_entry,
            commands::delete_time_entry,
            commands::delete_time_entries,
            commands::undo_deletion,
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
            commands::query_time_entries,
            commands::entry_rollup,
            commands::export_time_entries,
            commands::update_time_entries,
            commands::project_stats,
            commands::project_rules,
            commands::preview_project_hints,
            commands::discover_projects,
            commands::dismiss_project_suggestion,
            commands::import_projects_csv,
            commands::get_energy_summary,
            commands::query_energy_history,
            commands::reset_energy_history,
            commands::resize_pulse_panel,
            commands::hide_pulse_panel,
            commands::break_state,
            commands::start_break,
            commands::end_break,
            commands::snooze_break,
            commands::skip_break,
            commands::expand_break_reminder,
            commands::extend_break,
            commands::pause_break_reminders,
            commands::list_breaks,
            commands::resize_reminder_panel,
            #[cfg(debug_assertions)]
            commands::dev_sample_break_reminder,
            commands::open_break_settings,
            commands::preview_break_chime,
            commands::open_main_window,
            commands::update_status,
            commands::check_for_updates,
            commands::install_update,
            commands::list_extensions,
            commands::agent_board,
            commands::agent_report,
            commands::thread_days,
            commands::confirm_agent_job,
            commands::list_agent_entries,
        ])
        .build(tauri::generate_context!())
        .expect("error while building OpenRize")
        .run(|app, event| match event {
            // macOS: clicking the dock icon with no visible window reopens it.
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => tray::show_main_window(app),
            // Close the open segment at the real quit time. Left open, the
            // next launch can only close it at its own start, losing its time.
            RunEvent::Exit => {
                breaks::on_exit(app);
                if let Ok(mut store) = app.state::<AppState>().activity.lock() {
                    let _ = store.close_active_segment(timers::now_epoch_ms());
                }
            }
            _ => {}
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

#[cfg(test)]
mod cli_tests {
    #[test]
    fn independent_client_identity_matches_the_release_bundle() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], openrize_core::APP_IDENTIFIER);
    }
}
