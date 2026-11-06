//! Windows capture: the foreground window through Win32, its app named from the
//! executable's version resource (see `identity`), and a browser's URL via UI Automation.
//! Sampling runs on the sampler thread; UIA on its own worker; power on its own loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::core::{w, Interface, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Media::Audio::{EDataFlow, ERole, IMMDeviceEnumerator};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W;
use windows::Win32::System::RemoteDesktop::{
    WTSActive, WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW,
    WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, ProcessPowerThrottling, QueryFullProcessImageNameW,
    SetProcessInformation, PROCESS_NAME_WIN32, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetClassNameW, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId,
};

use super::{domain_of, WindowSample};

/// Keeps Windows 11 from putting the tray-only process into EcoQoS, which slows its
/// threads and coalesces its timers.
pub struct AppNapAssertion;

impl AppNapAssertion {
    pub fn begin(_reason: &str) -> Self {
        // Opting out is best effort: Windows before 10 1709 lacks the call.
        if let Err(error) = set_execution_speed_throttling(Some(false)) {
            eprintln!("could not opt out of power throttling: {error}");
        }
        Self
    }
}

impl Drop for AppNapAssertion {
    fn drop(&mut self) {
        let _ = set_execution_speed_throttling(None);
    }
}

/// Forces execution speed throttling on or off, or with `None` leaves it to the system.
fn set_execution_speed_throttling(forced: Option<bool>) -> windows::core::Result<()> {
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: if forced.is_some() {
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED
        } else {
            0
        },
        StateMask: if forced == Some(true) {
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED
        } else {
            0
        },
    };
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing,
    // and the buffer is a PROCESS_POWER_THROTTLING_STATE of the size passed.
    unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
}

/// The session's display is off, as last told by the power loop. Modern
/// Standby laptops turn the display off on lid close without ever sending
/// PBT_APMSUSPEND, so this is the only sign nobody is looking.
static DISPLAY_OFF: AtomicBool = AtomicBool::new(false);
static SESSION_LOCKED: AtomicBool = AtomicBool::new(false);
/// Lock notifications arrive, so `SESSION_LOCKED` is authoritative over the polled flag.
static SESSION_EVENTS_LIVE: AtomicBool = AtomicBool::new(false);

/// Input this recent proves the display is on, whatever the last display
/// notification said: a keypress or mouse move is what turns it back on.
const DISPLAY_OFF_STALE_MS: u64 = 2_000;

/// MONITOR_DISPLAY_STATE of GUID_SESSION_DISPLAY_STATUS: 0 off, 1 on, 2
/// dimmed. A dimmed display is still being looked at.
fn display_is_off(state: u32) -> bool {
    state == 0
}

/// Whether a WM_WTSSESSION_CHANGE code settles the lock state. Logon counts
/// as an unlock so a state left over from a previous session cannot stick.
fn locked_after(code: u32) -> Option<bool> {
    use windows::Win32::UI::WindowsAndMessaging::{
        WTS_SESSION_LOCK, WTS_SESSION_LOGON, WTS_SESSION_UNLOCK,
    };

    match code {
        WTS_SESSION_LOCK => Some(true),
        WTS_SESSION_UNLOCK | WTS_SESSION_LOGON => Some(false),
        _ => None,
    }
}

/// Message-only windows get no broadcasts implicitly: register this HWND for suspend/resume
/// and for the display and lock state that decide `user_is_away`.
pub fn register_sleep_listeners(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        if let Err(error) = power_message_loop(app) {
            eprintln!("could not listen for system sleep: {error}");
        }
    });
}

fn power_message_loop(app: tauri::AppHandle) -> windows::core::Result<()> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Power::{
        RegisterPowerSettingNotification, RegisterSuspendResumeNotification,
        UnregisterPowerSettingNotification, UnregisterSuspendResumeNotification,
    };
    use windows::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows::Win32::System::SystemServices::GUID_SESSION_DISPLAY_STATUS;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, RegisterClassW,
        SetWindowLongPtrW, UnregisterClassW, DEVICE_NOTIFY_WINDOW_HANDLE, GWLP_USERDATA,
        HWND_MESSAGE, MSG, WNDCLASSW,
    };

    // SAFETY: this thread owns the window and message pump. `app` stays at a
    // stable stack address until after DestroyWindow; the callback only borrows
    // it synchronously on this thread. No HWND or pointer escapes the loop.
    unsafe {
        let instance = GetModuleHandleW(None)?.into();
        let class = w!("OpenRizeSystemPower");
        let definition = WNDCLASSW {
            lpfnWndProc: Some(power_window_proc),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassW(&definition) == 0 {
            return Err(windows::core::Error::from_win32());
        }
        let window = match CreateWindowExW(
            Default::default(),
            class,
            w!(""),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        ) {
            Ok(window) => window,
            Err(error) => {
                let _ = UnregisterClassW(class, Some(instance));
                return Err(error);
            }
        };
        SetWindowLongPtrW(
            window,
            GWLP_USERDATA,
            (&app as *const tauri::AppHandle) as isize,
        );
        let result = (|| {
            let registration =
                RegisterSuspendResumeNotification(HANDLE(window.0), DEVICE_NOTIFY_WINDOW_HANDLE)?;
            // The rest is best effort: without it `user_is_away` falls back to
            // polling the session, which cannot see a display that is off.
            let display = RegisterPowerSettingNotification(
                HANDLE(window.0),
                &GUID_SESSION_DISPLAY_STATUS,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
            .map_err(|error| eprintln!("could not listen for display state: {error}"))
            .ok();
            let session = WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION)
                .map_err(|error| eprintln!("could not listen for session lock: {error}"))
                .is_ok();
            SESSION_EVENTS_LIVE.store(session, Ordering::Relaxed);
            let mut message = MSG::default();
            let result = loop {
                match GetMessageW(&mut message, None, 0, 0).0 {
                    -1 => break Err(windows::core::Error::from_win32()),
                    0 => break Ok(()),
                    _ => {
                        DispatchMessageW(&message);
                    }
                }
            };
            if session {
                let _ = WTSUnRegisterSessionNotification(window);
            }
            if let Some(display) = display {
                let _ = UnregisterPowerSettingNotification(display);
            }
            let _ = UnregisterSuspendResumeNotification(registration);
            SESSION_EVENTS_LIVE.store(false, Ordering::Relaxed);
            SESSION_LOCKED.store(false, Ordering::Relaxed);
            DISPLAY_OFF.store(false, Ordering::Relaxed);
            result
        })();
        let _ = DestroyWindow(window);
        let _ = UnregisterClassW(class, Some(instance));
        result
    }
}

unsafe extern "system" fn power_window_proc(
    window: HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::System::Power::POWERBROADCAST_SETTING;
    use windows::Win32::System::SystemServices::GUID_SESSION_DISPLAY_STATUS;
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, GetWindowLongPtrW, GWLP_USERDATA, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND,
        PBT_POWERSETTINGCHANGE, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE,
    };

    // SAFETY: Windows calls this on the window's owning thread. USERDATA is
    // either null during creation or the live AppHandle in power_message_loop.
    unsafe {
        if message == WM_WTSSESSION_CHANGE {
            if let Some(locked) = locked_after(wparam.0 as u32) {
                SESSION_LOCKED.store(locked, Ordering::Relaxed);
            }
            return LRESULT(0);
        }
        if message == WM_POWERBROADCAST && wparam.0 as u32 == PBT_POWERSETTINGCHANGE {
            // lparam points at a POWERBROADCAST_SETTING whose Data is
            // `DataLength` bytes long; display state is a single DWORD.
            let setting = lparam.0 as *const POWERBROADCAST_SETTING;
            if !setting.is_null()
                && (*setting).PowerSetting == GUID_SESSION_DISPLAY_STATUS
                && (*setting).DataLength as usize == size_of::<u32>()
            {
                let data = std::ptr::addr_of!((*setting).Data).cast::<u32>();
                DISPLAY_OFF.store(display_is_off(data.read_unaligned()), Ordering::Relaxed);
            }
            return LRESULT(1);
        }
        if message == WM_POWERBROADCAST {
            let event = match wparam.0 as u32 {
                PBT_APMSUSPEND => Some(super::PowerEvent::Suspend),
                PBT_APMRESUMEAUTOMATIC => Some(super::PowerEvent::Resume),
                _ => None,
            };
            let app = GetWindowLongPtrW(window, GWLP_USERDATA) as *const tauri::AppHandle;
            if let (Some(event), Some(app)) = (event, app.as_ref()) {
                super::power_event(app, event);
                return LRESULT(1);
            }
        }
        DefWindowProcW(window, message, wparam, lparam)
    }
}

/// Hosts the windows of packaged (UWP) apps; the app itself is a child
/// window owned by another process.
const FRAME_HOST: &str = "applicationframehost.exe";

const BROWSERS: [&str; 10] = [
    "chrome.exe",
    "msedge.exe",
    "brave.exe",
    "vivaldi.exe",
    "opera.exe",
    "arc.exe",
    "firefox.exe",
    "zen.exe",
    "librewolf.exe",
    "floorp.exe",
];

thread_local! {
    /// Display names by executable path; version resources never change.
    static APP_NAMES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Nobody can be at the PC: display off, session locked, or switched to another user or
/// remote session. Display sleep is not a `PowerEvent`: it ends the segment through this,
/// while the break engine only hears of real suspends.
pub fn user_is_away() -> bool {
    if DISPLAY_OFF.load(Ordering::Relaxed) {
        if idle_ms() < DISPLAY_OFF_STALE_MS {
            // Input turned the display back on and the notification was missed.
            DISPLAY_OFF.store(false, Ordering::Relaxed);
        } else {
            return true;
        }
    }
    SESSION_LOCKED.load(Ordering::Relaxed) || session_is_inactive()
}

/// Polls for switched out or disconnected and, while no lock notifications arrive, locked.
/// SessionFlags is read for that fallback only: its LOCK/UNLOCK values are documented
/// inconsistently between Windows versions.
fn session_is_inactive() -> bool {
    let mut buffer = PWSTR::null();
    let mut bytes = 0u32;
    // SAFETY: on success the call hands us a WTSINFOEXW it allocated, freed
    // below with WTSFreeMemory.
    let queried = unsafe {
        WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        )
    };
    if queried.is_err() || buffer.is_null() {
        return false;
    }
    // SAFETY: WTSSessionInfoEx fills a WTSINFOEXW; Level 1 is the only
    // level defined and selects the `WTSInfoExLevel1` arm of the union.
    let away = unsafe {
        let info = &*buffer.0.cast::<WTSINFOEXW>();
        info.Level == 1 && {
            let session = &info.Data.WTSInfoExLevel1;
            session.SessionState != WTSActive
                || (!SESSION_EVENTS_LIVE.load(Ordering::Relaxed)
                    && session.SessionFlags == WTS_SESSIONSTATE_LOCK as i32)
        }
    };
    // SAFETY: the buffer came from WTSQuerySessionInformationW above.
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    away
}

pub fn idle_ms() -> u64 {
    use windows::Win32::System::SystemInformation::{GetTickCount, GetTickCount64};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut input = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: GetLastInputInfo fills the struct sized above; the tick
    // counters take no arguments.
    unsafe {
        if !GetLastInputInfo(&mut input).as_bool() {
            return 0;
        }
        idle_between(GetTickCount(), input.dwTime, GetTickCount64())
    }
}

/// A difference this close to a full wrap of the 32-bit tick count is input
/// stamped just after the clock was read, not 49 days without any.
const WRAPPED_IDLE_FLOOR: u64 = u32::MAX as u64 - 60_000;

/// Idle time from two 32-bit tick counts. GetTickCount is coarse (~15 ms), so the last input can
/// be stamped slightly ahead of the clock, wrapping to ~49.7 days and faking an Idle break. Such
/// a difference, or one longer than uptime, reads as no idle time.
fn idle_between(now_tick: u32, last_input_tick: u32, uptime_ms: u64) -> u64 {
    let idle = u64::from(now_tick.wrapping_sub(last_input_tick));
    if idle > uptime_ms || idle > WRAPPED_IDLE_FLOOR {
        0
    } else {
        idle
    }
}

/// Whether the foreground app is being watched or listened to. Fullscreen counts on its own.
/// Otherwise the system-wide display request (which any background app can hold) must be backed
/// by an audio session playing or capturing in the foreground app, its executable or descendants.
fn keeps_display_awake(pid: u32) -> bool {
    watching(display_required, foreground_is_fullscreen, || {
        foreground_is_playing_or_recording(pid)
    })
}

/// The decision of `keeps_display_awake`, evaluating each signal only if earlier ones did not
/// settle it.
fn watching(
    display_required: impl FnOnce() -> bool,
    fullscreen: impl FnOnce() -> bool,
    audio: impl FnOnce() -> bool,
) -> bool {
    fullscreen() || (display_required() && audio())
}

/// Some process holds the display on (ES_DISPLAY_REQUIRED), as video and call apps do.
fn display_required() -> bool {
    use windows::Win32::System::Power::{
        CallNtPowerInformation, SystemExecutionState, ES_DISPLAY_REQUIRED,
    };

    let mut state = 0u32;
    // SAFETY: SystemExecutionState takes no input and writes one u32.
    let status = unsafe {
        CallNtPowerInformation(
            SystemExecutionState,
            None,
            0,
            Some((&mut state as *mut u32).cast()),
            size_of::<u32>() as u32,
        )
    };
    status.is_ok() && state & ES_DISPLAY_REQUIRED.0 != 0
}

/// The shell's own test for a fullscreen foreground app.
fn foreground_is_fullscreen() -> bool {
    use windows::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE, QUNS_BUSY,
        QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };

    fn fullscreen(state: QUERY_USER_NOTIFICATION_STATE) -> bool {
        matches!(
            state,
            QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
        )
    }

    // SAFETY: a plain query.
    unsafe { SHQueryUserNotificationState() }.is_ok_and(fullscreen)
}

/// Sound level below which a playback session counts as silent (-60 dB).
const AUDIBLE_PEAK: f32 = 0.001;

thread_local! {
    static AUDIO_DEVICES: RefCell<Option<IMMDeviceEnumerator>> = const { RefCell::new(None) };
}

/// The foreground app has an active audio session playing sound or capturing (even if muted).
/// Only the default playback and capture devices are looked at.
fn foreground_is_playing_or_recording(foreground_pid: u32) -> bool {
    use windows::Win32::Media::Audio::{eCapture, eCommunications, eConsole, eRender};

    let Some(executable) = process_path(foreground_pid).map(|path| file_name(&path).to_lowercase())
    else {
        return false;
    };
    let Some(devices) = audio_devices() else {
        return false;
    };
    // Fetched only when a session is not found by pid or executable name.
    let mut parents: Option<Rc<ParentMap>> = None;
    let mut belongs = |pid: u32| {
        pid == foreground_pid
            || process_path(pid)
                .is_some_and(|path| file_name(&path).eq_ignore_ascii_case(&executable))
            || descends_from(
                pid,
                foreground_pid,
                parents.get_or_insert_with(cached_process_parents),
            )
    };
    [
        (eRender, eConsole, true),
        (eRender, eCommunications, true),
        (eCapture, eConsole, false),
        (eCapture, eCommunications, false),
    ]
    .into_iter()
    .any(|(flow, role, needs_sound)| {
        // SAFETY: COM calls on objects created here, on the sampler thread
        // that joined the multithreaded apartment in `audio_devices`.
        unsafe { device_has_session(&devices, flow, role, needs_sound, &mut belongs) }
    })
}

fn audio_devices() -> Option<IMMDeviceEnumerator> {
    use windows::Win32::Media::Audio::MMDeviceEnumerator;

    AUDIO_DEVICES.with(|devices| {
        let mut devices = devices.borrow_mut();
        if devices.is_none() {
            // SAFETY: joins this thread to the multithreaded apartment for
            // its whole life; a second call is harmless (S_FALSE).
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                *devices = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER).ok();
            }
        }
        devices.clone()
    })
}

/// # Safety
/// COM must be initialized on the calling thread.
unsafe fn device_has_session(
    devices: &IMMDeviceEnumerator,
    flow: EDataFlow,
    role: ERole,
    needs_sound: bool,
    belongs: &mut impl FnMut(u32) -> bool,
) -> bool {
    use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
    use windows::Win32::Media::Audio::{
        AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
    };
    use windows::Win32::System::Com::CLSCTX_ALL;

    // SAFETY: every failure ends the search on this device.
    unsafe {
        let Ok(device) = devices.GetDefaultAudioEndpoint(flow, role) else {
            return false;
        };
        let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
            return false;
        };
        let Ok(sessions) = manager.GetSessionEnumerator() else {
            return false;
        };
        (0..sessions.GetCount().unwrap_or(0)).any(|index| {
            let Ok(control) = sessions.GetSession(index) else {
                return false;
            };
            if control.GetState().ok() != Some(AudioSessionStateActive) {
                return false;
            }
            if needs_sound {
                let peak = control
                    .cast::<IAudioMeterInformation>()
                    .and_then(|meter| meter.GetPeakValue());
                if !peak.is_ok_and(|peak| peak > AUDIBLE_PEAK) {
                    return false;
                }
            }
            control
                .cast::<IAudioSessionControl2>()
                .and_then(|control| control.GetProcessId())
                .is_ok_and(|pid| pid != 0 && belongs(pid))
        })
    }
}

/// How long a process-parent map is reused: a snapshot of every process costs far more than
/// the per-tick lookups; a process started within the window is found by the next map.
const PARENTS_TTL: Duration = Duration::from_secs(5);

type ParentMap = HashMap<u32, u32>;

thread_local! {
    static PARENTS: RefCell<Option<(Instant, Rc<ParentMap>)>> = const { RefCell::new(None) };
    /// The last process whose path could not be opened, and its executable name.
    static PROTECTED_NAMES: RefCell<Option<(u32, String)>> = const { RefCell::new(None) };
}

fn cached_process_parents() -> Rc<ParentMap> {
    PARENTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((taken, parents)) = cache.as_ref() {
            if taken.elapsed() < PARENTS_TTL {
                return Rc::clone(parents);
            }
        }
        let parents = Rc::new(process_parents());
        *cache = Some((Instant::now(), Rc::clone(&parents)));
        parents
    })
}

fn process_parents() -> ParentMap {
    let mut parents = HashMap::new();
    each_process(|entry| {
        parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        true
    });
    parents
}

/// The executable name of a process, read from a snapshot; unlike `process_path` it needs
/// no access to the process, so it also names SYSTEM, protected and elevated ones.
fn process_exe_name(pid: u32) -> Option<String> {
    if let Some(name) = PROTECTED_NAMES.with(|last| {
        last.borrow()
            .as_ref()
            .filter(|(cached, _)| *cached == pid)
            .map(|(_, name)| name.clone())
    }) {
        return Some(name);
    }
    let mut name = None;
    each_process(|entry| {
        if entry.th32ProcessID != pid {
            return true;
        }
        let file = &entry.szExeFile;
        let length = file
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(file.len());
        name = Some(String::from_utf16_lossy(&file[..length]));
        false
    });
    if let Some(name) = &name {
        PROTECTED_NAMES.with(|last| *last.borrow_mut() = Some((pid, name.clone())));
    }
    name
}

/// Calls `visit` on each running process of one snapshot until it returns false.
fn each_process(mut visit: impl FnMut(&PROCESSENTRY32W) -> bool) {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    // SAFETY: the snapshot handle is closed before returning, and the entry
    // is sized as the API requires.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more && visit(&entry) {
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
}

fn descends_from(pid: u32, ancestor: u32, parents: &HashMap<u32, u32>) -> bool {
    let mut current = pid;
    for _ in 0..8 {
        match parents.get(&current) {
            Some(&parent) if parent == ancestor => return true,
            Some(&parent) if parent != 0 && parent != current => current = parent,
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod presence_tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn idle_is_the_gap_between_ticks() {
        assert_eq!(idle_between(10_000, 4_000, u64::MAX), 6_000);
        assert_eq!(idle_between(7, 7, 1_000_000), 0);
    }

    #[test]
    fn idle_survives_the_tick_counter_wrapping() {
        assert_eq!(idle_between(6, u32::MAX - 4, u64::from(u32::MAX) * 2), 11);
    }

    #[test]
    fn input_stamped_ahead_of_the_clock_is_not_a_49_day_idle() {
        assert_eq!(idle_between(1_000, 1_016, u64::MAX), 0);
        assert_eq!(idle_between(0, 1, u64::MAX), 0);
    }

    #[test]
    fn idle_longer_than_the_uptime_is_impossible() {
        assert_eq!(idle_between(50_000, 10, 30_000), 0);
        assert_eq!(idle_between(50_000, 10, 60_000), 49_990);
    }

    #[test]
    fn a_display_request_alone_is_not_watching() {
        assert!(!watching(|| true, || false, || false));
        assert!(watching(|| true, || false, || true));
    }

    #[test]
    fn fullscreen_is_watching_without_a_display_request() {
        assert!(watching(|| false, || true, || false));
        assert!(watching(|| true, || true, || false));
    }

    #[test]
    fn audio_without_a_display_request_is_not_watching() {
        let asked = Cell::new(false);
        assert!(!watching(
            || false,
            || false,
            || {
                asked.set(true);
                true
            }
        ));
        assert!(!asked.get());
    }

    #[test]
    fn nothing_else_is_asked_once_fullscreen_settles_it() {
        let asked = Cell::new(0);
        let probe = || {
            asked.set(asked.get() + 1);
            true
        };
        assert!(watching(probe, || true, probe));
        assert_eq!(asked.get(), 0);
    }

    #[test]
    fn a_protected_process_is_named_by_its_executable() {
        assert_eq!(exe_stem("LogonUI.exe"), "LogonUI");
        assert_eq!(exe_stem("consent.exe"), "consent");
        assert_eq!(exe_stem("System"), "System");
    }

    #[test]
    fn a_session_belongs_to_the_app_it_descends_from() {
        // Teams (10) > WebView2 browser (20) > WebView2 audio service (30).
        let parents = HashMap::from([(20, 10), (30, 20), (40, 1)]);
        assert!(descends_from(30, 10, &parents));
        assert!(descends_from(20, 10, &parents));
        assert!(!descends_from(40, 10, &parents));
        assert!(!descends_from(99, 10, &parents));
    }

    #[test]
    fn ancestry_stops_at_cycles_and_the_root() {
        let parents = HashMap::from([(2, 3), (3, 2), (5, 0)]);
        assert!(!descends_from(2, 9, &parents));
        assert!(!descends_from(5, 9, &parents));
    }

    #[test]
    fn only_a_fully_off_display_is_away() {
        assert!(display_is_off(0));
        assert!(!display_is_off(1));
        assert!(!display_is_off(2));
    }

    #[test]
    fn lock_notifications_set_and_clear_the_lock() {
        use windows::Win32::UI::WindowsAndMessaging::{
            WTS_SESSION_LOCK, WTS_SESSION_LOGON, WTS_SESSION_UNLOCK,
        };

        assert_eq!(locked_after(WTS_SESSION_LOCK), Some(true));
        assert_eq!(locked_after(WTS_SESSION_UNLOCK), Some(false));
        assert_eq!(locked_after(WTS_SESSION_LOGON), Some(false));
        // WTS_CONSOLE_DISCONNECT: handled by the session-state poll.
        assert_eq!(locked_after(2), None);
    }
}

pub fn read_active_window() -> Option<WindowSample> {
    // SAFETY: a plain query; a null handle means no window is in front.
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    let title = window_title(window);

    let frame_pid = process_id(window)?;
    let mut process = identify(frame_pid)?;
    if process.executable == FRAME_HOST {
        // Never the frame host itself, or every packaged app would merge into one "Application
        // Frame Host". An unresolved frame reads as no sample, which the store takes as no change.
        process = identify(hosted_app_pid(window, frame_pid)?)?;
    }
    let Process {
        pid,
        app,
        executable,
    } = process;
    let identity = identity::identify(window, pid, &executable, &app);
    // A web app's window has no address bar, and its name is the app's.
    let url = (BROWSERS.contains(&executable.as_str())
        && !identity::is_web_app(&executable, &identity))
    .then(|| address_bar::url_of_titled(window, &title))
    .flatten();
    let domain = url.as_deref().and_then(domain_of);

    Some(WindowSample {
        private: address_bar::is_private_window(window, &executable, &title),
        app: identity.name,
        title,
        // The executable name is the stable identity: the full path moves with updates of
        // apps in versioned folders. Only a many-apps host is qualified (see `identity`).
        bundle_id: Some(identity.id),
        url,
        domain,
        keeps_display_awake: keeps_display_awake(pid),
    })
}

fn window_title(window: HWND) -> String {
    // SAFETY: plain queries on a window handle; a stale handle reads as 0.
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    // SAFETY: the buffer is one unit longer than the title was a moment ago;
    // GetWindowTextW truncates to it and returns the units copied.
    let copied = unsafe { GetWindowTextW(window, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

struct Process {
    pid: u32,
    app: String,
    executable: String,
}

/// Names a process. When its path cannot be opened (SYSTEM, protected or elevated processes
/// such as LogonUI refuse even limited rights) the name comes from a process snapshot,
/// so the same app reads the same either way.
fn identify(pid: u32) -> Option<Process> {
    if let Some(path) = process_path(pid) {
        return Some(Process {
            pid,
            app: app_name(&path),
            executable: file_name(&path).to_lowercase(),
        });
    }
    let name = process_exe_name(pid)?;
    Some(Process {
        pid,
        app: exe_stem(&name),
        executable: name.to_lowercase(),
    })
}

fn exe_stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string())
}

fn process_id(window: HWND) -> Option<u32> {
    let mut pid = 0u32;
    // SAFETY: writes the owning process id into `pid`.
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

fn process_path(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed before returning; limited query rights
    // are granted even for most elevated processes.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        queried.ok()?;
        Some(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

const CORE_WINDOW: &str = "Windows.UI.Core.CoreWindow";

/// The process behind a packaged app's frame: the `CORE_WINDOW` child if there is one, else
/// the first child owned by a process other than the frame host. `None` while the app starts
/// or is suspended.
fn hosted_app_pid(frame: HWND, host_pid: u32) -> Option<u32> {
    struct Search {
        host_pid: u32,
        found: Option<u32>,
        core: Option<u32>,
    }

    unsafe extern "system" fn visit(child: HWND, state: LPARAM) -> BOOL {
        // SAFETY: `state` is the `Search` passed below, alive for the call.
        let search = unsafe { &mut *(state.0 as *mut Search) };
        match process_id(child) {
            Some(pid) if pid != search.host_pid => {
                if window_class(child) == CORE_WINDOW {
                    search.core = Some(pid);
                    return BOOL(0);
                }
                search.found.get_or_insert(pid);
                BOOL(1)
            }
            _ => BOOL(1),
        }
    }

    let mut search = Search {
        host_pid,
        found: None,
        core: None,
    };
    // SAFETY: `visit` only runs during this call, while `search` lives.
    unsafe {
        let _ = EnumChildWindows(
            Some(frame),
            Some(visit),
            LPARAM(&mut search as *mut Search as isize),
        );
    }
    search.core.or(search.found)
}

fn window_class(window: HWND) -> String {
    let mut buffer = [0u16; 64];
    // SAFETY: fills the buffer and returns the units copied, 0 for a stale
    // handle; a longer class name is truncated.
    let copied = unsafe { GetClassNameW(window, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// The version resource's description ("Google Chrome" for chrome.exe), else the executable's name.
fn app_name(path: &str) -> String {
    if let Some(name) = APP_NAMES.with(|names| names.borrow().get(path).cloned()) {
        return name;
    }
    let name = version_string(path, "FileDescription")
        .or_else(|| version_string(path, "ProductName"))
        .unwrap_or_else(|| {
            Path::new(path)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string())
        });
    APP_NAMES.with(|names| names.borrow_mut().insert(path.to_string(), name.clone()));
    name
}

fn version_string(path: &str, key: &str) -> Option<String> {
    let path = wide(path);
    // SAFETY: the version block is read into a buffer of the size Windows
    // asks for, and every pointer VerQueryValueW returns points into it.
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut block = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, block.as_mut_ptr().cast()).ok()?;

        let mut value = std::ptr::null_mut();
        let mut length = 0u32;
        let found = VerQueryValueW(
            block.as_ptr().cast(),
            w!("\\VarFileInfo\\Translation"),
            &mut value,
            &mut length,
        );
        if !found.as_bool() || length < 4 {
            return None;
        }
        let translation = value.cast::<u16>();
        let (language, codepage) = (*translation, *translation.add(1));

        let query = wide(&format!(
            "\\StringFileInfo\\{language:04x}{codepage:04x}\\{key}"
        ));
        let found = VerQueryValueW(
            block.as_ptr().cast(),
            PCWSTR(query.as_ptr()),
            &mut value,
            &mut length,
        );
        if !found.as_bool() || length == 0 {
            return None;
        }
        let text = std::slice::from_raw_parts(value.cast::<u16>(), length as usize);
        let text = String::from_utf16_lossy(text);
        let text = text.trim_end_matches('\0').trim();
        (!text.is_empty()).then(|| text.to_string())
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[path = "windows/address_bar.rs"]
mod address_bar;

#[path = "windows/identity.rs"]
mod identity;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_come_from_windows_paths() {
        assert_eq!(
            file_name(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            "chrome.exe"
        );
        assert_eq!(file_name("chrome.exe"), "chrome.exe");
    }
}
