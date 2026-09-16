//! The macOS menu-bar icon. Icon only, no text (decision T1): the glyph shape
//! carries idle vs active, and the menu carries the running timers.

use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::timers::Timer;

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
    if active {
        tauri::include_image!("icons/tray-active.png")
    } else {
        tauri::include_image!("icons/tray-idle.png")
    }
}

pub fn init(app: &AppHandle, timers: &[Timer]) -> tauri::Result<()> {
    let menu = build_menu(app, timers)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(glyph(any_running(timers)))
        .icon_as_template(true)
        .tooltip(tooltip(timers))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;

    Ok(())
}

/// Repaints the tray from a snapshot, then tells the frontend what changed.
/// Called on every mutation, never on a timer: a native menu is expensive to
/// rebuild and nothing here is clock-driven.
pub fn refresh(app: &AppHandle, timers: &[Timer]) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        // Sets icon and template flag together; doing it in two calls flickers.
        tray.set_icon_with_as_template(Some(glyph(any_running(timers))), true)?;
        tray.set_tooltip(Some(tooltip(timers)))?;
        tray.set_menu(Some(build_menu(app, timers)?))?;
    }

    // The tray can pause a timer behind the window's back, so the authoritative
    // list is broadcast for the frontend to adopt.
    let _ = app.emit(crate::EVENT_TIMERS_CHANGED, timers);
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
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
