//! Platforms without capture yet: nothing is ever in front, and nobody is
//! ever away, so the sampler records nothing.

use super::WindowSample;

pub fn user_is_away() -> bool {
    false
}

pub fn read_active_window() -> Option<WindowSample> {
    None
}

pub struct AppNapAssertion;

impl AppNapAssertion {
    pub fn begin(_reason: &str) -> Self {
        Self
    }
}

pub fn register_sleep_listeners(_app: tauri::AppHandle) {}
