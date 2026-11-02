//! The main window's own caption: its frame, and the window buttons.
//!
//! On macOS the title bar is overlaid on the webview (`titleBarStyle` in
//! tauri.conf.json), so the app's top bar already is the caption. Windows draws
//! its caption above the webview by default; here it is removed and the top bar
//! carries the minimize, maximize and close buttons itself (`CaptionButtons.tsx`).
//! The window keeps its DWM shadow, rounded corners, resize edges, snapping and
//! system menu; only the caption strip goes. Every platform-specific part lives
//! in one file per platform, all exposing the same items.
//!
//! The one thing a DOM button cannot do is raise the Windows 11 Snap Layouts
//! flyout, which appears only when the window under the cursor answers
//! `WM_NCHITTEST` with `HTMAXBUTTON`. The webview covers the whole client area,
//! so `windows.rs` places a small native window over the maximize button; see
//! there for how it works. The frontend reports where the button is.

use serde::{Deserialize, Serialize};
use tauri::WebviewWindow;

#[cfg_attr(target_os = "macos", path = "caption/macos.rs")]
#[cfg_attr(windows, path = "caption/windows.rs")]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    path = "caption/unsupported.rs"
)]
mod platform;

/// Sent to the app when the pointer enters, leaves or presses the maximize
/// button's native overlay. The overlay swallows the pointer, so the page's own
/// `:hover`/`:active` never fire and the button draws these instead.
#[cfg_attr(not(windows), allow(dead_code))]
pub const EVENT_MAXIMIZE_BUTTON: &str = "caption-maximize-button";

/// Payload of [`EVENT_MAXIMIZE_BUTTON`].
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

/// Gives the window its final frame. Called once, before the window is first
/// shown.
pub fn prepare(window: &WebviewWindow) {
    platform::prepare(window);
}

/// Tells the native side where the page's maximize button is, or that it is
/// not on screen (`None`). The page calls this whenever the button moves.
///
/// A command without `async` runs on the main thread, which the native window
/// it creates and the messages it receives need.
#[tauri::command]
pub fn set_maximize_button(window: WebviewWindow, rect: Option<ButtonRect>) {
    platform::place_maximize_button(&window, rect);
}

/// Opens the system window menu (Restore, Move, Size, Minimize, Maximize,
/// Close) at the pointer, as a right click on a native caption does.
#[tauri::command]
pub fn show_system_menu(window: WebviewWindow) {
    platform::show_system_menu(&window);
}
