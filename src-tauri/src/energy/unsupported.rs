//! No telemetry on this platform yet: zero usage, never on battery.

use super::*;

/// No telemetry here, so the sampler stays off.
pub const HAS_ENERGY_COUNTER: bool = false;

pub fn read_proc_rusage(_pid: i32) -> Option<ProcessRusage> {
    Some(ProcessRusage::default())
}

#[cfg(test)]
pub fn resident_bytes(_pid: i32) -> Option<u64> {
    None
}

#[cfg(test)]
pub fn peak_memory_bytes(_pid: i32) -> Option<u64> {
    None
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
