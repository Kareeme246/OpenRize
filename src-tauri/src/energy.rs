//! Process energy impact and battery monitoring.
//!
//! Samples process CPU time and energy counters (via macOS `proc_pid_rusage`
//! and IOKit power sources) on a 15-second cadence and records telemetry into
//! `energy_samples` in SQLite.
//!
//! Tracks OpenRize's main process and any child sidecar process (`openrize-ml`),
//! correlating energy spikes with AI classification and model retraining.
//! Surfaces relative energy impact (low / medium / high) and cumulative
//! battery usage for Settings and benchmarking.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::ai::AiRuntime;
use crate::timers::now_epoch_ms;

pub const EVENT_ENERGY_CHANGED: &str = "energy-changed";
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(15);
pub const DEFAULT_HISTORY_DAYS: u32 = 7;

/// Standard MacBook nominal battery capacity: ~70 Watt-hours = 252,000 Joules.
const NOMINAL_BATTERY_JOULES: f64 = 70.0 * 3600.0;
/// Standard 1 mWh = 3.6 Joules.
const JOULES_PER_MWH: f64 = 3.6;

/// Power thresholds in Watts for energy impact levels.
pub const THRESHOLD_LOW_WATTS: f64 = 0.5;
pub const THRESHOLD_MEDIUM_WATTS: f64 = 2.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnergySample {
    pub id: i64,
    pub sampled_at: u64,
    pub duration_ms: u32,
    pub cpu_time_ms: u32,
    pub energy_nj: u64,
    pub power_watts: f64,
    pub impact_level: String,
    pub ai_active: bool,
    pub on_battery: bool,
    pub battery_level: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnergySummary {
    pub current_impact: String,
    pub current_power_watts: f64,
    pub baseline_power_watts: f64,
    pub peak_power_watts: f64,
    pub impact_description: String,
    pub battery_used_pct: f64,
    pub battery_used_mwh: f64,
    pub total_energy_joules: f64,
    pub total_cpu_time_ms: u64,
    pub ai_energy_pct: f64,
    pub on_battery: bool,
    pub current_battery_pct: Option<f64>,
    pub samples_count: u32,
    pub window_days: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessRusage {
    pub cpu_time_ns: u64,
    pub billed_energy_nj: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BatteryInfo {
    pub on_battery: bool,
    pub battery_percent: Option<f64>,
}

pub fn impact_level_for_power(power_watts: f64) -> &'static str {
    if power_watts < THRESHOLD_LOW_WATTS {
        "low"
    } else if power_watts < THRESHOLD_MEDIUM_WATTS {
        "medium"
    } else {
        "high"
    }
}

// --- macOS native telemetry --------------------------------------------------

#[cfg(target_os = "macos")]
mod native {
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
            pub fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef)
                -> CFDictionaryRef;
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
            let on_battery =
                !kind.is_null() && CFString::wrap_under_get_rule(kind) == "Battery Power";

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

                    let cur_ptr =
                        CFDictionaryGetValue(desc, cur_cap_key.as_concrete_TypeRef() as _);
                    let max_ptr =
                        CFDictionaryGetValue(desc, max_cap_key.as_concrete_TypeRef() as _);

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
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;

    pub fn read_proc_rusage(_pid: i32) -> Option<ProcessRusage> {
        Some(ProcessRusage::default())
    }

    pub fn read_battery_info() -> BatteryInfo {
        BatteryInfo {
            on_battery: false,
            battery_percent: None,
        }
    }
}

pub fn read_process_rusage(pid: i32) -> Option<ProcessRusage> {
    native::read_proc_rusage(pid)
}

pub fn read_battery_info() -> BatteryInfo {
    native::read_battery_info()
}

// --- Background energy sampler -----------------------------------------------

pub fn spawn_sampler(app: AppHandle, db_path: PathBuf) {
    let running = Arc::new(AtomicBool::new(true));
    let handle = app.clone();

    std::thread::Builder::new()
        .name("energy-sampler".to_string())
        .spawn(move || {
            let conn = match Connection::open(&db_path) {
                Ok(conn) => {
                    let _ = conn.busy_timeout(Duration::from_secs(5));
                    conn
                }
                Err(error) => {
                    eprintln!("energy sampler could not open db: {error}");
                    return;
                }
            };

            run_sampler_loop(handle, conn, running);
        })
        .expect("spawn energy sampler");
}

fn run_sampler_loop(app: AppHandle, conn: Connection, running: Arc<AtomicBool>) {
    let main_pid = std::process::id() as i32;

    let mut last_sample_time = Instant::now();
    let mut last_total_cpu_ns = read_total_cpu_ns(&app, main_pid);
    let mut last_total_energy_nj = read_total_energy_nj(&app, main_pid);

    while running.load(Ordering::Relaxed) {
        std::thread::sleep(SAMPLE_INTERVAL);

        let now_instant = Instant::now();
        let elapsed = now_instant.duration_since(last_sample_time);
        last_sample_time = now_instant;

        let cur_cpu_ns = read_total_cpu_ns(&app, main_pid);
        let cur_energy_nj = read_total_energy_nj(&app, main_pid);

        let delta_cpu_ns = cur_cpu_ns.saturating_sub(last_total_cpu_ns);
        let mut delta_energy_nj = cur_energy_nj.saturating_sub(last_total_energy_nj);

        last_total_cpu_ns = cur_cpu_ns;
        last_total_energy_nj = cur_energy_nj;

        let delta_time_s = elapsed.as_secs_f64().max(0.1);
        let cpu_seconds = delta_cpu_ns as f64 / 1_000_000_000.0;

        // If hardware energy counters are not available or returned zero,
        // estimate energy from active CPU time (~3.0 Watts per core-second).
        if delta_energy_nj == 0 {
            let estimated_joules = cpu_seconds * 3.0;
            delta_energy_nj = (estimated_joules * 1_000_000_000.0) as u64;
        }

        let power_watts = (delta_energy_nj as f64 / 1_000_000_000.0) / delta_time_s;
        let impact = impact_level_for_power(power_watts);

        let ai_runtime = app.try_state::<AiRuntime>();
        let ai_active = ai_runtime.map(|rt| rt.is_busy()).unwrap_or(false);

        let battery = read_battery_info();
        let now_ms = now_epoch_ms();
        let duration_ms = (delta_time_s * 1000.0) as u32;
        let cpu_time_ms = (cpu_seconds * 1000.0) as u32;

        let insert_res = conn.execute(
            "INSERT INTO energy_samples (
                sampled_at, duration_ms, cpu_time_ms, energy_nj, power_watts,
                impact_level, ai_active, on_battery, battery_level
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9);",
            params![
                now_ms as i64,
                duration_ms as i64,
                cpu_time_ms as i64,
                delta_energy_nj as i64,
                power_watts,
                impact,
                if ai_active { 1 } else { 0 },
                if battery.on_battery { 1 } else { 0 },
                battery.battery_percent,
            ],
        );

        if let Err(error) = insert_res {
            eprintln!("failed to persist energy sample: {error}");
            continue;
        }

        if let Ok(summary) = get_summary(
            &conn,
            DEFAULT_HISTORY_DAYS,
            Some(battery),
            Some(power_watts),
        ) {
            let _ = app.emit(EVENT_ENERGY_CHANGED, &summary);
        }
    }
}

fn read_total_cpu_ns(app: &AppHandle, main_pid: i32) -> u64 {
    let main_rusage = read_process_rusage(main_pid).unwrap_or_default();
    let sidecar_pid = app.try_state::<AiRuntime>().and_then(|rt| rt.sidecar_pid());

    let sidecar_rusage = sidecar_pid
        .and_then(|pid| read_process_rusage(pid as i32))
        .unwrap_or_default();

    main_rusage.cpu_time_ns + sidecar_rusage.cpu_time_ns
}

fn read_total_energy_nj(app: &AppHandle, main_pid: i32) -> u64 {
    let main_rusage = read_process_rusage(main_pid).unwrap_or_default();
    let sidecar_pid = app.try_state::<AiRuntime>().and_then(|rt| rt.sidecar_pid());

    let sidecar_rusage = sidecar_pid
        .and_then(|pid| read_process_rusage(pid as i32))
        .unwrap_or_default();

    main_rusage.billed_energy_nj + sidecar_rusage.billed_energy_nj
}

// --- Query and summary helpers -----------------------------------------------

pub fn get_summary(
    conn: &Connection,
    days: u32,
    battery: Option<BatteryInfo>,
    latest_power_override: Option<f64>,
) -> Result<EnergySummary, String> {
    let days = days.clamp(1, 365);
    let now = now_epoch_ms();
    let since = now.saturating_sub(u64::from(days) * 24 * 60 * 60 * 1000);

    let mut stmt = conn
        .prepare(
            "SELECT id, sampled_at, duration_ms, cpu_time_ms, energy_nj, power_watts,
                    impact_level, ai_active, on_battery, battery_level
             FROM energy_samples
             WHERE sampled_at >= ?1
             ORDER BY sampled_at ASC;",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![since as i64], |row| {
            Ok(EnergySample {
                id: row.get(0)?,
                sampled_at: row.get::<_, i64>(1)? as u64,
                duration_ms: row.get::<_, i64>(2)? as u32,
                cpu_time_ms: row.get::<_, i64>(3)? as u32,
                energy_nj: row.get::<_, i64>(4)? as u64,
                power_watts: row.get(5)?,
                impact_level: row.get(6)?,
                ai_active: row.get::<_, i64>(7)? != 0,
                on_battery: row.get::<_, i64>(8)? != 0,
                battery_level: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let samples: Vec<EnergySample> = rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?;

    let battery_info = battery.unwrap_or_else(read_battery_info);

    let samples_count = samples.len() as u32;
    if samples.is_empty() {
        let current_power = latest_power_override.unwrap_or(0.0);
        let impact = impact_level_for_power(current_power);
        return Ok(EnergySummary {
            current_impact: impact.to_string(),
            current_power_watts: current_power,
            baseline_power_watts: 0.05,
            peak_power_watts: current_power,
            impact_description: "Measuring energy impact…".to_string(),
            battery_used_pct: 0.0,
            battery_used_mwh: 0.0,
            total_energy_joules: 0.0,
            total_cpu_time_ms: 0,
            ai_energy_pct: 0.0,
            on_battery: battery_info.on_battery,
            current_battery_pct: battery_info.battery_percent,
            samples_count: 0,
            window_days: days,
        });
    }

    let mut total_energy_nj: u64 = 0;
    let mut ai_energy_nj: u64 = 0;
    let mut total_cpu_time_ms: u64 = 0;
    let mut peak_power: f64 = 0.0;

    let mut idle_power_sum: f64 = 0.0;
    let mut idle_power_count: u32 = 0;

    for s in &samples {
        total_energy_nj += s.energy_nj;
        total_cpu_time_ms += u64::from(s.cpu_time_ms);
        if s.power_watts > peak_power {
            peak_power = s.power_watts;
        }
        if s.ai_active {
            ai_energy_nj += s.energy_nj;
        } else {
            idle_power_sum += s.power_watts;
            idle_power_count += 1;
        }
    }

    let baseline_power = if idle_power_count > 0 {
        idle_power_sum / f64::from(idle_power_count)
    } else {
        0.05
    };

    let latest_sample = samples.last().unwrap();
    let current_power = latest_power_override.unwrap_or(latest_sample.power_watts);
    let current_impact = impact_level_for_power(current_power);

    let total_joules = total_energy_nj as f64 / 1_000_000_000.0;
    let battery_used_mwh = total_joules / JOULES_PER_MWH;
    let battery_used_pct = ((total_joules / NOMINAL_BATTERY_JOULES) * 100.0).clamp(0.0, 100.0);

    let ai_energy_pct = if total_energy_nj > 0 {
        ((ai_energy_nj as f64 / total_energy_nj as f64) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    let ratio = if baseline_power > 0.001 {
        current_power / baseline_power
    } else {
        1.0
    };

    let impact_description = match current_impact {
        "low" => format!(
            "Low ({:.2} W) · {:.1}x baseline ({:.2} W) · Ambient tracking",
            current_power, ratio, baseline_power
        ),
        "medium" => format!(
            "Medium ({:.2} W) · {:.1}x baseline ({:.2} W) · Normal activity & UI",
            current_power, ratio, baseline_power
        ),
        _ => format!(
            "High ({:.2} W) · {:.1}x baseline ({:.2} W) · On-device AI active",
            current_power, ratio, baseline_power
        ),
    };

    Ok(EnergySummary {
        current_impact: current_impact.to_string(),
        current_power_watts: (current_power * 100.0).round() / 100.0,
        baseline_power_watts: (baseline_power * 100.0).round() / 100.0,
        peak_power_watts: (peak_power * 100.0).round() / 100.0,
        impact_description,
        battery_used_pct: (battery_used_pct * 100.0).round() / 100.0,
        battery_used_mwh: (battery_used_mwh * 10.0).round() / 10.0,
        total_energy_joules: (total_joules * 10.0).round() / 10.0,
        total_cpu_time_ms,
        ai_energy_pct: (ai_energy_pct * 10.0).round() / 10.0,
        on_battery: battery_info.on_battery,
        current_battery_pct: battery_info.battery_percent,
        samples_count,
        window_days: days,
    })
}

pub fn query_history(
    conn: &Connection,
    since_ms: Option<u64>,
    limit: Option<u32>,
) -> Result<Vec<EnergySample>, String> {
    let since = since_ms.unwrap_or(0);
    let limit = limit.unwrap_or(500).clamp(1, 5000);

    let mut stmt = conn
        .prepare(
            "SELECT id, sampled_at, duration_ms, cpu_time_ms, energy_nj, power_watts,
                    impact_level, ai_active, on_battery, battery_level
             FROM energy_samples
             WHERE sampled_at >= ?1
             ORDER BY sampled_at DESC
             LIMIT ?2;",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![since as i64, limit as i64], |row| {
            Ok(EnergySample {
                id: row.get(0)?,
                sampled_at: row.get::<_, i64>(1)? as u64,
                duration_ms: row.get::<_, i64>(2)? as u32,
                cpu_time_ms: row.get::<_, i64>(3)? as u32,
                energy_nj: row.get::<_, i64>(4)? as u64,
                power_watts: row.get(5)?,
                impact_level: row.get(6)?,
                ai_active: row.get::<_, i64>(7)? != 0,
                on_battery: row.get::<_, i64>(8)? != 0,
                battery_level: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut samples: Vec<EnergySample> =
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    samples.reverse();
    Ok(samples)
}

pub fn purge_older_than(conn: &Connection, days: u32, now_ms: u64) -> Result<usize, String> {
    if days == 0 {
        return Ok(0);
    }
    let cutoff = now_ms.saturating_sub(u64::from(days) * 24 * 60 * 60 * 1000);
    conn.execute(
        "DELETE FROM energy_samples WHERE sampled_at < ?1;",
        params![cutoff as i64],
    )
    .map_err(|e| e.to_string())
}

pub fn reset_history(conn: &Connection) -> Result<(), String> {
    conn.execute("DELETE FROM energy_samples;", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_level_thresholds() {
        assert_eq!(impact_level_for_power(0.1), "low");
        assert_eq!(impact_level_for_power(0.49), "low");
        assert_eq!(impact_level_for_power(0.50), "medium");
        assert_eq!(impact_level_for_power(1.99), "medium");
        assert_eq!(impact_level_for_power(2.0), "high");
        assert_eq!(impact_level_for_power(5.5), "high");
    }

    #[test]
    fn summary_computation_on_samples() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::run_migrations(&mut conn).unwrap();

        let now = now_epoch_ms();
        conn.execute(
            "INSERT INTO energy_samples (
                sampled_at, duration_ms, cpu_time_ms, energy_nj, power_watts,
                impact_level, ai_active, on_battery, battery_level
             ) VALUES (?1, 15000, 150, 1500000000, 0.1, 'low', 0, 1, 85.0),
                      (?2, 15000, 3000, 30000000000, 2.0, 'high', 1, 1, 84.5);",
            params![now as i64 - 30_000, now as i64 - 15_000],
        )
        .unwrap();

        let summary = get_summary(&conn, 7, None, None).unwrap();
        assert_eq!(summary.samples_count, 2);
        assert_eq!(summary.peak_power_watts, 2.0);
        assert!(summary.ai_energy_pct > 90.0);
        assert!(summary.battery_used_mwh > 0.0);
    }
}
