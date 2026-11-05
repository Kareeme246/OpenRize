//! Pulse placement on Windows: the work-area corner beside the taskbar.

use tauri::{AppHandle, Rect, WebviewWindow};

use super::{anchor, placement, WIDTH};

/// Space between the taskbar, the screen edge and the panel, as the native
/// Windows 11 flyouts leave.
const FLYOUT_MARGIN: f64 = 12.0;

/// Puts the panel in the work-area corner beside the taskbar, on the icon's
/// monitor. The icon only picks the corner: one in the overflow flyout sits
/// mid-screen, but the panel should still hug the taskbar.
pub fn place(app: &AppHandle, window: &WebviewWindow, icon: Rect, height: f64) {
    let anchor = anchor(app, icon);
    let centre = (anchor.x, (anchor.top + anchor.bottom) / 2.0);
    let monitor = app
        .monitor_from_point(centre.0, centre.1)
        .ok()
        .flatten()
        .or_else(|| {
            let cursor = app.cursor_position().ok()?;
            app.monitor_from_point(cursor.x, cursor.y).ok().flatten()
        })
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let (x, y) = placement::flyout_origin(
        placement::PixelRect::new(*monitor.position(), *monitor.size()),
        placement::PixelRect::new(work.position, work.size),
        centre,
        placement::physical_size((WIDTH, height), scale),
        FLYOUT_MARGIN * scale,
    );
    placement::set_bounds(window, scale, (x.round(), y.round()), (WIDTH, height));
}
