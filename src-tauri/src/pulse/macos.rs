//! Pulse placement on macOS: under the menu bar item.

use tauri::{AppHandle, Rect, WebviewWindow};

use super::placement;

pub fn place(app: &AppHandle, window: &WebviewWindow, icon: Rect, height: f64) {
    placement::place_under_icon(app, window, icon, height);
}
