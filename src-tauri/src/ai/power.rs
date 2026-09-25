//! When background retraining is allowed: only while the Mac is idle, on AC
//! power, and not in Low Power Mode, so learning never costs the user
//! battery or a stutter while they work. "Retrain now" in Settings skips
//! this gate, since the user asked for it.

use std::time::Duration;

/// How long the user must have been away from the keyboard and mouse.
pub const MIN_IDLE: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conditions {
    pub idle: Duration,
    pub on_ac: bool,
    pub low_power: bool,
}

impl Conditions {
    pub fn read() -> Self {
        Self {
            idle: Duration::from_millis(crate::activity::read_idle_ms()),
            on_ac: on_ac_power(),
            low_power: low_power_mode(),
        }
    }

    /// Why background retraining has to wait, or `None` when it may run.
    pub fn blocker(&self) -> Option<&'static str> {
        if self.low_power {
            Some("waiting for Low Power Mode to turn off")
        } else if !self.on_ac {
            Some("waiting for the Mac to be on power")
        } else if self.idle < MIN_IDLE {
            Some("waiting for the Mac to be idle")
        } else {
            None
        }
    }
}

#[cfg(target_os = "macos")]
mod iokit {
    use core_foundation_sys::base::CFTypeRef;
    use core_foundation_sys::string::CFStringRef;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        /// Create rule: the caller releases the snapshot.
        pub fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        /// Get rule: the string belongs to the snapshot.
        pub fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFStringRef;
    }
}

/// Whether the Mac draws from AC (or a UPS) rather than its battery. A Mac
/// without a battery reports AC.
#[cfg(target_os = "macos")]
fn on_ac_power() -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_foundation_sys::base::CFRelease;

    // SAFETY: IOPSCopyPowerSourcesInfo returns an owned snapshot (or null),
    // which is released below; the providing type is borrowed from it and
    // copied into a Rust string before the release.
    unsafe {
        let snapshot = iokit::IOPSCopyPowerSourcesInfo();
        if snapshot.is_null() {
            return true;
        }
        let kind = iokit::IOPSGetProvidingPowerSourceType(snapshot);
        let on_battery = !kind.is_null() && CFString::wrap_under_get_rule(kind) == "Battery Power";
        CFRelease(snapshot);
        !on_battery
    }
}

#[cfg(not(target_os = "macos"))]
fn on_ac_power() -> bool {
    true
}

#[cfg(target_os = "macos")]
fn low_power_mode() -> bool {
    objc2_foundation::NSProcessInfo::processInfo().isLowPowerModeEnabled()
}

#[cfg(not(target_os = "macos"))]
fn low_power_mode() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conditions(idle_min: u64, on_ac: bool, low_power: bool) -> Conditions {
        Conditions {
            idle: Duration::from_secs(idle_min * 60),
            on_ac,
            low_power,
        }
    }

    #[test]
    fn retraining_runs_only_idle_on_power_outside_low_power_mode() {
        assert_eq!(conditions(10, true, false).blocker(), None);
        assert!(conditions(1, true, false)
            .blocker()
            .unwrap()
            .contains("idle"));
        assert!(conditions(10, false, false)
            .blocker()
            .unwrap()
            .contains("power"));
        assert!(conditions(10, true, true)
            .blocker()
            .unwrap()
            .contains("Low Power"));
    }

    #[test]
    fn reading_the_real_conditions_does_not_panic() {
        let _ = Conditions::read();
    }
}
