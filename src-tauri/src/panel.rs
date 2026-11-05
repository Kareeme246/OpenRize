//! Turning a Tauri window into a floating panel for Pulse and the reminder. macOS makes it an
//! `NSPanel` (`panel/macos.rs`); on Windows the builder's flags do the work and only the reminder
//! needs a touch after each show.

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

pub fn make_panel(window: &WebviewWindow, kind: Kind) {
    platform::make_panel(window, kind);
}

/// Restates, after each show, that the panel must not activate the app (and,
/// on Windows, that it stays on top).
pub fn prevent_activation(window: &WebviewWindow) {
    platform::prevent_activation(window);
}
