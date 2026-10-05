use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub required: bool,
    pub granted: bool,
    pub settings_url: String,
}

#[cfg_attr(target_os = "macos", path = "permissions/macos.rs")]
#[cfg_attr(windows, path = "permissions/windows.rs")]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    path = "permissions/unsupported.rs"
)]
mod platform;

pub fn check_permissions() -> Vec<PermissionInfo> {
    platform::check_permissions()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_structure() {
        let perms = check_permissions();
        #[cfg(target_os = "macos")]
        {
            assert_eq!(perms.len(), 2);
            let accessibility = perms.iter().find(|p| p.id == "accessibility").unwrap();
            assert!(accessibility.required);
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
}
