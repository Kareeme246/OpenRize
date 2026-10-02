//! Break reminders.
//!
//! A break is never a second concept: a taken break is a `break` activity
//! segment (`activity::KIND_BREAK`), which the entry builder already keeps out
//! of time entries and work hours. The only new durable record is the
//! *decision* around a break - taken, skipped, missed, or credited from idle
//! time - in the `breaks` table (`store.rs`).
//!
//! - `engine.rs` is the pure state machine: the work clock, the reminder
//!   lifecycle, scheduled breaks and the quiet rules. It returns `Effect`s.
//! - `store.rs` reads and writes the table.
//! - `surface.rs` is the top-right reminder panel.
//! - This file is the driver. The 1 Hz activity sampler calls `after_tick`;
//!   the frontend's commands come through `command`. Both run under the
//!   engine's lock, apply the effects (open or close the segment, pause or
//!   resume stopwatches, write the row), then publish the new
//!   `break-state-changed` event whenever the state changed.
//!
//! Lock order: the engine lock is taken first, then the activity or timer
//! store locks. Nothing holds an activity lock while asking for the engine.

pub mod engine;
pub mod store;
pub mod surface;

use std::sync::Mutex;

use chrono::{Datelike, Local, TimeZone, Timelike};
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::BreakSettings;
use crate::timers::now_epoch_ms;
use crate::AppState;
use engine::{
    BreakRecord, BreakState, Command, Effect, Engine, Input, LocalTime, Phase, SegmentNow,
};

/// Payload: `BreakState`.
pub const EVENT_BREAK_STATE: &str = "break-state-changed";

/// Runtime key in the activity DB `settings` table (epoch ms).
const PAUSED_UNTIL_KEY: &str = "breaks_paused_until";

/// Extra minutes the "+5 min" control adds to a running break.
pub const EXTEND_MINUTES: u16 = 5;

pub struct BreakRuntime {
    engine: Mutex<Engine>,
    last_published: Mutex<Option<BreakState>>,
}

/// Builds the engine from what is on disk and registers the managed state.
/// Call once at setup, after `AppState` is managed.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = state.settings_snapshot().breaks;
    let now = now_epoch_ms();
    let (streak, paused_until) = {
        let store = state.activity.lock().map_err(|error| error.to_string())?;
        store::close_orphans(store.conn(), now)?;
        let break_ms = u64::from(settings.break_minutes) * 60_000;
        let streak = store::rebuild_streak(store.conn(), local_day_start(now), now, break_ms)?;
        let paused_until = store
            .read_setting(PAUSED_UNTIL_KEY)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|until| *until > now);
        (streak, paused_until)
    };
    app.manage(BreakRuntime {
        engine: Mutex::new(Engine::new(streak, paused_until)),
        last_published: Mutex::new(None),
    });
    app.manage(surface::SurfaceState::default());
    Ok(())
}

/// Called by the sampler after every activity tick.
pub fn after_tick(app: &AppHandle, keeps_display_awake: bool, away: bool) {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let settings = state.settings_snapshot().breaks;
    let live = match state.activity.lock() {
        Ok(store) => store.live_state(now),
        Err(_) => return,
    };
    let segment = live.segment_info().map(|info| SegmentNow {
        id: info.id,
        kind: info.kind,
        label: info.label,
    });
    let input = Input {
        now,
        settings: &settings,
        tracking_active: live.tracking_active,
        idle_ms: live.last_idle_ms,
        idle_threshold_ms: live.idle_threshold_ms,
        away,
        keeps_display_awake,
        segment,
        local: local_time(now),
    };
    run(app, &settings, now, |engine| engine.step(&input));
}

/// The frontend (or the tray) asks for a break action.
pub fn command(app: &AppHandle, command: Command) -> Result<BreakState, String> {
    let now = now_epoch_ms();
    let settings = app.state::<AppState>().settings_snapshot().breaks;
    run(app, &settings, now, |engine| {
        engine.command(command, now, &settings)
    })
    .ok_or_else(|| "break state is unavailable".to_string())
}

/// Silences reminders until `until` (epoch ms), or turns them back on for `None`.
pub fn pause_reminders(app: &AppHandle, until: Option<u64>) -> Result<BreakState, String> {
    let now = now_epoch_ms();
    let settings = app.state::<AppState>().settings_snapshot().breaks;
    {
        let state = app.state::<AppState>();
        let store = state.activity.lock().map_err(|error| error.to_string())?;
        store.write_setting(PAUSED_UNTIL_KEY, &until.unwrap_or(0).to_string())?;
    }
    run(app, &settings, now, |engine| {
        engine.set_paused_until(until);
        Vec::new()
    })
    .ok_or_else(|| "break state is unavailable".to_string())
}

pub fn current_state(app: &AppHandle) -> Result<BreakState, String> {
    let now = now_epoch_ms();
    let settings = app.state::<AppState>().settings_snapshot().breaks;
    let runtime = app.state::<BreakRuntime>();
    let engine = runtime.engine.lock().map_err(|error| error.to_string())?;
    Ok(engine.view(now, &settings, local_time(now)))
}

pub fn list_breaks(
    app: &AppHandle,
    since: u64,
    until: u64,
) -> Result<Vec<store::BreakEntry>, String> {
    let state = app.state::<AppState>();
    let reader = state
        .activity_reader
        .lock()
        .map_err(|_| "activity reader lock poisoned".to_string())?;
    store::list(&reader, since, until)
}

/// The app is quitting: close an open break so its row has an end.
pub fn on_exit(app: &AppHandle) {
    let Some(runtime) = app.try_state::<BreakRuntime>() else {
        return;
    };
    let now = now_epoch_ms();
    let Ok(mut engine) = runtime.engine.lock() else {
        return;
    };
    for effect in engine.end_for_exit(now) {
        if let Effect::Finish { id, ended_at, .. } = effect {
            with_store(app, |conn| store::set_ended(conn, &id, ended_at, now));
        }
    }
}

/// Raises a sample reminder so the panel can be looked at without waiting an
/// hour. Dev builds only.
#[cfg(debug_assertions)]
pub fn dev_sample(app: &AppHandle) -> Result<BreakState, String> {
    let now = now_epoch_ms();
    let settings = app.state::<AppState>().settings_snapshot().breaks;
    run(app, &settings, now, |engine| {
        engine.dev_sample_reminder(now, &settings);
        Vec::new()
    })
    .ok_or_else(|| "break state is unavailable".to_string())
}

/// Plays the reminder chime (the setting's "Preview" button uses it too).
pub fn chime() {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSSound;
        use objc2_foundation::NSString;
        if let Some(sound) = NSSound::soundNamed(&NSString::from_str("Glass")) {
            sound.play();
        }
    }
    #[cfg(windows)]
    {
        use windows::core::w;
        use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
        // SAFETY: plays a named system sound asynchronously; the alias is a
        // static string.
        unsafe {
            let _ = PlaySoundW(
                w!("SystemNotification"),
                None,
                SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }
}

/// Runs `act` on the engine under its lock, applies the effects it returns,
/// and publishes the state if it changed.
fn run(
    app: &AppHandle,
    settings: &BreakSettings,
    now: u64,
    act: impl FnOnce(&mut Engine) -> Vec<Effect>,
) -> Option<BreakState> {
    let runtime = app.try_state::<BreakRuntime>()?;
    let mut engine = runtime.engine.lock().ok()?;
    let effects = act(&mut engine);
    apply(app, &mut engine, effects, now);
    let view = engine.view(now, settings, local_time(now));
    drop(engine);
    publish(app, &runtime, &view);
    Some(view)
}

fn publish(app: &AppHandle, runtime: &BreakRuntime, view: &BreakState) {
    let Ok(mut last) = runtime.last_published.lock() else {
        return;
    };
    if last.as_ref() == Some(view) {
        return;
    }
    *last = Some(view.clone());
    drop(last);
    surface::sync(app, view.phase != Phase::Idle);
    let _ = app.emit(EVENT_BREAK_STATE, view);
}

fn apply(app: &AppHandle, engine: &mut Engine, effects: Vec<Effect>, now: u64) {
    for effect in effects {
        match effect {
            Effect::Begin {
                record,
                label,
                pause_timers,
            } => begin(app, engine, record, &label, pause_timers, now),
            Effect::Finish {
                id,
                ended_at,
                close_segment,
                resume_timers,
            } => {
                if close_segment {
                    if let Err(error) = crate::commands::stop_session(app.clone()) {
                        eprintln!("could not end the break segment: {error}");
                    }
                }
                with_store(app, |conn| store::set_ended(conn, &id, ended_at, now));
                for timer in resume_timers {
                    // A stopwatch deleted during the break is simply gone.
                    let _ = crate::commands::start_timer(app.clone(), timer);
                }
            }
            Effect::Record(record) => {
                with_store(app, |conn| store::insert(conn, &record, now));
            }
            Effect::Extend { id, planned_ms } => {
                with_store(app, |conn| store::set_planned(conn, &id, planned_ms, now));
            }
            Effect::Chime => chime(),
        }
    }
}

fn begin(
    app: &AppHandle,
    engine: &mut Engine,
    mut record: BreakRecord,
    label: &str,
    pause_timers: bool,
    now: u64,
) {
    if let Err(error) =
        crate::commands::start_session(app.clone(), "break".to_string(), Some(label.to_string()))
    {
        eprintln!("could not start the break: {error}");
        engine.abort_begin();
        return;
    }
    let segment_id = app
        .state::<AppState>()
        .activity
        .lock()
        .ok()
        .and_then(|store| store.live_state(now).segment_info())
        .map(|info| info.id);
    record.segment_id = segment_id;
    engine.attach_segment(segment_id);
    with_store(app, |conn| store::insert(conn, &record, now));
    if pause_timers {
        engine.attach_paused_timers(pause_running_timers(app));
    }
}

/// Pauses every running stopwatch and returns the ones it paused.
fn pause_running_timers(app: &AppHandle) -> Vec<(String, String)> {
    let running: Vec<(String, String)> = app
        .state::<AppState>()
        .store
        .lock()
        .ok()
        .and_then(|store| store.snapshot().ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|timer| timer.is_running())
        .map(|timer| (timer.id, timer.label))
        .collect();
    let mut paused = Vec::new();
    for (id, label) in running {
        if crate::commands::pause_timer_by_id(app, &id).is_ok() {
            paused.push((id, label));
        }
    }
    paused
}

fn with_store(app: &AppHandle, write: impl FnOnce(&rusqlite::Connection) -> Result<(), String>) {
    let state = app.state::<AppState>();
    let result = match state.activity.lock() {
        Ok(store) => write(store.conn()),
        Err(_) => Err("activity store lock poisoned".to_string()),
    };
    if let Err(error) = result {
        eprintln!("could not record the break: {error}");
    }
}

fn local_time(now: u64) -> LocalTime {
    let local = chrono::DateTime::from_timestamp_millis(now as i64)
        .map(|utc| utc.with_timezone(&Local))
        .unwrap_or_else(Local::now);
    LocalTime {
        day_key: i64::from(local.date_naive().num_days_from_ce()),
        minutes_of_day: local.hour() * 60 + local.minute(),
        weekday: local.weekday(),
    }
}

/// Local midnight at or before `now`, in epoch milliseconds.
fn local_day_start(now: u64) -> u64 {
    let local = chrono::DateTime::from_timestamp_millis(now as i64)
        .map(|utc| utc.with_timezone(&Local))
        .unwrap_or_else(Local::now);
    local
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| Local.from_local_datetime(&midnight).earliest())
        .map_or(0, |start| start.timestamp_millis().max(0) as u64)
}
