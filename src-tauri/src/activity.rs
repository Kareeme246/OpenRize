//! Activity capture - the automatic tracker.
//!
//! Samples the OS foreground window, whether that app keeps the display awake,
//! and user idle time every second and folds consecutive samples into the
//! segments `openrize_core::activity` stores.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

pub use openrize_core::activity::*;
use openrize_core::timers::now_epoch_ms;

const HEARTBEAT_MS: u64 = 30_000;

pub fn snapshot_for(app: &AppHandle, since_ms: u64) -> Result<ActivitySnapshot, String> {
    let state = app.state::<crate::AppState>();
    let now = now_epoch_ms();
    let live = {
        let mut store = state
            .activity
            .lock()
            .map_err(|_| "activity store lock poisoned".to_string())?;
        store.watch_since_ms = Some(since_ms);
        store.live_state(now)
    };
    let reader = state
        .activity_reader
        .lock()
        .map_err(|_| "activity reader lock poisoned".to_string())?;
    build_snapshot(&reader, &live, since_ms, now)
}

pub fn emit_full(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let now = now_epoch_ms();
    let result = (|| -> Result<ActivitySnapshot, String> {
        let live = {
            let store = state
                .activity
                .lock()
                .map_err(|_| "activity store lock poisoned".to_string())?;
            store.live_state(now)
        };
        let since_ms = live.watch_since_ms.unwrap_or(0);
        let reader = state
            .activity_reader
            .lock()
            .map_err(|_| "activity reader lock poisoned".to_string())?;
        build_snapshot(&reader, &live, since_ms, now)
    })();
    match result {
        Ok(snapshot) => {
            let _ = app.emit(crate::EVENT_ACTIVITY_CHANGED, snapshot);
        }
        Err(error) => eprintln!("activity snapshot failed: {error}"),
    }
}

fn emit_tick(app: &AppHandle, now: u64) {
    let state = app.state::<crate::AppState>();
    let result = (|| -> Result<ActivityTick, String> {
        let live = {
            let store = state
                .activity
                .lock()
                .map_err(|_| "activity store lock poisoned".to_string())?;
            store.live_state(now)
        };
        let since_ms = live.watch_since_ms.unwrap_or(0);
        let reader = state
            .activity_reader
            .lock()
            .map_err(|_| "activity reader lock poisoned".to_string())?;
        build_tick(&reader, &live, since_ms, now)
    })();
    match result {
        Ok(tick) => {
            let _ = app.emit(crate::EVENT_ACTIVITY_TICK, tick);
        }
        Err(error) => eprintln!("activity tick failed: {error}"),
    }
}

pub(crate) fn read_idle_ms() -> u64 {
    user_idle3::UserIdle::get_time()
        .map(|idle| idle.duration().as_millis() as u64)
        .unwrap_or(0)
}

pub fn spawn_sampler(app: AppHandle) {
    std::thread::spawn(move || {
        // App Nap assertion: prevents throttling while capture runs in background
        let _app_nap =
            crate::capture::AppNapAssertion::begin("OpenRize background activity capture");

        let mut last_push_at: u64 = 0;
        let mut last_sample_time: u64 = 0;

        loop {
            std::thread::sleep(Duration::from_secs(SAMPLE_SECS));

            let now = now_epoch_ms();
            let away = crate::capture::user_is_away();
            let sample = crate::capture::read_active_window();
            let keeps_display_awake = sample.as_ref().is_some_and(|s| s.keeps_display_awake);
            let idle_ms = read_idle_ms();

            let state = app.state::<crate::AppState>();

            // Sleep / lid-close defense: if a gap of >10s occurred between 1s ticks,
            // close active segment at last_sample_time + 1000 so sleep time is not counted.
            if last_sample_time > 0 && now.saturating_sub(last_sample_time) > 10_000 {
                if let Ok(mut store) = state.activity.lock() {
                    let _ = store.close_active_segment(last_sample_time + 1000);
                }
            }
            last_sample_time = now;

            let tick_result = state
                .activity
                .lock()
                .map_err(|_| "activity store lock poisoned".to_string())
                .and_then(|mut store| {
                    if away {
                        store.tick_away(now)
                    } else {
                        store.tick(sample, idle_ms, now)
                    }
                });

            match tick_result {
                Ok(true) => {
                    emit_full(&app);
                    // A segment opened or closed, so an entry may have just
                    // closed: let the AI worker rebuild and classify it.
                    crate::ai::nudge(&app);
                    last_push_at = now;
                }
                Ok(false) => {
                    let foreground = state.foreground.load(Ordering::Relaxed);
                    if foreground || now.saturating_sub(last_push_at) >= HEARTBEAT_MS {
                        emit_tick(&app, now);
                        last_push_at = now;
                    }
                }
                Err(error) => eprintln!("activity sample failed: {error}"),
            }

            crate::breaks::after_tick(&app, keeps_display_awake, away);
        }
    });
}
