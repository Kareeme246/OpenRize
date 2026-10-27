use super::accessibility::{AccessibilityState, Reading};
use super::PermissionInfo;

pub fn check_permissions(_accessibility: AccessibilityState) -> Vec<PermissionInfo> {
    Vec::new()
}

pub fn request_accessibility() {}

/// Nothing here needs Accessibility, so there is nothing to watch.
pub fn accessibility_reading() -> Option<Reading> {
    None
}
