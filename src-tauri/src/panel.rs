//! Turning a Tauri window into a floating panel. The Pulse and reminder
//! panels both go through here; each platform decides what that takes.
//!
//! - macOS: the window becomes an `NSPanel` (see `panel/macos.rs`).
//! - Windows: the builder's flags do the work, and only the reminder needs a
//!   touch after each show (see `panel/windows.rs`).

use tauri::WebviewWindow;

/// Which panel is being made; the two differ in whether they may take focus.
#[derive(Clone, Copy)]
pub enum Kind {
    /// Never takes focus: clicking a button must not activate OpenRize.
    Reminder,
    /// Can be key (typing, blur closes it) but never the app's main window.
    Pulse,
}

#[cfg_attr(target_os = "macos", path = "panel/macos.rs")]
#[cfg_attr(windows, path = "panel/windows.rs")]
#[cfg_attr(not(any(target_os = "macos", windows)), path = "panel/unsupported.rs")]
mod platform;

/// Converts `window` into a panel of the given kind. Call once, right after
/// the window is built.
pub fn make_panel(window: &WebviewWindow, kind: Kind) {
    platform::make_panel(window, kind);
}

/// Restates, after each show, that the panel must not activate the app (and,
/// on Windows, that it stays on top).
pub fn prevent_activation(window: &WebviewWindow) {
    platform::prevent_activation(window);
}
