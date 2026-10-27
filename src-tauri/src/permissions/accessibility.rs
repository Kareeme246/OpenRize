//! What the running process knows about its own Accessibility access, and
//! when it has to restart to get it.
//!
//! macOS answers "is this app trusted" from a flag that can read true while the
//! Accessibility calls in the same process still fail: a grant made after the
//! process started, or an entry that was removed and re-added in System
//! Settings, are the known ways. The trust flag alone therefore decides
//! nothing here. Each poll pairs it with a real probe call, and
//! [`Tracker`] turns that stream of readings into one state. Only a grant seen
//! happen in this run, followed by a probe that keeps failing, asks for a
//! relaunch; a process that starts with the flag set and a failing probe says
//! "allowed but not working" and never asks, which is what keeps a relaunch
//! from looping.
//!
//! The OS calls live in `macos.rs`; everything decided here is plain data in,
//! plain data out, so it is tested without a Mac.

// The readings are only produced on macOS; other platforms compile the types.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// How often the trust flag and probe are read.
pub const POLL: Duration = Duration::from_secs(2);

/// Consecutive failing readings, with the flag set, before the failure is
/// believed. At the poll interval this is a few seconds, long enough for a
/// grant that macOS is still applying to take effect and for one busy moment
/// in the system's Accessibility service to pass.
pub const SETTLE_READINGS: u32 = 3;

/// `kAXErrorNoValue`: the service answered, there was just nothing focused.
const AX_ERROR_NO_VALUE: i32 = -25212;

/// What one call to the Accessibility service came back with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Works,
    Fails(i32),
}

impl Probe {
    /// An `AXError` from asking the system-wide element for the focused app.
    /// "No value" is a working service with nothing focused at that moment.
    pub fn from_ax_error(code: i32) -> Self {
        match code {
            0 | AX_ERROR_NO_VALUE => Probe::Works,
            other => Probe::Fails(other),
        }
    }
}

/// One poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// The trust flag is off.
    Untrusted,
    /// The flag is on; the probe says whether it is true in this process.
    Trusted(Probe),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessibilityState {
    /// This platform asks for no Accessibility access.
    NotApplicable,
    /// Not allowed in System Settings.
    NotGranted,
    /// Allowed, and a probe has failed but not for long enough to believe.
    Checking,
    /// Allowed and the probe works: titles are being read.
    Working,
    /// Allowed, but the probe keeps failing, in a process that started that
    /// way. Relaunching has not been tried, so it does not help to ask again.
    NotWorking,
    /// Allowed while this process was running, and the probe still fails: only
    /// a relaunch gets the access.
    RelaunchRequired,
}

#[derive(Debug)]
pub struct Tracker {
    state: AccessibilityState,
    /// The flag has been off at some reading in this run, so any later grant
    /// is one this process watched happen.
    untrusted_this_run: bool,
    failing: u32,
}

impl Tracker {
    pub fn new() -> Self {
        Self {
            state: AccessibilityState::NotApplicable,
            untrusted_this_run: false,
            failing: 0,
        }
    }

    pub fn state(&self) -> AccessibilityState {
        self.state
    }

    /// Folds in one reading. Returns the new state when it changed.
    pub fn observe(&mut self, reading: Reading) -> Option<AccessibilityState> {
        let next = match reading {
            Reading::Untrusted => {
                self.untrusted_this_run = true;
                self.failing = 0;
                AccessibilityState::NotGranted
            }
            Reading::Trusted(Probe::Works) => {
                self.failing = 0;
                AccessibilityState::Working
            }
            Reading::Trusted(Probe::Fails(_)) => {
                self.failing = self.failing.saturating_add(1);
                if self.failing < SETTLE_READINGS {
                    // One bad moment does not undo access that was working.
                    match self.state {
                        AccessibilityState::Working => AccessibilityState::Working,
                        _ => AccessibilityState::Checking,
                    }
                } else if self.untrusted_this_run {
                    AccessibilityState::RelaunchRequired
                } else {
                    AccessibilityState::NotWorking
                }
            }
        };
        (next != self.state).then(|| {
            self.state = next;
            next
        })
    }
}

/// The tracker as app state, with the event and window behaviour that goes
/// with a change.
pub struct Monitor {
    tracker: Mutex<Tracker>,
}

impl Monitor {
    pub fn new() -> Self {
        Self {
            tracker: Mutex::new(Tracker::new()),
        }
    }

    pub fn state(&self) -> AccessibilityState {
        self.tracker
            .lock()
            .map_or(AccessibilityState::NotApplicable, |tracker| tracker.state())
    }

    fn apply(&self, app: &AppHandle, reading: Reading) {
        let Some(state) = self
            .tracker
            .lock()
            .ok()
            .and_then(|mut tracker| tracker.observe(reading))
        else {
            return;
        };
        eprintln!("accessibility: {state:?} ({reading:?})");
        let _ = app.emit(crate::EVENT_PERMISSIONS_CHANGED, state);
        if state == AccessibilityState::RelaunchRequired {
            // The relaunch prompt is in the main window; a grant made with
            // that window closed must bring it forward.
            crate::tray::show_main_window(app);
        }
    }
}

/// Reads once now, so the state is known before the window asks, then keeps
/// reading for as long as the app runs. A revoke and a later re-grant are
/// both worth seeing.
pub fn spawn_monitor(app: AppHandle) {
    let Some(first) = super::platform::accessibility_reading() else {
        return;
    };
    app.state::<Monitor>().apply(&app, first);
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL);
        if let Some(reading) = super::platform::accessibility_reading() {
            app.state::<Monitor>().apply(&app, reading);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROKEN: Reading = Reading::Trusted(Probe::Fails(-25211));
    const FINE: Reading = Reading::Trusted(Probe::Works);

    /// The state after each reading, so a test reads as the story it tells.
    fn run(readings: &[Reading]) -> Vec<AccessibilityState> {
        let mut tracker = Tracker::new();
        readings
            .iter()
            .map(|reading| {
                tracker.observe(*reading);
                tracker.state()
            })
            .collect()
    }

    use AccessibilityState::{Checking, NotGranted, NotWorking, RelaunchRequired, Working};

    #[test]
    fn the_probe_reads_no_value_as_a_working_service() {
        assert_eq!(Probe::from_ax_error(0), Probe::Works);
        assert_eq!(Probe::from_ax_error(-25212), Probe::Works);
        assert_eq!(Probe::from_ax_error(-25211), Probe::Fails(-25211));
        assert_eq!(Probe::from_ax_error(-25204), Probe::Fails(-25204));
    }

    #[test]
    fn a_launch_with_access_working_never_asks_for_anything() {
        assert_eq!(run(&[FINE, FINE, FINE]), vec![Working, Working, Working]);
    }

    #[test]
    fn a_launch_without_access_says_so() {
        assert_eq!(
            run(&[Reading::Untrusted, Reading::Untrusted]),
            vec![NotGranted, NotGranted]
        );
    }

    #[test]
    fn a_grant_that_works_at_once_never_asks_for_a_relaunch() {
        let states = run(&[Reading::Untrusted, FINE, FINE]);
        assert_eq!(states, vec![NotGranted, Working, Working]);
        assert!(!states.contains(&RelaunchRequired));
    }

    #[test]
    fn a_grant_that_starts_working_within_the_settling_time_never_asks() {
        let states = run(&[Reading::Untrusted, BROKEN, BROKEN, FINE]);
        assert_eq!(states, vec![NotGranted, Checking, Checking, Working]);
    }

    #[test]
    fn a_grant_that_keeps_failing_asks_for_a_relaunch_after_the_settling_time() {
        let states = run(&[Reading::Untrusted, BROKEN, BROKEN, BROKEN, BROKEN]);
        assert_eq!(
            states,
            vec![
                NotGranted,
                Checking,
                Checking,
                RelaunchRequired,
                RelaunchRequired
            ]
        );
    }

    #[test]
    fn a_relaunch_does_not_loop() {
        // The relaunched process starts with the flag set. Whatever the probe
        // says, it never saw an ungranted reading, so it never asks again.
        let readings = vec![BROKEN; 50];
        let states = run(&readings);
        assert_eq!(states[0], Checking);
        assert!(states[SETTLE_READINGS as usize - 1..]
            .iter()
            .all(|state| *state == NotWorking));
        assert!(!states.contains(&RelaunchRequired));
    }

    #[test]
    fn a_relaunch_that_worked_stays_quiet() {
        assert_eq!(run(&[FINE; 10]).last(), Some(&Working));
    }

    #[test]
    fn one_bad_reading_does_not_undo_access_that_was_working() {
        assert_eq!(run(&[FINE, BROKEN, FINE]), vec![Working, Working, Working]);
    }

    #[test]
    fn working_access_that_keeps_failing_is_not_working_but_never_asks() {
        let states = run(&[FINE, BROKEN, BROKEN, BROKEN]);
        assert_eq!(states, vec![Working, Working, Working, NotWorking]);
    }

    #[test]
    fn removing_and_re_adding_the_entry_asks_for_a_relaunch() {
        // Trusted at launch with a failing probe, then the user removes the
        // entry and adds it again: a grant this process watched happen.
        let states = run(&[
            BROKEN,
            BROKEN,
            BROKEN,
            Reading::Untrusted,
            BROKEN,
            BROKEN,
            BROKEN,
        ]);
        assert_eq!(
            states,
            vec![
                Checking,
                Checking,
                NotWorking,
                NotGranted,
                Checking,
                Checking,
                RelaunchRequired
            ]
        );
    }

    #[test]
    fn revoking_while_a_relaunch_is_asked_for_withdraws_the_request() {
        let states = run(&[
            Reading::Untrusted,
            BROKEN,
            BROKEN,
            BROKEN,
            Reading::Untrusted,
        ]);
        assert_eq!(states.last(), Some(&NotGranted));
    }

    #[test]
    fn a_probe_that_starts_working_withdraws_the_request_too() {
        let states = run(&[Reading::Untrusted, BROKEN, BROKEN, BROKEN, FINE]);
        assert_eq!(states.last(), Some(&Working));
    }

    #[test]
    fn only_changes_are_reported() {
        let mut tracker = Tracker::new();
        assert_eq!(tracker.observe(FINE), Some(Working));
        assert_eq!(tracker.observe(FINE), None);
        assert_eq!(tracker.observe(Reading::Untrusted), Some(NotGranted));
        assert_eq!(tracker.observe(Reading::Untrusted), None);
    }

    #[test]
    fn states_cross_to_the_frontend_in_camel_case() {
        assert_eq!(
            serde_json::to_string(&RelaunchRequired).unwrap(),
            "\"relaunchRequired\""
        );
        assert_eq!(
            serde_json::to_string(&NotWorking).unwrap(),
            "\"notWorking\""
        );
    }
}
