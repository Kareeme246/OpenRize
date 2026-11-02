//! Tray glyph colour and mouse behaviour on Windows.

use tauri::image::Image;
use tauri::tray::MouseButtonState;

/// The right button opens the menu on release, as the shell's own icons do.
pub const MENU_BUTTON_STATE: MouseButtonState = MouseButtonState::Up;

/// Windows draws tray icons as they are, so the black template is painted
/// white on a dark taskbar. The theme is read on every icon change, and again
/// when the app's theme changes (see `tray::restyle`).
pub fn tint(template: Image<'static>) -> Image<'static> {
    if taskbar_is_light() {
        return template;
    }
    let mut rgba = template.rgba().to_vec();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel[..3].fill(u8::MAX);
    }
    Image::new_owned(rgba, template.width(), template.height())
}

/// The taskbar follows the "default Windows mode", which is dark unless the
/// user picked light.
fn taskbar_is_light() -> bool {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        .is_ok_and(|light| light == 1)
}
