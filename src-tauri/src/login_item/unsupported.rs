//! No launch-at-login implementation on this platform yet.

use super::LoginItemState;

pub fn state() -> LoginItemState {
    LoginItemState::Unsupported
}

pub fn set(_enabled: bool) -> Result<(), String> {
    Err("Launch at login is not available on this platform yet".to_string())
}
