//! Launch at login, through macOS's own `SMAppService` login-item API
//! (macOS 13+). The OS is the source of truth, not `settings.json`: the user
//! can switch it off under System Settings > General > Login Items, so every
//! read asks macOS instead of trusting a stored flag.
//!
//! `SMAppService.mainAppService` registers the running app itself, so there
//! is no helper bundle or LaunchAgent plist to ship.

use serde::Serialize;

/// What Settings shows for the login item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LoginItemState {
    Enabled,
    Disabled,
    /// Registered, but the user has to allow it in System Settings first.
    RequiresApproval,
    /// macOS older than 13, or not macOS at all.
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

#[cfg(target_os = "macos")]
mod imp {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObject};
    use objc2_foundation::NSString;

    use super::LoginItemState;

    // SMAppService lives in ServiceManagement, which nothing else links.
    #[link(name = "ServiceManagement", kind = "framework")]
    extern "C" {}

    /// `SMAppService.mainAppService`, or `None` before macOS 13.
    fn main_app_service() -> Option<Retained<AnyObject>> {
        let class = AnyClass::get(c"SMAppService")?;
        // SAFETY: `mainAppService` is a class property returning the shared
        // service object for the running app bundle.
        unsafe { msg_send![class, mainAppService] }
    }

    pub fn state() -> LoginItemState {
        match main_app_service() {
            // SAFETY: `status` is a plain NSInteger property getter.
            Some(service) => LoginItemState::from_status(unsafe { msg_send![&service, status] }),
            None => LoginItemState::Unsupported,
        }
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let service = main_app_service().ok_or("Launch at login needs macOS 13 or later")?;
        // SAFETY: both selectors take one trailing `NSError **` and return
        // BOOL; `_` makes objc2 supply the out-parameter and map NO to Err.
        let result: Result<(), Retained<NSObject>> = unsafe {
            if enabled {
                msg_send![&service, registerAndReturnError: _]
            } else {
                msg_send![&service, unregisterAndReturnError: _]
            }
        };
        result.map_err(|error| {
            // SAFETY: the error is an NSError, whose localizedDescription is
            // a non-null NSString.
            let description: Retained<NSString> =
                unsafe { msg_send![&error, localizedDescription] };
            description.to_string()
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::LoginItemState;

    pub fn state() -> LoginItemState {
        LoginItemState::Unsupported
    }

    pub fn set(_enabled: bool) -> Result<(), String> {
        Err("Launch at login is only available on macOS".to_string())
    }
}

pub fn state() -> LoginItemState {
    imp::state()
}

/// Registers or unregisters the app, then reports what macOS now says: a
/// successful register can still land on `RequiresApproval`.
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
