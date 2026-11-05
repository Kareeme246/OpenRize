//! The main window's own caption. macOS overlays the title bar on the webview; on Windows the
//! caption is removed and the top bar carries the window buttons (`CaptionButtons.tsx`), keeping
//! shadow, corners and snapping. A DOM button cannot raise Snap Layouts, so `windows.rs` covers it.

use serde::{Deserialize, Serialize};
use tauri::WebviewWindow;

#[cfg_attr(target_os = "macos", path = "caption/macos.rs")]
#[cfg_attr(windows, path = "caption/windows.rs")]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    path = "caption/unsupported.rs"
)]
mod platform;

/// Sent when the pointer enters, leaves or presses the maximize button's native overlay. The
/// overlay swallows the pointer, so the page's own `:hover`/`:active` never fire and the button
/// draws these.
#[cfg_attr(not(windows), allow(dead_code))]
pub const EVENT_MAXIMIZE_BUTTON: &str = "caption-maximize-button";

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaximizeButtonState {
    pub hovered: bool,
    pub pressed: bool,
}

/// Where the page's maximize button is, in physical pixels from the top-left of
/// the window's client area (CSS pixels times `devicePixelRatio`).
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Gives the window its final frame; called once, before it is first shown.
pub fn prepare(window: &WebviewWindow) {
    platform::prepare(window);
}

/// Tells the native side where the page's maximize button is, or `None` when off screen. Not
/// `async`, so it runs on the main thread, which the native window it creates needs.
#[tauri::command]
pub fn set_maximize_button(window: WebviewWindow, rect: Option<ButtonRect>) {
    platform::place_maximize_button(&window, rect);
}

/// Opens the system window menu at the pointer, as a right click on a native caption does.
#[tauri::command]
pub fn show_system_menu(window: WebviewWindow) {
    platform::show_system_menu(&window);
}
