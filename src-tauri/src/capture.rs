use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFRelease, CFTypeRef};
use std::ffi::c_void;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSample {
    pub app: String,
    pub title: String,
    pub bundle_id: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
    /// The app holds a power assertion keeping the display awake, which is
    /// how a browser playing video or a call app tells macOS someone is
    /// watching. Watching gives no keyboard or mouse input, so this is the
    /// only sign the user is still there.
    pub keeps_display_awake: bool,
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct __AXUIElement(c_void);
#[cfg(target_os = "macos")]
type AXUIElementRef = *mut __AXUIElement;

#[cfg(target_os = "macos")]
#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPMCopyAssertionsByProcess(
        assertions_by_pid: *mut core_foundation_sys::dictionary::CFDictionaryRef,
    ) -> i32;
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: libc::pid_t) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: core_foundation_sys::string::CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
}

#[cfg(target_os = "macos")]
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

    // Switch window title capture from active-win-pos-rs to macOS Accessibility API
    // (AXUIElement / AXFocusedWindow -> AXTitle) so Screen Recording permission is NOT required.
    let title = get_window_title_ax(pid).unwrap_or_default();

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

#[cfg(not(target_os = "macos"))]
pub fn read_active_window() -> Option<WindowSample> {
    None
}

#[cfg(target_os = "macos")]
fn get_window_title_ax(pid: libc::pid_t) -> Option<String> {
    unsafe {
        let app_element = AXUIElementCreateApplication(pid);
        if app_element.is_null() {
            return None;
        }

        let attr_focused_window = CFString::new("AXFocusedWindow");
        let mut window_ref: CFTypeRef = std::ptr::null_mut();
        let err = AXUIElementCopyAttributeValue(
            app_element,
            attr_focused_window.as_concrete_TypeRef(),
            &mut window_ref,
        );

        CFRelease(app_element as CFTypeRef);

        if err != 0 || window_ref.is_null() {
            return None;
        }

        let attr_title = CFString::new("AXTitle");
        let mut title_ref: CFTypeRef = std::ptr::null_mut();
        let err = AXUIElementCopyAttributeValue(
            window_ref as AXUIElementRef,
            attr_title.as_concrete_TypeRef(),
            &mut title_ref,
        );

        CFRelease(window_ref);

        if err != 0 || title_ref.is_null() {
            return None;
        }

        let title_cf =
            CFString::wrap_under_create_rule(title_ref as core_foundation_sys::string::CFStringRef);
        let title = title_cf.to_string();
        Some(title)
    }
}

/// Whether `pid` holds an active display-sleep assertion, directly or through
/// a system service acting on its behalf. Only the assertion's type is read:
/// its name (for example "video-playing") is never stored.
#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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
            if !raw_url.is_empty()
                && (raw_url.starts_with("http://") || raw_url.starts_with("https://"))
            {
                let domain = url::Url::parse(&raw_url).ok().and_then(|u| {
                    u.host_str()
                        .map(|h| h.trim_start_matches("www.").to_string())
                });
                return (Some(raw_url), domain);
            }
        }
    }

    (None, None)
}

/// App Nap assertion: prevents OS from throttling background sampler while capture is active.
#[cfg(target_os = "macos")]
pub struct AppNapAssertion {
    activity: Option<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
    >,
}

#[cfg(target_os = "macos")]
impl AppNapAssertion {
    pub fn begin(reason: &str) -> Self {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        const NS_ACTIVITY_USER_INITIATED_ALLOWING_IDLE_SYSTEM_SLEEP: u64 = 0x00FFFFFF & !0x00000001;

        let process_info = NSProcessInfo::processInfo();
        let reason_str = NSString::from_str(reason);
        let activity = process_info.beginActivityWithOptions_reason(
            NSActivityOptions(NS_ACTIVITY_USER_INITIATED_ALLOWING_IDLE_SYSTEM_SLEEP),
            &reason_str,
        );
        Self {
            activity: Some(activity),
        }
    }
}

#[cfg(target_os = "macos")]
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

#[cfg(not(target_os = "macos"))]
pub struct AppNapAssertion;

#[cfg(not(target_os = "macos"))]
impl AppNapAssertion {
    pub fn begin(_reason: &str) -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
pub fn register_sleep_listeners(app: tauri::AppHandle) {
    use block2::RcBlock;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceWillSleepNotification};
    use objc2_foundation::NSNotification;
    use tauri::Manager;

    let workspace = NSWorkspace::sharedWorkspace();
    let center = workspace.notificationCenter();

    let app_handle = app.clone();
    let block = RcBlock::new(move |_notif: std::ptr::NonNull<NSNotification>| {
        let now = crate::timers::now_epoch_ms();
        let state = app_handle.state::<crate::AppState>();
        if let Ok(mut store) = state.activity.lock() {
            let _ = store.close_active_segment(now);
        };
    });

    unsafe {
        let _ = center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceWillSleepNotification),
            None,
            None,
            &block,
        );
    }
}

#[cfg(not(target_os = "macos"))]
pub fn register_sleep_listeners(_app: tauri::AppHandle) {}

#[cfg(all(test, target_os = "macos"))]
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
