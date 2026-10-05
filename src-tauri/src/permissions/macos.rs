use super::PermissionInfo;

fn check_accessibility() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}

const BROWSERS: &[&str] = &[
    "com.apple.Safari",
    "com.google.Chrome",
    "com.brave.Browser",
    "com.microsoft.edgemac",
    "company.thebrowser.Browser",
];

fn check_automation() -> bool {
    #[repr(C)]
    struct AEAddressDesc {
        descriptor_type: u32,
        data_handle: *mut std::ffi::c_void,
    }

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn AECreateDesc(
            type_code: u32,
            data_ptr: *const u8,
            data_size: usize,
            result: *mut AEAddressDesc,
        ) -> i16;
        fn AEDeterminePermissionToAutomateTarget(
            the_target: *const AEAddressDesc,
            the_ae_event_class: u32,
            the_ae_event_id: u32,
            ask_user_if_needed: bool,
        ) -> i32;
        fn AEDisposeDesc(desc: *mut AEAddressDesc) -> i16;
    }

    let type_bundle_id = u32::from_be_bytes(*b"bund");
    let type_wildcard = u32::from_be_bytes(*b"****");

    for bundle_id in BROWSERS {
        let mut target = AEAddressDesc {
            descriptor_type: 0,
            data_handle: std::ptr::null_mut(),
        };
        let err = unsafe {
            AECreateDesc(
                type_bundle_id,
                bundle_id.as_ptr(),
                bundle_id.len(),
                &mut target,
            )
        };
        if err != 0 {
            continue;
        }

        let perm = unsafe {
            AEDeterminePermissionToAutomateTarget(&target, type_wildcard, type_wildcard, false)
        };
        unsafe {
            let _ = AEDisposeDesc(&mut target);
        }

        if perm == 0 {
            return true;
        }
    }

    false
}

pub fn check_permissions() -> Vec<PermissionInfo> {
    vec![
        PermissionInfo {
            id: "accessibility".to_string(),
            name: "Accessibility".to_string(),
            description: "Allows OpenRize to read window titles to accurately track and categorize active applications.".to_string(),
            required: true,
            granted: check_accessibility(),
            settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility".to_string(),
        },
        PermissionInfo {
            id: "automation".to_string(),
            name: "Automation".to_string(),
            description: "Allows OpenRize to read active tab URLs from supported web browsers to categorize time spent on websites.".to_string(),
            required: false,
            granted: check_automation(),
            settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation".to_string(),
        },
    ]
}
