//! The break state machine: a pure function of time, settings and what the
//! activity sampler sees. It performs no I/O; `step` and the command methods
//! change its state and return `Effect`s for the driver in `mod.rs` to apply.
//!
//! Vocabulary:
//! - The *work clock* (`streak_ms`) counts continuous work. It grows while the
//!   user is tracked and active, and resets when a qualifying rest ends: any
//!   taken break, an idle stretch at least `break_minutes` long, or a gap (sleep,
//!   time outside tracking hours) at least that long.
//! - A *reminder* is a break the engine wants the user to take. It is either an
//!   interval reminder (the work clock reached `work_minutes`) or a scheduled
//!   break (the clock reached its time). Only one is pending at a time.
//! - A *break* is a taken reminder or a manual break. It owns a `break`
//!   activity segment; this module never touches segments, it asks for them.

use std::collections::HashSet;

use serde::Serialize;

use crate::settings::{BreakSettings, ScheduledBreak};

/// A visible reminder that gets no answer collapses to the corner capsule.
pub const NUDGE_AFTER_MS: u64 = 20_000;
/// How long the "welcome back" card stays after a break ends.
pub const WELCOME_MS: u64 = 5_000;
/// After this many snoozes in a row the reminder offers only Start and Skip.
pub const MAX_SNOOZES_IN_ROW: u32 = 3;
/// An interval reminder due this close before a scheduled break gives way.
pub const SCHEDULE_LEAD_MS: u64 = 15 * 60_000;
/// A reminder held back by a playing video this long is dropped as missed.
pub const DEFER_DROP_MS: u64 = 30 * 60_000;
/// Input this recent counts as the user being at the keyboard mid-break.
pub const BACK_INPUT_MS: u64 = 30_000;
/// Input this steady during a break means the user is back.
pub const SUSTAINED_INPUT_MS: u64 = 120_000;
/// Right after Start break the user is still at the keyboard; the sustained
/// input rule ignores that first moment.
const START_GRACE_MS: u64 = 15_000;
/// Fresh input this recent after the planned end is "the first input".
const FRESH_INPUT_MS: u64 = 1_500;
/// The sampler ticks each second; a longer gap means the Mac slept.
const TICK_GAP_MS: u64 = 10_000;
/// A scheduled break not shown within this long after its time is missed.
const SCHEDULE_WINDOW_MIN: u32 = 10;
const MINUTE_MS: u64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Interval,
    Scheduled,
    Manual,
    Idle,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Interval => "interval",
            Source::Scheduled => "scheduled",
            Source::Manual => "manual",
            Source::Idle => "idle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Due,
    Nudge,
    OnBreak,
    Over,
}

/// One row of the `breaks` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakRecord {
    pub id: String,
    pub source: Source,
    pub schedule_id: Option<String>,
    pub due_at: Option<u64>,
    pub planned_ms: u64,
    pub status: &'static str,
    pub snoozes: u32,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub segment_id: Option<i64>,
}

pub const STATUS_TAKEN: &str = "taken";
pub const STATUS_SKIPPED: &str = "skipped";
pub const STATUS_MISSED: &str = "missed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Start a `break` segment and record the taken break. `pause_timers`
    /// asks the driver to pause running stopwatches.
    Begin {
        record: BreakRecord,
        label: String,
        pause_timers: bool,
    },
    /// The break ended: close its segment (if still open), stamp the row and
    /// resume the stopwatches the break paused.
    Finish {
        id: String,
        ended_at: u64,
        close_segment: bool,
        resume_timers: Vec<String>,
    },
    /// Store a decision that involves no segment of its own.
    Record(BreakRecord),
    Extend {
        id: String,
        planned_ms: u64,
    },
    Chime,
}

#[derive(Debug, Clone)]
pub struct SegmentNow {
    pub id: i64,
    pub kind: String,
    pub label: Option<String>,
}

impl SegmentNow {
    fn is_idle_break(&self) -> bool {
        self.kind == crate::activity::KIND_BREAK
            && self.label.as_deref() == Some(crate::activity::IDLE_LABEL)
    }
}

/// Everything one tick reads from the outside world.
pub struct Input<'a> {
    pub now: u64,
    pub settings: &'a BreakSettings,
    pub tracking_active: bool,
    pub idle_ms: u64,
    pub idle_threshold_ms: u64,
    /// Locked screen, switched session or every display asleep.
    pub away: bool,
    /// The frontmost app is keeping the display awake (video, a presentation).
    pub keeps_display_awake: bool,
    pub segment: Option<SegmentNow>,
    pub local: LocalTime,
}

/// Where `now` falls on the local calendar.
#[derive(Debug, Clone, Copy)]
pub struct LocalTime {
    /// Local calendar day, as days from the common era.
    pub day_key: i64,
    pub minutes_of_day: u32,
    pub weekday: chrono::Weekday,
}

#[derive(Debug, Clone)]
struct Reminder {
    source: Source,
    schedule_id: Option<String>,
    label: String,
    planned_ms: u64,
    due_at: u64,
    /// Time worked when it came due (interval reminders).
    worked_ms: u64,
    snoozes: u32,
    snoozed_until: Option<u64>,
    /// When the card last expanded; it collapses to a capsule after a while.
    shown_at: u64,
    nudged: bool,
}

#[derive(Debug, Clone)]
struct ActiveBreak {
    id: String,
    source: Source,
    label: String,
    planned_ms: u64,
    started_at: u64,
    segment_id: Option<i64>,
    /// Labels of the stopwatches this break paused.
    paused: Vec<(String, String)>,
    /// Since when input has been steady (see `SUSTAINED_INPUT_MS`).
    back_since: Option<u64>,
}

#[derive(Debug, Clone)]
struct Welcome {
    label: String,
    planned_ms: u64,
    started_at: u64,
    ended_at: u64,
    resumed: Vec<String>,
    /// Set when the engine, not the user, ended the break.
    note: Option<String>,
    until: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    StartBreak,
    Snooze(u16),
    Skip,
    EndBreak,
    /// +N minutes on the running break.
    Extend(u16),
    /// Re-expand a collapsed reminder.
    Expand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderView {
    pub source: Source,
    pub schedule_id: Option<String>,
    pub label: String,
    pub planned_ms: u64,
    pub due_at: u64,
    pub worked_ms: u64,
    pub snoozes: u32,
    /// The snooze button's default length.
    pub snooze_minutes: u16,
    /// Absent unless the user wrote one.
    pub message: Option<String>,
    /// Snooze is no longer offered after too many in a row.
    pub can_snooze: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakView {
    pub id: Option<String>,
    pub source: Source,
    pub label: String,
    pub planned_ms: u64,
    pub started_at: u64,
    /// Set once the break is over (the welcome-back card).
    pub ended_at: Option<u64>,
    /// Labels of the stopwatches this break paused.
    pub paused_timers: Vec<String>,
    /// Labels of the stopwatches resumed when it ended.
    pub resumed_timers: Vec<String>,
    /// Why the engine ended it, e.g. "Looks like you're back".
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextBreak {
    pub at: u64,
    pub source: Source,
    pub label: String,
}

/// The whole picture the surface and every window render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakState {
    pub phase: Phase,
    pub reminder: Option<ReminderView>,
    pub current: Option<BreakView>,
    pub next: Option<NextBreak>,
    pub paused_until: Option<u64>,
    pub snoozed_until: Option<u64>,
}

pub struct Engine {
    streak_ms: u64,
    last_tick: Option<u64>,
    day_key: Option<i64>,
    inactive_since: Option<u64>,
    reminder: Option<Reminder>,
    active: Option<ActiveBreak>,
    welcome: Option<Welcome>,
    /// The Idle segment the user is currently in: (segment id, start).
    idle_seg: Option<(i64, u64)>,
    paused_until: Option<u64>,
    /// (schedule id, day) pairs already shown, credited or skipped.
    fired: HashSet<(String, i64)>,
    deferred_since: Option<u64>,
}

impl Engine {
    pub fn new(streak_ms: u64, paused_until: Option<u64>) -> Self {
        Self {
            streak_ms,
            last_tick: None,
            day_key: None,
            inactive_since: None,
            reminder: None,
            active: None,
            welcome: None,
            idle_seg: None,
            paused_until,
            fired: HashSet::new(),
            deferred_since: None,
        }
    }

    #[cfg(test)]
    pub fn streak_ms(&self) -> u64 {
        self.streak_ms
    }

    pub fn set_paused_until(&mut self, until: Option<u64>) {
        self.paused_until = until;
    }

    /// The driver opened the break's segment; remember it.
    pub fn attach_segment(&mut self, segment_id: Option<i64>) {
        if let Some(active) = &mut self.active {
            active.segment_id = segment_id;
        }
    }

    /// The driver paused these stopwatches (id, label) for the break.
    pub fn attach_paused_timers(&mut self, paused: Vec<(String, String)>) {
        if let Some(active) = &mut self.active {
            active.paused = paused;
        }
    }

    /// Starting the break's segment failed: forget the break.
    pub fn abort_begin(&mut self) {
        self.active = None;
    }

    // --- tick -----------------------------------------------------------

    pub fn step(&mut self, input: &Input<'_>) -> Vec<Effect> {
        let mut effects = Vec::new();
        let now = input.now;
        let previous_tick = self.last_tick;
        let dt = previous_tick.map_or(0, |last| now.saturating_sub(last));
        self.last_tick = Some(now);
        let settings = input.settings;
        let break_ms = u64::from(settings.break_minutes) * MINUTE_MS;

        if self.day_key != Some(input.local.day_key) {
            // The first tick after launch keeps the clock rebuilt from today.
            if self.day_key.is_some() {
                self.streak_ms = 0;
            }
            self.day_key = Some(input.local.day_key);
            self.fired.retain(|(_, day)| *day == input.local.day_key);
        }
        let slept = dt > TICK_GAP_MS;
        if slept && dt >= break_ms {
            self.streak_ms = 0;
        }
        if input.tracking_active {
            if let Some(since) = self.inactive_since.take() {
                if now.saturating_sub(since) >= break_ms {
                    self.streak_ms = 0;
                }
            }
        } else if self.inactive_since.is_none() {
            self.inactive_since = Some(now);
        }

        if let Some(welcome) = &self.welcome {
            if now >= welcome.until {
                self.welcome = None;
            }
        }

        if self.active.is_some() {
            self.step_break(input, previous_tick, &mut effects);
            self.track_idle_segment(input, break_ms, &mut effects);
            return effects;
        }

        self.track_idle_segment(input, break_ms, &mut effects);

        // Grow the work clock.
        let working = input.tracking_active
            && !input.away
            && input.idle_ms < input.idle_threshold_ms
            && input
                .segment
                .as_ref()
                .is_none_or(|segment| segment.kind == crate::activity::KIND_ACTIVITY);
        if working && !slept {
            self.streak_ms += dt;
        }

        if !settings.enabled
            && self
                .reminder
                .as_ref()
                .is_some_and(|reminder| reminder.source == Source::Interval)
        {
            self.reminder = None;
        }

        self.advance_reminder(input, &mut effects);
        // A scheduled break takes the place of a pending interval reminder.
        let outranked = self
            .reminder
            .as_ref()
            .is_some_and(|reminder| reminder.source == Source::Interval)
            && settings
                .schedules
                .iter()
                .any(|schedule| self.schedule_due(schedule, input));
        if self.reminder.is_none() || outranked {
            self.fire_new_reminder(input, break_ms, &mut effects);
        }
        effects
    }

    /// Mid-break bookkeeping: the segment vanishing, the planned end, and the
    /// user coming back early.
    fn step_break(
        &mut self,
        input: &Input<'_>,
        previous_tick: Option<u64>,
        effects: &mut Vec<Effect>,
    ) {
        let now = input.now;
        let Some(active) = &mut self.active else {
            return;
        };
        let segment_gone = match (&input.segment, active.segment_id) {
            (Some(segment), Some(id)) => segment.id != id,
            (None, Some(_)) => true,
            (_, None) => false,
        };
        if segment_gone {
            // A sleep or a lock closed it. It ended when we last saw it.
            let ended = previous_tick.map_or(now, |tick| (tick + 1_000).min(now));
            self.finish(
                ended,
                false,
                Some("The break was cut short".to_string()),
                effects,
            );
            return;
        }

        let planned_end = active.started_at + active.planned_ms;
        let fresh_input = input.idle_ms < FRESH_INPUT_MS && !input.away;
        if now >= planned_end && fresh_input && now > active.started_at + START_GRACE_MS {
            self.finish(now, true, None, effects);
            return;
        }

        if input.idle_ms < BACK_INPUT_MS && !input.away {
            if now >= active.started_at + START_GRACE_MS {
                let since = *active.back_since.get_or_insert(now);
                if now.saturating_sub(since) >= SUSTAINED_INPUT_MS {
                    let note = format!("Looks like you're back - break ended at {}", clock(since));
                    self.finish(now, true, Some(note), effects);
                }
            }
        } else {
            active.back_since = None;
        }
    }

    fn finish(
        &mut self,
        ended_at: u64,
        close_segment: bool,
        note: Option<String>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(active) = self.active.take() else {
            return;
        };
        let ended_at = ended_at.max(active.started_at);
        let resumed: Vec<String> = active
            .paused
            .iter()
            .map(|(_, label)| label.clone())
            .collect();
        effects.push(Effect::Finish {
            id: active.id,
            ended_at,
            close_segment,
            resume_timers: active.paused.iter().map(|(id, _)| id.clone()).collect(),
        });
        self.streak_ms = 0;
        self.welcome = Some(Welcome {
            label: active.label,
            planned_ms: active.planned_ms,
            started_at: active.started_at,
            ended_at,
            resumed,
            note,
            until: self.last_tick.unwrap_or(ended_at) + WELCOME_MS,
        });
    }

    /// Credits a finished Idle stretch as a break taken, and remembers the
    /// stretch while it lasts.
    fn track_idle_segment(&mut self, input: &Input<'_>, break_ms: u64, effects: &mut Vec<Effect>) {
        let now = input.now;
        let current_idle = input
            .segment
            .as_ref()
            .filter(|segment| segment.is_idle_break())
            .map(|segment| segment.id);
        match (self.idle_seg, current_idle) {
            (None, Some(id)) => {
                // The segment was backdated to the last input; the engine
                // only knows it from now, so use the earliest moment it can
                // prove: the idle time the sampler reports.
                self.idle_seg = Some((id, now.saturating_sub(input.idle_ms.max(1))));
            }
            (Some((old, started)), current) if current != Some(old) => {
                self.idle_seg = current.map(|id| (id, now.saturating_sub(input.idle_ms.max(1))));
                self.credit_idle(input, old, started, now, break_ms, effects);
            }
            _ => {}
        }
    }

    fn credit_idle(
        &mut self,
        input: &Input<'_>,
        segment_id: i64,
        started: u64,
        ended: u64,
        break_ms: u64,
        effects: &mut Vec<Effect>,
    ) {
        let scheduled = self.scheduled_covered_by(input, started, ended);
        let long_enough = ended.saturating_sub(started) >= break_ms;
        if !long_enough && scheduled.is_none() {
            return;
        }
        let (source, schedule_id, planned_ms) = match scheduled {
            Some(schedule) => {
                self.fired
                    .insert((schedule.id.clone(), input.local.day_key));
                (
                    Source::Scheduled,
                    Some(schedule.id.clone()),
                    u64::from(schedule.minutes) * MINUTE_MS,
                )
            }
            None => (Source::Idle, None, break_ms),
        };
        effects.push(Effect::Record(BreakRecord {
            id: uuid::Uuid::now_v7().to_string(),
            source,
            schedule_id,
            due_at: None,
            planned_ms,
            status: STATUS_TAKEN,
            snoozes: 0,
            started_at: Some(started),
            ended_at: Some(ended),
            segment_id: Some(segment_id),
        }));
        self.streak_ms = 0;
        self.reminder = None;
        self.deferred_since = None;
    }

    /// A scheduled break today that the idle stretch covers at least half of.
    fn scheduled_covered_by<'s>(
        &self,
        input: &'s Input<'_>,
        started: u64,
        ended: u64,
    ) -> Option<&'s ScheduledBreak> {
        input
            .settings
            .schedules
            .iter()
            .filter(|schedule| self.schedule_open_today(schedule, input.local))
            .find(|schedule| {
                let planned = u64::from(schedule.minutes) * MINUTE_MS;
                let at = schedule_epoch(schedule, input.now, input.local);
                let from = started.max(at);
                let to = ended.min(at + planned);
                at <= ended && to > from && (to - from) * 2 >= planned
            })
    }

    fn schedule_open_today(&self, schedule: &ScheduledBreak, local: LocalTime) -> bool {
        schedule.enabled
            && schedule.days.includes(local.weekday)
            && !self.fired.contains(&(schedule.id.clone(), local.day_key))
    }

    /// Moves a pending reminder along: snooze wake-ups, the nudge, and the
    /// cases where something more important takes its place.
    fn advance_reminder(&mut self, input: &Input<'_>, effects: &mut Vec<Effect>) {
        let now = input.now;
        let Some(reminder) = &mut self.reminder else {
            return;
        };
        if let Some(until) = reminder.snoozed_until {
            if now >= until {
                reminder.snoozed_until = None;
                reminder.shown_at = now;
                reminder.nudged = false;
                if input.settings.chime {
                    effects.push(Effect::Chime);
                }
            }
            return;
        }
        if !reminder.nudged && now.saturating_sub(reminder.shown_at) >= NUDGE_AFTER_MS {
            reminder.nudged = true;
        }
    }

    fn fire_new_reminder(&mut self, input: &Input<'_>, break_ms: u64, effects: &mut Vec<Effect>) {
        let now = input.now;
        let quiet = self.quiet_reason(input);
        let due_schedule = input
            .settings
            .schedules
            .iter()
            .find(|schedule| self.schedule_due(schedule, input));
        let interval_due = input.settings.enabled
            && self.streak_ms >= u64::from(input.settings.work_minutes) * MINUTE_MS;

        if due_schedule.is_none() && !interval_due {
            self.deferred_since = None;
            return;
        }
        if quiet.is_some() {
            if quiet == Some(Quiet::DisplayAwake) {
                let since = *self.deferred_since.get_or_insert(now);
                if now.saturating_sub(since) >= DEFER_DROP_MS {
                    self.deferred_since = None;
                    self.drop_as_missed(input, due_schedule, break_ms, effects);
                }
            } else {
                self.deferred_since = None;
            }
            return;
        }
        self.deferred_since = None;

        if let Some(schedule) = due_schedule {
            self.fired
                .insert((schedule.id.clone(), input.local.day_key));
            self.reminder = Some(Reminder {
                source: Source::Scheduled,
                schedule_id: Some(schedule.id.clone()),
                label: schedule.label.clone(),
                planned_ms: u64::from(schedule.minutes) * MINUTE_MS,
                due_at: now,
                worked_ms: 0,
                snoozes: 0,
                snoozed_until: None,
                shown_at: now,
                nudged: false,
            });
        } else if interval_due && !self.interval_suppressed(input.settings, input.local) {
            self.reminder = Some(Reminder {
                source: Source::Interval,
                schedule_id: None,
                label: "Break".to_string(),
                planned_ms: break_ms,
                due_at: now,
                worked_ms: self.streak_ms,
                snoozes: 0,
                snoozed_until: None,
                shown_at: now,
                nudged: false,
            });
        } else {
            return;
        }
        if input.settings.chime {
            effects.push(Effect::Chime);
        }
    }

    fn drop_as_missed(
        &mut self,
        input: &Input<'_>,
        schedule: Option<&ScheduledBreak>,
        break_ms: u64,
        effects: &mut Vec<Effect>,
    ) {
        let (source, schedule_id, planned_ms) = match schedule {
            Some(schedule) => {
                self.fired
                    .insert((schedule.id.clone(), input.local.day_key));
                (
                    Source::Scheduled,
                    Some(schedule.id.clone()),
                    u64::from(schedule.minutes) * MINUTE_MS,
                )
            }
            None => {
                self.streak_ms = 0;
                (Source::Interval, None, break_ms)
            }
        };
        effects.push(Effect::Record(BreakRecord {
            id: uuid::Uuid::now_v7().to_string(),
            source,
            schedule_id,
            due_at: Some(input.now),
            planned_ms,
            status: STATUS_MISSED,
            snoozes: 0,
            started_at: None,
            ended_at: None,
            segment_id: None,
        }));
    }

    /// A scheduled break is due from its time until its window closes.
    fn schedule_due(&self, schedule: &ScheduledBreak, input: &Input<'_>) -> bool {
        if !self.schedule_open_today(schedule, input.local) {
            return false;
        }
        let at = schedule.at_minutes();
        let window = SCHEDULE_WINDOW_MIN.max(u32::from(schedule.minutes));
        input.local.minutes_of_day >= at && input.local.minutes_of_day < at + window
    }

    /// An interval reminder gives way to a scheduled break that is close.
    fn interval_suppressed(&self, settings: &BreakSettings, local: LocalTime) -> bool {
        let lead = (SCHEDULE_LEAD_MS / MINUTE_MS) as u32;
        settings.schedules.iter().any(|schedule| {
            self.schedule_open_today(schedule, local) && {
                let at = schedule.at_minutes();
                local.minutes_of_day < at && local.minutes_of_day + lead >= at
            }
        })
    }

    fn quiet_reason(&self, input: &Input<'_>) -> Option<Quiet> {
        let idle_now =
            input.away || input.idle_ms >= input.idle_threshold_ms || self.idle_seg.is_some();
        let busy_segment = input.segment.as_ref().is_some_and(|segment| {
            segment.kind == crate::activity::KIND_BREAK
                || segment.kind == crate::activity::KIND_FOCUS
        });
        if !input.tracking_active {
            Some(Quiet::NotTracking)
        } else if idle_now || busy_segment {
            Some(Quiet::Away)
        } else if self.paused_until.is_some_and(|until| input.now < until) {
            Some(Quiet::Paused)
        } else if input.keeps_display_awake {
            Some(Quiet::DisplayAwake)
        } else {
            None
        }
    }

    // --- commands -------------------------------------------------------

    pub fn command(&mut self, command: Command, now: u64, settings: &BreakSettings) -> Vec<Effect> {
        match command {
            Command::StartBreak => self.start_break(now, settings),
            Command::Snooze(minutes) => {
                if let Some(reminder) = &mut self.reminder {
                    if reminder.snoozed_until.is_none() {
                        reminder.snoozes += 1;
                        reminder.snoozed_until = Some(now + u64::from(minutes) * MINUTE_MS);
                    }
                }
                Vec::new()
            }
            Command::Skip => self.skip(now),
            Command::EndBreak => {
                let mut effects = Vec::new();
                self.finish(now, true, None, &mut effects);
                effects
            }
            Command::Extend(minutes) => match &mut self.active {
                Some(active) => {
                    active.planned_ms += u64::from(minutes) * MINUTE_MS;
                    vec![Effect::Extend {
                        id: active.id.clone(),
                        planned_ms: active.planned_ms,
                    }]
                }
                None => Vec::new(),
            },
            Command::Expand => {
                if let Some(reminder) = &mut self.reminder {
                    reminder.nudged = false;
                    reminder.shown_at = now;
                }
                Vec::new()
            }
        }
    }

    fn start_break(&mut self, now: u64, settings: &BreakSettings) -> Vec<Effect> {
        if self.active.is_some() {
            return Vec::new();
        }
        let break_ms = u64::from(settings.break_minutes) * MINUTE_MS;
        let reminder = self.reminder.take();
        let (source, schedule_id, label, planned_ms, due_at, snoozes) = match reminder {
            Some(reminder) => (
                reminder.source,
                reminder.schedule_id,
                reminder.label,
                reminder.planned_ms,
                Some(reminder.due_at),
                reminder.snoozes,
            ),
            None => (Source::Manual, None, "Break".to_string(), break_ms, None, 0),
        };
        let id = uuid::Uuid::now_v7().to_string();
        self.active = Some(ActiveBreak {
            id: id.clone(),
            source,
            label: label.clone(),
            planned_ms,
            started_at: now,
            segment_id: None,
            paused: Vec::new(),
            back_since: None,
        });
        self.welcome = None;
        self.streak_ms = 0;
        self.deferred_since = None;
        vec![Effect::Begin {
            record: BreakRecord {
                id,
                source,
                schedule_id,
                due_at,
                planned_ms,
                status: STATUS_TAKEN,
                snoozes,
                started_at: Some(now),
                ended_at: None,
                segment_id: None,
            },
            label,
            pause_timers: settings.pause_stopwatches,
        }]
    }

    fn skip(&mut self, now: u64) -> Vec<Effect> {
        let Some(reminder) = self.reminder.take() else {
            return Vec::new();
        };
        // A skipped interval reminder starts a fresh interval; a skipped
        // scheduled break leaves the work clock alone.
        if reminder.source == Source::Interval {
            self.streak_ms = 0;
        }
        vec![Effect::Record(BreakRecord {
            id: uuid::Uuid::now_v7().to_string(),
            source: reminder.source,
            schedule_id: reminder.schedule_id,
            due_at: Some(reminder.due_at),
            planned_ms: reminder.planned_ms,
            status: STATUS_SKIPPED,
            snoozes: reminder.snoozes,
            started_at: None,
            ended_at: Some(now),
            segment_id: None,
        })]
    }

    /// The app is quitting: end the break at `now`.
    pub fn end_for_exit(&mut self, now: u64) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.finish(now, true, None, &mut effects);
        effects
    }

    /// A sample interval reminder, for looking at the panel in a dev build.
    #[cfg(debug_assertions)]
    pub fn dev_sample_reminder(&mut self, now: u64, settings: &BreakSettings) {
        if self.active.is_some() {
            return;
        }
        self.reminder = Some(Reminder {
            source: Source::Interval,
            schedule_id: None,
            label: "Break".to_string(),
            planned_ms: u64::from(settings.break_minutes) * MINUTE_MS,
            due_at: now,
            worked_ms: u64::from(settings.work_minutes) * MINUTE_MS,
            snoozes: 0,
            snoozed_until: None,
            shown_at: now,
            nudged: false,
        });
    }

    // --- view -----------------------------------------------------------

    pub fn view(&self, now: u64, settings: &BreakSettings, local: LocalTime) -> BreakState {
        let mut phase = Phase::Idle;
        let mut reminder_view = None;
        let mut current = None;
        let mut snoozed_until = None;

        if let Some(active) = &self.active {
            let over = now >= active.started_at + active.planned_ms;
            phase = if over { Phase::Over } else { Phase::OnBreak };
            current = Some(BreakView {
                id: Some(active.id.clone()),
                source: active.source,
                label: active.label.clone(),
                planned_ms: active.planned_ms,
                started_at: active.started_at,
                ended_at: None,
                paused_timers: active
                    .paused
                    .iter()
                    .map(|(_, label)| label.clone())
                    .collect(),
                resumed_timers: Vec::new(),
                note: None,
            });
        } else if let Some(welcome) = &self.welcome {
            phase = Phase::Over;
            current = Some(BreakView {
                id: None,
                source: Source::Manual,
                label: welcome.label.clone(),
                planned_ms: welcome.planned_ms,
                started_at: welcome.started_at,
                ended_at: Some(welcome.ended_at),
                paused_timers: Vec::new(),
                resumed_timers: welcome.resumed.clone(),
                note: welcome.note.clone(),
            });
        } else if let Some(reminder) = &self.reminder {
            snoozed_until = reminder.snoozed_until;
            if reminder.snoozed_until.is_none() {
                phase = if reminder.nudged {
                    Phase::Nudge
                } else {
                    Phase::Due
                };
            }
            reminder_view = Some(ReminderView {
                source: reminder.source,
                schedule_id: reminder.schedule_id.clone(),
                label: reminder.label.clone(),
                planned_ms: reminder.planned_ms,
                due_at: reminder.due_at,
                worked_ms: reminder.worked_ms,
                snoozes: reminder.snoozes,
                snooze_minutes: settings.snooze_minutes,
                message: Some(settings.message.clone()).filter(|text| !text.is_empty()),
                can_snooze: reminder.snoozes < MAX_SNOOZES_IN_ROW,
            });
        }

        BreakState {
            phase,
            reminder: reminder_view,
            current,
            next: self.next_break(now, settings, local),
            paused_until: self.paused_until.filter(|until| *until > now),
            snoozed_until,
        }
    }

    /// The soonest thing that will interrupt the user: the work clock's
    /// interval reminder or today's next scheduled break.
    fn next_break(
        &self,
        now: u64,
        settings: &BreakSettings,
        local: LocalTime,
    ) -> Option<NextBreak> {
        if self.active.is_some() {
            return None;
        }
        let mut best: Option<NextBreak> = None;
        if settings.enabled && !self.interval_suppressed(settings, local) {
            let work_ms = u64::from(settings.work_minutes) * MINUTE_MS;
            let remaining = work_ms.saturating_sub(self.streak_ms);
            // Half-minute buckets keep the view from changing every tick.
            let at = (now + remaining + 15_000) / 30_000 * 30_000;
            best = Some(NextBreak {
                at,
                source: Source::Interval,
                label: "Break".to_string(),
            });
        }
        for schedule in &settings.schedules {
            if !self.schedule_open_today(schedule, local) {
                continue;
            }
            let at = schedule_epoch(schedule, now, local);
            if at + MINUTE_MS <= now {
                continue;
            }
            if best.as_ref().is_none_or(|current| at < current.at) {
                best = Some(NextBreak {
                    at,
                    source: Source::Scheduled,
                    label: schedule.label.clone(),
                });
            }
        }
        best
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quiet {
    NotTracking,
    Away,
    Paused,
    DisplayAwake,
}

/// Epoch milliseconds of the schedule's time today.
fn schedule_epoch(schedule: &ScheduledBreak, now: u64, local: LocalTime) -> u64 {
    let delta = i64::from(schedule.at_minutes()) - i64::from(local.minutes_of_day);
    (now as i64 + delta * MINUTE_MS as i64) as u64
}

fn clock(epoch_ms: u64) -> String {
    use chrono::{DateTime, Local};
    DateTime::from_timestamp_millis(epoch_ms as i64)
        .map(|utc| utc.with_timezone(&Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

/// The work clock after a restart, rebuilt from today's segments:
/// `(kind, label, start, end)` in time order.
pub fn streak_from_segments(
    segments: &[(String, Option<String>, u64, u64)],
    now: u64,
    break_ms: u64,
) -> u64 {
    let mut streak = 0u64;
    let mut last_end: Option<u64> = None;
    for (kind, label, start, end) in segments {
        if let Some(previous) = last_end {
            if start.saturating_sub(previous) >= break_ms {
                streak = 0;
            }
        }
        let end = (*end).max(*start);
        if kind == crate::activity::KIND_BREAK {
            let idle = label.as_deref() == Some(crate::activity::IDLE_LABEL);
            if !idle || end - start >= break_ms {
                streak = 0;
            }
        } else {
            streak += end - start;
        }
        last_end = Some(end);
    }
    match last_end {
        Some(end) if now.saturating_sub(end) >= break_ms => 0,
        Some(_) => streak,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;

    const MIN: u64 = 60_000;
    const DAY: i64 = 740_000;

    fn settings() -> BreakSettings {
        BreakSettings::default()
    }

    struct Sim {
        engine: Engine,
        now: u64,
        settings: BreakSettings,
        idle_ms: u64,
        away: bool,
        awake: bool,
        tracking: bool,
        segment: Option<SegmentNow>,
        minutes_of_day: u32,
        weekday: Weekday,
        into_minute_ms: u64,
    }

    impl Sim {
        fn new() -> Self {
            let mut sim = Self {
                engine: Engine::new(0, None),
                now: 1_000_000,
                settings: settings(),
                idle_ms: 0,
                away: false,
                awake: false,
                tracking: true,
                segment: Some(SegmentNow {
                    id: 1,
                    kind: "activity".to_string(),
                    label: None,
                }),
                minutes_of_day: 9 * 60,
                weekday: Weekday::Mon,
                into_minute_ms: 0,
            };
            sim.tick();
            sim
        }

        fn input(&self) -> Input<'_> {
            Input {
                now: self.now,
                settings: &self.settings,
                tracking_active: self.tracking,
                idle_ms: self.idle_ms,
                idle_threshold_ms: 5 * MIN,
                away: self.away,
                keeps_display_awake: self.awake,
                segment: self.segment.clone(),
                local: self.local(),
            }
        }

        fn local(&self) -> LocalTime {
            LocalTime {
                day_key: DAY,
                minutes_of_day: self.minutes_of_day,
                weekday: self.weekday,
            }
        }

        fn tick(&mut self) -> Vec<Effect> {
            let mut engine = std::mem::replace(&mut self.engine, Engine::new(0, None));
            let effects = engine.step(&self.input());
            self.engine = engine;
            effects
        }

        /// Advances by whole seconds, ticking each one.
        fn run(&mut self, ms: u64) -> Vec<Effect> {
            let mut all = Vec::new();
            for _ in 0..(ms / 1000) {
                self.now += 1000;
                self.into_minute_ms += 1000;
                if self.into_minute_ms >= MIN {
                    self.into_minute_ms -= MIN;
                    self.minutes_of_day += 1;
                }
                all.extend(self.tick());
            }
            all
        }

        fn state(&self) -> BreakState {
            self.engine.view(self.now, &self.settings, self.local())
        }

        fn command(&mut self, command: Command) -> Vec<Effect> {
            self.engine.command(command, self.now, &self.settings)
        }

        /// Plays the driver: the break's segment opens and replaces the activity one.
        fn begin(&mut self, effects: &[Effect]) {
            assert!(matches!(effects.first(), Some(Effect::Begin { .. })));
            self.segment = Some(SegmentNow {
                id: 2,
                kind: "break".to_string(),
                label: Some("Break".to_string()),
            });
            self.engine.attach_segment(Some(2));
        }
    }

    #[test]
    fn a_reminder_appears_after_the_configured_work_time() {
        let mut sim = Sim::new();
        sim.run(49 * MIN);
        assert_eq!(sim.state().phase, Phase::Idle);
        sim.run(MIN + 1000);
        let state = sim.state();
        assert_eq!(state.phase, Phase::Due);
        let reminder = state.reminder.unwrap();
        assert_eq!(reminder.source, Source::Interval);
        assert_eq!(reminder.planned_ms, 5 * MIN);
        assert_eq!(reminder.message, None);
        assert!(reminder.can_snooze);
    }

    #[test]
    fn a_custom_message_is_shown_only_when_set() {
        let mut sim = Sim::new();
        sim.settings.message = "Stretch your legs".to_string();
        sim.run(51 * MIN);
        let reminder = sim.state().reminder.unwrap();
        assert_eq!(reminder.message.as_deref(), Some("Stretch your legs"));
    }

    #[test]
    fn an_unanswered_reminder_collapses_and_expands_on_click() {
        let mut sim = Sim::new();
        sim.run(50 * MIN + 2000);
        assert_eq!(sim.state().phase, Phase::Due);
        sim.run(NUDGE_AFTER_MS + 1000);
        assert_eq!(sim.state().phase, Phase::Nudge);
        sim.command(Command::Expand);
        assert_eq!(sim.state().phase, Phase::Due);
    }

    #[test]
    fn snooze_hides_the_reminder_until_it_wakes() {
        let mut sim = Sim::new();
        sim.run(51 * MIN);
        sim.command(Command::Snooze(10));
        assert_eq!(sim.state().phase, Phase::Idle);
        assert!(sim.state().snoozed_until.is_some());
        sim.run(9 * MIN);
        assert_eq!(sim.state().phase, Phase::Idle);
        sim.run(MIN + 1000);
        let state = sim.state();
        assert_eq!(state.phase, Phase::Due);
        assert_eq!(state.reminder.unwrap().snoozes, 1);
    }

    #[test]
    fn the_third_snooze_removes_the_snooze_button() {
        let mut sim = Sim::new();
        sim.run(51 * MIN);
        for _ in 0..3 {
            sim.command(Command::Snooze(5));
            sim.run(5 * MIN + 1000);
        }
        let reminder = sim.state().reminder.unwrap();
        assert_eq!(reminder.snoozes, 3);
        assert!(!reminder.can_snooze);
    }

    #[test]
    fn skipping_resets_the_work_clock_and_records_the_decision() {
        let mut sim = Sim::new();
        sim.run(51 * MIN);
        let effects = sim.command(Command::Skip);
        let [Effect::Record(record)] = effects.as_slice() else {
            panic!("expected one record, got {effects:?}");
        };
        assert_eq!(record.status, STATUS_SKIPPED);
        assert_eq!(record.source, Source::Interval);
        assert_eq!(sim.engine.streak_ms(), 0);
        assert_eq!(sim.state().phase, Phase::Idle);
        // A full interval away, not immediately again.
        sim.run(10 * MIN);
        assert_eq!(sim.state().phase, Phase::Idle);
    }

    #[test]
    fn start_break_records_a_taken_break_and_asks_for_the_segment() {
        let mut sim = Sim::new();
        sim.run(51 * MIN);
        let effects = sim.command(Command::StartBreak);
        let Some(Effect::Begin {
            record,
            pause_timers,
            label,
        }) = effects.first()
        else {
            panic!("expected Begin, got {effects:?}");
        };
        assert_eq!(record.status, STATUS_TAKEN);
        assert_eq!(record.planned_ms, 5 * MIN);
        assert_eq!(label, "Break");
        // Stopwatches keep running unless the user opted in.
        assert!(!*pause_timers);
        sim.begin(&effects);
        assert_eq!(sim.state().phase, Phase::OnBreak);
        assert_eq!(sim.engine.streak_ms(), 0);
    }

    #[test]
    fn start_break_pauses_stopwatches_when_the_user_opted_in() {
        let mut sim = Sim::new();
        sim.settings.pause_stopwatches = true;
        let effects = sim.command(Command::StartBreak);
        let Some(Effect::Begin { pause_timers, .. }) = effects.first() else {
            panic!("expected Begin");
        };
        assert!(*pause_timers);
        sim.begin(&effects);
        sim.engine
            .attach_paused_timers(vec![("t1".into(), "Acme site".into())]);
        let finish = sim.command(Command::EndBreak);
        let Some(Effect::Finish { resume_timers, .. }) = finish.first() else {
            panic!("expected Finish");
        };
        assert_eq!(resume_timers, &["t1".to_string()]);
        let welcome = sim.state().current.unwrap();
        assert_eq!(welcome.resumed_timers, ["Acme site"]);
    }

    #[test]
    fn ending_a_break_shows_welcome_back_then_clears() {
        let mut sim = Sim::new();
        let effects = sim.command(Command::StartBreak);
        sim.begin(&effects);
        sim.run(2 * MIN);
        sim.idle_ms = 2 * MIN;
        let finish = sim.command(Command::EndBreak);
        assert!(matches!(finish.first(), Some(Effect::Finish { .. })));
        let state = sim.state();
        assert_eq!(state.phase, Phase::Over);
        assert!(state.current.unwrap().ended_at.is_some());
        sim.segment = Some(SegmentNow {
            id: 3,
            kind: "activity".into(),
            label: None,
        });
        sim.run(WELCOME_MS + 2000);
        assert_eq!(sim.state().phase, Phase::Idle);
    }

    #[test]
    fn a_break_over_waits_for_the_user_then_closes_at_their_input() {
        let mut sim = Sim::new();
        let effects = sim.command(Command::StartBreak);
        sim.begin(&effects);
        sim.idle_ms = 0;
        sim.run(30_000);
        // The user walked away.
        sim.idle_ms = 60_000;
        for _ in 0..(5 * MIN / 1000) {
            sim.idle_ms += 1000;
            sim.run(1000);
        }
        assert_eq!(sim.state().phase, Phase::Over);
        assert!(sim.state().current.unwrap().ended_at.is_none());
        // First input after the planned end.
        sim.idle_ms = 200;
        let effects = sim.run(1000);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Finish {
                close_segment: true,
                ..
            }
        )));
    }

    #[test]
    fn sustained_input_ends_a_break_and_says_so() {
        let mut sim = Sim::new();
        let effects = sim.command(Command::StartBreak);
        sim.begin(&effects);
        // The user never left: 2 minutes of steady input.
        sim.idle_ms = 500;
        let mut finished = false;
        for _ in 0..(4 * MIN / 1000) {
            finished = sim
                .run(1000)
                .iter()
                .any(|effect| matches!(effect, Effect::Finish { .. }));
            if finished {
                break;
            }
        }
        assert!(finished);
        let note = sim.state().current.and_then(|current| current.note);
        assert!(note.is_some_and(|note| note.starts_with("Looks like you're back")));
    }

    #[test]
    fn a_break_whose_segment_vanished_is_finished() {
        let mut sim = Sim::new();
        let effects = sim.command(Command::StartBreak);
        sim.begin(&effects);
        sim.idle_ms = 60_000;
        sim.run(5_000);
        sim.segment = None;
        let effects = sim.run(1_000);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Finish {
                close_segment: false,
                ..
            }
        )));
    }

    #[test]
    fn nothing_fires_while_paused_idle_or_outside_tracking() {
        for setup in [
            |sim: &mut Sim| sim.engine.set_paused_until(Some(u64::MAX)),
            |sim: &mut Sim| sim.tracking = false,
            |sim: &mut Sim| sim.awake = true,
        ] {
            let mut sim = Sim::new();
            setup(&mut sim);
            sim.run(60 * MIN);
            assert_eq!(sim.state().phase, Phase::Idle);
        }
    }

    #[test]
    fn a_deferred_reminder_shows_when_the_quiet_rule_clears() {
        let mut sim = Sim::new();
        sim.run(49 * MIN);
        sim.awake = true;
        sim.run(5 * MIN);
        assert_eq!(sim.state().phase, Phase::Idle);
        sim.awake = false;
        sim.run(1000);
        assert_eq!(sim.state().phase, Phase::Due);
    }

    #[test]
    fn a_reminder_held_back_too_long_is_dropped_as_missed() {
        let mut sim = Sim::new();
        sim.run(49 * MIN);
        sim.awake = true;
        let effects = sim.run(35 * MIN);
        let missed = effects.iter().find_map(|effect| match effect {
            Effect::Record(record) if record.status == STATUS_MISSED => Some(record),
            _ => None,
        });
        assert!(missed.is_some());
        // Only the last few minutes since the drop have counted.
        assert!(sim.engine.streak_ms() <= 5 * MIN);
    }

    #[test]
    fn an_idle_stretch_as_long_as_a_break_is_credited_without_a_reminder() {
        let mut sim = Sim::new();
        sim.run(49 * MIN);
        // Goes idle: the sampler opens an Idle segment.
        sim.idle_ms = 5 * MIN;
        sim.segment = Some(SegmentNow {
            id: 9,
            kind: "break".into(),
            label: Some("Idle".into()),
        });
        sim.run(1000);
        sim.idle_ms = 6 * MIN;
        sim.run(MIN);
        // Back at the keyboard: the Idle segment closes, activity resumes.
        sim.idle_ms = 0;
        sim.segment = Some(SegmentNow {
            id: 10,
            kind: "activity".into(),
            label: None,
        });
        let effects = sim.run(1000);
        let credit = effects.iter().find_map(|effect| match effect {
            Effect::Record(record) => Some(record),
            _ => None,
        });
        let credit = credit.expect("the idle stretch should be credited");
        assert_eq!(credit.source, Source::Idle);
        assert_eq!(credit.status, STATUS_TAKEN);
        assert_eq!(sim.state().phase, Phase::Idle);
        assert!(sim.engine.streak_ms() <= 2_000);
        sim.run(10 * MIN);
        assert_eq!(sim.state().phase, Phase::Idle);
    }

    #[test]
    fn a_short_idle_stretch_does_not_reset_the_work_clock() {
        let mut sim = Sim::new();
        sim.settings.break_minutes = 10;
        sim.run(30 * MIN);
        sim.idle_ms = 5 * MIN;
        sim.segment = Some(SegmentNow {
            id: 9,
            kind: "break".into(),
            label: Some("Idle".into()),
        });
        sim.run(2 * MIN);
        sim.idle_ms = 0;
        sim.segment = Some(SegmentNow {
            id: 10,
            kind: "activity".into(),
            label: None,
        });
        let effects = sim.run(1000);
        assert!(effects
            .iter()
            .all(|effect| !matches!(effect, Effect::Record(_))));
        assert!(sim.engine.streak_ms() >= 30 * MIN);
    }

    fn lunch() -> ScheduledBreak {
        ScheduledBreak {
            id: "lunch".into(),
            label: "Lunch".into(),
            at: "12:30".into(),
            minutes: 45,
            ..ScheduledBreak::default()
        }
    }

    #[test]
    fn a_scheduled_break_fires_at_its_time_with_its_own_name() {
        let mut sim = Sim::new();
        sim.settings.schedules = vec![lunch()];
        sim.minutes_of_day = 12 * 60 + 29;
        sim.run(30_000);
        assert_eq!(sim.state().phase, Phase::Idle);
        sim.run(31_000);
        let state = sim.state();
        assert_eq!(state.phase, Phase::Due);
        let reminder = state.reminder.unwrap();
        assert_eq!(reminder.source, Source::Scheduled);
        assert_eq!(reminder.label, "Lunch");
        assert_eq!(reminder.planned_ms, 45 * MIN);
    }

    #[test]
    fn scheduled_breaks_keep_their_own_days() {
        let mut sim = Sim::new();
        sim.settings.schedules = vec![lunch()];
        sim.weekday = Weekday::Sat;
        sim.minutes_of_day = 12 * 60 + 30;
        sim.run(5_000);
        assert_eq!(sim.state().phase, Phase::Idle);
    }

    #[test]
    fn a_schedule_ten_minutes_ahead_suppresses_a_due_interval_reminder() {
        let mut sim = Sim::new();
        sim.settings.schedules = vec![lunch()];
        sim.minutes_of_day = 12 * 60 + 20;
        sim.engine = Engine::new(60 * MIN, None);
        sim.run(5_000);
        assert_eq!(sim.state().phase, Phase::Idle);
        let next = sim.state().next.unwrap();
        assert_eq!(next.source, Source::Scheduled);
        assert_eq!(next.label, "Lunch");
    }

    #[test]
    fn a_scheduled_break_replaces_a_pending_interval_reminder() {
        let mut sim = Sim::new();
        sim.settings.schedules = vec![lunch()];
        sim.minutes_of_day = 11 * 60 + 50;
        sim.engine = Engine::new(50 * MIN - 2_000, None);
        sim.run(5_000);
        assert_eq!(
            sim.state().reminder.as_ref().unwrap().source,
            Source::Interval
        );
        // Snoozed past lunch, it merges into the scheduled break.
        sim.command(Command::Snooze(15));
        sim.run(41 * MIN);
        let reminder = sim.state().reminder.unwrap();
        assert_eq!(reminder.source, Source::Scheduled);
        assert_eq!(reminder.label, "Lunch");
        assert_eq!(reminder.snoozes, 0);
    }

    #[test]
    fn skipping_a_scheduled_break_leaves_the_work_clock_alone() {
        let mut sim = Sim::new();
        sim.settings.schedules = vec![lunch()];
        sim.minutes_of_day = 12 * 60 + 30;
        sim.engine = Engine::new(20 * MIN, None);
        sim.run(2_000);
        assert_eq!(sim.state().reminder.as_ref().unwrap().label, "Lunch");
        let before = sim.engine.streak_ms();
        let effects = sim.command(Command::Skip);
        let [Effect::Record(record)] = effects.as_slice() else {
            panic!("expected one record");
        };
        assert_eq!(record.source, Source::Scheduled);
        assert_eq!(record.schedule_id.as_deref(), Some("lunch"));
        assert_eq!(sim.engine.streak_ms(), before);
        // It does not come back today.
        sim.run(60_000);
        assert_eq!(sim.state().phase, Phase::Idle);
    }

    #[test]
    fn taking_a_scheduled_break_resets_the_work_clock() {
        let mut sim = Sim::new();
        sim.settings.schedules = vec![lunch()];
        sim.minutes_of_day = 12 * 60 + 30;
        sim.engine = Engine::new(20 * MIN, None);
        sim.run(2_000);
        let effects = sim.command(Command::StartBreak);
        let Some(Effect::Begin { record, label, .. }) = effects.first() else {
            panic!("expected Begin");
        };
        assert_eq!(record.source, Source::Scheduled);
        assert_eq!(label, "Lunch");
        assert_eq!(sim.engine.streak_ms(), 0);
    }

    #[test]
    fn being_away_at_the_scheduled_time_credits_the_break() {
        let mut sim = Sim::new();
        sim.settings.schedules = vec![lunch()];
        sim.minutes_of_day = 12 * 60 + 25;
        // Idle from 12:25 to 13:15, straight through lunch.
        sim.idle_ms = 5 * MIN;
        sim.segment = Some(SegmentNow {
            id: 9,
            kind: "break".into(),
            label: Some("Idle".into()),
        });
        sim.run(50 * MIN);
        assert_eq!(sim.state().phase, Phase::Idle, "no reminder while away");
        sim.idle_ms = 0;
        sim.segment = Some(SegmentNow {
            id: 10,
            kind: "activity".into(),
            label: None,
        });
        let effects = sim.run(1000);
        let credit = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Record(record) => Some(record),
                _ => None,
            })
            .expect("credited");
        assert_eq!(credit.source, Source::Scheduled);
        assert_eq!(credit.schedule_id.as_deref(), Some("lunch"));
        assert_eq!(credit.status, STATUS_TAKEN);
        assert_eq!(sim.state().phase, Phase::Idle);
    }

    #[test]
    fn a_disabled_setting_stops_interval_reminders_only() {
        let mut sim = Sim::new();
        sim.settings.enabled = false;
        sim.settings.schedules = vec![lunch()];
        sim.run(90 * MIN);
        assert_eq!(sim.state().phase, Phase::Idle);
        sim.minutes_of_day = 12 * 60 + 30;
        sim.run(2_000);
        assert_eq!(sim.state().phase, Phase::Due);
    }

    #[test]
    fn a_long_gap_between_ticks_is_a_rest() {
        let mut sim = Sim::new();
        sim.run(30 * MIN);
        assert!(sim.engine.streak_ms() > 29 * MIN);
        // The Mac slept for ten minutes.
        sim.now += 10 * MIN;
        sim.tick();
        assert_eq!(sim.engine.streak_ms(), 0);
    }

    #[test]
    fn the_work_clock_is_rebuilt_from_todays_segments() {
        let segments =
            |rows: &[(&str, Option<&str>, u64, u64)]| -> Vec<(String, Option<String>, u64, u64)> {
                rows.iter()
                    .map(|(kind, label, start, end)| {
                        (kind.to_string(), label.map(str::to_string), *start, *end)
                    })
                    .collect()
            };
        let now = 200 * MIN;
        // 40 minutes of work, a 6-minute Idle stretch, then 20 more minutes.
        let rows = segments(&[
            ("activity", None, 100 * MIN, 140 * MIN),
            ("break", Some("Idle"), 140 * MIN, 146 * MIN),
            ("activity", None, 146 * MIN, 166 * MIN),
        ]);
        // The idle stretch was long enough to be a rest, and the last segment
        // ended long ago (34 minutes), so the clock is 0 either way.
        assert_eq!(streak_from_segments(&rows, now, 5 * MIN), 0);
        assert_eq!(streak_from_segments(&rows, 168 * MIN, 5 * MIN), 20 * MIN);
        // A short idle stretch is not a rest.
        let short = segments(&[
            ("activity", None, 100 * MIN, 140 * MIN),
            ("break", Some("Idle"), 140 * MIN, 142 * MIN),
            ("activity", None, 142 * MIN, 160 * MIN),
        ]);
        assert_eq!(streak_from_segments(&short, 161 * MIN, 5 * MIN), 58 * MIN);
        // A manual break of any length resets it.
        let manual = segments(&[
            ("activity", None, 100 * MIN, 140 * MIN),
            ("break", Some("Break"), 140 * MIN, 141 * MIN),
            ("activity", None, 141 * MIN, 150 * MIN),
        ]);
        assert_eq!(streak_from_segments(&manual, 151 * MIN, 5 * MIN), 9 * MIN);
    }
}
