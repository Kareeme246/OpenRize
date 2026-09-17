//! IPC surface. Every command returns the complete timer list so the frontend
//! never has to guess at state it did not just write.
//!
//! Each one takes `AppHandle` (not `State`) so the tray's own event handler can
//! call the same helper — one code path for "mutate, persist, refresh".

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::activity::{self, ActivitySnapshot};
use crate::settings::Settings;
use crate::timers::{now_epoch_ms, Timer};
use crate::tray;
use crate::AppState;

type Timers = Result<Vec<Timer>, String>;

/// The tray is a mirror, not a source of truth: if redrawing it fails, the
/// mutation already succeeded and must not be reported as a failure. Log it.
fn refresh_tray(app: &AppHandle, timers: &[Timer]) {
    if let Err(error) = tray::refresh(app, timers) {
        eprintln!("tray refresh failed: {error}");
    }
}

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

/// Shared by the command above and the tray's "Pause <label>" menu items.
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

// --- activity capture --------------------------------------------------
//
// The read command takes the caller's local-midnight bound and also caches it
// on the writer, so the sampler's tick/heartbeat pushes know what range to
// aggregate (see activity.rs's `snapshot_for`). Mutations broadcast the full
// snapshot on EVENT_ACTIVITY_CHANGED via `activity::emit_full` — the same
// path the sampler uses for a structural change — so every view and the tray
// adopt it directly instead of round-tripping another `invoke`.

#[tauri::command]
pub fn activity_snapshot(app: AppHandle, since_ms: u64) -> Result<ActivitySnapshot, String> {
    activity::snapshot_for(&app, since_ms)
}

#[tauri::command]
pub fn set_capture_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.set_capture_enabled(enabled)?;
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

// --- preferences -------------------------------------------------------

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    app.state::<AppState>().settings_snapshot()
}

/// Replaces the whole preference set. A full object (rather than per-field
/// setters) keeps the two sides in lockstep: a new preference is a struct field
/// and a default, not a new command.
///
/// Side effects that live outside the file are applied here so there is one
/// path that reacts to a change: the tray is added/removed, and retention is
/// swept immediately when the window shrinks.
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

    let _ = app.emit(crate::EVENT_SETTINGS_CHANGED, &next);
    Ok(next)
}

/// Deletes activity history past the retention window, including the rollup
/// aggregates for those days. Returns how many raw rows went, so callers can
/// decide whether a UI refresh is warranted.
pub fn sweep_retention(app: &AppHandle) -> Result<u64, String> {
    let days = app.state::<AppState>().settings_snapshot().retention_days;
    let removed = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.purge_older_than(days, now_epoch_ms())?
    };
    if removed > 0 {
        // A structural change to the segment set: push the full snapshot the
        // same way the sampler does, not a bare ping.
        activity::emit_full(app);
    }
    Ok(removed)
}

/// Where things live on disk, for the Settings page. Paths are absolute and
/// sent as strings because the webview has no path type.
///
/// There is no separate timers file any more: trackers and activity share
/// `activity.db` (decision A8), so `database_file` is what to point at.
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
