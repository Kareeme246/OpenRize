//! The per-user `Run` key, honouring Task Manager's Startup apps switch.

use windows_registry::CURRENT_USER;

use super::LoginItemState;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const VALUE: &str = "OpenRize";

/// The command Windows runs at sign-in: this executable, quoted because
/// the install path usually has spaces.
fn command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    Ok(format!("\"{}\"", exe.display()))
}

pub fn state() -> LoginItemState {
    let registered = CURRENT_USER
        .open(RUN)
        .and_then(|key| key.get_string(VALUE))
        .ok();
    let ours = registered.is_some_and(|value| command().is_ok_and(|ours| value == ours));
    if ours && !switched_off_in_task_manager() {
        LoginItemState::Enabled
    } else {
        LoginItemState::Disabled
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
