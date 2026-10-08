//! Long-running stopwatch reminders. Discovery runs once a minute; pending
//! cards follow the break panel's quiet rules and yield to every break state.
//! A run is identified by its timer id and start, never accumulated time.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::settings::StopwatchReminder;
use crate::timers::Timer;

use super::engine::{Quiet, DEFER_DROP_MS};

const MINUTE_MS: u64 = 60_000;
type Run = (String, u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderView {
    pub id: String,
    pub label: String,
    pub started_at: u64,
}

impl ReminderView {
    fn run(&self) -> Run {
        (self.id.clone(), self.started_at)
    }
}

#[derive(Default)]
pub(super) struct Engine {
    last_check: Option<u64>,
    pub(super) fired: HashSet<Run>,
    pending: Vec<ReminderView>,
    current: Option<ReminderView>,
    deferred_since: HashMap<Run, u64>,
    pub view: Option<ReminderView>,
}

impl Engine {
    pub fn step(
        &mut self,
        now: u64,
        settings: &StopwatchReminder,
        timers: &[Timer],
        quiet: Option<Quiet>,
        break_visible: bool,
    ) {
        // Reconcile controls immediately even between the minute checks: a
        // paused, reset or deleted run must not leave a stale card behind.
        let running: HashMap<Run, &Timer> = timers
            .iter()
            .filter_map(|timer| {
                timer
                    .started_at
                    .map(|start| ((timer.id.clone(), start), timer))
            })
            .collect();
        self.fired.retain(|run| running.contains_key(run));
        self.deferred_since
            .retain(|run, _| running.contains_key(run));
        self.pending
            .retain(|card| running.contains_key(&card.run()));
        if self
            .current
            .as_ref()
            .is_some_and(|card| !running.contains_key(&card.run()))
        {
            self.current = None;
        }
        if !settings.enabled {
            self.pending.clear();
            self.current = None;
            self.deferred_since.clear();
            self.view = None;
            return;
        }

        let threshold = u64::from(settings.after_minutes) * MINUTE_MS;
        if self
            .last_check
            .is_none_or(|last| now.saturating_sub(last) >= MINUTE_MS)
        {
            self.last_check = Some(now);
            self.pending = timers
                .iter()
                .filter_map(|timer| {
                    let started_at = timer.started_at?;
                    let run = (timer.id.clone(), started_at);
                    (now.saturating_sub(started_at) >= threshold && !self.fired.contains(&run))
                        .then(|| ReminderView {
                            id: timer.id.clone(),
                            label: timer.label.clone(),
                            started_at,
                        })
                })
                .collect();
        }
        self.pending
            .retain(|card| now.saturating_sub(card.started_at) >= threshold);
        for card in self.pending.iter_mut().chain(self.current.iter_mut()) {
            if let Some(timer) = running.get(&card.run()) {
                card.label.clone_from(&timer.label);
            }
        }

        if quiet == Some(Quiet::DisplayAwake) {
            self.pending.retain(|card| {
                let run = card.run();
                let since = *self.deferred_since.entry(run.clone()).or_insert(now);
                if now.saturating_sub(since) >= DEFER_DROP_MS {
                    self.fired.insert(run);
                    false
                } else {
                    true
                }
            });
        } else {
            self.deferred_since.clear();
        }

        if quiet.is_some() || break_visible {
            self.view = None;
            return;
        }
        if self.current.is_none() && !self.pending.is_empty() {
            let card = self.pending.remove(0);
            self.fired.insert(card.run());
            self.current = Some(card);
        }
        self.view.clone_from(&self.current);
    }

    pub fn dismiss(&mut self, id: &str, started_at: u64) {
        if self
            .current
            .as_ref()
            .is_some_and(|card| card.id == id && card.started_at == started_at)
        {
            self.current = None;
            self.view = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer(started_at: Option<u64>) -> Timer {
        Timer {
            id: "acme".into(),
            label: "Acme site".into(),
            accumulated_ms: 20 * 3_600_000,
            started_at,
            created_at: 0,
        }
    }

    #[test]
    fn current_run_triggers_once_on_a_minute_check_and_resume_can_trigger_again() {
        let settings = StopwatchReminder::default();
        let threshold = u64::from(settings.after_minutes) * MINUTE_MS;
        let mut engine = Engine::default();
        engine.step(threshold - 1, &settings, &[timer(Some(0))], None, false);
        assert!(engine.view.is_none());
        engine.step(threshold, &settings, &[timer(Some(0))], None, false);
        assert!(engine.view.is_none());
        engine.step(
            threshold + MINUTE_MS,
            &settings,
            &[timer(Some(0))],
            None,
            false,
        );
        assert_eq!(engine.view.as_ref().unwrap().started_at, 0);
        engine.dismiss("acme", 0);
        engine.step(
            threshold + 2 * MINUTE_MS,
            &settings,
            &[timer(Some(0))],
            None,
            false,
        );
        assert!(engine.view.is_none());
        engine.step(
            threshold + 2 * MINUTE_MS + 1,
            &settings,
            &[timer(None)],
            None,
            false,
        );
        assert!(engine.view.is_none());
        let resumed = threshold + 3 * MINUTE_MS;
        engine.step(resumed, &settings, &[timer(Some(resumed))], None, false);
        assert!(engine.view.is_none());
        engine.step(
            resumed + threshold,
            &settings,
            &[timer(Some(resumed))],
            None,
            false,
        );
        assert_eq!(engine.view.as_ref().unwrap().started_at, resumed);
    }

    #[test]
    fn quiet_rules_and_break_priority_defer_without_consuming_the_reminder() {
        let settings = StopwatchReminder::default();
        for quiet in [
            Quiet::NotTracking,
            Quiet::Away,
            Quiet::Paused,
            Quiet::DisplayAwake,
        ] {
            let mut engine = Engine::default();
            engine.step(
                3_600_000 * 3,
                &settings,
                &[timer(Some(0))],
                Some(quiet),
                false,
            );
            assert!(engine.view.is_none());
            engine.step(3_600_000 * 3 + 1, &settings, &[timer(Some(0))], None, true);
            assert!(engine.view.is_none());
            engine.step(3_600_000 * 3 + 2, &settings, &[timer(Some(0))], None, false);
            assert!(engine.view.is_some());
        }
    }

    #[test]
    fn video_deferred_over_thirty_minutes_is_dropped_for_the_run() {
        let mut engine = Engine::default();
        let settings = StopwatchReminder::default();
        let now = 3 * 3_600_000;
        engine.step(
            now,
            &settings,
            &[timer(Some(0))],
            Some(Quiet::DisplayAwake),
            false,
        );
        engine.step(
            now + DEFER_DROP_MS,
            &settings,
            &[timer(Some(0))],
            Some(Quiet::DisplayAwake),
            false,
        );
        engine.step(
            now + DEFER_DROP_MS + MINUTE_MS,
            &settings,
            &[timer(Some(0))],
            None,
            false,
        );
        assert!(engine.view.is_none());
    }

    #[test]
    fn concurrent_runs_queue_and_stale_dismissals_leave_the_current_card_alone() {
        let mut engine = Engine::default();
        let settings = StopwatchReminder::default();
        let now = 3 * 3_600_000;
        let mut other = timer(Some(0));
        other.id = "other".into();
        let timers = [timer(Some(0)), other];
        engine.step(now, &settings, &timers, None, false);
        assert_eq!(engine.view.as_ref().unwrap().id, "acme");
        engine.dismiss("acme", 1);
        engine.step(now + 1, &settings, &timers, None, false);
        assert_eq!(engine.view.as_ref().unwrap().id, "acme");
        engine.step(now + 2, &settings, &timers, Some(Quiet::Away), false);
        assert!(engine.view.is_none());
        engine.step(now + 3, &settings, &timers, None, false);
        assert_eq!(engine.view.as_ref().unwrap().id, "acme");
        engine.dismiss("acme", 0);
        engine.step(now + 4, &settings, &timers, None, false);
        assert_eq!(engine.view.as_ref().unwrap().id, "other");
        engine.dismiss("other", 0);
        engine.step(now + MINUTE_MS, &settings, &timers, None, false);
        assert!(engine.view.is_none());
    }

    #[test]
    fn saved_firings_suppress_the_same_run_after_restart_but_allow_a_new_run() {
        let mut engine = Engine::default();
        let settings = StopwatchReminder::default();
        let now = 3 * 3_600_000;
        engine.step(now, &settings, &[timer(Some(0))], None, false);
        let saved = serde_json::to_string(&engine.fired).unwrap();
        let mut restarted = Engine {
            fired: serde_json::from_str(&saved).unwrap(),
            ..Engine::default()
        };
        restarted.step(now + MINUTE_MS, &settings, &[timer(Some(0))], None, false);
        assert!(restarted.view.is_none());
        restarted.step(
            now + 2 * MINUTE_MS,
            &settings,
            &[timer(Some(MINUTE_MS))],
            None,
            false,
        );
        assert_eq!(restarted.view.as_ref().unwrap().started_at, MINUTE_MS);
        assert_eq!(restarted.fired.len(), 1);
    }

    #[test]
    fn pausing_resetting_deleting_and_disabling_clear_the_card() {
        for timers in [vec![timer(None)], vec![], vec![timer(Some(3 * 3_600_000))]] {
            let settings = StopwatchReminder::default();
            let mut engine = Engine::default();
            engine.step(3 * 3_600_000, &settings, &[timer(Some(0))], None, false);
            assert!(engine.view.is_some());
            engine.step(3 * 3_600_000 + 1, &settings, &timers, None, false);
            assert!(engine.view.is_none());
        }
        let mut engine = Engine::default();
        let mut settings = StopwatchReminder::default();
        engine.step(3 * 3_600_000, &settings, &[timer(Some(0))], None, false);
        settings.enabled = false;
        engine.step(3 * 3_600_000 + 1, &settings, &[timer(Some(0))], None, false);
        assert!(engine.view.is_none());
    }
}
