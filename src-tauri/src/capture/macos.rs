//! macOS capture: frontmost app via NSWorkspace, window title via the Accessibility API (no
//! Screen Recording permission needed), browser URL and window mode via AppleScript.

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;
use core_foundation_sys::base::CFTypeRef;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use openrize_core::capture::BrowserReply;

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

/// Nobody can be at the Mac: locked, session switched out, or every display asleep. macOS wakes a
/// sleeping Mac for background work (Power Nap, iCloud) with the display off, and the idle clock
/// alone reads as fresh input then.
pub fn user_is_away() -> bool {
    screen_is_locked() || all_displays_asleep()
}

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

    let title = window_title(pid, bundle_id.as_deref());

    let reply = bundle_id.as_deref().and_then(ask_browser);
    // A Chromium window says so when asked; Firefox-family and Safari windows say so in their
    // title. Whatever cannot be told is private (`window_is_private`).
    let private = bundle_id.as_deref().is_some_and(|bid| {
        openrize_core::capture::window_is_private(bid, &title, reply.as_ref().map(|r| r.mode))
    });
    let url = reply.and_then(|reply| reply.url);
    let domain = url.as_deref().and_then(domain_of);

    Some(WindowSample {
        app: app_name,
        title,
        bundle_id,
        url,
        domain,
        private,
        keeps_display_awake: keeps_display_awake(pid),
    })
}

const AX_ERROR_NO_VALUE: i32 = -25212;

/// How long one Accessibility call may wait on a stuck app, instead of the system default stalling
/// the sampler.
const AX_MESSAGING_TIMEOUT_SECS: f32 = 1.0;

const MAX_WINDOWS_READ: usize = 8;

/// One failure line per kind per this long; the sampler runs every second.
const FAILURE_LOG_INTERVAL_MS: u64 = 5 * 60 * 1000;

fn failure_log() -> &'static Mutex<FailureLog> {
    static LOG: OnceLock<Mutex<FailureLog>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(FailureLog::new(FAILURE_LOG_INTERVAL_MS)))
}

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

/// Directly or through a system service acting for `pid`. Only the assertion's type is read;
/// its name is never stored.
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

/// How long a browser gets to answer before `osascript` is killed, so a hung browser or an
/// unanswered Automation prompt cannot stall the sampler (the default Apple Event timeout is two
/// minutes). The script's own `with timeout` normally ends it first.
const SCRIPT_DEADLINE: Duration = Duration::from_millis(1500);

/// The script asking a Chromium browser for its front window's URL and mode, one per line. An
/// error, such as a browser without `mode`, fails the script, which reads as unknown.
fn chromium_script(bundle_id: &str) -> String {
    format!(
        r#"with timeout of 1 second
    tell application id "{bundle_id}"
        if (count of windows) = 0 then return linefeed & "normal"
        return (URL of active tab of front window) & linefeed & ((mode of front window) as text)
    end tell
end timeout"#
    )
}

/// Safari's dictionary has no window mode, so its "normal" only says that it answered: the
/// window's title is what tells a private one (`capture::title_marks_private`).
const SAFARI_SCRIPT: &str = r#"with timeout of 1 second
    tell application id "com.apple.Safari"
        set pageURL to ""
        if (count of documents) > 0 then
            set pageURL to URL of front document
            if pageURL is missing value then set pageURL to ""
        end if
        return pageURL & linefeed & "normal"
    end tell
end timeout"#;

/// The browsers that are asked, by exact bundle id: the id is what the script addresses, so an
/// app merely named like a browser is never scripted and none is launched by name. `None` for
/// every other app; a browser that does not answer in time gives the unknown reply.
fn ask_browser(bundle_id: &str) -> Option<BrowserReply> {
    let script = match bundle_id {
        "com.apple.Safari" => SAFARI_SCRIPT.to_string(),
        "com.google.Chrome"
        | "com.brave.Browser"
        | "com.microsoft.edgemac"
        | "company.thebrowser.Browser" => chromium_script(bundle_id),
        _ => return None,
    };
    Some(
        run_script(&script, SCRIPT_DEADLINE)
            .map(|stdout| openrize_core::capture::parse_browser_reply(&stdout))
            .unwrap_or_default(),
    )
}

/// Runs an AppleScript and returns what it printed, or `None` if it failed or outlived `deadline`
/// (it is then killed). `osascript` is run by absolute path with an empty environment, so neither
/// `PATH` nor an `OSA*`/`DYLD_*` variable chooses what runs with the app's Automation grants.
fn run_script(script: &str, deadline: Duration) -> Option<String> {
    let mut child = Command::new("/usr/bin/osascript")
        .env_clear()
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    // The reply is two short lines, far below the pipe's buffer, so the child never waited on
    // this read to exit.
    let mut stdout = String::new();
    child.stdout.take()?.read_to_string(&mut stdout).ok()?;
    status.success().then_some(stdout)
}

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
    use super::{ask_browser, chromium_script, keeps_display_awake, run_script, SAFARI_SCRIPT};
    use openrize_core::capture::{parse_browser_reply, window_is_private, Mode};
    use std::time::{Duration, Instant};

    #[test]
    fn a_script_that_answers_is_read_and_one_that_hangs_is_killed() {
        let reply = run_script(
            r#"return "https://example.com/a?token=1#x" & linefeed & "normal""#,
            Duration::from_secs(10),
        )
        .expect("osascript ships with macOS");
        let reply = parse_browser_reply(&reply);
        assert_eq!(reply.url.as_deref(), Some("https://example.com/a"));
        assert_eq!(reply.mode, Mode::Normal);

        let started = Instant::now();
        assert_eq!(run_script("delay 30", Duration::from_millis(300)), None);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the child was not killed"
        );
        // A script that errors, as a refused Automation request does, is no answer either.
        assert_eq!(
            run_script(r#"error "refused" number -1743"#, Duration::from_secs(10)),
            None
        );
    }

    #[test]
    fn only_known_bundle_ids_are_asked_and_the_scripts_address_them_by_id() {
        // None of these runs a script: an app that only shares a browser's name, a browser
        // that is not scripted, and an id crafted to break out of the script's string.
        for id in [
            "com.example.Safari",
            "com.vivaldi.Vivaldi",
            "org.mozilla.firefox",
            "com.google.Chrome\" to quit\ntell application \"Finder",
        ] {
            assert!(ask_browser(id).is_none(), "{id}");
        }
        // Not asked, so a Chromium browser outside the list is private.
        assert!(window_is_private(
            "com.vivaldi.Vivaldi",
            "Inbox - Vivaldi",
            None
        ));
        // A Chromium browser that is asked and does not answer is private; Safari's title decides.
        assert!(!window_is_private(
            "com.apple.Safari",
            "Page",
            Some(Mode::Unknown)
        ));
        for id in [
            "com.google.Chrome",
            "com.brave.Browser",
            "com.microsoft.edgemac",
            "company.thebrowser.Browser",
        ] {
            assert!(window_is_private(id, "Page", Some(Mode::Unknown)), "{id}");
        }

        assert!(SAFARI_SCRIPT.contains(r#"tell application id "com.apple.Safari""#));
        let chrome = chromium_script("com.google.Chrome");
        assert!(chrome.contains(r#"tell application id "com.google.Chrome""#));
        for script in [SAFARI_SCRIPT, chrome.as_str()] {
            assert!(script.starts_with("with timeout of 1 second"));
        }
    }

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
