//! Tray glyph colour and mouse behaviour where the icon is a template image.

use tauri::image::Image;
use tauri::tray::MouseButtonState;

/// The menu opens as the button goes down, like a native menu-bar item.
pub const MENU_BUTTON_STATE: MouseButtonState = MouseButtonState::Down;

/// The system tints a template image for the current menu bar, so the glyph
/// is used as it is.
pub fn tint(template: Image<'static>) -> Image<'static> {
    template
}
