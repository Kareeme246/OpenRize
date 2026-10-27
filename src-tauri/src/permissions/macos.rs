use core_foundation::base::{CFType, TCFType};
use core_foundation::string::CFString;
use core_foundation_sys::base::CFTypeRef;

use super::accessibility::{AccessibilityState, Probe, Reading};
use super::PermissionInfo;

use crate::ax::{
    kAXTrustedCheckOptionPrompt, AXIsProcessTrusted, AXIsProcessTrustedWithOptions,
    AXUIElementCopyAttributeValue, AXUIElementCreateSystemWide,
};

fn check_accessibility() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Asks the Accessibility service which app has focus. This is the question
/// capture depends on, put to the system-wide element, which needs no
/// particular app to be running or cooperative. A process that is not really
/// trusted gets `kAXErrorAPIDisabled` here whatever the trust flag says.
fn probe() -> Probe {
    // SAFETY: the element is created here and released when the wrapper
    // drops; a successful copy hands back a +1 value that is released too.
    unsafe {
        let element = AXUIElementCreateSystemWide();
        if element.is_null() {
            return Probe::Fails(-1);
        }
        let _element = CFType::wrap_under_create_rule(element as CFTypeRef);
        let attribute = CFString::from_static_string("AXFocusedApplication");
        let mut value: CFTypeRef = std::ptr::null_mut();
        let code =
            AXUIElementCopyAttributeValue(element, attribute.as_concrete_TypeRef(), &mut value);
        if code == 0 && !value.is_null() {
            drop(CFType::wrap_under_create_rule(value));
        }
        Probe::from_ax_error(code)
    }
}

/// Asks macOS to show its Accessibility prompt, which also lists OpenRize in
/// System Settings so there is something to switch on.
pub fn request_accessibility() {
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;

    // SAFETY: the key is a constant CFString owned by the framework.
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
    // SAFETY: `options` outlives the call.
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) };
}

pub fn accessibility_reading() -> Option<Reading> {
    Some(if check_accessibility() {
        Reading::Trusted(probe())
    } else {
        Reading::Untrusted
    })
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

pub fn check_permissions(accessibility: AccessibilityState) -> Vec<PermissionInfo> {
    let automation = check_automation();
    vec![
        PermissionInfo {
            id: "accessibility".to_string(),
            name: "Accessibility".to_string(),
            description: "Lets OpenRize read window titles, which make categories and projects more accurate. Without it OpenRize still tracks which apps you use.".to_string(),
            required: false,
            state: accessibility,
            granted: accessibility == AccessibilityState::Working,
            settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility".to_string(),
        },
        PermissionInfo {
            id: "automation".to_string(),
            name: "Automation".to_string(),
            description: "Allows OpenRize to read active tab URLs from supported web browsers to categorize time spent on websites.".to_string(),
            required: false,
            state: if automation {
                AccessibilityState::Working
            } else {
                AccessibilityState::NotGranted
            },
            granted: automation,
            settings_url: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation".to_string(),
        },
    ]
}
