//! Launch at login. The OS is the source of truth, not `settings.json` (the user can switch it off
//! behind our back), so every read asks the OS. macOS: `SMAppService.mainAppService`. Windows: a
//! per-user `Run` value; Task Manager's switch records an override in `StartupApproved\Run`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub enum LoginItemState {
    Enabled,
    Disabled,
    /// Registered, but the user has to allow it in System Settings first.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    RequiresApproval,
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
}

#[cfg_attr(target_os = "macos", path = "login_item/macos.rs")]
#[cfg_attr(windows, path = "login_item/windows.rs")]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    path = "login_item/unsupported.rs"
)]
mod imp;

pub fn state() -> LoginItemState {
    imp::state()
}

/// Windows: rewrites a Run value an earlier version wrote without `--background`, run at startup so
/// users who never open Settings are migrated too. A no-op when nothing is registered and
/// elsewhere.
pub fn refresh() {
    imp::refresh();
}

/// Registers or unregisters, then reports what the OS says: on macOS a successful register can
/// still land on `RequiresApproval`.
pub fn set(enabled: bool) -> Result<LoginItemState, String> {
    // Unregistering an app that is not registered is an error in macOS but
    // not in intent, so asking for the state it already has is a no-op.
    let current = state();
    let already = match current {
        LoginItemState::Enabled | LoginItemState::RequiresApproval => enabled,
        LoginItemState::Disabled => !enabled,
        LoginItemState::Unsupported => false,
    };
    if !already {
        imp::set(enabled)?;
    }
    Ok(state())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_serialize_for_the_frontend() {
        assert_eq!(
            serde_json::to_string(&LoginItemState::RequiresApproval).unwrap(),
            "\"requiresApproval\""
        );
        assert_eq!(
            serde_json::to_string(&LoginItemState::Unsupported).unwrap(),
            "\"unsupported\""
        );
    }
}
