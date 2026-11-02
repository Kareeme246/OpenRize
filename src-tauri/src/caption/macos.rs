//! macOS: the title bar is overlaid on the webview and the system draws the
//! traffic lights, so there is nothing to do.

use tauri::WebviewWindow;

use crate::caption::ButtonRect;

pub fn prepare(_window: &WebviewWindow) {}

pub fn place_maximize_button(_window: &WebviewWindow, _rect: Option<ButtonRect>) {}

pub fn show_system_menu(_window: &WebviewWindow) {}
