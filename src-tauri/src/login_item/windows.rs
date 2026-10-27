//! The per-user `Run` key, honouring Task Manager's Startup apps switch.

use std::path::Path;

use windows_registry::CURRENT_USER;

use super::LoginItemState;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
// The installer's NSIS hooks delete this exact value on uninstall
// (`src-tauri/windows/hooks.nsh`); keep the two in step.
const VALUE: &str = "OpenRize";

/// The executable's path, quoted because the install path usually has spaces.
fn quoted(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// The command Windows runs at sign-in: this executable, started in the
/// background like `rize app start`, so signing in does not open the window.
fn command_for(exe: &Path) -> String {
    format!("{} {}", quoted(exe), crate::BACKGROUND_ARG)
}

fn command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    Ok(command_for(&exe))
}

/// How a `Run` value relates to this executable.
#[derive(Debug, PartialEq, Eq)]
enum Registration {
    /// This executable, started in the background.
    Current,
    /// This executable without the flag, as earlier versions wrote it. It
    /// opens the window at every sign-in but is still the user's choice.
    Legacy,
    /// Anything else, such as another install.
    Other,
}

/// Windows paths ignore case, and the install folder's case differs between
/// releases (`openrize` in earlier releases, `OpenRize` now), so this does too.
fn classify(value: &str, exe: &Path) -> Registration {
    let value = value.trim();
    if value.eq_ignore_ascii_case(&command_for(exe)) {
        Registration::Current
    } else if value.eq_ignore_ascii_case(&quoted(exe)) {
        Registration::Legacy
    } else {
        Registration::Other
    }
}

fn registration() -> Option<Registration> {
    let value = CURRENT_USER
        .open(RUN)
        .and_then(|key| key.get_string(VALUE))
        .ok()?;
    let exe = std::env::current_exe().ok()?;
    Some(classify(&value, &exe))
}

pub fn state() -> LoginItemState {
    let ours = matches!(
        registration(),
        Some(Registration::Current | Registration::Legacy)
    );
    if ours && !switched_off_in_task_manager() {
        LoginItemState::Enabled
    } else {
        LoginItemState::Disabled
    }
}

/// Moves a value written by an earlier version onto the background command,
/// so people who already launch at login stop getting a window at every
/// sign-in. Task Manager's override is left as the user set it.
pub fn refresh() {
    if registration() == Some(Registration::Legacy) {
        if let (Ok(key), Ok(command)) = (CURRENT_USER.create(RUN), command()) {
            let _ = key.set_string(VALUE, command);
        }
    }
}

/// Task Manager stores 12 bytes per entry; an odd first byte means the
/// user disabled it there.
fn switched_off_in_task_manager() -> bool {
    CURRENT_USER
        .open(APPROVED)
        .and_then(|key| key.get_value(VALUE))
        .is_ok_and(|value| value.first().is_some_and(|flag| flag % 2 == 1))
}

pub fn set(enabled: bool) -> Result<(), String> {
    let key = CURRENT_USER
        .create(RUN)
        .map_err(|error| format!("could not open the Run key: {error}"))?;
    if enabled {
        key.set_string(VALUE, command()?)
            .map_err(|error| format!("could not register OpenRize: {error}"))?;
        // Turning it on here overrides an earlier "Disabled" in Task
        // Manager; Windows would otherwise keep skipping it.
        if let Ok(approved) = CURRENT_USER.open(APPROVED) {
            let _ = approved.remove_value(VALUE);
        }
        Ok(())
    } else {
        key.remove_value(VALUE)
            .map_err(|error| format!("could not unregister OpenRize: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Users\Ada Lovelace\AppData\Local\OpenRize\openrize.exe";

    #[test]
    fn the_command_starts_in_the_background() {
        assert_eq!(
            command_for(Path::new(EXE)),
            format!("\"{EXE}\" --background")
        );
    }

    #[test]
    fn classifies_the_registered_value() {
        let exe = Path::new(EXE);
        assert_eq!(classify(&command_for(exe), exe), Registration::Current);
        assert_eq!(classify(&format!("\"{EXE}\""), exe), Registration::Legacy);
        assert_eq!(
            classify(r#""D:\other\openrize.exe" --background"#, exe),
            Registration::Other
        );
        assert_eq!(classify("", exe), Registration::Other);
    }

    #[test]
    fn folder_case_and_padding_do_not_matter() {
        let exe = Path::new(EXE);
        let lowered = format!("\"{}\"", EXE.replace("OpenRize", "openrize"));
        assert_eq!(classify(&lowered, exe), Registration::Legacy);
        assert_eq!(
            classify(&format!(" {} ", command_for(exe)), exe),
            Registration::Current
        );
    }
}
