//! IPC surface. Every command returns the complete timer list so the frontend
//! never has to guess at state it did not just write.
//!
//! Each one takes `AppHandle` (not `State`) so the tray's own event handler can
//! call the same helper — one code path for "mutate, persist, refresh".

use tauri::{AppHandle, Manager};

use crate::activity::{self, ActivitySnapshot};
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
