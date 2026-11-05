//! Menu-bar (macOS) or notification-area (Windows) icon, icon only (decision T1).
//! Left click opens the Pulse panel; right click opens the native menu.

use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::timers::Timer;
use crate::AppState;

pub const TRAY_ID: &str = "openrize";
const OPEN_WINDOW: &str = "open-window";
const RELAUNCH: &str = "relaunch";
const QUIT: &str = "quit";
const PAUSE_PREFIX: &str = "pause:";
const MAX_LABEL_CHARS: usize = 32;

/// Template images: black with alpha, so macOS tints the glyph and no brand colour
/// can appear. Cut from `app-icon.png` with ImageMagick at 44px (18pt on retina);
/// tray-idle.png additionally paints out the centre bead.
fn glyph(active: bool) -> Image<'static> {
    let template = if active {
        tauri::include_image!("icons/tray-active.png")
    } else {
        tauri::include_image!("icons/tray-idle.png")
    };
    platform::tint(template)
}

#[cfg_attr(target_os = "macos", path = "tray/macos.rs")]
#[cfg_attr(windows, path = "tray/windows.rs")]
#[cfg_attr(not(any(target_os = "macos", windows)), path = "tray/unsupported.rs")]
mod platform;

pub fn init(app: &AppHandle, timers: &[Timer]) -> tauri::Result<()> {
    // No menu here: see `show_menu`.
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(glyph(any_running(timers)))
        .icon_as_template(true)
        .tooltip(tooltip(timers))
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(on_tray_icon_event)
        .build(app)?;

    Ok(())
}

/// Idempotent: a redundant call is a no-op.
pub fn set_enabled(app: &AppHandle, enabled: bool, timers: &[Timer]) -> tauri::Result<()> {
    let mounted = app.tray_by_id(TRAY_ID).is_some();
    match (enabled, mounted) {
        (true, false) => init(app, timers),
        (false, true) => {
            app.remove_tray_by_id(TRAY_ID);
            // The panel belongs to the icon; it never outlives it.
            crate::pulse::destroy(app);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Repaints the tray from a snapshot, then tells the frontend what changed.
/// Called on every mutation, never on a timer; the menu is built when it opens.
pub fn refresh(app: &AppHandle, timers: &[Timer]) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        // Sets icon and template flag together; doing it in two calls flickers.
        tray.set_icon_with_as_template(Some(glyph(any_running(timers))), true)?;
        tray.set_tooltip(Some(tooltip(timers)))?;
    }

    // The tray can pause a timer behind the window's back, so the authoritative
    // list is broadcast for the frontend to adopt.
    let _ = app.emit(crate::EVENT_TIMERS_CHANGED, timers);
    Ok(())
}

/// Repaints just the glyph when its colour should change without any timer
/// changing (Windows light/dark). A taskbar-only colour change may not reach
/// here and shows up at the next icon change.
pub fn restyle(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let glyph = glyph(any_running(&current_timers(app)));
    if let Err(error) = tray.set_icon_with_as_template(Some(glyph), true) {
        eprintln!("could not repaint the tray icon: {error}");
    }
}

fn current_timers(app: &AppHandle) -> Vec<Timer> {
    app.state::<AppState>()
        .store
        .lock()
        .ok()
        .and_then(|store| store.snapshot().ok())
        .unwrap_or_default()
}

pub fn show_main_window(app: &AppHandle) {
    // A background launch (`rize app start`) starts with no Dock icon.
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn any_running(timers: &[Timer]) -> bool {
    timers.iter().any(Timer::is_running)
}

fn tooltip(timers: &[Timer]) -> String {
    let running = timers.iter().filter(|timer| timer.is_running()).count();
    let paused = timers.len() - running;
    match (running, paused) {
        (0, 0) => "OpenRize — no trackers".to_string(),
        (0, paused) => format!("OpenRize — nothing running, {paused} paused"),
        (running, 0) => format!("OpenRize — {running} running"),
        (running, paused) => format!("OpenRize — {running} running, {paused} paused"),
    }
}

fn build_menu(app: &AppHandle, timers: &[Timer]) -> tauri::Result<Menu<Wry>> {
    let running: Vec<&Timer> = timers.iter().filter(|timer| timer.is_running()).collect();
    let mut builder = MenuBuilder::new(app);

    if running.is_empty() {
        // A disabled item is the only honest way to say "nothing here" in a
        // native menu; an empty menu looks broken.
        builder = builder.item(&disabled(app, "nothing-running", "Nothing running")?);
    } else {
        // Native menus have no right-aligned action column, so each item *is*
        // the action, labelled with the timer it pauses.
        for timer in running {
            let label = format!("Pause {}", shorten(&timer.label, MAX_LABEL_CHARS));
            builder = builder.item(
                &MenuItemBuilder::with_id(format!("{PAUSE_PREFIX}{}", timer.id), label)
                    .build(app)?,
            );
        }
    }

    builder = builder.separator();
    if app.state::<crate::permissions::Monitor>().state()
        == crate::permissions::AccessibilityState::RelaunchRequired
    {
        builder = builder
            .item(&MenuItemBuilder::with_id(RELAUNCH, "Relaunch to finish setup").build(app)?);
    }
    builder = builder.item(&MenuItemBuilder::with_id(OPEN_WINDOW, "Open OpenRize").build(app)?);
    builder = builder.item(&MenuItemBuilder::with_id(QUIT, "Quit OpenRize").build(app)?);
    builder.build()
}

fn disabled(app: &AppHandle, id: &str, text: &str) -> tauri::Result<tauri::menu::MenuItem<Wry>> {
    MenuItemBuilder::with_id(id, text).enabled(false).build(app)
}

/// The panel opens on left button down, like a native menu-bar item. The menu
/// opens on `platform::MENU_BUTTON_STATE` (down on macOS, up on Windows). A
/// double click (Windows only) opens the app.
fn on_tray_icon_event(tray: &TrayIcon, event: TrayIconEvent) {
    let app = tray.app_handle();
    match event {
        TrayIconEvent::Click {
            rect,
            button: MouseButton::Left,
            button_state: MouseButtonState::Down,
            ..
        } => crate::pulse::toggle(app, rect),
        TrayIconEvent::Click {
            button: MouseButton::Right,
            button_state,
            ..
        } if button_state == platform::MENU_BUTTON_STATE => show_menu(tray),
        TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => {
            // The first click of the double click opened the panel.
            crate::pulse::hide(app);
            show_main_window(app);
        }
        _ => {}
    }
}

/// Opens the native menu, attached only while open: attached, macOS opens it on
/// every click, left included, leaving none for the panel. Built fresh from the
/// timers, so never stale.
fn show_menu(tray: &TrayIcon) {
    let app = tray.app_handle();
    crate::pulse::hide(app);
    let timers = current_timers(app);
    let menu = match build_menu(app, &timers) {
        Ok(menu) => menu,
        Err(error) => {
            eprintln!("could not build the tray menu: {error}");
            return;
        }
    };
    if tray.set_menu(Some(menu)).is_err() {
        return;
    }
    // Opens the menu; it tracks the mouse modally and returns once closed.
    let _ = tray.with_inner_tray_icon(|inner| inner.show_menu());
    let _ = tray.set_menu(None::<Menu<Wry>>);
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();

    if let Some(timer_id) = id.strip_prefix(PAUSE_PREFIX) {
        if let Err(error) = crate::commands::pause_timer_by_id(app, timer_id) {
            eprintln!("could not pause {timer_id} from the tray: {error}");
        }
    } else if id == OPEN_WINDOW {
        show_main_window(app);
    } else if id == RELAUNCH {
        app.request_restart();
    } else if id == QUIT {
        app.exit(0);
    }
}

/// Char-boundary safe, so a multi-byte label can never panic the menu build.
fn shorten(label: &str, max_chars: usize) -> String {
    let mut shortened: String = label.chars().take(max_chars).collect();
    if label.chars().count() > max_chars {
        shortened.push('…');
    }
    shortened
}
