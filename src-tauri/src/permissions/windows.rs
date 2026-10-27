use super::accessibility::Reading;
use super::PermissionInfo;

pub fn check_permissions() -> Vec<PermissionInfo> {
    Vec::new()
}

/// Nothing here needs Accessibility, so there is nothing to watch.
pub fn accessibility_reading() -> Option<Reading> {
    None
}
