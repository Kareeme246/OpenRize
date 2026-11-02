//! The Pulse panel: a small glance at capture and today, anchored to the
//! tray icon (design board option A). Left click on the icon toggles it;
//! right click keeps the native menu.
//!
//! The panel exists only while the icon does. It is created on the first
//! click, hidden (not destroyed) between opens so reopening is instant, and
//! destroyed when the icon is turned off.
//!
//! Sizing is driven by the page: it measures its content and calls
//! `resize_pulse_panel`, and the window is re-anchored under the icon on every
//! resize. The first open waits for that first measurement, so the panel is
//! never shown at a guessed height.
//!
//! On Windows it opens in the work-area corner next to the taskbar, like the
//! native flyouts, whichever monitor holds the icon and whether the icon is
//! pinned or inside the overflow (^) flyout; see `placement`. It takes focus
//! when it opens (Escape closes it and Ctrl+O opens the app, both from the
//! page) and hides when focus leaves, so a second tray click closes it.
//!
//! On macOS the window is a panel that takes key focus but never becomes the
//! app's main window, like a native menu bar extra. Tiling window managers
//! (komorebi, for one) adopt whatever window an app reports as its main
//! window, so a plain window here would be tiled the moment it took focus.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Rect, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub mod placement;

#[cfg_attr(target_os = "macos", path = "pulse/macos.rs")]
#[cfg_attr(windows, path = "pulse/windows.rs")]
#[cfg_attr(not(any(target_os = "macos", windows)), path = "pulse/unsupported.rs")]
mod platform;

pub const LABEL: &str = "pulse";

/// Logical points, matching the approved mock.
const WIDTH: f64 = 320.0;
/// A placeholder until the page reports its real height.
const INITIAL_HEIGHT: f64 = 360.0;

/// A click on the icon also takes focus from the open panel, so the blur that
/// hides it lands just before the click that would reopen it. A click this
/// soon after a blur-hide is that same gesture and means "close".
const REOPEN_GUARD: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct PulseState(Mutex<Placement>);

#[derive(Default)]
struct Placement {
    /// The icon's rect from the last click, in physical pixels.
    icon: Option<Rect>,
    /// Logical content height the page last reported.
    height: Option<f64>,
    /// Show as soon as the page reports its height (first open).
    show_when_sized: bool,
    hidden_by_blur_at: Option<Instant>,
}

/// Opens the panel under the icon, or closes it if it is open.
///
/// No window call is made while the placement lock is held: hiding or
/// focusing a window can deliver focus events synchronously, and the blur
/// handler takes the same lock.
pub fn toggle(app: &AppHandle, icon: Rect) {
    let Some(window) = app.get_webview_window(LABEL) else {
        remember(app, |placement| {
            placement.icon = Some(icon);
            placement.show_when_sized = true;
        });
        if let Err(error) = create(app) {
            eprintln!("could not open the pulse panel: {error}");
        }
        return;
    };

    let visible = window.is_visible().unwrap_or(false);
    let action = remember(app, |placement| {
        let just_hidden = placement
            .hidden_by_blur_at
            .take()
            .is_some_and(|at| at.elapsed() < REOPEN_GUARD);
        if visible || just_hidden {
            return Toggle::Close;
        }
        placement.icon = Some(icon);
        match placement.height {
            Some(height) => Toggle::Open(height),
            None => {
                // The page has not measured itself yet; its resize shows it.
                placement.show_when_sized = true;
                Toggle::WaitForSize
            }
        }
    });
    match action {
        Some(Toggle::Close) => {
            let _ = window.hide();
        }
        Some(Toggle::Open(height)) => {
            platform::place(app, &window, icon, height);
            reveal(&window);
        }
        Some(Toggle::WaitForSize) | None => {}
    }
}

enum Toggle {
    Close,
    Open(f64),
    WaitForSize,
}

/// The page measured its content: size the window to it and keep it anchored.
pub fn resize(app: &AppHandle, height: f64) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let height = height.clamp(1.0, 2000.0);
    let placed = remember(app, |placement| {
        placement.height = Some(height);
        let show = std::mem::take(&mut placement.show_when_sized);
        placement.icon.map(|icon| (icon, show))
    });
    if let Some(Some((icon, show))) = placed {
        platform::place(app, &window, icon, height);
        if show {
            reveal(&window);
        }
    }
}

/// Focus left the panel (click away, another app, a Space switch).
pub fn on_blur(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    remember(app, |placement| {
        placement.hidden_by_blur_at = Some(Instant::now());
    });
    let _ = window.hide();
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

/// The icon was turned off, so the panel goes with it.
pub fn destroy(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
    remember(app, |placement| *placement = Placement::default());
}

/// Runs `update` under the placement lock; `None` if the lock is poisoned.
fn remember<T>(app: &AppHandle, update: impl FnOnce(&mut Placement) -> T) -> Option<T> {
    let state = app.state::<PulseState>();
    let mut placement = state.0.lock().ok()?;
    Some(update(&mut placement))
}

fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    // Same bundle as the main window; the page picks its root by window label.
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("OpenRize")
        .inner_size(WIDTH, INITIAL_HEIGHT)
        .decorations(false)
        // Rounded corners are drawn by the page; the window itself is clear.
        .transparent(true)
        // On Windows the shadow is DWM's, drawn around the rectangle (and
        // rounded to Windows 11's 8px, not the card's 12px), so it shows as a
        // square halo behind the card's corners. Without it the card is the
        // whole window, and tao stops padding the window by the shadow's
        // hidden frame, so sizes and positions are exact.
        .shadow(cfg!(not(windows)))
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .build()?;
    crate::panel::make_panel(&window, crate::panel::Kind::Pulse);
    Ok(window)
}

fn reveal(window: &WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
}

/// The icon's horizontal centre and vertical extent in physical pixels.
struct Anchor {
    x: f64,
    top: f64,
    bottom: f64,
}

fn anchor(app: &AppHandle, icon: Rect) -> Anchor {
    // The tray reports physical pixels on macOS and Windows; any logical
    // value is scaled with the primary monitor as a best effort.
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0);
    let position = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    Anchor {
        x: position.x + size.width / 2.0,
        top: position.y,
        bottom: position.y + size.height,
    }
}
