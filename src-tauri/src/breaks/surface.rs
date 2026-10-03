//! The reminder panel: a small, borderless window in the top-right corner of
//! the active display, just below the menu bar. It carries the break reminder,
//! the countdown capsule and the welcome-back card, and is the only surface
//! break reminders use.
//!
//! Like the Pulse panel it is created on first need, hidden (not destroyed)
//! between uses, and sized by its page: the page measures its card and calls
//! `resize_reminder_panel`, and the window is re-anchored to the corner on
//! every resize. When it becomes visible it waits for the page's next
//! measurement, so it never shows at a stale size.
//!
//! It never takes focus. On macOS the window is converted into a
//! non-activating `NSPanel`, so clicking a button acts without activating
//! OpenRize (which would raise the main window over the user's work).
//! Pulse opening over it needs no coordination.

use std::sync::Mutex;

use crate::settings::NotificationPlacement;

use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const LABEL: &str = "reminder";

/// Space between a screen's usable edge and a notification panel.
const EDGE: f64 = 12.0;
/// A placeholder until the page reports its real size.
const INITIAL_SIZE: (f64, f64) = (360.0, 120.0);

#[derive(Default)]
pub struct SurfaceState(Mutex<Placement>);

#[derive(Default)]
struct Placement {
    /// Logical size the page last reported.
    size: Option<(f64, f64)>,
    /// Whether the engine wants the panel on screen.
    wanted: bool,
    /// Visible next time the page reports a size.
    show_when_sized: bool,
}

/// Shows or hides the panel to match the engine's phase.
///
/// No window call is made while the placement lock is held: showing or hiding
/// a window can deliver events synchronously.
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
        // The page re-measures on every state change and shows the panel.
    }
}

/// The page measured its card: size the window to it, keep it anchored, and
/// show it if it was waiting.
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
        // The window server only learns a window's "does not activate the
        // app" tag once the window exists on screen, so state it again now.
        prevent_activation(&window);
    }
}

/// Puts the window in the corner of the display the user is working on.
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
        (width * scale, height * scale),
        EDGE * scale,
    );
    // Order matters on macOS: a resize keeps the bottom-left corner fixed, so
    // the position is set after it to pin the top edge.
    let _ = window.set_size(LogicalSize::new(width, height));
    let _ = window.set_position(PhysicalPosition::new(x.round(), y.round()));
}

/// Re-anchor an already visible notification immediately after a preference change.
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
        NotificationPlacement::CenterMiddle => (
            left + ((area_width - width) / 2.0).max(0.0),
            top + ((area_height - height) / 2.0).max(0.0),
        ),
        NotificationPlacement::BottomLeft => (left_edge, bottom),
        NotificationPlacement::BottomRight => (right, bottom),
    }
}

/// Runs `update` under the placement lock; `None` if the lock is poisoned.
fn remember<T>(app: &AppHandle, update: impl FnOnce(&mut Placement) -> T) -> Option<T> {
    let state = app.state::<SurfaceState>();
    let mut placement = state.0.lock().ok()?;
    Some(update(&mut placement))
}

fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    // Same bundle as the main window; the page picks its root by window label.
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("OpenRize reminder")
        .inner_size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .decorations(false)
        // The page draws the rounded card and its shadow; the window is clear.
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .focusable(false)
        .accept_first_mouse(true)
        // Best effort: keeps the panel out of screen shares and recordings.
        // Dev builds stay capturable so `pnpm dev:drive` can screenshot it.
        .content_protected(!cfg!(debug_assertions))
        .build()?;
    make_non_activating(&window);
    Ok(window)
}

#[cfg(target_os = "macos")]
fn prevent_activation(window: &WebviewWindow) {
    use objc2_app_kit::{NSPanel, NSWindowStyleMask};
    use objc2_foundation::NSObjectProtocol;

    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(pointer) = target.ns_window() else {
            return;
        };
        // SAFETY: `make_non_activating` made this window an NSPanel, and this
        // runs on the main thread.
        let panel = unsafe { &*pointer.cast::<NSPanel>() };
        let mask = panel.styleMask();
        panel.setStyleMask(mask - NSWindowStyleMask::NonactivatingPanel);
        panel.setStyleMask(mask | NSWindowStyleMask::NonactivatingPanel);
        // The style mask alone does not stop a click from activating the app
        // (the mask bit is only honoured when the window is created with it).
        // AppKit's own flag for it is private, so it is used only when present.
        let selector = objc2::sel!(_setPreventsActivation:);
        if panel.respondsToSelector(selector) {
            // SAFETY: the selector takes one BOOL and returns nothing.
            let _: () = unsafe { objc2::msg_send![panel, _setPreventsActivation: true] };
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn prevent_activation(_window: &WebviewWindow) {}

#[cfg(target_os = "macos")]
fn make_non_activating(window: &WebviewWindow) {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{define_class, ClassType, MainThreadOnly};
    use objc2_app_kit::{NSPanel, NSWindowCollectionBehavior, NSWindowStyleMask};

    define_class!(
        // SAFETY: NSPanel has no subclassing requirements, the class adds no
        // instance variables, and it overrides nothing but two predicates.
        #[unsafe(super(NSPanel))]
        #[thread_kind = MainThreadOnly]
        #[name = "OpenRizeReminderPanel"]
        struct ReminderPanel;

        impl ReminderPanel {
            #[unsafe(method(canBecomeKeyWindow))]
            fn can_become_key_window(&self) -> bool {
                false
            }

            #[unsafe(method(canBecomeMainWindow))]
            fn can_become_main_window(&self) -> bool {
                false
            }
        }
    );

    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(pointer) = target.ns_window() else {
            return;
        };
        let object = pointer.cast::<AnyObject>();
        let class: &AnyClass = ReminderPanel::class();
        // SAFETY: `ns_window` is the live NSWindow behind this Tauri window and
        // this runs on the main thread. Swapping an NSWindow to an NSPanel
        // subclass with no extra instance variables is how the
        // `tauri-nspanel` crate makes windows non-activating; the panel then
        // overrides the predicates tao's own subclass used for focusability.
        unsafe { objc2::ffi::object_setClass(object, class) };
        // SAFETY: the object is now a `ReminderPanel`, an NSPanel.
        let panel = unsafe { &*object.cast::<NSPanel>() };
        panel.setStyleMask(panel.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        panel.setFloatingPanel(true);
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(true);
        panel.setCollectionBehavior(
            panel.collectionBehavior()
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
    });
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
            notification_origin(
                NotificationPlacement::CenterMiddle,
                origin,
                area,
                size,
                12.0
            ),
            (-1140.0, 495.0)
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

#[cfg(not(target_os = "macos"))]
fn make_non_activating(window: &WebviewWindow) {
    let _ = window.set_visible_on_all_workspaces(true);
}
