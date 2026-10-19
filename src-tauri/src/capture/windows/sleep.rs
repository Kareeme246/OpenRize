//! Explicit suspend/resume registration on a message-only window. Such windows
//! do not receive broadcasts automatically; RegisterSuspendResumeNotification
//! opts this HWND in, including on Modern Standby systems (Windows 8+).
//! https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registersuspendresumenotification

use tauri::AppHandle;
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    RegisterSuspendResumeNotification, UnregisterSuspendResumeNotification,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW, UnregisterClassW,
    DEVICE_NOTIFY_WINDOW_HANDLE, GWLP_USERDATA, HWND_MESSAGE, MSG, PBT_APMRESUMEAUTOMATIC,
    PBT_APMSUSPEND, WINDOW_EX_STYLE, WINDOW_STYLE, WM_POWERBROADCAST, WNDCLASSW,
};

use crate::capture::SystemEvent;

fn power_event(message: u32, parameter: usize) -> Option<SystemEvent> {
    if message != WM_POWERBROADCAST {
        return None;
    }
    match parameter as u32 {
        PBT_APMSUSPEND => Some(SystemEvent::Suspend),
        PBT_APMRESUMEAUTOMATIC => Some(SystemEvent::Resume),
        _ => None,
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    parameter: WPARAM,
    data: LPARAM,
) -> LRESULT {
    // SAFETY: this is our private window, on its creating thread. USERDATA
    // points to the AppHandle kept on that thread's stack until after destroy.
    let app = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const AppHandle;
    if let (Some(event), Some(app)) = (power_event(message, parameter.0), unsafe { app.as_ref() }) {
        crate::breaks::system_event(app, event);
        return LRESULT(1);
    }
    unsafe { DefWindowProcW(window, message, parameter, data) }
}

pub fn register_sleep_listeners(app: AppHandle) -> Result<(), String> {
    let (ready, started) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("system-power".to_string())
        .spawn(move || {
            // SAFETY: this thread owns the class, HWND and its message pump. All
            // resources outlive dispatch and are released on the same thread.
            let result = unsafe { listen(&app, &ready) };
            if let Err(error) = result {
                eprintln!("system power listener stopped: {error}");
                let _ = ready.send(Err(error));
            }
        })
        .map_err(|error| error.to_string())?;
    started.recv().map_err(|error| error.to_string())?
}

unsafe fn listen(
    app: &AppHandle,
    ready: &std::sync::mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    unsafe {
        let module = GetModuleHandleW(None).map_err(|error| error.to_string())?;
        let instance = HINSTANCE(module.0);
        let name = w!("OpenRizeSystemPower");
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: name,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_win32().to_string());
        }
        let result = (|| {
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                name,
                name,
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
            .map_err(|error| error.to_string())?;
            SetWindowLongPtrW(window, GWLP_USERDATA, app as *const AppHandle as isize);
            let registration =
                RegisterSuspendResumeNotification(HANDLE(window.0), DEVICE_NOTIFY_WINDOW_HANDLE);
            let result = match registration {
                Ok(registration) => {
                    let _ = ready.send(Ok(()));
                    let mut message = MSG::default();
                    let result = loop {
                        let status = GetMessageW(&mut message, None, 0, 0).0;
                        if status == -1 {
                            break Err(windows::core::Error::from_win32().to_string());
                        }
                        if status == 0 {
                            break Ok(());
                        }
                        DispatchMessageW(&message);
                    };
                    let _ = UnregisterSuspendResumeNotification(registration);
                    result
                }
                Err(error) => Err(error.to_string()),
            };
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            let _ = DestroyWindow(window);
            result
        })();
        let _ = UnregisterClassW(name, Some(instance));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_system_suspend_and_automatic_resume_are_inputs() {
        assert_eq!(
            power_event(WM_POWERBROADCAST, PBT_APMSUSPEND as usize),
            Some(SystemEvent::Suspend)
        );
        assert_eq!(
            power_event(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC as usize),
            Some(SystemEvent::Resume)
        );
        assert_eq!(power_event(WM_POWERBROADCAST, 0), None);
        assert_eq!(power_event(0, PBT_APMSUSPEND as usize), None);
    }
}
