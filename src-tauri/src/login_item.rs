//! Launch at login. The OS is the source of truth, not `settings.json`: the
//! user can switch it off behind the app's back, so every read asks the OS
//! instead of trusting a stored flag.
//!
//! - macOS: `SMAppService.mainAppService` (macOS 13+) registers the running
//!   app itself, so there is no helper bundle or LaunchAgent plist to ship.
//!   The user can switch it off under System Settings > General > Login Items.
//! - Windows: a value under the per-user `Run` key. The user can switch it
//!   off in Task Manager > Startup apps, which records an override under
//!   `StartupApproved\Run` rather than deleting our value.

use serde::Serialize;

/// What Settings shows for the login item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginItemState {
    Enabled,
    Disabled,
    /// Registered, but the user has to allow it in System Settings first.
    RequiresApproval,
    /// macOS older than 13, or a platform without an implementation.
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
}

impl LoginItemState {
    /// Maps `SMAppServiceStatus`. `NotFound` (3) means macOS has no record of
    /// the service, which for the main app just means it was never
    /// registered, so it reads as off.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn from_status(status: isize) -> Self {
        match status {
            1 => Self::Enabled,
            2 => Self::RequiresApproval,
            _ => Self::Disabled,
        }
    }
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

/// Registers or unregisters the app, then reports what the OS now says: on
/// macOS a successful register can still land on `RequiresApproval`.
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
    fn status_codes_map_to_states() {
        assert_eq!(LoginItemState::from_status(0), LoginItemState::Disabled);
        assert_eq!(LoginItemState::from_status(1), LoginItemState::Enabled);
        assert_eq!(
            LoginItemState::from_status(2),
            LoginItemState::RequiresApproval
        );
        assert_eq!(LoginItemState::from_status(3), LoginItemState::Disabled);
    }

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
