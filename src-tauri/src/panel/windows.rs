//! Panels on Windows are plain tao windows: the builder's flags (see
//! `breaks::surface::create`) keep the reminder from taking focus, and Pulse
//! takes it on purpose.

use tauri::WebviewWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_TOPMOST, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
};

use super::Kind;

/// Nothing to convert: windows have no panel class, and tao ignores
/// `set_visible_on_all_workspaces` here.
pub fn make_panel(_window: &WebviewWindow, _kind: Kind) {}

/// Puts the window back at the top of the topmost band: it can drop below another topmost window
/// while hidden, and `always_on_top(true)` only applies once. `SWP_NOACTIVATE` keeps focus where it
/// is.
pub fn prevent_activation(window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // SAFETY: `hwnd` is the live window behind this Tauri window; the call
    // moves and sizes nothing, and `SWP_ASYNCWINDOWPOS` posts it to the
    // window's own thread so it cannot block this one.
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
    };
}
