//! Platforms without a custom caption keep the window frame they are given.

use tauri::WebviewWindow;

use crate::caption::ButtonRect;

pub fn prepare(_window: &WebviewWindow) {}

pub fn place_maximize_button(_window: &WebviewWindow, _rect: Option<ButtonRect>) {}

pub fn show_system_menu(_window: &WebviewWindow) {}
