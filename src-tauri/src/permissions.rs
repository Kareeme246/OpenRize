use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub required: bool,
    /// What is true of it right now. For Accessibility this comes from a probe
    /// of the running process, not from the trust flag alone.
    pub state: AccessibilityState,
    pub granted: bool,
    pub settings_url: String,
}

mod accessibility;

pub use accessibility::{spawn_monitor, AccessibilityState, Monitor};

#[cfg_attr(target_os = "macos", path = "permissions/macos.rs")]
#[cfg_attr(windows, path = "permissions/windows.rs")]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    path = "permissions/unsupported.rs"
)]
mod platform;

/// Every permission this platform has, with Accessibility described by
/// `accessibility`, the state the monitor has worked out. A simulated
/// Automation answer (dev builds only, see dev_tools.rs) replaces the real one.
pub fn check_permissions(
    accessibility: AccessibilityState,
    automation: Option<bool>,
) -> Vec<PermissionInfo> {
    let mut permissions = platform::check_permissions(accessibility);
    if let (Some(granted), Some(entry)) = (
        automation,
        permissions
            .iter_mut()
            .find(|permission| permission.id == "automation"),
    ) {
        entry.granted = granted;
        entry.state = if granted {
            AccessibilityState::Working
        } else {
            AccessibilityState::NotGranted
        };
    }
    permissions
}

/// Shows macOS's own Accessibility prompt. Only ever called from a button the
/// user pressed: a prompt on launch, or on every poll, would nag.
pub fn request_accessibility() {
    platform::request_accessibility();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_structure() {
        let perms = check_permissions(AccessibilityState::NotGranted, None);
        #[cfg(target_os = "macos")]
        {
            assert_eq!(perms.len(), 2);
            let accessibility = perms.iter().find(|p| p.id == "accessibility").unwrap();
            assert!(!accessibility.required);
            assert_eq!(accessibility.state, AccessibilityState::NotGranted);
            assert!(!accessibility.granted);
            assert_eq!(
                accessibility.settings_url,
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            );

            let automation = perms.iter().find(|p| p.id == "automation").unwrap();
            assert!(!automation.required);
            assert_eq!(
                automation.settings_url,
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
            );
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(perms.is_empty());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn only_a_working_probe_counts_as_granted() {
        use AccessibilityState::*;
        for (state, granted) in [
            (NotGranted, false),
            (Checking, false),
            (NotWorking, false),
            (RelaunchRequired, false),
            (Working, true),
        ] {
            let perms = check_permissions(state, None);
            let accessibility = perms.iter().find(|p| p.id == "accessibility").unwrap();
            assert_eq!(accessibility.state, state);
            assert_eq!(accessibility.granted, granted, "{state:?}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_simulated_automation_answer_replaces_the_real_one_both_ways() {
        use AccessibilityState::*;
        for granted in [true, false] {
            let perms = check_permissions(Checking, Some(granted));
            let automation = perms.iter().find(|p| p.id == "automation").unwrap();
            assert_eq!(automation.granted, granted);
            assert_eq!(automation.state, if granted { Working } else { NotGranted });

            let accessibility = perms.iter().find(|p| p.id == "accessibility").unwrap();
            assert_eq!(accessibility.state, Checking);
            assert!(!accessibility.granted);
        }
    }
}
