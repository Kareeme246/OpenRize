//! The menu-bar (macOS) or notification-area (Windows) icon. Icon only, no text (decision T1): the glyph shape
//! carries idle vs active. Left click opens the Pulse panel (pulse.rs); right
//! click opens the native menu with the running timers, Open, and Quit.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::timers::Timer;
use crate::AppState;

pub const TRAY_ID: &str = "openrize";
const OPEN_WINDOW: &str = "open-window";
const QUIT: &str = "quit";
const PAUSE_PREFIX: &str = "pause:";
const MAX_LABEL_CHARS: usize = 32;

/// Template images: black with alpha. macOS uses the alpha as a mask and tints
/// the glyph for the current menu bar, which is why no brand colour can appear
/// here. Both are cut from the Chrono Bloom master, `app-icon.png` — run from
/// the repo root, replacing the final path with `tray-idle.png` for the second
/// file and adding `-fill black -draw "circle 512,512 512,362"` after `-level`
/// to paint out the centre bead:
///
/// ```text
/// magick app-icon.png -fx "g-max(r,b)" -alpha off -level "15%,40%" \
///   -gravity center -crop 680x680+0+0 +repage -resize 42x42 \
///   -colorspace sRGB -alpha set -channel A -fx "u.r" +channel \
///   -channel RGB -evaluate set 0 +channel \
///   -background none -gravity center -extent 44x44 \
///   png32:src-tauri/icons/tray-active.png
/// ```
///
/// 44px is deliberate: macOS draws the tray glyph at 18pt, so a 44px bitmap
/// stays crisp on retina where the old 22px one was blurry.
fn glyph(active: bool) -> Image<'static> {
    let template = match (active, REVIEW_DOT.load(Ordering::Relaxed)) {
        (true, false) => tauri::include_image!("icons/tray-active.png"),
        (false, false) => tauri::include_image!("icons/tray-idle.png"),
        (true, true) => tauri::include_image!("icons/tray-active-dot.png"),
        (false, true) => tauri::include_image!("icons/tray-idle-dot.png"),
    };
    #[cfg(windows)]
    if !taskbar_is_light() {
        return whitened(&template);
    }
    template
}

/// Whether the glyph carries the review dot: an agent needs the person, or
/// has finished and waits to be looked at. The `-dot` images are the same
/// glyphs with a dot in the top right corner (a ring is cut out around it so
/// it reads on any menu bar).
static REVIEW_DOT: AtomicBool = AtomicBool::new(false);

/// Shows or hides the review dot. A no-op while the state is unchanged.
pub fn set_review_dot(app: &AppHandle, dot: bool) {
    if REVIEW_DOT.swap(dot, Ordering::Relaxed) == dot {
        return;
    }
    let timers = app
        .state::<AppState>()
        .store
        .lock()
        .ok()
        .and_then(|store| store.snapshot().ok())
        .unwrap_or_default();
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon_with_as_template(Some(glyph(any_running(&timers))), true);
    }
}

/// Windows draws tray icons as they are, so the black template is painted
/// white on a dark taskbar.
#[cfg(windows)]
fn whitened(template: &Image<'_>) -> Image<'static> {
    let mut rgba = template.rgba().to_vec();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel[..3].fill(u8::MAX);
    }
    Image::new_owned(rgba, template.width(), template.height())
}

/// The taskbar follows the "default Windows mode", which is dark unless the
/// user picked light; read on every icon change, so a switch shows up the
/// next time the icon changes state.
#[cfg(windows)]
fn taskbar_is_light() -> bool {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("SystemUsesLightTheme"))
        .is_ok_and(|light| light == 1)
}

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

/// Adds or removes the menu-bar icon to match the preference. Idempotent: a
/// redundant call is a no-op, so the settings command does not have to track
/// whether the tray is already mounted.
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
/// Called on every mutation, never on a timer: nothing here is clock-driven.
/// The menu is built when it opens, so it needs no refresh.
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
    builder = builder.item(&MenuItemBuilder::with_id(OPEN_WINDOW, "Open OpenRize").build(app)?);
    builder = builder.item(&MenuItemBuilder::with_id(QUIT, "Quit OpenRize").build(app)?);
    builder.build()
}

fn disabled(app: &AppHandle, id: &str, text: &str) -> tauri::Result<tauri::menu::MenuItem<Wry>> {
    MenuItemBuilder::with_id(id, text).enabled(false).build(app)
}

/// Mouse down, like a native menu-bar item: the panel or menu opens as the
/// button goes down, not after it comes back up.
fn on_tray_icon_event(tray: &TrayIcon, event: TrayIconEvent) {
    let TrayIconEvent::Click {
        rect,
        button,
        button_state: MouseButtonState::Down,
        ..
    } = event
    else {
        return;
    };
    match button {
        MouseButton::Left => crate::pulse::toggle(tray.app_handle(), rect),
        MouseButton::Right => show_menu(tray),
        MouseButton::Middle => {}
    }
}

/// Opens the native menu, attached to the icon only while it is open. With a
/// menu attached, macOS opens it on every click, left included, which would
/// leave no click for the panel (`show_menu_on_left_click(false)` does not
/// stop it). Built fresh from the timers, so it is never stale.
fn show_menu(tray: &TrayIcon) {
    let app = tray.app_handle();
    crate::pulse::hide(app);
    let timers = app
        .state::<AppState>()
        .store
        .lock()
        .ok()
        .and_then(|store| store.snapshot().ok())
        .unwrap_or_default();
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
