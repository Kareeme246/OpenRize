//! Pulse panel anchored to the tray icon: created on first click, hidden between opens, destroyed
//! when the icon is off. The page measures itself and calls `resize_pulse_panel`; the first open
//! waits for it. macOS: a panel that never becomes the main window, so tiling WMs don't tile it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Rect, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub mod placement;

#[cfg_attr(target_os = "macos", path = "pulse/macos.rs")]
#[cfg_attr(windows, path = "pulse/windows.rs")]
#[cfg_attr(not(any(target_os = "macos", windows)), path = "pulse/unsupported.rs")]
mod platform;

pub const LABEL: &str = "pulse";

const WIDTH: f64 = 320.0;
const INITIAL_HEIGHT: f64 = 360.0;

/// A click on the icon blurs (hides) the panel just before it lands, so a click this
/// soon after a blur-hide is the same gesture and means "close".
const REOPEN_GUARD: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct PulseState(Mutex<Placement>);

#[derive(Default)]
struct Placement {
    icon: Option<Rect>,
    height: Option<f64>,
    show_when_sized: bool,
    hidden_by_blur_at: Option<Instant>,
}

/// Opens the panel under the icon, or closes it. No window call while the placement lock is
/// held: window calls can deliver focus events synchronously and the blur handler takes the lock.
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

pub fn destroy(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
    remember(app, |placement| *placement = Placement::default());
}

fn remember<T>(app: &AppHandle, update: impl FnOnce(&mut Placement) -> T) -> Option<T> {
    let state = app.state::<PulseState>();
    let mut placement = state.0.lock().ok()?;
    Some(update(&mut placement))
}

fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("OpenRize")
        .inner_size(WIDTH, INITIAL_HEIGHT)
        .decorations(false)
        .transparent(true)
        // Windows' DWM shadow is a square halo behind the card's rounded corners; without
        // it the card is the whole window and tao's hidden-frame padding goes, so sizes are exact.
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
