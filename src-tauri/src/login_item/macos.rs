//! `SMAppService.mainAppService` (macOS 13+).

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject};
use objc2_foundation::NSString;

use super::LoginItemState;

// SMAppService lives in ServiceManagement, which nothing else links.
#[link(name = "ServiceManagement", kind = "framework")]
extern "C" {}

fn main_app_service() -> Option<Retained<AnyObject>> {
    let class = AnyClass::get(c"SMAppService")?;
    // SAFETY: `mainAppService` is a class property returning the shared
    // service object for the running app bundle.
    unsafe { msg_send![class, mainAppService] }
}

/// Maps `SMAppServiceStatus`; `NotFound` (3) means never registered, so it reads as off.
fn from_status(status: isize) -> LoginItemState {
    match status {
        1 => LoginItemState::Enabled,
        2 => LoginItemState::RequiresApproval,
        _ => LoginItemState::Disabled,
    }
}

pub fn refresh() {}

pub fn state() -> LoginItemState {
    match main_app_service() {
        // SAFETY: `status` is a plain NSInteger property getter.
        Some(service) => from_status(unsafe { msg_send![&service, status] }),
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
        let description: Retained<NSString> = unsafe { msg_send![&error, localizedDescription] };
        description.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_map_to_states() {
        assert_eq!(from_status(0), LoginItemState::Disabled);
        assert_eq!(from_status(1), LoginItemState::Enabled);
        assert_eq!(from_status(2), LoginItemState::RequiresApproval);
        assert_eq!(from_status(3), LoginItemState::Disabled);
    }
}
