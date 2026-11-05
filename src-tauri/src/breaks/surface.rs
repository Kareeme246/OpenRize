//! The reminder panel: a borderless window anchored in the active display's work area,
//! created on first need, hidden between uses and sized by its page. It never takes
//! focus: a non-activating `NSPanel` on macOS, builder flags on Windows (see `create`).

use std::sync::Mutex;

use crate::settings::NotificationPlacement;

use crate::pulse::placement;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "reminder";

const EDGE: f64 = 12.0;
const INITIAL_SIZE: (f64, f64) = (360.0, 120.0);

#[derive(Default)]
pub struct SurfaceState(Mutex<Placement>);

#[derive(Default)]
struct Placement {
    size: Option<(f64, f64)>,
    wanted: bool,
    show_when_sized: bool,
}

/// No window call is made while the placement lock is held: showing or hiding a window
/// can deliver events synchronously.
pub fn sync(app: &AppHandle, visible: bool) {
    let existing = app.get_webview_window(LABEL);
    let show_when_sized = remember(app, |placement| {
        placement.wanted = visible;
        if !visible {
            placement.show_when_sized = false;
            return false;
        }
        let already_visible = existing
            .as_ref()
            .is_some_and(|window| window.is_visible().unwrap_or(false));
        if !already_visible {
            placement.show_when_sized = true;
        }
        placement.show_when_sized
    })
    .unwrap_or(false);

    if !visible {
        if let Some(window) = existing {
            let _ = window.hide();
        }
        return;
    }
    if existing.is_none() {
        if let Err(error) = create(app) {
            eprintln!("could not open the reminder panel: {error}");
        }
    } else if show_when_sized {
    }
}

pub fn resize(app: &AppHandle, width: f64, height: f64) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let size = (width.clamp(1.0, 1200.0), height.clamp(1.0, 2000.0));
    let outcome = remember(app, |placement| {
        placement.size = Some(size);
        let show = std::mem::take(&mut placement.show_when_sized);
        (placement.wanted, show)
    });
    let Some((wanted, show)) = outcome else {
        return;
    };
    if !wanted {
        return;
    }
    place(app, &window, size);
    if show {
        let _ = window.show();
        // The window server learns the "does not activate the app" tag only once the window
        // is on screen, so state it again now. On Windows this re-asserts topmost.
        crate::panel::prevent_activation(&window);
    }
}

fn place(app: &AppHandle, window: &WebviewWindow, (width, height): (f64, f64)) {
    let cursor = app.cursor_position().ok();
    let monitor = cursor
        .and_then(|point| app.monitor_from_point(point.x, point.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let preference = app
        .state::<crate::AppState>()
        .settings_snapshot()
        .notification_placement;
    let (x, y) = notification_origin(
        preference,
        (f64::from(area.position.x), f64::from(area.position.y)),
        (f64::from(area.size.width), f64::from(area.size.height)),
        placement::physical_size((width, height), scale),
        EDGE * scale,
    );
    placement::set_bounds(window, scale, (x.round(), y.round()), (width, height));
}

pub fn reposition(app: &AppHandle) {
    let size = remember(app, |placement| placement.size).flatten();
    if let (Some(window), Some(size)) = (app.get_webview_window(LABEL), size) {
        place(app, &window, size);
    }
}

fn notification_origin(
    placement: NotificationPlacement,
    (left, top): (f64, f64),
    (area_width, area_height): (f64, f64),
    (width, height): (f64, f64),
    edge: f64,
) -> (f64, f64) {
    let right = left + (area_width - width - edge).max(0.0);
    let bottom = top + (area_height - height - edge).max(0.0);
    let left_edge = left + edge.min((area_width - width).max(0.0));
    let top_edge = top + edge.min((area_height - height).max(0.0));
    match placement {
        NotificationPlacement::TopLeft => (left_edge, top_edge),
        NotificationPlacement::TopRight => (right, top_edge),
        NotificationPlacement::TopCenter => {
            (left + ((area_width - width) / 2.0).max(0.0), top_edge)
        }
        NotificationPlacement::BottomLeft => (left_edge, bottom),
        NotificationPlacement::BottomRight => (right, bottom),
    }
}

fn remember<T>(app: &AppHandle, update: impl FnOnce(&mut Placement) -> T) -> Option<T> {
    let state = app.state::<SurfaceState>();
    let mut placement = state.0.lock().ok()?;
    Some(update(&mut placement))
}

fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("OpenRize reminder")
        .inner_size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        // Windows: tao gives a non-focusable window WS_EX_NOACTIVATE and shows an unfocused
        // one with SW_SHOWNOACTIVATE on every show, so it never takes focus.
        .focused(false)
        .focusable(false)
        .accept_first_mouse(true)
        // Best effort: keeps the panel out of screen shares. Dev builds stay capturable
        // so `pnpm dev:drive` can screenshot it.
        .content_protected(!cfg!(debug_assertions))
        .build()?;
    crate::panel::make_panel(&window, crate::panel::Kind::Reminder);
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_uses_work_area_and_survives_negative_monitor_coordinates() {
        let origin = (-1920.0, 30.0);
        let area = (1920.0, 1050.0);
        let size = (360.0, 120.0);
        assert_eq!(
            notification_origin(NotificationPlacement::TopLeft, origin, area, size, 12.0),
            (-1908.0, 42.0)
        );
        assert_eq!(
            notification_origin(NotificationPlacement::TopCenter, origin, area, size, 12.0),
            (-1140.0, 42.0)
        );
        assert_eq!(
            notification_origin(NotificationPlacement::TopRight, origin, area, size, 12.0),
            (-372.0, 42.0)
        );
        assert_eq!(
            notification_origin(NotificationPlacement::BottomLeft, origin, area, size, 12.0),
            (-1908.0, 948.0)
        );
        assert_eq!(
            notification_origin(NotificationPlacement::BottomRight, origin, area, size, 12.0),
            (-372.0, 948.0)
        );
    }
}
