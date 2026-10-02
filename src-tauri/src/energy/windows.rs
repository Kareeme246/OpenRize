//! Process CPU time and `GetSystemPowerStatus`.

use super::*;
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Kernel plus user CPU time. Windows has no per-process energy counter,
/// so `billed_energy_nj` stays 0 and the sampler estimates from CPU time.
pub fn read_proc_rusage(pid: i32) -> Option<ProcessRusage> {
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the handle is closed before returning, and the out
    // parameters are plain FILETIMEs on this stack.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid as u32).ok()?;
        let read = GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user);
        let _ = CloseHandle(process);
        read.ok()?;
    }
    // FILETIME counts 100 ns intervals.
    Some(ProcessRusage {
        cpu_time_ns: (ticks(kernel) + ticks(user)).saturating_mul(100),
        billed_energy_nj: 0,
    })
}

pub fn read_battery_info() -> BatteryInfo {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: fills the struct on this stack.
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        return BatteryInfo::default();
    }
    // ACLineStatus: 0 offline, 1 online, 255 unknown. BatteryFlag 128
    // means there is no battery; BatteryLifePercent 255 means unknown.
    let has_battery = status.BatteryFlag & 128 == 0;
    BatteryInfo {
        on_battery: has_battery && status.ACLineStatus == 0,
        battery_percent: (has_battery && status.BatteryLifePercent <= 100)
            .then(|| f64::from(status.BatteryLifePercent)),
    }
}
