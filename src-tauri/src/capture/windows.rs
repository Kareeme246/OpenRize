//! Windows capture: the foreground window through Win32, its app named from
//! the executable's version resource, and a browser's URL read from its
//! address bar through UI Automation.
//!
//! Everything here runs on the activity sampler thread, which owns the COM
//! apartment and the per-thread caches below.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use windows::core::{w, Interface, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::RemoteDesktop::{
    WTSActive, WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW,
    WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationValuePattern,
    UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId,
};

use super::{domain_of, WindowSample};

/// Windows does not throttle a background thread the way App Nap does.
pub struct AppNapAssertion;

impl AppNapAssertion {
    pub fn begin(_reason: &str) -> Self {
        Self
    }
}

/// Sleep needs no hook: the sampler thread is suspended with the machine,
/// and the gap it sees on wake closes the open segment (activity.rs).
pub fn register_sleep_listeners(_app: tauri::AppHandle) {}

/// Hosts the windows of packaged (UWP) apps; the app itself is a child
/// window owned by another process.
const FRAME_HOST: &str = "applicationframehost.exe";

/// Executables whose address bar is read for the URL. Chromium and Firefox
/// both expose it as the first Edit control outside the page.
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

/// How far the address bar search may go before giving up on a window.
const SEARCH_MAX_DEPTH: usize = 12;
const SEARCH_MAX_NODES: usize = 600;

thread_local! {
    /// Display names by executable path; version resources never change
    /// while the file exists, and reading one every second is wasteful.
    static APP_NAMES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    static ADDRESS_BAR: RefCell<AddressBar> = const { RefCell::new(AddressBar::new()) };
}

/// Nobody can be at the PC: the session is locked, or switched out to
/// another user or a disconnected remote session.
pub fn user_is_away() -> bool {
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
                || session.SessionFlags == WTS_SESSIONSTATE_LOCK as i32
        }
    };
    // SAFETY: the buffer came from WTSQuerySessionInformationW above.
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    away
}

pub fn read_active_window() -> Option<WindowSample> {
    // SAFETY: a plain query; a null handle means no window is in front.
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    let title = window_title(window);

    let mut pid = process_id(window)?;
    let mut path = process_path(pid)?;
    if file_name(&path).eq_ignore_ascii_case(FRAME_HOST) {
        if let Some(app_pid) = hosted_app_pid(window, pid) {
            pid = app_pid;
            path = process_path(pid)?;
        }
    }

    let executable = file_name(&path).to_lowercase();
    let app = app_name(&path);
    let url = BROWSERS
        .contains(&executable.as_str())
        .then(|| ADDRESS_BAR.with(|bar| bar.borrow_mut().read(window)))
        .flatten();
    let domain = url.as_deref().and_then(domain_of);

    Some(WindowSample {
        app,
        title,
        // The executable name is the stable identity: the full path moves
        // with every update of apps that install into versioned folders.
        bundle_id: Some(executable),
        url,
        domain,
        keeps_display_awake: false,
    })
}

fn window_title(window: HWND) -> String {
    // SAFETY: plain queries on a window handle; a stale handle reads as 0.
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(window, &mut buffer) };
    String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
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

/// The process behind a packaged app's frame: the first child window that
/// belongs to some other process than the frame host.
fn hosted_app_pid(frame: HWND, host_pid: u32) -> Option<u32> {
    struct Search {
        host_pid: u32,
        found: Option<u32>,
    }

    unsafe extern "system" fn visit(child: HWND, state: LPARAM) -> BOOL {
        // SAFETY: `state` is the `Search` passed below, alive for the call.
        let search = unsafe { &mut *(state.0 as *mut Search) };
        match process_id(child) {
            Some(pid) if pid != search.host_pid => {
                search.found = Some(pid);
                BOOL(0)
            }
            _ => BOOL(1),
        }
    }

    let mut search = Search {
        host_pid,
        found: None,
    };
    // SAFETY: `visit` only runs during this call, while `search` lives.
    unsafe {
        let _ = EnumChildWindows(
            Some(frame),
            Some(visit),
            LPARAM(&mut search as *mut Search as isize),
        );
    }
    search.found
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// What the user would call the app: the version resource's description
/// ("Google Chrome" for chrome.exe), else the executable's name.
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

/// The address bar of the browser window last read, kept so the tree is
/// searched once per window rather than once per second.
struct AddressBar {
    automation: Option<IUIAutomation>,
    window: Option<HWND>,
    element: Option<IUIAutomationElement>,
    /// What the bar last showed while the user was not typing in it.
    last_url: Option<String>,
}

impl AddressBar {
    const fn new() -> Self {
        Self {
            automation: None,
            window: None,
            element: None,
            last_url: None,
        }
    }

    fn read(&mut self, window: HWND) -> Option<String> {
        if self.window != Some(window) {
            self.window = Some(window);
            self.element = None;
            self.last_url = None;
        }
        if self.element.is_none() {
            let automation = self.automation()?;
            self.element = find_address_bar(&automation, window);
        }
        let element = self.element.as_ref()?;
        // While the user types in the bar it holds their text, not the page.
        // SAFETY: COM calls on a live element; failures are handled below.
        let typing =
            unsafe { element.CurrentHasKeyboardFocus() }.is_ok_and(|focus| focus.as_bool());
        if typing {
            return self.last_url.clone();
        }
        match unsafe { address_value(element) } {
            Some(text) => {
                self.last_url = normalize_address(&text);
                self.last_url.clone()
            }
            // The element went away (tab torn off, window rebuilt): find it
            // again on the next sample.
            None => {
                self.element = None;
                None
            }
        }
    }

    fn automation(&mut self) -> Option<IUIAutomation> {
        if self.automation.is_none() {
            // SAFETY: joins this thread to the multithreaded apartment for
            // its whole life; a second call is harmless (S_FALSE).
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                self.automation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok();
            }
        }
        self.automation.clone()
    }
}

/// Breadth-first over the browser's own controls for the first Edit, never
/// descending into the page (a Document): walking web content would make
/// Chromium turn on full accessibility for every tab.
fn find_address_bar(automation: &IUIAutomation, window: HWND) -> Option<IUIAutomationElement> {
    // SAFETY: COM calls on live UI Automation objects; every failure ends
    // the search for that branch.
    unsafe {
        let walker = automation.ControlViewWalker().ok()?;
        let root = automation.ElementFromHandle(window).ok()?;
        let mut level = vec![root];
        let mut visited = 0;
        for _ in 0..SEARCH_MAX_DEPTH {
            let mut next = Vec::new();
            for parent in &level {
                let mut child = walker.GetFirstChildElement(parent).ok();
                while let Some(element) = child {
                    visited += 1;
                    if visited > SEARCH_MAX_NODES {
                        return None;
                    }
                    match element.CurrentControlType() {
                        Ok(kind) if kind == UIA_EditControlTypeId => return Some(element),
                        Ok(kind) if kind == UIA_DocumentControlTypeId => {}
                        _ => next.push(element.clone()),
                    }
                    child = walker.GetNextSiblingElement(&element).ok();
                }
            }
            if next.is_empty() {
                return None;
            }
            level = next;
        }
        None
    }
}

/// # Safety
/// `element` must be a live UI Automation element.
unsafe fn address_value(element: &IUIAutomationElement) -> Option<String> {
    let pattern: IUIAutomationValuePattern = unsafe {
        element
            .GetCurrentPattern(UIA_ValuePatternId)
            .ok()?
            .cast()
            .ok()?
    };
    unsafe { pattern.CurrentValue() }
        .ok()
        .map(|value| value.to_string())
}

/// Address bars hide the scheme ("github.com/openrize"), and an empty or
/// search-text bar is no page at all. Only web pages count, as on macOS.
fn normalize_address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let url = if text.contains("://") {
        text.to_string()
    } else {
        format!("https://{text}")
    };
    let parsed = url::Url::parse(&url).ok()?;
    let web = matches!(parsed.scheme(), "http" | "https");
    let host = parsed.host_str()?;
    (web && (host.contains('.') || host == "localhost")).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_bar_text_becomes_a_web_url() {
        assert_eq!(
            normalize_address("github.com/openrize").as_deref(),
            Some("https://github.com/openrize")
        );
        assert_eq!(
            normalize_address("http://localhost:1420/").as_deref(),
            Some("http://localhost:1420/")
        );
        assert_eq!(normalize_address(""), None);
        assert_eq!(normalize_address("how to rust"), None);
        assert_eq!(normalize_address("edge://settings"), None);
        assert_eq!(normalize_address("newtab"), None);
    }

    #[test]
    fn file_names_come_from_windows_paths() {
        assert_eq!(
            file_name(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            "chrome.exe"
        );
        assert_eq!(file_name("chrome.exe"), "chrome.exe");
    }
}
