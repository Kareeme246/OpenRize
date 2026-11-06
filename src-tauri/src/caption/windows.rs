//! Windows: removes the native caption and puts a small native child window over the page's
//! maximize button to answer `WM_NCHITTEST` with `HTMAXBUTTON` (WebView2's child windows hide the
//! top-level procedure from the system), and reports [`EVENT_MAXIMIZE_BUTTON`] to the page.

use std::cell::Cell;
use std::sync::Once;

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, EnableMenuItem, FindWindowExW, GetCursorPos, GetParent,
    GetSystemMenu, GetWindowLongPtrW, IsIconic, IsZoomed, LoadCursorW, PostMessageW,
    RegisterClassExW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TrackPopupMenu, CREATESTRUCTW, GWLP_USERDATA, HTMAXBUTTON, HWND_TOP, IDC_ARROW, MF_BYCOMMAND,
    MF_ENABLED, MF_GRAYED, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE,
    SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_ERASEBKGND,
    WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE,
    WM_NCMOUSEMOVE, WM_SYSCOMMAND, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};

use crate::caption::{ButtonRect, MaximizeButtonState, EVENT_MAXIMIZE_BUTTON};

const CLASS_NAME: PCWSTR = w!("OPENRIZE_MAXIMIZE_BUTTON");

static REGISTER_CLASS: Once = Once::new();

/// Removes the native caption but keeps tao's shadow and resize edges; Windows 11 only
/// rounds an undecorated window's corners when asked.
pub fn prepare(window: &WebviewWindow) {
    if let Err(error) = window.set_decorations(false) {
        eprintln!("could not remove the native caption: {error}");
    }
    if let Err(error) = window.set_shadow(true) {
        eprintln!("could not enable the window shadow: {error}");
    }
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let preference = DWMWCP_ROUND;
    // SAFETY: `hwnd` is the live main window and `preference` outlives the call;
    // the size passed is that of the value behind the pointer. Windows 10
    // rejects the attribute, which is fine: its corners are square.
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&preference).cast(),
            std::mem::size_of_val(&preference) as u32,
        )
    };
}

/// Puts the native window over the maximize button, hiding it when the button is off screen.
pub fn place_maximize_button(window: &WebviewWindow, rect: Option<ButtonRect>) {
    let Ok(parent) = window.hwnd() else {
        return;
    };
    // SAFETY: `parent` is the live main window and this runs on its thread (a
    // synchronous command runs on the main thread).
    let existing = unsafe { FindWindowExW(Some(parent), None, CLASS_NAME, PCWSTR::null()) }.ok();

    let Some(rect) = rect.filter(|rect| rect.width > 0 && rect.height > 0) else {
        if let Some(overlay) = existing {
            // SAFETY: `overlay` was just found as a child of the live main window.
            let _ = unsafe { ShowWindow(overlay, SW_HIDE) };
        }
        return;
    };

    let Some(overlay) = existing.or_else(|| create_overlay(parent, window.app_handle())) else {
        return;
    };
    // SAFETY: `overlay` is a live child of the main window. It goes to the top
    // of its siblings every time, because the webview is also one and must not
    // end up above it.
    unsafe {
        let _ = SetWindowPos(
            overlay,
            Some(HWND_TOP),
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            SWP_NOACTIVATE,
        );
        let _ = ShowWindow(overlay, SW_SHOWNOACTIVATE);
    }
}

/// Opens the system window menu at the pointer and runs what is chosen. WebView2's own menu
/// shows Restore on a window that is not maximized, so the real state is asked for here.
pub fn show_system_menu(window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let resizable = window.is_resizable().unwrap_or(false);
    let maximizable = window.is_maximizable().unwrap_or(false);
    // SAFETY: `hwnd` is the live main window and this runs on its thread. The
    // menu belongs to the window and must not be destroyed; `point` and the
    // menu outlive the calls that use them.
    unsafe {
        let menu = GetSystemMenu(hwnd, false);
        if menu.is_invalid() {
            return;
        }
        let zoomed = IsZoomed(hwnd).as_bool();
        let iconic = IsIconic(hwnd).as_bool();
        let enable = |command: u32, on: bool| {
            let state = if on { MF_ENABLED } else { MF_GRAYED };
            let _ = EnableMenuItem(menu, command, MF_BYCOMMAND | state);
        };
        enable(SC_RESTORE, zoomed || iconic);
        enable(SC_MOVE, !zoomed && !iconic);
        enable(SC_SIZE, resizable && !zoomed && !iconic);
        enable(SC_MINIMIZE, !iconic);
        enable(SC_MAXIMIZE, maximizable && !zoomed);
        enable(SC_CLOSE, true);

        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() {
            return;
        }
        // A menu is dismissed by a click elsewhere only if its window is the
        // foreground one.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        );
        if chosen.as_bool() {
            let _ = PostMessageW(
                Some(hwnd),
                WM_SYSCOMMAND,
                WPARAM(chosen.0 as usize),
                LPARAM(0),
            );
        }
    }
}

/// What the overlay's window procedure knows; owned by the window and freed
/// with it. `Cell`s because emitting can pump messages back into the procedure.
struct Overlay {
    app: AppHandle,
    hovered: Cell<bool>,
    pressed: Cell<bool>,
    tracking: Cell<bool>,
}

impl Overlay {
    fn report(&self) {
        let _ = self.app.emit(
            EVENT_MAXIMIZE_BUTTON,
            MaximizeButtonState {
                hovered: self.hovered.get(),
                pressed: self.pressed.get(),
            },
        );
    }
}

fn create_overlay(parent: HWND, app: &AppHandle) -> Option<HWND> {
    // SAFETY: plain Win32 calls on the main thread; the class is registered once. The state
    // pointer given to `CreateWindowExW` is taken back by `WM_NCCREATE` and freed by
    // `WM_NCDESTROY`, or here when creation fails before that.
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(PCWSTR::null()).ok()?.0);
        REGISTER_CLASS.call_once(|| {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH::default(),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            RegisterClassExW(&class);
        });
        let state = Box::into_raw(Box::new(Overlay {
            app: app.clone(),
            hovered: Cell::new(false),
            pressed: Cell::new(false),
            tracking: Cell::new(false),
        }));
        let created = CreateWindowExW(
            Default::default(),
            CLASS_NAME,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            Some(instance),
            Some(state.cast_const().cast()),
        );
        if created.is_err() {
            // The window never took ownership, so nothing else will free it.
            drop(Box::from_raw(state));
        }
        created.ok()
    }
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        // SAFETY: for this message `lparam` points at the `CREATESTRUCTW`, whose
        // `lpCreateParams` is the state pointer passed to `CreateWindowExW`.
        let state = unsafe { (*(lparam.0 as *const CREATESTRUCTW)).lpCreateParams };
        // SAFETY: `hwnd` is the window being created.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize) };
        // SAFETY: the default procedure accepts any message.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: the user data is the pointer stored above (or null before it) and
    // is only freed in `WM_NCDESTROY`, the last message the window gets.
    let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Overlay).as_ref() };
    let Some(state) = state else {
        // SAFETY: the default procedure accepts any message.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };

    match msg {
        // The whole point: the system takes this answer as "the pointer is on
        // the maximize button" and raises the snap flyout.
        WM_NCHITTEST => return LRESULT(HTMAXBUTTON as isize),
        WM_NCMOUSEMOVE => {
            if !state.tracking.get() {
                let mut request = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: `request` is fully initialized and outlives the call.
                state
                    .tracking
                    .set(unsafe { TrackMouseEvent(&mut request) }.is_ok());
            }
            if !state.hovered.replace(true) {
                state.report();
            }
            // The default procedure is how the system learns the pointer is on
            // the maximize button, so it must see this message as well.
        }
        WM_NCMOUSELEAVE => {
            state.tracking.set(false);
            state.hovered.set(false);
            state.pressed.set(false);
            state.report();
            return LRESULT(0);
        }
        WM_NCLBUTTONDOWN => {
            state.pressed.set(true);
            state.report();
            return LRESULT(0);
        }
        WM_NCLBUTTONUP => {
            if state.pressed.replace(false) {
                state.report();
                toggle_maximized(hwnd);
            }
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_NCDESTROY => {
            // SAFETY: the state was leaked by `create_overlay`; this is the last
            // message, so nothing reads the pointer afterwards.
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(std::ptr::from_ref(state).cast_mut()));
            }
            // SAFETY: the default procedure accepts any message.
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
        _ => {}
    }
    // SAFETY: the default procedure accepts any message.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Maximizes or restores the main window like its caption button; posted so it runs after this
/// message.
fn toggle_maximized(overlay: HWND) {
    // SAFETY: `overlay` is the window currently handling a message; its parent
    // is the main window.
    unsafe {
        let Ok(top) = GetParent(overlay) else {
            return;
        };
        let command = if IsZoomed(top).as_bool() {
            SC_RESTORE
        } else {
            SC_MAXIMIZE
        };
        let _ = PostMessageW(
            Some(top),
            WM_SYSCOMMAND,
            WPARAM(command as usize),
            LPARAM(0),
        );
    }
}
