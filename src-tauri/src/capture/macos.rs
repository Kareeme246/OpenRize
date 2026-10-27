//! macOS capture: the frontmost app through NSWorkspace, its window title
//! through the Accessibility API (so Screen Recording permission is not
//! needed), and a browser's URL through AppleScript.

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;
use core_foundation_sys::base::CFTypeRef;
use std::sync::{Mutex, OnceLock};

use super::title::{self, Attempt, FailureLog, Resolution, TitleSource};
use super::{domain_of, WindowSample};
use crate::ax::{
    AXUIElementCopyAttributeValue, AXUIElementCreateApplication, AXUIElementRef,
    AXUIElementSetMessagingTimeout,
};

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPMCopyAssertionsByProcess(
        assertions_by_pid: *mut core_foundation_sys::dictionary::CFDictionaryRef,
    ) -> i32;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGSessionCopyCurrentDictionary() -> core_foundation_sys::dictionary::CFDictionaryRef;
    fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
}

/// Nobody can be at the Mac: the screen is locked, the session is switched
/// out, or every display is asleep. macOS briefly wakes a sleeping Mac for
/// background work (Power Nap, iCloud) with the display off, and the idle
/// clock alone can read as fresh input then.
pub fn user_is_away() -> bool {
    screen_is_locked() || all_displays_asleep()
}

/// Milliseconds since the last keyboard or mouse input.
pub fn idle_ms() -> u64 {
    user_idle3::UserIdle::get_time()
        .map(|idle| idle.duration().as_millis() as u64)
        .unwrap_or(0)
}

fn screen_is_locked() -> bool {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;

    let raw = unsafe { CGSessionCopyCurrentDictionary() };
    if raw.is_null() {
        // No window-server session for this process: nobody is at the screen.
        return true;
    }
    let session: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(raw) };
    let flag = |key: &str| {
        session
            .find(CFString::new(key))
            .and_then(|value| value.downcast::<CFBoolean>())
            .map(bool::from)
    };
    flag("CGSSessionScreenIsLocked").unwrap_or(false)
        || !flag("kCGSSessionOnConsoleKey").unwrap_or(true)
}

fn all_displays_asleep() -> bool {
    let mut displays = [0u32; 16];
    let mut count = 0u32;
    let status =
        unsafe { CGGetOnlineDisplayList(displays.len() as u32, displays.as_mut_ptr(), &mut count) };
    if status != 0 || count == 0 {
        return false;
    }
    displays[..count as usize]
        .iter()
        .all(|&display| unsafe { CGDisplayIsAsleep(display) } != 0)
}

pub fn read_active_window() -> Option<WindowSample> {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let front_app = workspace.frontmostApplication()?;

    let pid = front_app.processIdentifier();
    let app_name = front_app
        .localizedName()
        .map(|s| s.to_string())
        .unwrap_or_default();

    if app_name.is_empty() {
        return None;
    }

    let bundle_id = front_app.bundleIdentifier().map(|s| s.to_string());

    // Titles come from the Accessibility API (see `title` for the order tried),
    // so Screen Recording permission is not needed.
    let title = window_title(pid, bundle_id.as_deref());

    let (url, domain) = if let Some(ref bid) = bundle_id {
        get_browser_url_and_domain(bid, &app_name)
    } else {
        (None, None)
    };

    Some(WindowSample {
        app: app_name,
        title,
        bundle_id,
        url,
        domain,
        keeps_display_awake: keeps_display_awake(pid),
    })
}

/// `kAXErrorNoValue`: the attribute exists but nothing is set.
const AX_ERROR_NO_VALUE: i32 = -25212;

/// How long one Accessibility call may wait on a stuck app before giving up,
/// instead of the system default holding up the sampler thread.
const AX_MESSAGING_TIMEOUT_SECS: f32 = 1.0;

/// An app with many windows is not walked past this many.
const MAX_WINDOWS_READ: usize = 8;

/// One failure line per kind per this long; the sampler runs every second.
const FAILURE_LOG_INTERVAL_MS: u64 = 5 * 60 * 1000;

fn failure_log() -> &'static Mutex<FailureLog> {
    static LOG: OnceLock<Mutex<FailureLog>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(FailureLog::new(FAILURE_LOG_INTERVAL_MS)))
}

/// The title of `pid`'s frontmost window, or an empty string. Why a title was
/// not found is logged, since the sampler would otherwise record nothing and
/// say nothing.
fn window_title(pid: libc::pid_t, bundle_id: Option<&str>) -> String {
    let resolution = resolve_title(pid);
    if let Some(key) = title::failure_to_log(bundle_id, &resolution) {
        let now = openrize_core::timers::now_epoch_ms();
        let admitted = failure_log()
            .lock()
            .ok()
            .and_then(|mut log| log.admit(&key, now));
        if let Some(held_back) = admitted {
            eprintln!(
                "capture: no window title for {key}{}",
                if held_back == 0 {
                    String::new()
                } else {
                    format!(" (and {held_back} more since the last note)")
                }
            );
        }
    }
    resolution.title.unwrap_or_default()
}

fn resolve_title(pid: libc::pid_t) -> Resolution {
    // SAFETY: the element is created here and released when `app` drops.
    let app = unsafe {
        let element = AXUIElementCreateApplication(pid);
        if element.is_null() {
            return Resolution {
                title: None,
                attempts: vec![(
                    TitleSource::FocusedWindow,
                    Attempt::Failed(AX_ERROR_NO_VALUE),
                )],
            };
        }
        AXUIElementSetMessagingTimeout(element, AX_MESSAGING_TIMEOUT_SECS);
        CFType::wrap_under_create_rule(element as CFTypeRef)
    };
    title::resolve(|source| read_title_from(&app, source))
}

fn read_title_from(app: &CFType, source: TitleSource) -> Attempt {
    match source {
        TitleSource::FocusedWindow => window_attempt(copy_attribute(app, "AXFocusedWindow")),
        TitleSource::MainWindow => window_attempt(copy_attribute(app, "AXMainWindow")),
        TitleSource::FocusedElementWindow => window_attempt(
            copy_attribute(app, "AXFocusedUIElement")
                .and_then(|element| copy_attribute(&element, "AXWindow")),
        ),
        TitleSource::WindowList => match copy_attribute(app, "AXWindows") {
            Err(code) => Attempt::Failed(code),
            Ok(list) if list.type_of() != CFArray::<CFType>::type_id() => Attempt::Empty,
            // SAFETY: checked to be a CFArray just above, and `list` keeps it alive.
            Ok(list) => {
                let windows: CFArray<CFType> =
                    unsafe { CFArray::wrap_under_get_rule(list.as_CFTypeRef() as _) };
                title::pick_from_windows(
                    windows
                        .iter()
                        .take(MAX_WINDOWS_READ)
                        .map(|window| title_of(&window)),
                )
            }
        },
    }
}

fn window_attempt(window: Result<CFType, i32>) -> Attempt {
    match window {
        Ok(window) => title_of(&window),
        Err(code) => Attempt::Failed(code),
    }
}

fn title_of(window: &CFType) -> Attempt {
    match copy_attribute(window, "AXTitle") {
        Err(AX_ERROR_NO_VALUE) => Attempt::Empty,
        Err(code) => Attempt::Failed(code),
        Ok(value) => match value.downcast::<CFString>() {
            Some(title) => Attempt::Title(title.to_string()),
            None => Attempt::Empty,
        },
    }
}

fn copy_attribute(element: &CFType, name: &'static str) -> Result<CFType, i32> {
    let name = CFString::from_static_string(name);
    let mut value: CFTypeRef = std::ptr::null_mut();
    // SAFETY: `element` is an AXUIElement kept alive by the caller; on success
    // the call hands back a +1 value that the wrapper releases.
    let code = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_CFTypeRef() as AXUIElementRef,
            name.as_concrete_TypeRef(),
            &mut value,
        )
    };
    if code != 0 {
        return Err(code);
    }
    if value.is_null() {
        return Err(AX_ERROR_NO_VALUE);
    }
    Ok(unsafe { CFType::wrap_under_create_rule(value) })
}

/// Whether `pid` holds an active display-sleep assertion, directly or through
/// a system service acting on its behalf. Only the assertion's type is read:
/// its name (for example "video-playing") is never stored.
fn keeps_display_awake(pid: libc::pid_t) -> bool {
    use core_foundation::array::CFArray;
    use core_foundation::base::CFType;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;

    // IOPMLib.h: kIOPMAssertionTypePreventUserIdleDisplaySleep and its
    // deprecated alias kIOPMAssertionTypeNoDisplaySleep, which Firefox-based
    // browsers such as Zen still use for video.
    const DISPLAY_TYPES: [&str; 2] = ["PreventUserIdleDisplaySleep", "NoDisplaySleepAssertion"];
    const LEVEL_OFF: i64 = 0;

    let mut by_pid_ref: core_foundation_sys::dictionary::CFDictionaryRef = std::ptr::null();
    // SAFETY: on success the call hands us a +1 dictionary, released by the
    // wrapper below.
    if unsafe { IOPMCopyAssertionsByProcess(&mut by_pid_ref) } != 0 || by_pid_ref.is_null() {
        return false;
    }
    let by_pid: CFDictionary<CFNumber, CFArray<CFDictionary<CFString, CFType>>> =
        unsafe { CFDictionary::wrap_under_create_rule(by_pid_ref) };

    let key = |name: &'static str| CFString::from_static_string(name);
    let number = |assertion: &CFDictionary<CFString, CFType>, name: &'static str| {
        assertion
            .find(key(name))
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|value| value.to_i64())
    };

    let (owners, lists) = by_pid.get_keys_and_values();
    owners.into_iter().zip(lists).any(|(owner, list)| {
        // SAFETY: the dictionary maps CFNumber pids to CFArrays of assertion
        // dictionaries, and both outlive this borrow.
        let owner = unsafe { CFNumber::wrap_under_get_rule(owner as _) }.to_i64();
        let list: CFArray<CFDictionary<CFString, CFType>> =
            unsafe { CFArray::wrap_under_get_rule(list as _) };
        list.iter().any(|assertion| {
            let is_display = assertion
                .find(key("AssertType"))
                .and_then(|value| value.downcast::<CFString>())
                .is_some_and(|kind| DISPLAY_TYPES.contains(&kind.to_string().as_str()));
            let on = number(&assertion, "AssertLevel") != Some(LEVEL_OFF);
            let holder = number(&assertion, "AssertionOnBehalfOfPID").or(owner);
            is_display && on && holder == Some(i64::from(pid))
        })
    })
}

fn get_browser_url_and_domain(bundle_id: &str, app_name: &str) -> (Option<String>, Option<String>) {
    let script = match bundle_id {
        "com.apple.Safari" => {
            r#"tell application "Safari" to if (count of documents) > 0 then return URL of front document"#
        }
        "com.google.Chrome" => {
            r#"tell application "Google Chrome" to if (count of windows) > 0 then return URL of active tab of front window"#
        }
        "com.brave.Browser" => {
            r#"tell application "Brave Browser" to if (count of windows) > 0 then return URL of active tab of front window"#
        }
        "com.microsoft.edgemac" => {
            r#"tell application "Microsoft Edge" to if (count of windows) > 0 then return URL of active tab of front window"#
        }
        "company.thebrowser.Browser" => {
            r#"tell application "Arc" to if (count of windows) > 0 then return URL of active tab of front window"#
        }
        _ => {
            if app_name == "Safari" {
                r#"tell application "Safari" to if (count of documents) > 0 then return URL of front document"#
            } else if app_name == "Google Chrome" {
                r#"tell application "Google Chrome" to if (count of windows) > 0 then return URL of active tab of front window"#
            } else {
                return (None, None);
            }
        }
    };

    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok();

    if let Some(out) = output {
        if out.status.success() {
            let raw_url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if raw_url.starts_with("http://") || raw_url.starts_with("https://") {
                let domain = domain_of(&raw_url);
                return (Some(raw_url), domain);
            }
        }
    }

    (None, None)
}

/// App Nap assertion: prevents OS from throttling background sampler while capture is active.
pub struct AppNapAssertion {
    activity: Option<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
    >,
}

impl AppNapAssertion {
    pub fn begin(reason: &str) -> Self {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

        let process_info = NSProcessInfo::processInfo();
        let reason_str = NSString::from_str(reason);
        // Keeps the sampler off App Nap without holding a
        // PreventUserIdleSystemSleep assertion, so the Mac still idle-sleeps.
        let activity = process_info.beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
            &reason_str,
        );
        Self {
            activity: Some(activity),
        }
    }
}

impl Drop for AppNapAssertion {
    fn drop(&mut self) {
        if let Some(activity) = self.activity.take() {
            let process_info = objc2_foundation::NSProcessInfo::processInfo();
            unsafe {
                process_info.endActivity(&activity);
            }
        }
    }
}

pub fn register_sleep_listeners(app: tauri::AppHandle) {
    use super::PowerEvent;
    use block2::RcBlock;
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::NSNotification;

    let workspace = NSWorkspace::sharedWorkspace();
    let center = workspace.notificationCenter();

    // These are system transitions, not NSWorkspaceScreensDidSleep/Wake:
    // display sleep and dark wakes must not be inferred from user_is_away.
    for (name, event) in unsafe {
        [
            (NSWorkspaceWillSleepNotification, PowerEvent::Suspend),
            (NSWorkspaceDidWakeNotification, PowerEvent::Resume),
        ]
    } {
        let app = app.clone();
        let block = RcBlock::new(move |_notif: std::ptr::NonNull<NSNotification>| {
            super::power_event(&app, event);
        });
        // The notification center retains the observer and block for the
        // process lifetime. A nil queue delivers synchronously before sleep.
        unsafe {
            let _ =
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::keeps_display_awake;
    use std::time::{Duration, Instant};

    #[test]
    fn a_display_assertion_is_attributed_to_the_process_holding_it() {
        // `caffeinate -d` holds a PreventUserIdleDisplaySleep assertion, the
        // same kind a browser takes while it plays video.
        let mut caffeinate = std::process::Command::new("caffeinate")
            .args(["-d", "-t", "30"])
            .spawn()
            .expect("caffeinate ships with macOS");
        let pid = caffeinate.id() as libc::pid_t;

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = keeps_display_awake(pid);
        while !seen && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            seen = keeps_display_awake(pid);
        }
        let this_process = keeps_display_awake(std::process::id() as libc::pid_t);
        caffeinate.kill().ok();
        caffeinate.wait().ok();

        assert!(seen, "caffeinate's display assertion was not found");
        assert!(
            !this_process,
            "a process with no assertion counted as watching"
        );
    }
}
