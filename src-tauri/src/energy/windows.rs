//! Process CPU time and `GetSystemPowerStatus`.

use super::*;
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
#[cfg(test)]
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Windows has no per-process energy counter, so the sampler stays off and
/// the UI hides the monitor.
pub const HAS_ENERGY_COUNTER: bool = false;

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

/// The process's peak working set, in bytes.
#[cfg(test)]
pub fn peak_memory_bytes(pid: i32) -> Option<u64> {
    memory_counters(pid).map(|counters| counters.PeakWorkingSetSize as u64)
}

/// The process's working set now, in bytes.
#[cfg(test)]
pub fn resident_bytes(pid: i32) -> Option<u64> {
    memory_counters(pid).map(|counters| counters.WorkingSetSize as u64)
}

#[cfg(test)]
fn memory_counters(pid: i32) -> Option<PROCESS_MEMORY_COUNTERS> {
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: the handle is closed before returning, and the counters are
    // a plain struct on this stack.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid as u32).ok()?;
        let read = GetProcessMemoryInfo(process, &mut counters, size);
        let _ = CloseHandle(process);
        read.ok()?;
    }
    Some(counters)
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

/// Windows' Battery Saver, its counterpart to Low Power Mode.
pub fn low_power_mode() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: fills the struct on this stack.
    unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.SystemStatusFlag == 1
}
