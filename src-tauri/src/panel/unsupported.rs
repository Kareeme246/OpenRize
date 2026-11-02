//! No panel conversion on this platform.

use tauri::WebviewWindow;

use super::Kind;

pub fn make_panel(window: &WebviewWindow, _kind: Kind) {
    let _ = window.set_visible_on_all_workspaces(true);
}

pub fn prevent_activation(_window: &WebviewWindow) {}
