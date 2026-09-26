//! IPC surface.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::activity::{self, ActivitySnapshot};
use crate::ai::metrics::AiMetrics;
use crate::ai::{self, AiRuntime, AiStatus};
use crate::models::{
    AppRecord, Category, Client, EntryDetail, NewCategory, NewClient, NewProject, NewTimeEntry,
    Project, RuleSuggestion, TimeEntry, UpdateCategory, UpdateClient, UpdateProject,
    UpdateTimeEntry,
};
use crate::projects::{HintPreview, ImportSummary, ProjectRule, ProjectStats, ProjectSuggestion};
use crate::reports::{self, EntryFilter, ExportFormat, ExportResult, GroupBy, RollupCell};
use crate::settings::Settings;
use crate::timers::{now_epoch_ms, Timer};
use crate::tray;
use crate::AppState;

type Timers = Result<Vec<Timer>, String>;

fn refresh_tray(app: &AppHandle, timers: &[Timer]) {
    if let Err(error) = tray::refresh(app, timers) {
        eprintln!("tray refresh failed: {error}");
    }
}

// --- Timers -------------------------------------------------------------

#[tauri::command]
pub fn list_timers(app: AppHandle) -> Timers {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|error| error.to_string())?;
    store.snapshot()
}

#[tauri::command]
pub fn create_timer(app: AppHandle, label: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.create(&label)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn start_timer(app: AppHandle, id: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.start(&id)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn pause_timer(app: AppHandle, id: String) -> Timers {
    pause_timer_by_id(&app, &id)
}

pub fn pause_timer_by_id(app: &AppHandle, id: &str) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.pause(id)?
    };
    refresh_tray(app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn reset_timer(app: AppHandle, id: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.reset(&id)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn rename_timer(app: AppHandle, id: String, label: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.rename(&id, &label)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn delete_timer(app: AppHandle, id: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.delete(&id)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

// --- Activity capture ---------------------------------------------------

#[tauri::command]
pub fn activity_snapshot(app: AppHandle, since_ms: u64) -> Result<ActivitySnapshot, String> {
    activity::snapshot_for(&app, since_ms)
}

#[tauri::command]
pub fn set_capture_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.set_capture_enabled(enabled, crate::timers::now_epoch_ms())?;
    }
    activity::emit_full(&app);
    Ok(())
}

#[tauri::command]
pub fn set_idle_threshold(app: AppHandle, minutes: u64) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.set_idle_threshold_ms(minutes.saturating_mul(60_000))?;
    }
    activity::emit_full(&app);
    Ok(())
}

#[tauri::command]
pub fn start_session(app: AppHandle, kind: String, label: Option<String>) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.start_session(&kind, label.as_deref(), now_epoch_ms())?;
    }
    activity::emit_full(&app);
    Ok(())
}

#[tauri::command]
pub fn stop_session(app: AppHandle) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.stop_session(now_epoch_ms())?;
    }
    activity::emit_full(&app);
    Ok(())
}

#[tauri::command]
pub fn mark_segment_reviewed(app: AppHandle, id: i64) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.mark_reviewed(id)?;
    }
    activity::emit_full(&app);
    Ok(())
}

// --- P1: Categories -----------------------------------------------------

#[tauri::command]
pub fn list_categories(app: AppHandle) -> Result<Vec<Category>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_categories()
}

#[tauri::command]
pub fn create_category(app: AppHandle, category: NewCategory) -> Result<Category, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.create_category(category, now)
}

#[tauri::command]
pub fn update_category(
    app: AppHandle,
    id: String,
    patch: UpdateCategory,
) -> Result<Category, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.update_category(&id, patch, now)
}

#[tauri::command]
pub fn delete_category(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.delete_category(&id, now)
}

// --- P1: Projects -------------------------------------------------------

#[tauri::command]
pub fn list_projects(app: AppHandle) -> Result<Vec<Project>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_projects()
}

#[tauri::command]
pub fn create_project(app: AppHandle, project: NewProject) -> Result<Project, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.create_project(project, now)
}

#[tauri::command]
pub fn update_project(app: AppHandle, id: String, patch: UpdateProject) -> Result<Project, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.update_project(&id, patch, now)
}

#[tauri::command]
pub fn delete_project(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.delete_project(&id, now)
}

// --- P1: Clients --------------------------------------------------------

#[tauri::command]
pub fn list_clients(app: AppHandle) -> Result<Vec<Client>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_clients()
}

#[tauri::command]
pub fn create_client(app: AppHandle, client: NewClient) -> Result<Client, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.create_client(client, now)
}

#[tauri::command]
pub fn update_client(app: AppHandle, id: String, patch: UpdateClient) -> Result<Client, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.update_client(&id, patch, now)
}

#[tauri::command]
pub fn delete_client(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.delete_client(&id, now)
}

// --- P1: Time Entries ---------------------------------------------------

/// Tells views to refetch, and wakes the AI worker: an entry mutation can
/// queue work (an approval queues its embedding, a rebuild a classification).
fn entries_changed(app: &AppHandle) {
    let _ = app.emit(crate::EVENT_ENTRIES_CHANGED, ());
    ai::nudge(app);
}

#[tauri::command]
pub fn list_time_entries(
    app: AppHandle,
    start_ms: u64,
    end_ms: u64,
) -> Result<Vec<TimeEntry>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_time_entries(start_ms, end_ms)
}

#[tauri::command]
pub fn get_entry_detail(app: AppHandle, id: String) -> Result<EntryDetail, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.get_entry_detail(&id)
}

#[tauri::command]
pub fn update_time_entry(
    app: AppHandle,
    id: String,
    patch: UpdateTimeEntry,
) -> Result<TimeEntry, String> {
    let now = now_epoch_ms();
    let entry = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.update_time_entry(&id, patch, now)?
    };
    entries_changed(&app);
    Ok(entry)
}

/// Returns the approved entries, so a caller adopts them without a refetch.
#[tauri::command]
pub fn approve_time_entries(app: AppHandle, ids: Vec<String>) -> Result<Vec<TimeEntry>, String> {
    let now = now_epoch_ms();
    let entries = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.approve_time_entries(&ids, "user", now)?;
        store.time_entries(&ids)?
    };
    entries_changed(&app);
    Ok(entries)
}

/// One patch applied to many entries (My Timesheet's bulk bar). Returns the
/// updated entries.
#[tauri::command]
pub fn update_time_entries(
    app: AppHandle,
    ids: Vec<String>,
    patch: UpdateTimeEntry,
) -> Result<Vec<TimeEntry>, String> {
    let now = now_epoch_ms();
    let entries = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        let mut updated = Vec::with_capacity(ids.len());
        for id in &ids {
            updated.push(store.update_time_entry(id, patch.clone(), now)?);
        }
        updated
    };
    entries_changed(&app);
    Ok(entries)
}

#[tauri::command]
pub fn reject_time_entry(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.reject_time_entry(&id, now)?;
    }
    entries_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn split_time_entry(
    app: AppHandle,
    id: String,
    at_ms: u64,
) -> Result<(TimeEntry, TimeEntry), String> {
    let now = now_epoch_ms();
    let res = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.split_time_entry(&id, at_ms, now)?
    };
    entries_changed(&app);
    Ok(res)
}

#[tauri::command]
pub fn delete_time_entry(app: AppHandle, id: String) -> Result<(), String> {
    delete_time_entries(app, vec![id])
}

#[tauri::command]
pub fn delete_time_entries(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    let now = now_epoch_ms();
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.delete_time_entries(&ids, now)?;
    }
    entries_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn create_time_entry(app: AppHandle, entry: NewTimeEntry) -> Result<TimeEntry, String> {
    let now = now_epoch_ms();
    let created = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.create_manual_entry(entry, now)?
    };
    entries_changed(&app);
    Ok(created)
}

#[tauri::command]
pub fn rebuild_time_entries(
    app: AppHandle,
    start_ms: u64,
    end_ms: u64,
) -> Result<Vec<TimeEntry>, String> {
    let now = now_epoch_ms();
    let entries = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.rebuild_time_entries_in_range(start_ms, end_ms, now)?
    };
    entries_changed(&app);
    Ok(entries)
}

// --- P1: Apps -----------------------------------------------------------

#[tauri::command]
pub fn list_apps(app: AppHandle) -> Result<Vec<AppRecord>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_apps()
}

#[tauri::command]
pub fn update_app(
    app: AppHandle,
    id: String,
    default_category_id: Option<String>,
    default_project_id: Option<String>,
    excluded: Option<bool>,
) -> Result<AppRecord, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.update_app(&id, default_category_id, default_project_id, excluded, now)
}

// --- P2: AI suggestions --------------------------------------------------

#[tauri::command]
pub fn ai_status(app: AppHandle) -> AiStatus {
    app.state::<AiRuntime>().status()
}

/// "Couldn't categorize · Retry", or re-running a suggestion on demand.
#[tauri::command]
pub fn retry_classification(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.queue_classification(&id, now)?;
    }
    entries_changed(&app);
    Ok(())
}

/// Accepts (creating a rule) or dismisses an inline rule suggestion.
#[tauri::command]
pub fn resolve_rule_suggestion(
    app: AppHandle,
    suggestion: RuleSuggestion,
    accept: bool,
) -> Result<(), String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.resolve_rule_suggestion(&suggestion, accept, now)
}

// --- P4: Learning loop -----------------------------------------------------

/// Settings → Categories & AI: effectiveness, calibration, the threshold
/// preview, and personal-model versions over the last `days` days.
#[tauri::command]
pub fn ai_metrics(app: AppHandle, days: u32) -> Result<AiMetrics, String> {
    let state = app.state::<AppState>();
    let reader = state.activity_reader.lock().map_err(|e| e.to_string())?;
    ai::metrics::metrics(&reader, days, now_epoch_ms())
}

/// "Retrain now": queues a personal-model retrain that skips the idle/power
/// gate. The result arrives as `ai-status-changed`.
#[tauri::command]
pub fn ai_retrain(app: AppHandle) -> AiStatus {
    ai::update_status(&app, |s| {
        s.retrain = "queued".to_string();
        s.retrain_note = None;
    });
    app.state::<AiRuntime>().request_retrain();
    app.state::<AiRuntime>().status()
}

/// "Reset learned data": forgets the personal models, calibration, and the
/// kNN store, keeping entries, categories, projects, and rules. Returns the
/// updated metrics.
#[tauri::command]
pub fn ai_reset_learned(app: AppHandle, days: u32) -> Result<AiMetrics, String> {
    let now = now_epoch_ms();
    let metrics = {
        let state = app.state::<AppState>();
        let store = state.activity.lock().map_err(|e| e.to_string())?;
        let paths = ai::store::reset_learned(store.conn(), now)?;
        for path in paths {
            if let Err(error) = std::fs::remove_dir_all(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("could not delete model {path}: {error}");
                }
            }
        }
        ai::metrics::metrics(store.conn(), days, now)?
    };
    ai::update_status(&app, |s| {
        s.outcomes = 0;
        s.calibrated = false;
        s.personal_model = false;
        s.retrain_note = None;
    });
    ai::nudge(&app);
    Ok(metrics)
}

// --- P3: Reports (Time Entries, My Timesheet, Calendar Month) -----------
//
// Read-only queries go through the reader connection so an aggregation over
// a year never holds the writer the sampler needs (decision A6).

fn with_reader<T>(
    app: &AppHandle,
    read: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.state::<AppState>();
    let conn = state.activity_reader.lock().map_err(|e| e.to_string())?;
    read(&conn)
}

#[tauri::command]
pub fn query_time_entries(
    app: AppHandle,
    filter: EntryFilter,
    limit: Option<u32>,
) -> Result<Vec<TimeEntry>, String> {
    with_reader(&app, |conn| {
        reports::query_entries(conn, &filter, limit.unwrap_or(reports::MAX_QUERY_ROWS))
    })
}

#[tauri::command]
pub fn entry_rollup(
    app: AppHandle,
    filter: EntryFilter,
    boundaries: Vec<u64>,
    group_by: String,
) -> Result<Vec<RollupCell>, String> {
    let group_by = GroupBy::parse(&group_by)?;
    with_reader(&app, |conn| {
        reports::rollup(conn, &filter, &boundaries, group_by)
    })
}

/// Writes the filtered entries to the Downloads folder as CSV or JSON.
#[tauri::command]
pub fn export_time_entries(
    app: AppHandle,
    filter: EntryFilter,
    format: String,
) -> Result<ExportResult, String> {
    let format = ExportFormat::parse(&format)?;
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| e.to_string())?;
    with_reader(&app, |conn| {
        reports::export_entries(conn, &filter, format, &dir)
    })
}

// --- P3: Projects ------------------------------------------------------

#[tauri::command]
pub fn project_stats(
    app: AppHandle,
    range_start: u64,
    range_end: u64,
    month_start: u64,
) -> Result<Vec<ProjectStats>, String> {
    with_reader(&app, |conn| {
        crate::projects::project_stats(conn, range_start, range_end, month_start)
    })
}

#[tauri::command]
pub fn project_rules(app: AppHandle, project_id: String) -> Result<Vec<ProjectRule>, String> {
    with_reader(&app, |conn| {
        crate::projects::project_rules(conn, &project_id)
    })
}

/// "Would have matched 14h in the last 30 days", per hint.
#[tauri::command]
pub fn preview_project_hints(
    app: AppHandle,
    hints: String,
    since_ms: u64,
) -> Result<HintPreview, String> {
    with_reader(&app, |conn| {
        crate::projects::preview_hints(conn, &hints, since_ms)
    })
}

#[tauri::command]
pub fn discover_projects(app: AppHandle, since_ms: u64) -> Result<Vec<ProjectSuggestion>, String> {
    with_reader(&app, |conn| crate::projects::discover(conn, since_ms))
}

#[tauri::command]
pub fn dismiss_project_suggestion(app: AppHandle, key: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    crate::projects::dismiss_suggestion(store.conn(), &key)
}

#[tauri::command]
pub fn import_projects_csv(app: AppHandle, text: String) -> Result<ImportSummary, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    crate::projects::import_csv(&mut store, &text, now)
}

// --- Preferences -------------------------------------------------------

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    app.state::<AppState>().settings_snapshot()
}

#[tauri::command]
pub fn update_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let (previous, next) = {
        let state = app.state::<AppState>();
        let mut store = state.settings.lock().map_err(|error| error.to_string())?;
        let previous = store.snapshot();
        let next = store.set(settings)?;
        (previous, next)
    };

    if next.tray_enabled != previous.tray_enabled {
        let timers = {
            let state = app.state::<AppState>();
            state
                .store
                .lock()
                .ok()
                .and_then(|store| store.snapshot().ok())
        };
        if let Some(timers) = timers {
            if let Err(error) = tray::set_enabled(&app, next.tray_enabled, &timers) {
                eprintln!("could not toggle the tray: {error}");
            }
        }
    }

    if next.retention_days != previous.retention_days {
        let _ = sweep_retention(&app);
    }

    if next.tracking_hours != previous.tracking_hours {
        let state = app.state::<AppState>();
        if let Ok(mut store) = state.activity.lock() {
            store.set_tracking_hours(next.tracking_hours.clone());
        }
        activity::emit_full(&app);
    }

    let _ = app.emit(crate::EVENT_SETTINGS_CHANGED, &next);
    Ok(next)
}

pub fn sweep_retention(app: &AppHandle) -> Result<u64, String> {
    let days = app.state::<AppState>().settings_snapshot().retention_days;
    let removed = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        let removed = store.purge_older_than(days, now_epoch_ms())?;
        let _ = crate::energy::purge_older_than(store.conn(), days, now_epoch_ms());
        removed
    };
    if removed > 0 {
        activity::emit_full(app);
    }
    Ok(removed)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoragePaths {
    pub config_file: String,
    pub data_dir: String,
    pub database_file: String,
}

#[tauri::command]
pub fn storage_paths(app: AppHandle) -> Result<StoragePaths, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let config_dir = crate::settings::config_dir();
    Ok(StoragePaths {
        config_file: config_dir.join("settings.json").display().to_string(),
        data_dir: data_dir.display().to_string(),
        database_file: data_dir
            .join(crate::activity::DB_FILE)
            .display()
            .to_string(),
    })
}

#[tauri::command]
pub fn get_energy_summary(
    app: AppHandle,
    days: Option<u32>,
) -> Result<crate::energy::EnergySummary, String> {
    let state = app.state::<AppState>();
    let conn = state.activity_reader.lock().map_err(|e| e.to_string())?;
    crate::energy::get_summary(
        &conn,
        days.unwrap_or(crate::energy::DEFAULT_HISTORY_DAYS),
        None,
        None,
    )
}

#[tauri::command]
pub fn query_energy_history(
    app: AppHandle,
    since_ms: Option<u64>,
    limit: Option<u32>,
) -> Result<Vec<crate::energy::EnergySample>, String> {
    let state = app.state::<AppState>();
    let conn = state.activity_reader.lock().map_err(|e| e.to_string())?;
    crate::energy::query_history(&conn, since_ms, limit)
}

#[tauri::command]
pub fn reset_energy_history(app: AppHandle) -> Result<crate::energy::EnergySummary, String> {
    let state = app.state::<AppState>();
    {
        let store = state.activity.lock().map_err(|e| e.to_string())?;
        crate::energy::reset_history(store.conn())?;
    }
    let conn = state.activity_reader.lock().map_err(|e| e.to_string())?;
    let summary =
        crate::energy::get_summary(&conn, crate::energy::DEFAULT_HISTORY_DAYS, None, None)?;
    let _ = app.emit(crate::energy::EVENT_ENERGY_CHANGED, &summary);
    Ok(summary)
}
