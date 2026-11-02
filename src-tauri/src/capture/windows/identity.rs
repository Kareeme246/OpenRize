//! Who an app is on Windows. macOS gives every app, each installed web app
//! (PWA) included, its own bundle id; here the lowercased executable name is
//! the identity (`slack.exe`, `winword.exe`), which merges unrelated apps
//! wherever one executable hosts many:
//!
//! - a browser hosting installed web apps: a Chrome or Edge PWA runs inside
//!   `chrome.exe` / `msedge.exe`;
//! - a generic host: `javaw.exe`, `python.exe`, `node.exe`, `dotnet.exe`,
//!   `mmc.exe` (every `.msc` is a different tool), `rundll32.exe`,
//!   `wwahost.exe` (packaged JavaScript apps), `electron.exe`, ...
//!
//! Only those get a qualifier, written `<executable>/<qualifier>`
//! (`msedge.exe/msedge._crx_abc`, `mmc.exe/compmgmt`). Every other app keeps
//! the bare executable name, so history, rules and exclusions keyed on it
//! keep matching. The qualifier is, in order of trust:
//!
//! 1. the window's explicit AppUserModelID (what the taskbar groups by, and
//!    what Chromium sets per window for a PWA), read from the window's property
//!    store; for a browser only an app window's counts, since a plain browser
//!    window carries the browser's or its profile's id;
//! 2. for a packaged (MSIX/UWP) process, its package application id;
//! 3. for a host that runs a script, jar, snap-in or dll, that file's name
//!    without folders, extension or version, taken from the process command
//!    line. The line itself is never stored: it can carry tokens.
//!
//! The app is named by the window's relaunch name, else the shell's Apps
//! folder entry for the id, else the file the host runs, else the host.
//!
//! The sampler asks once a second, so answers are cached per process and
//! window: ordinary apps cost nothing, hosts and browsers one lookup per cache
//! lifetime.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HWND, PROPERTYKEY, UNICODE_STRING};
use windows::Win32::Storage::EnhancedStorage::{
    PKEY_AppUserModel_ID, PKEY_AppUserModel_RelaunchCommand,
    PKEY_AppUserModel_RelaunchDisplayNameResource,
};
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, IBindCtx, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::Shell::{
    IShellItem, SHCreateItemFromParsingName, SHLoadIndirectString, SIGDN_NORMALDISPLAY,
};

use super::{wide, BROWSERS};

/// An app as the sampler records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Identity {
    /// The stable identifier: the executable name, qualified where that
    /// alone would merge different apps.
    pub id: String,
    /// What the user would call the app.
    pub name: String,
}

/// Executables that run other programs and so say nothing about which one.
/// Lowercase.
const HOSTS: [&str; 12] = [
    "dotnet.exe",
    "electron.exe",
    "java.exe",
    "javaw.exe",
    "mmc.exe",
    "mshta.exe",
    "msedgewebview2.exe",
    "node.exe",
    "python.exe",
    "pythonw.exe",
    "rundll32.exe",
    "wwahost.exe",
];

/// How long an answer that found nothing to tell the app apart is kept: a
/// window sets its AppUserModelID shortly after it is created, so it is asked
/// again soon.
const UNQUALIFIED_TTL: Duration = Duration::from_secs(3);
/// How long a qualified answer is kept.
const QUALIFIED_TTL: Duration = Duration::from_secs(60);
/// Most answers kept; past this the cache starts over.
const CACHE_LIMIT: usize = 128;

type Key = (u32, isize);

thread_local! {
    static IDENTITIES: RefCell<HashMap<Key, (Instant, Identity)>> = RefCell::new(HashMap::new());
    /// Names the Apps folder gave an application id (or none).
    static SHELL_NAMES: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
    static COM_READY: Cell<bool> = const { Cell::new(false) };
}

/// Names the app behind `window`, whose process `pid` runs `executable`
/// (lowercase) and is called `app` by its version resource.
pub(super) fn identify(window: HWND, pid: u32, executable: &str, app: &str) -> Identity {
    let browser = BROWSERS.contains(&executable);
    if !browser && !is_host(executable) {
        return Identity {
            id: executable.to_string(),
            name: app.to_string(),
        };
    }
    let key = (pid, window.0 as isize);
    let cached = IDENTITIES.with(|cache| {
        let cache = cache.borrow();
        let (taken, identity) = cache.get(&key)?;
        let lifetime = if identity.id == executable {
            UNQUALIFIED_TTL
        } else {
            QUALIFIED_TTL
        };
        (taken.elapsed() < lifetime).then(|| identity.clone())
    });
    if let Some(identity) = cached {
        return identity;
    }
    let identity = resolve(window, pid, executable, app, browser);
    IDENTITIES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, (Instant::now(), identity.clone()));
    });
    identity
}

/// Whether the window is a web app's window inside a browser, not a window of
/// the browser itself: such a window has no address bar to read.
pub(super) fn is_web_app(executable: &str, identity: &Identity) -> bool {
    BROWSERS.contains(&executable) && identity.id != executable
}

fn is_host(executable: &str) -> bool {
    HOSTS.contains(&executable)
}

fn resolve(window: HWND, pid: u32, executable: &str, app: &str, browser: bool) -> Identity {
    let bare = Identity {
        id: executable.to_string(),
        name: app.to_string(),
    };
    ensure_com();
    let store = window_properties(window);
    let window_id = store
        .as_ref()
        .and_then(|store| string_property(store, &PKEY_AppUserModel_ID));

    if browser {
        let command = store
            .as_ref()
            .and_then(|store| string_property(store, &PKEY_AppUserModel_RelaunchCommand));
        let Some(web_app) = web_app_id(window_id.as_deref(), command.as_deref()) else {
            return bare;
        };
        let name = relaunch_name(store.as_ref())
            .or_else(|| window_id.as_deref().and_then(shell_name))
            .unwrap_or_else(|| format!("{app} web app {}", &web_app[..web_app.len().min(6)]));
        return Identity {
            id: qualified(executable, &web_app),
            name,
        };
    }

    let window_id = window_id.filter(|id| !is_generic_app_id(executable, id));
    if let Some(app_id) = window_id.or_else(|| package_app_id(pid)) {
        let name = relaunch_name(store.as_ref())
            .or_else(|| shell_name(&app_id))
            .unwrap_or_else(|| app.to_string());
        return Identity {
            id: qualified(executable, &app_id),
            name,
        };
    }
    match command_line(pid).and_then(|line| script_name(executable, &split_command_line(&line))) {
        Some(file) => Identity {
            id: qualified(executable, &file),
            name: shown_name(executable, &file, app),
        },
        None => bare,
    }
}

// ---- Pure rules --------------------------------------------------------

/// The id of the installed web app a Chromium window shows, if it is one.
/// Chromium gives an app window an AppUserModelID built on the app's internal
/// name, `_crx_<app id>`, and a relaunch command that carries
/// `--app-id=<app id>`; the browser's own windows have neither (their id is the
/// install's or profile's, and their relaunch command is the bare browser).
/// Either names the app, the same way for every profile and signal, so the id
/// does not flip with which one was readable.
fn web_app_id(app_user_model_id: Option<&str>, relaunch_command: Option<&str>) -> Option<String> {
    app_user_model_id
        .and_then(|id| id_after(id, "_crx_"))
        .or_else(|| relaunch_command.and_then(|command| id_after(command, "--app-id=")))
}

/// Whether an AppUserModelID is a framework's or host's default, shared by
/// every app built on it, and so says nothing about which one this is:
/// Electron's `electron.app.Electron`, or the host's own name (`Python`).
fn is_generic_app_id(executable: &str, app_id: &str) -> bool {
    let id = app_id.trim().to_ascii_lowercase();
    let stem = executable.strip_suffix(".exe").unwrap_or(executable);
    id == "electron.app.electron" || id == stem || id == executable
}

/// The run of letters and digits right after `marker` (any case), lowercased,
/// if it is long enough to be an app id (Chromium's are 32 letters).
fn id_after(text: &str, marker: &str) -> Option<String> {
    let start = text.to_ascii_lowercase().find(marker)? + marker.len();
    let id: String = text[start..]
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    (16..=64).contains(&id.len()).then_some(id)
}

/// `<executable>/<qualifier>`, the qualifier folded to a form safe to store,
/// compare and show anywhere: lowercase, nothing but letters, digits and
/// `._-!+@~()`, no longer than `QUALIFIER_LIMIT` characters.
fn qualified(executable: &str, qualifier: &str) -> String {
    let key: String = qualifier
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if c.is_alphanumeric() || "._-!+@~()".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(QUALIFIER_LIMIT)
        .collect();
    format!("{executable}/{key}")
}

const QUALIFIER_LIMIT: usize = 96;

/// Splits a command line as the C runtime does: spaces separate, quotes group,
/// and a backslash escapes a quote only when it comes right before one.
fn split_command_line(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let mut slashes = 1;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    slashes += 1;
                }
                started = true;
                if chars.peek() == Some(&'"') {
                    current.extend(std::iter::repeat_n('\\', slashes / 2));
                    if slashes % 2 == 1 {
                        chars.next();
                        current.push('"');
                    }
                } else {
                    current.extend(std::iter::repeat_n('\\', slashes));
                }
            }
            '"' => {
                started = true;
                quoted = !quoted;
            }
            ' ' | '\t' if !quoted => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                started = true;
                current.push(c);
            }
        }
    }
    if started {
        args.push(current);
    }
    args
}

/// The one file name (or class, or module) a host's command line runs, the
/// first argument being the host itself. Only that name is returned; the rest
/// of the line, which may carry paths and secrets, is dropped here.
fn script_name(executable: &str, args: &[String]) -> Option<String> {
    let args = args.get(1..)?;
    match executable {
        "mmc.exe" => args
            .iter()
            .find(|arg| has_extension(arg, "msc"))
            .and_then(|arg| file_stem(arg)),
        "mshta.exe" => args
            .iter()
            .find(|arg| has_extension(arg, "hta"))
            .and_then(|arg| file_stem(arg)),
        "dotnet.exe" => args
            .iter()
            .find(|arg| has_extension(arg, "dll"))
            .and_then(|arg| file_stem(arg)),
        "rundll32.exe" => rundll32_name(args),
        "msedgewebview2.exe" => args
            .iter()
            .find_map(|arg| arg.strip_prefix("--webview-exe-name=").and_then(file_stem)),
        "java.exe" | "javaw.exe" => java_name(args),
        "python.exe" | "pythonw.exe" => python_name(args),
        "node.exe" | "electron.exe" => node_name(args),
        _ => None,
    }
}

fn has_extension(arg: &str, extension: &str) -> bool {
    arg.rsplit_once('.')
        .is_some_and(|(_, found)| found.eq_ignore_ascii_case(extension))
}

/// `shell32.dll,Control_RunDLL desk.cpl` is the Display applet; any other
/// call is named for its dll.
fn rundll32_name(args: &[String]) -> Option<String> {
    let mut positional = args.iter().filter(|arg| !arg.starts_with('/'));
    let call = positional.next()?;
    let dll = call.split(',').next()?;
    if let Some(applet) = positional
        .map(|arg| arg.split(',').next().unwrap_or(arg))
        .find(|arg| has_extension(arg, "cpl"))
    {
        return file_stem(applet);
    }
    file_stem(dll)
}

fn java_name(args: &[String]) -> Option<String> {
    const WITH_VALUE: [&str; 13] = [
        "-cp",
        "-classpath",
        "--class-path",
        "-p",
        "--module-path",
        "--upgrade-module-path",
        "--add-modules",
        "--limit-modules",
        "--add-exports",
        "--add-opens",
        "--add-reads",
        "--patch-module",
        "--enable-native-access",
    ];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-jar" => return args.next().and_then(|jar| file_stem(jar)),
            "-m" | "--module" => {
                return args
                    .next()
                    .and_then(|module| module.split('/').next())
                    .and_then(clean_name);
            }
            option if WITH_VALUE.contains(&option) => {
                args.next();
            }
            option if option.starts_with('-') || option.starts_with('@') => {}
            // The main class, e.g. `com.example.Main`.
            class => return clean_name(class),
        }
    }
    None
}

fn python_name(args: &[String]) -> Option<String> {
    const WITH_VALUE: [&str; 4] = ["-W", "-X", "-Q", "--check-hash-based-pycs"];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-m" => return args.next().and_then(|module| clean_name(module)),
            "-c" | "-" => return None,
            option if WITH_VALUE.contains(&option) => {
                args.next();
            }
            option if option.starts_with('-') => {}
            script => return file_stem(script),
        }
    }
    None
}

fn node_name(args: &[String]) -> Option<String> {
    const WITH_VALUE: [&str; 7] = [
        "-r",
        "--require",
        "--import",
        "--loader",
        "--experimental-loader",
        "-C",
        "--conditions",
    ];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-e" | "--eval" | "-p" | "--print" | "-" => return None,
            option if WITH_VALUE.contains(&option) => {
                args.next();
            }
            option if option.starts_with('-') => {}
            script => return file_stem(script),
        }
    }
    None
}

/// The last folder or file name of a path argument, without extension or
/// version: `"C:\apps\Tool-1.4.2.jar"` is `Tool`.
fn file_stem(arg: &str) -> Option<String> {
    let name = arg.trim().trim_matches('"').trim_end_matches(['\\', '/']);
    let name = name.rsplit(['\\', '/']).next()?;
    let name = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && extension.len() <= 5 => stem,
        _ => name,
    };
    clean_name(name)
}

/// A name for the app, without a trailing version (`tool-1.4.2` is `tool`)
/// and `None` if nothing but punctuation is left.
fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    let mut end = name.len();
    for (index, separator) in name.char_indices() {
        if index > 0 && matches!(separator, '-' | '_' | ' ') && is_version(&name[index + 1..]) {
            end = index;
            break;
        }
    }
    let name = name[..end].trim();
    name.chars()
        .any(char::is_alphanumeric)
        .then(|| name.to_string())
}

/// Whether the text starts like a version: `1.2`, `v2.0.1-beta`.
fn is_version(text: &str) -> bool {
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    let number = text.split(['-', '_', ' ']).next().unwrap_or("");
    number.starts_with(|c: char| c.is_ascii_digit())
        && number.contains('.')
        && number.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// What the user would call a host running `file`.
fn shown_name(executable: &str, file: &str, host: &str) -> String {
    if executable == "mmc.exe" {
        if let Some(known) = snap_in_name(file) {
            return known.to_string();
        }
    }
    format!("{file} ({host})")
}

/// Console snap-ins that ship with Windows, by file name.
fn snap_in_name(file: &str) -> Option<&'static str> {
    Some(match file.to_ascii_lowercase().as_str() {
        "compmgmt" => "Computer Management",
        "devmgmt" => "Device Manager",
        "diskmgmt" => "Disk Management",
        "eventvwr" => "Event Viewer",
        "gpedit" => "Local Group Policy Editor",
        "lusrmgr" => "Local Users and Groups",
        "perfmon" => "Performance Monitor",
        "services" => "Services",
        "taskschd" => "Task Scheduler",
        "wf" => "Windows Defender Firewall with Advanced Security",
        _ => return None,
    })
}

// ---- Win32 -------------------------------------------------------------

/// The property store calls and the Apps folder need COM on the calling thread.
fn ensure_com() {
    COM_READY.with(|ready| {
        if !ready.get() {
            // SAFETY: joins this thread to the multithreaded apartment for its
            // life; a second call, or a thread already in another apartment,
            // only returns an error code, which changes nothing here.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            ready.set(true);
        }
    });
}

fn window_properties(window: HWND) -> Option<IPropertyStore> {
    // SAFETY: a plain query; a stale handle is an error.
    unsafe { SHGetPropertyStoreForWindow::<IPropertyStore>(window) }.ok()
}

fn string_property(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    // SAFETY: GetValue fills a PROPVARIANT, which clears itself when dropped.
    let value: PROPVARIANT = unsafe { store.GetValue(key) }.ok()?;
    if value.vt() != VT_LPWSTR {
        return None;
    }
    let text = value.to_string();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The window's own name for its app: set by the app for the taskbar's
/// jump list, and by Chromium for every web app window. Either text or a
/// `@file,-id` resource reference.
fn relaunch_name(store: Option<&IPropertyStore>) -> Option<String> {
    let text = string_property(store?, &PKEY_AppUserModel_RelaunchDisplayNameResource)?;
    if !text.starts_with('@') {
        return Some(text);
    }
    let source = wide(&text);
    let mut buffer = [0u16; 256];
    // SAFETY: `source` is NUL terminated and the output buffer is writable.
    unsafe { SHLoadIndirectString(PCWSTR(source.as_ptr()), &mut buffer, None) }.ok()?;
    let length = buffer.iter().position(|&unit| unit == 0)?;
    let name = String::from_utf16_lossy(&buffer[..length]);
    (!name.trim().is_empty()).then(|| name.trim().to_string())
}

/// The name the shell lists an application id under, as in the Start menu.
fn shell_name(app_id: &str) -> Option<String> {
    if let Some(known) = SHELL_NAMES.with(|names| names.borrow().get(app_id).cloned()) {
        return known;
    }
    let name = shell_name_uncached(app_id);
    SHELL_NAMES.with(|names| {
        let mut names = names.borrow_mut();
        if names.len() >= CACHE_LIMIT {
            names.clear();
        }
        names.insert(app_id.to_string(), name.clone());
    });
    name
}

fn shell_name_uncached(app_id: &str) -> Option<String> {
    let path = wide(&format!("shell:AppsFolder\\{app_id}"));
    // SAFETY: `path` is NUL terminated; the display name is a CoTaskMem string
    // that is copied and freed.
    unsafe {
        let item: IShellItem =
            SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None::<&IBindCtx>).ok()?;
        let display = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
        let name = display.to_string().ok();
        CoTaskMemFree(Some(display.0.cast()));
        name.map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
    }
}

/// The application id of a packaged (MSIX/UWP) process, as
/// `PackageFamilyName!App`. `None` for an ordinary process.
fn package_app_id(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed before returning; the buffer is as long as
    // the length passed in, which is updated to the units written.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 256];
        let mut length = buffer.len() as u32;
        let status =
            GetApplicationUserModelId(process, &mut length, Some(PWSTR(buffer.as_mut_ptr())));
        let _ = CloseHandle(process);
        if status != ERROR_SUCCESS {
            return None;
        }
        // The length counts the terminating NUL.
        let text = String::from_utf16_lossy(&buffer[..length.saturating_sub(1) as usize]);
        (!text.is_empty()).then_some(text)
    }
}

/// Longest command line read: Windows allows 32,767 characters.
const COMMAND_LINE_LIMIT: usize = 64 * 1024;

/// A process's command line. The kernel copies it out (ProcessCommandLine
/// Information, Windows 8.1 and later), so no memory of the other process is
/// read, and limited query rights are enough.
fn command_line(pid: u32) -> Option<String> {
    use windows::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessCommandLineInformation,
    };

    // SAFETY: the handle is closed before returning. The buffer is 8-byte
    // aligned, as long as the length passed, and on success holds a
    // UNICODE_STRING followed by its text, which `Buffer` points into; the
    // pointer is checked to lie inside the buffer before it is read.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u64; 1024];
        let mut line = None;
        for _ in 0..2 {
            let mut needed = 0u32;
            let status = NtQueryInformationProcess(
                process,
                ProcessCommandLineInformation,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 8) as u32,
                &mut needed,
            );
            if status.is_ok() {
                let string = &*buffer.as_ptr().cast::<UNICODE_STRING>();
                let start = buffer.as_ptr() as usize;
                let text = string.Buffer.0 as usize;
                let bytes = usize::from(string.Length);
                if text >= start && text + bytes <= start + buffer.len() * 8 && bytes % 2 == 0 {
                    let units = std::slice::from_raw_parts(string.Buffer.0, bytes / 2);
                    line = Some(String::from_utf16_lossy(units));
                }
                break;
            }
            let needed = needed as usize;
            if needed <= buffer.len() * 8 || needed > COMMAND_LINE_LIMIT {
                break;
            }
            buffer = vec![0u64; needed.div_ceil(8)];
        }
        let _ = CloseHandle(process);
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        split_command_line(line)
    }

    #[test]
    fn command_lines_split_like_the_c_runtime() {
        assert_eq!(
            args(r#""C:\Program Files\Java\bin\javaw.exe" -jar "C:\my apps\tool.jar" x"#),
            [
                r"C:\Program Files\Java\bin\javaw.exe",
                "-jar",
                r"C:\my apps\tool.jar",
                "x"
            ]
        );
        assert_eq!(
            args(r#"a "b \"c\" d" e\\ f"#),
            ["a", r#"b "c" d"#, r"e\\", "f"]
        );
        assert_eq!(args(r#"a "" b"#), ["a", "", "b"]);
        assert!(args("   ").is_empty());
    }

    fn name(executable: &str, line: &str) -> Option<String> {
        script_name(executable, &args(line))
    }

    #[test]
    fn a_snap_in_is_named_for_its_msc_file() {
        assert_eq!(
            name(
                "mmc.exe",
                r"C:\Windows\system32\mmc.exe C:\Windows\system32\compmgmt.msc /s"
            ),
            Some("compmgmt".into())
        );
        assert_eq!(name("mmc.exe", r"mmc.exe"), None);
        assert_eq!(
            shown_name("mmc.exe", "compmgmt", "x"),
            "Computer Management"
        );
        assert_eq!(
            shown_name("mmc.exe", "custom", "Microsoft Management Console"),
            "custom (Microsoft Management Console)"
        );
    }

    #[test]
    fn java_is_named_for_its_jar_module_or_main_class() {
        assert_eq!(
            name(
                "javaw.exe",
                r#"javaw.exe -Xmx2G -Dx=1 -jar "C:\g\Launcher-2.1.0.jar" --flag"#
            ),
            Some("Launcher".into())
        );
        assert_eq!(
            name(
                "java.exe",
                "java -cp lib\\a.jar;lib\\b.jar com.example.app.Main arg"
            ),
            Some("com.example.app.Main".into())
        );
        assert_eq!(
            name("java.exe", "java --module-path mods -m my.app/my.app.Main"),
            Some("my.app".into())
        );
        assert_eq!(name("java.exe", "java -version"), None);
    }

    #[test]
    fn python_is_named_for_its_script_or_module() {
        assert_eq!(
            name("python.exe", r"python.exe -u C:\tools\sync_notes.py --once"),
            Some("sync_notes".into())
        );
        assert_eq!(
            name("pythonw.exe", "pythonw -m idlelib"),
            Some("idlelib".into())
        );
        assert_eq!(
            name("python.exe", "python -W ignore app.pyw"),
            Some("app".into())
        );
        assert_eq!(name("python.exe", "python -c print(1)"), None);
        assert_eq!(name("python.exe", "python"), None);
    }

    #[test]
    fn node_and_electron_are_named_for_their_entry() {
        assert_eq!(
            name(
                "node.exe",
                r"node.exe --require ./setup.js C:\code\server\index.js"
            ),
            Some("index".into())
        );
        assert_eq!(
            name("electron.exe", r"electron.exe C:\code\my-app"),
            Some("my-app".into())
        );
        assert_eq!(name("node.exe", "node -e 1"), None);
    }

    #[test]
    fn dotnet_rundll_hta_and_webview_are_named_for_what_they_run() {
        assert_eq!(
            name("dotnet.exe", r"dotnet exec C:\a\Tool.dll"),
            Some("Tool".into())
        );
        assert_eq!(
            name(
                "rundll32.exe",
                "rundll32.exe shell32.dll,Control_RunDLL desk.cpl,,3"
            ),
            Some("desk".into())
        );
        assert_eq!(
            name("rundll32.exe", "rundll32 url.dll,FileProtocolHandler x"),
            Some("url".into())
        );
        assert_eq!(
            name("mshta.exe", r#"mshta "C:\x\Setup.hta""#),
            Some("Setup".into())
        );
        assert_eq!(
            name(
                "msedgewebview2.exe",
                "msedgewebview2.exe --type=renderer --webview-exe-name=Contoso.exe"
            ),
            Some("Contoso".into())
        );
        assert_eq!(name("notepad.exe", "notepad a.txt"), None);
    }

    #[test]
    fn versions_are_not_part_of_the_name() {
        assert_eq!(clean_name("tool-1.4.2"), Some("tool".into()));
        assert_eq!(clean_name("my tool v2.0-beta"), Some("my tool".into()));
        assert_eq!(clean_name("tool_3.10"), Some("tool".into()));
        assert_eq!(clean_name("2fa-tool"), Some("2fa-tool".into()));
        assert_eq!(clean_name("app-2"), Some("app-2".into()));
        assert_eq!(clean_name("..."), None);
        assert_eq!(file_stem(r"C:\x\"), Some("x".into()));
        assert_eq!(file_stem("a.b.c.jar"), Some("a.b.c".into()));
        assert_eq!(file_stem(".."), None);
    }

    #[test]
    fn identifiers_are_folded_and_safe() {
        assert_eq!(
            qualified("msedge.exe", "MSEdge._crx_ABC.Default"),
            "msedge.exe/msedge._crx_abc.default"
        );
        assert_eq!(
            qualified("wwahost.exe", "Contoso.App_8wekyb3d8bbwe!App"),
            "wwahost.exe/contoso.app_8wekyb3d8bbwe!app"
        );
        assert_eq!(
            qualified("mmc.exe", "My Tool/x,y\"z"),
            "mmc.exe/my_tool_x_y_z"
        );
        let long = qualified("a.exe", &"é".repeat(300));
        assert_eq!(long.chars().count(), "a.exe/".len() + QUALIFIER_LIMIT);
    }

    #[test]
    fn only_a_web_apps_windows_name_an_app() {
        const ID: &str = "fcfpmbpedhjfjjalkigdnbhmjghlglba";
        let from_aumid = format!("Chrome._crx_{ID}");
        let from_command =
            format!(r#""C:/Chrome/chrome.exe" --profile-directory=Default --app-id={ID}"#);
        // Either signal gives the same id, whatever the profile.
        assert_eq!(web_app_id(Some(&from_aumid), None).as_deref(), Some(ID));
        assert_eq!(
            web_app_id(
                Some(&format!("MSEdge._CRX_{}.Profile 1", ID.to_uppercase())),
                None
            )
            .as_deref(),
            Some(ID)
        );
        assert_eq!(web_app_id(None, Some(&from_command)).as_deref(), Some(ID));
        assert_eq!(
            web_app_id(Some(&from_aumid), Some(&from_command)).as_deref(),
            Some(ID)
        );
        // The browser's own windows, and Edge's `--app=URL` window.
        assert_eq!(
            web_app_id(Some("Chrome"), Some(r#""C:/Chrome/chrome.exe""#)),
            None
        );
        assert_eq!(web_app_id(Some("MSEdge.Profile 1"), None), None);
        assert_eq!(
            web_app_id(Some("MSEdge.example.com_/.edgeprof.Default"), None),
            None
        );
        assert_eq!(web_app_id(Some("F0DC299D809B9700"), None), None);
        assert_eq!(web_app_id(Some("Chrome._crx_abc"), None), None);
        assert_eq!(web_app_id(None, None), None);
    }

    /// A real window carrying the properties Chromium sets on an installed web
    /// app's window, and one carrying a plain browser window's.
    #[test]
    fn a_web_apps_window_is_named_by_its_own_properties() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_OVERLAPPED,
        };

        const ID: &str = "fcfpmbpedhjfjjalkigdnbhmjghlglba";
        fn window_with(properties: &[(&PROPERTYKEY, &str)]) -> HWND {
            // SAFETY: a STATIC control that is destroyed by the caller.
            let window = unsafe {
                CreateWindowExW(
                    Default::default(),
                    w!("STATIC"),
                    w!("test"),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .expect("a test window");
            ensure_com();
            let store = window_properties(window).expect("the window's property store");
            for (key, value) in properties {
                // SAFETY: the store is live and the variant owns its string.
                unsafe { store.SetValue(*key, &PROPVARIANT::from(*value)) }.expect("set");
            }
            // SAFETY: as above.
            unsafe { store.Commit() }.expect("commit");
            window
        }

        let app = window_with(&[
            (&PKEY_AppUserModel_ID, &format!("Chrome._crx_{ID}.Default")),
            (&PKEY_AppUserModel_RelaunchDisplayNameResource, "Team Docs"),
        ]);
        let browser = window_with(&[
            (&PKEY_AppUserModel_ID, "Chrome"),
            (
                &PKEY_AppUserModel_RelaunchDisplayNameResource,
                "Google Chrome",
            ),
        ]);
        let packaged = window_with(&[(&PKEY_AppUserModel_ID, "Contoso.App_8wekyb3d8bbwe!Main")]);

        let web_app = resolve(app, 0, "chrome.exe", "Google Chrome", true);
        assert_eq!(web_app.id, format!("chrome.exe/{ID}"));
        assert_eq!(web_app.name, "Team Docs");
        let plain = resolve(browser, 0, "chrome.exe", "Google Chrome", true);
        assert_eq!(plain.id, "chrome.exe");
        assert_eq!(plain.name, "Google Chrome");
        let host = resolve(packaged, 0, "wwahost.exe", "Windows Web App Host", false);
        assert_eq!(host.id, "wwahost.exe/contoso.app_8wekyb3d8bbwe!main");
        // The cache returns the same answer, and a different window another.
        assert_eq!(identify(app, 0, "chrome.exe", "Google Chrome"), web_app);
        assert_ne!(identify(browser, 0, "chrome.exe", "Google Chrome"), web_app);

        for window in [app, browser, packaged] {
            // SAFETY: the windows were created above, on this thread.
            let _ = unsafe { DestroyWindow(window) };
        }
    }

    #[test]
    fn a_frameworks_default_app_id_does_not_name_an_app() {
        assert!(is_generic_app_id("electron.exe", "electron.app.Electron"));
        assert!(is_generic_app_id("node.exe", "Node"));
        assert!(is_generic_app_id("pythonw.exe", "pythonw.exe"));
        assert!(!is_generic_app_id("electron.exe", "electron.app.Notes"));
        assert!(!is_generic_app_id("pythonw.exe", "Contoso.Tool"));
    }

    /// An electron dev app with the default id is told apart by its entry.
    #[test]
    fn a_default_window_id_yields_to_the_command_line() {
        // The host branch of `resolve` filters the window id like this before
        // it falls back to the package id and then the command-line name.
        let window_id = Some("electron.app.Electron".to_string())
            .filter(|id| !is_generic_app_id("electron.exe", id));
        assert_eq!(window_id, None);
        assert_eq!(
            script_name(
                "electron.exe",
                &split_command_line(r"electron.exe C:\code\notes-app")
            ),
            Some("notes-app".into())
        );
    }

    #[test]
    fn ordinary_apps_keep_their_executable_as_the_id() {
        // No lookups happen for these, so no window is needed.
        let identity = identify(HWND::default(), 0, "slack.exe", "Slack");
        assert_eq!(identity.id, "slack.exe");
        assert_eq!(identity.name, "Slack");
        assert!(is_host("javaw.exe"));
        assert!(!is_host("code.exe"));
        assert!(!is_web_app(
            "chrome.exe",
            &Identity {
                id: "chrome.exe".into(),
                name: "Google Chrome".into()
            }
        ));
        assert!(is_web_app(
            "chrome.exe",
            &Identity {
                id: "chrome.exe/chrome._crx_x".into(),
                name: "App".into()
            }
        ));
        assert!(!is_web_app("slack.exe", &identity));
    }

    /// Prints what every visible top-level window would be recorded as. Run by
    /// hand: `cargo test --lib identity::tests::live_identities -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the windows open on this machine"]
    fn live_identities() {
        use windows::core::BOOL;
        use windows::Win32::Foundation::LPARAM;
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindowVisible};

        unsafe extern "system" fn collect(window: HWND, state: LPARAM) -> BOOL {
            // SAFETY: `state` is the `Vec` passed below, alive for the call.
            let found = unsafe { &mut *(state.0 as *mut Vec<HWND>) };
            // SAFETY: a plain query on a window handle.
            if unsafe { IsWindowVisible(window) }.as_bool()
                && !super::super::window_title(window).is_empty()
            {
                found.push(window);
            }
            BOOL(1)
        }

        let mut windows: Vec<HWND> = Vec::new();
        // SAFETY: `collect` only runs during this call, while `windows` lives.
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize));
        }
        for window in windows {
            let Some(pid) = super::super::process_id(window) else {
                continue;
            };
            let Some(process) = super::super::identify(pid) else {
                continue;
            };
            ensure_com();
            let store = window_properties(window);
            let raw = store
                .as_ref()
                .and_then(|store| string_property(store, &PKEY_AppUserModel_ID));
            let started = Instant::now();
            let identity = identify(window, pid, &process.executable, &process.app);
            println!(
                "{:<24} window-aumid={:<44} package={:<40} => id={:?} name={:?} ({:?})",
                process.executable,
                format!("{raw:?}"),
                format!("{:?}", package_app_id(pid)),
                identity.id,
                identity.name,
                started.elapsed()
            );
        }
    }
}
