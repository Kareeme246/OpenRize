//! No telemetry on this platform yet: zero usage, never on battery.

use super::*;

pub const HAS_ENERGY_COUNTER: bool = false;

pub fn read_proc_rusage(_pid: i32) -> Option<ProcessRusage> {
    Some(ProcessRusage::default())
}

pub fn read_battery_info() -> BatteryInfo {
    BatteryInfo {
        on_battery: false,
        battery_percent: None,
    }
}

pub fn low_power_mode() -> bool {
    false
}
