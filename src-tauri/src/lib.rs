mod accounts;
mod activity;
mod ai;
#[cfg(target_os = "macos")]
mod ax;
mod breaks;
mod caption;
mod capture;
mod cli_install;
mod commands;
mod dev_tools;
mod energy;
mod invoices;
mod login_item;
mod net;
mod onboarding;
mod panel;
mod permissions;
mod pulse;
mod rpc;
mod tray;
mod updater;

use openrize_core::{categories, models, projects, reports, settings, timers};

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{Manager, RunEvent, WindowEvent};

use settings::CloseBehavior;

/// The tray can change timer state without the window asking.
pub const EVENT_TIMERS_CHANGED: &str = "timers-changed";

/// Carries every section.
pub const EVENT_TIMER_SECTIONS_CHANGED: &str = "timer-sections-changed";

/// Carries the full `ActivitySnapshot`.
pub const EVENT_ACTIVITY_CHANGED: &str = "activity-changed";

/// 1Hz while focused, else every 30s; carries `ActivityTick`.
pub const EVENT_ACTIVITY_TICK: &str = "activity-tick";

pub const EVENT_SETTINGS_CHANGED: &str = "settings-changed";

pub const EVENT_ENTRIES_CHANGED: &str = "entries-changed";

pub const EVENT_ACCOUNTS_CHANGED: &str = "accounts-changed";

/// Carries a `permissions::AccessibilityState`.
pub const EVENT_PERMISSIONS_CHANGED: &str = "permissions-changed";

pub const EVENT_ONBOARDING_CHANGED: &str = "onboarding-changed";

pub const EVENT_OPEN_BREAK_SETTINGS: &str = "open-break-settings";

pub const EVENT_OPEN_REVIEW: &str = "open-review";

pub use openrize_core::state::AppState;

/// Run in the menu bar with no window until one is asked for.
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
            cli_install::install_bundled_cli();
            // Before the window is shown or any saved size is applied to it.
            if let Some(main) = app.get_webview_window("main") {
                caption::prepare(&main);
            }
            login_item::refresh();
            // Local AppData on Windows; older builds used Roaming, so existing data is
            // moved across first, before any store or lock.
            let root = openrize_core::data_location::adopt(
                &app.path().app_data_dir()?,
                &app.path().app_local_data_dir()?,
            );
            std::fs::create_dir_all(&root)?;
            // Before any store opens: repairs the account list and finishes a requested move.
            let launch = openrize_core::accounts::launch(&root);
            let dir = launch.resolved.dir.clone();
            let handoff = launch.handoff.clone();
            app.manage(accounts::ActiveAccount::new(root, &launch));
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
            app.manage(permissions::Monitor::new());
            app.manage(onboarding::Rerun::default());
            breaks::init(app.handle())?;
            app.manage(updater::UpdaterState::new(
                app.package_info().version.to_string(),
            ));

            if preferences.tray_enabled {
                tray::init(app.handle(), &timers)?;
            }
            permissions::spawn_monitor(app.handle().clone());
            activity::spawn_sampler(app.handle().clone());
            ai::worker::spawn(
                app.handle().clone(),
                dir.join(activity::DB_FILE),
                dir.join("ml"),
            );
            energy::spawn_sampler(app.handle().clone(), dir.join(activity::DB_FILE));
            capture::register_sleep_listeners(app.handle().clone());
            spawn_retention_sweeper(app.handle().clone());
            updater::spawn_scheduler(app.handle().clone());
            // One sweep on startup so a dormant install is cleaned before the first wait.
            if let Err(error) = commands::sweep_retention(app.handle()) {
                eprintln!("retention sweep failed: {error}");
            }
            rpc::serve(app.handle().clone(), dir);
            // A switch restarts with the original arguments, so a window that was
            // showing must come back even if this copy began in the background.
            if let Some(bounds) = handoff.as_ref().and_then(|handoff| handoff.window) {
                accounts::restore_window(app.handle(), bounds);
            }
            let show = handoff.is_some_and(|handoff| handoff.show);
            // The window is created hidden (tauri.conf.json), so a background
            // launch never flashes it.
            if background && !show {
                #[cfg(target_os = "macos")]
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            } else {
                tray::show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // The panels have no close control; anything that asks (Alt+F4, the
            // taskbar) just hides them rather than ending the app.
            WindowEvent::CloseRequested { api, .. }
                if matches!(window.label(), pulse::LABEL | breaks::surface::LABEL) =>
            {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::ThemeChanged(_) => tray::restyle(window.app_handle()),
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
            // 1Hz while any OpenRize window is focused, else 30s. Recomputed from every
            // window so blur/focus ordering across windows does not matter.
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
            caption::set_maximize_button,
            caption::show_system_menu,
            commands::list_timers,
            commands::create_timer,
            commands::start_timer,
            commands::pause_timer,
            commands::reset_timer,
            commands::rename_timer,
            commands::set_timer_tag,
            commands::start_timer_tag,
            commands::pause_timer_tag,
            commands::add_timer_segment,
            commands::update_timer_segment,
            commands::delete_timer_segment,
            commands::save_timer_state,
            commands::list_timer_saved_states,
            commands::delete_timer,
            commands::reorder_timers,
            commands::move_timer,
            commands::move_timer_to_section,
            commands::list_timer_sections,
            commands::create_timer_section,
            commands::rename_timer_section,
            commands::delete_timer_section,
            commands::reorder_timer_sections,
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
            commands::check_permissions,
            commands::accessibility_state,
            commands::request_accessibility,
            onboarding::onboarding_needed,
            onboarding::finish_onboarding,
            dev_tools::dev_tools_info,
            dev_tools::dev_simulate_accessibility,
            dev_tools::dev_simulate_automation,
            dev_tools::dev_rerun_onboarding,
            dev_tools::dev_reset,
            commands::relaunch_app,
            commands::list_categories,
            commands::create_category,
            commands::update_category,
            commands::delete_category,
            commands::remove_category,
            commands::reorder_categories,
            commands::category_stats,
            commands::list_projects,
            commands::create_project,
            commands::update_project,
            commands::delete_project,
            commands::list_clients,
            commands::create_client,
            commands::update_client,
            commands::delete_client,
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
            commands::delete_void_invoice,
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
            commands::merge_time_entries,
            commands::delete_time_entry,
            commands::delete_time_entries,
            commands::undo,
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
            commands::dismiss_stopwatch_reminder,
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
            commands::installed_release_notes,
            commands::install_update,
            accounts::accounts_state,
            accounts::existing_data_summary,
            accounts::switch_preview,
            accounts::switch_account,
            accounts::create_account,
            accounts::rename_account,
            accounts::delete_account,
            accounts::dismiss_missing_account,
            accounts::paused_on_switch,
            accounts::resume_paused,
            accounts::dismiss_paused,
        ])
        .build(tauri::generate_context!())
        .expect("error while building OpenRize")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => tray::show_main_window(app),
            RunEvent::Exit => flush_on_exit(app),
            _ => {}
        });
}

/// Ends an open break and closes the activity segment at the real quit time, or
/// the next launch can only close it at its own start. Shared with the Windows
/// updater, which exits without a `RunEvent::Exit`.
pub(crate) fn flush_on_exit(app: &tauri::AppHandle) {
    breaks::on_exit(app);
    if let Ok(mut store) = app.state::<AppState>().activity.lock() {
        let _ = store.close_active_segment(timers::now_epoch_ms());
    }
}

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
