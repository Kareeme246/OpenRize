//! What the user is looking at: frontmost app, window title and browser URL, plus the OS hooks the
//! sampler needs. Each platform file provides the same items (`read_active_window`, `user_is_away`,
//! `idle_ms`, `AppNapAssertion`, `register_sleep_listeners`); `unsupported.rs` captures nothing.

#[cfg_attr(target_os = "macos", path = "capture/macos.rs")]
#[cfg_attr(windows, path = "capture/windows.rs")]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    path = "capture/unsupported.rs"
)]
mod platform;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod title;

pub use platform::{
    idle_ms, read_active_window, register_sleep_listeners, user_is_away, AppNapAssertion,
};

pub use openrize_core::capture::WindowSample;

/// Actual system power transitions, not display sleep or session locking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PowerEvent {
    Suspend,
    Resume,
}

fn power_event(app: &tauri::AppHandle, event: PowerEvent) {
    use tauri::Manager;

    crate::breaks::power_event(app, event);
    if event == PowerEvent::Suspend {
        let state = app.state::<crate::AppState>();
        if let Ok(mut store) = state.activity.lock() {
            let _ = store.close_active_segment(crate::timers::now_epoch_ms());
        };
    }
    crate::activity::emit_full(app);
}

/// The host a browser URL points at, without a leading `www.`.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn domain_of(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .host_str()
        .map(|host| host.trim_start_matches("www.").to_string())
}
