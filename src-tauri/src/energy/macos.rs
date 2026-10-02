//! `proc_pid_rusage` CPU and energy counters, and IOKit power sources.

use super::*;

#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct RusageInfoV4 {
    pub ri_uuid: [u8; 16],
    pub ri_user_time: u64,
    pub ri_system_time: u64,
    pub ri_pkg_idle_wkups: u64,
    pub ri_interrupt_wkups: u64,
    pub ri_pageins: u64,
    pub ri_wired_size: u64,
    pub ri_resident_size: u64,
    pub ri_phys_footprint: u64,
    pub ri_proc_start_abstime: u64,
    pub ri_proc_exit_abstime: u64,
    pub ri_child_user_time: u64,
    pub ri_child_system_time: u64,
    pub ri_child_pkg_idle_wkups: u64,
    pub ri_child_interrupt_wkups: u64,
    pub ri_child_pageins: u64,
    pub ri_child_elapsed_abstime: u64,
    pub ri_diskio_bytesread: u64,
    pub ri_diskio_byteswritten: u64,
    pub ri_cpu_time_qos_default: u64,
    pub ri_cpu_time_qos_maintenance: u64,
    pub ri_cpu_time_qos_background: u64,
    pub ri_cpu_time_qos_utility: u64,
    pub ri_cpu_time_qos_legacy: u64,
    pub ri_cpu_time_qos_user_initiated: u64,
    pub ri_cpu_time_qos_user_interactive: u64,
    pub ri_billed_system_time: u64,
    pub ri_serviced_system_time: u64,
    pub ri_logical_writes: u64,
    pub ri_lifetime_max_phys_footprint: u64,
    pub ri_instructions: u64,
    pub ri_cycles: u64,
    pub ri_billed_energy: u64,
    pub ri_serviced_energy: u64,
    pub ri_interval_max_phys_footprint: u64,
    pub ri_runnable_time: u64,
}

const RUSAGE_INFO_V4: libc::c_int = 4;

extern "C" {
    fn proc_pid_rusage(
        pid: libc::c_int,
        flavor: libc::c_int,
        buffer: *mut libc::c_void,
    ) -> libc::c_int;
}

pub fn read_proc_rusage(pid: i32) -> Option<ProcessRusage> {
    let mut info = RusageInfoV4::default();
    let ret = unsafe {
        proc_pid_rusage(
            pid,
            RUSAGE_INFO_V4,
            &mut info as *mut RusageInfoV4 as *mut libc::c_void,
        )
    };
    if ret == 0 {
        Some(ProcessRusage {
            cpu_time_ns: info.ri_user_time
                + info.ri_system_time
                + info.ri_child_user_time
                + info.ri_child_system_time,
            billed_energy_nj: info.ri_billed_energy,
        })
    } else {
        None
    }
}

mod iokit {
    use core_foundation_sys::array::CFArrayRef;
    use core_foundation_sys::base::CFTypeRef;
    use core_foundation_sys::dictionary::CFDictionaryRef;
    use core_foundation_sys::string::CFStringRef;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        pub fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        pub fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFStringRef;
        pub fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
        pub fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
    }
}

pub fn read_battery_info() -> BatteryInfo {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_foundation_sys::array::CFArrayGetCount;
    use core_foundation_sys::array::CFArrayGetValueAtIndex;
    use core_foundation_sys::base::CFRelease;
    use core_foundation_sys::dictionary::CFDictionaryGetValue;
    use core_foundation_sys::number::{kCFNumberSInt64Type, CFNumberGetValue};

    unsafe {
        let snapshot = iokit::IOPSCopyPowerSourcesInfo();
        if snapshot.is_null() {
            return BatteryInfo {
                on_battery: false,
                battery_percent: None,
            };
        }
        let kind = iokit::IOPSGetProvidingPowerSourceType(snapshot);
        let on_battery = !kind.is_null() && CFString::wrap_under_get_rule(kind) == "Battery Power";

        let mut battery_percent = None;
        let list = iokit::IOPSCopyPowerSourcesList(snapshot);
        if !list.is_null() {
            let count = CFArrayGetCount(list);
            let cur_cap_key = CFString::new("Current Capacity");
            let max_cap_key = CFString::new("Max Capacity");

            for i in 0..count {
                let ps = CFArrayGetValueAtIndex(list, i);
                if ps.is_null() {
                    continue;
                }
                let desc = iokit::IOPSGetPowerSourceDescription(snapshot, ps);
                if desc.is_null() {
                    continue;
                }

                let cur_ptr = CFDictionaryGetValue(desc, cur_cap_key.as_concrete_TypeRef() as _);
                let max_ptr = CFDictionaryGetValue(desc, max_cap_key.as_concrete_TypeRef() as _);

                if !cur_ptr.is_null() && !max_ptr.is_null() {
                    let mut cur_val: i64 = 0;
                    let mut max_val: i64 = 0;
                    let got_cur = CFNumberGetValue(
                        cur_ptr as _,
                        kCFNumberSInt64Type,
                        &mut cur_val as *mut i64 as _,
                    );
                    let got_max = CFNumberGetValue(
                        max_ptr as _,
                        kCFNumberSInt64Type,
                        &mut max_val as *mut i64 as _,
                    );

                    if got_cur && got_max && max_val > 0 {
                        battery_percent =
                            Some((cur_val as f64 / max_val as f64 * 100.0).clamp(0.0, 100.0));
                        break;
                    }
                }
            }
            CFRelease(list as _);
        }
        CFRelease(snapshot);

        BatteryInfo {
            on_battery,
            battery_percent,
        }
    }
}
