//! When background retraining is allowed: idle, on AC power, and not in battery
//! saver, so learning never costs battery or a stutter. "Retrain now" in
//! Settings skips this gate.

use std::time::Duration;

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
            // A machine without a battery reports no battery power, so it counts as on AC.
            on_ac: !crate::energy::read_battery_info().on_battery,
            low_power: crate::energy::low_power_mode(),
        }
    }

    pub fn blocker(&self) -> Option<&'static str> {
        if self.low_power {
            Some("waiting for power saving to turn off")
        } else if !self.on_ac {
            Some("waiting for power")
        } else if self.idle < MIN_IDLE {
            Some("waiting for the computer to be idle")
        } else {
            None
        }
    }
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
            .contains("saving"));
    }

    #[test]
    fn reading_the_real_conditions_does_not_panic() {
        let _ = Conditions::read();
    }
}
