//! The billing rules for agent loops: what each project bills when a person
//! prompts an agent, works elsewhere while it runs, then reviews the result.
//!
//! The idea (design board, sections 00 and 03): bill each project what the
//! loop would have taken if the person had done it alone. Prompting 5 min, the
//! agent working 45 min and reviewing 10 min is an hour, even when the 45 min
//! were spent on project B. B still bills its own 45 min. *Work time* (the
//! human partition of the wall clock) stays an hour; this module computes the
//! separate *billable* measure and never feeds work time.
//!
//! `Billable(p) = |You(p) ∪ AgentWorking(p)|` after the Standard guardrails:
//!
//! - **G1 working only.** Only the agent's running state bills. Needs you,
//!   ready to review, idle and stalled never do.
//! - **G2 union per project.** Any number of agents plus you on one project
//!   count once per minute.
//! - **G3 while you're working.** Agent minutes count only while the person is
//!   in an active session (`Input::session`), so overnight and lunch runs do
//!   not bill by themselves.
//! - **G4 supervision confidence.** High and medium turns count (medium is
//!   flagged for a one-click confirm); a low turn is pending until confirmed.
//!   Minutes are never scaled.
//! - **G5 parallel cap.** At most `parallel_cap` projects bill in any minute.
//!   The person's own project keeps its minute and the other slots rotate
//!   fairly between the projects with working agents.
//! - **G6 stall cut.** A working stretch with no output for 10 min stops
//!   counting. The tracker records that as a `Stalled` state, so by the time
//!   a timeline reaches this module the cut is already in it.
//! - **G7 per-project policy.** A project can be set to bill "You only".
//!
//! Everything here is a pure function over spans, so the numbers of the
//! design board's test morning are reproduced exactly in the tests below.

use std::collections::BTreeMap;

use serde::Serialize;

use super::spans::{self, Span};

pub const MINUTE_MS: u64 = 60_000;

/// A running stretch shorter than this after dispatch is the person typing the
/// prompt, not watching the agent.
pub const DISPATCH_GRACE_MS: u64 = 30_000;
/// Time in an agent's pane while it works that counts as having supervised it.
pub const SUPERVISED_MS: u64 = 60_000;
/// Opening the pane this soon after the agent stopped is an immediate reaction.
pub const REACT_HIGH_MS: u64 = 30_000;
/// ...and this soon is a prompt one.
pub const REACT_MEDIUM_MS: u64 = 5 * MINUTE_MS;

/// What an agent was doing, normalized from every source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Running,
    NeedsYou,
    Ready,
    Idle,
    /// Working with no output for the stall cut (G6): not billed.
    Stalled,
    Gone,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::NeedsYou => "needs_you",
            Self::Ready => "ready",
            Self::Idle => "idle",
            Self::Stalled => "stalled",
            Self::Gone => "gone",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "running" => Self::Running,
            "needs_you" => Self::NeedsYou,
            "ready" => Self::Ready,
            "idle" => Self::Idle,
            "stalled" => Self::Stalled,
            "gone" => Self::Gone,
            _ => return None,
        })
    }

    /// The agent is waiting on the person.
    pub fn is_waiting(self) -> bool {
        matches!(self, Self::NeedsYou | Self::Ready)
    }
}

/// G4: how closely the person supervised one agent turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            _ => return None,
        })
    }
}

/// How one agent turn was supervised, from where the person's attention was.
pub struct TurnFacts<'a> {
    pub started_at: u64,
    /// The turn's own time (running, needs you, stalled), normalized.
    pub turn: &'a [Span],
    /// When the agent stopped, or `None` while it is still working.
    pub stopped_at: Option<u64>,
    /// Time the person had this agent's pane in front of them, normalized.
    pub focus: &'a [Span],
    /// The end of the calendar day the agent stopped on: a review before it
    /// still counts as "later the same day".
    pub day_end: u64,
}

/// G4. High: the person was in the pane while it ran, or reacted within 30 s.
/// Medium: reacted within 5 min, or reviewed it later the same day. Low: no
/// engagement.
pub fn confidence(facts: &TurnFacts<'_>) -> Confidence {
    let watched_from = facts.started_at.saturating_add(DISPATCH_GRACE_MS);
    let watched = spans::clip(facts.turn, watched_from, u64::MAX);
    if spans::total(&spans::intersect(&watched, facts.focus)) >= SUPERVISED_MS {
        return Confidence::High;
    }
    let Some(stopped_at) = facts.stopped_at else {
        return Confidence::Low;
    };
    // The first focus that reaches the stop: the person was already there,
    // or arrived afterwards.
    let delay = facts
        .focus
        .iter()
        .filter(|focus| focus.end > stopped_at)
        .map(|focus| focus.start.saturating_sub(stopped_at))
        .min();
    match delay {
        Some(delay) if delay <= REACT_HIGH_MS => Confidence::High,
        Some(delay) if delay <= REACT_MEDIUM_MS => Confidence::Medium,
        Some(_) => {
            let reviewed_that_day = facts
                .focus
                .iter()
                .any(|focus| focus.end > stopped_at && focus.start < facts.day_end);
            if reviewed_that_day {
                Confidence::Medium
            } else {
                Confidence::Low
            }
        }
        None => Confidence::Low,
    }
}

/// G7: what a project bills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// The person's time plus guarded agent time.
    #[default]
    YouAndAgents,
    YouOnly,
}

/// The tunable guardrails. Standard is the default; the others exist so the
/// board's alternative numbers stay reproducible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guardrails {
    /// G5: most projects that may bill in one minute.
    pub parallel_cap: usize,
    /// G4 on: a low-confidence turn waits for the person to confirm it.
    pub require_supervision: bool,
}

impl Guardrails {
    pub const STANDARD: Self = Self {
        parallel_cap: 3,
        require_supervision: true,
    };
}

impl Default for Guardrails {
    fn default() -> Self {
        Self::STANDARD
    }
}

/// One stretch of a job's state timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateSpan {
    pub span: Span,
    pub state: JobState,
    /// The last stretch of a job that is still going: it has no end yet and
    /// is clipped to "now".
    pub open: bool,
}

/// One agent turn (prompt to stop) on one project.
#[derive(Debug, Clone)]
pub struct JobTimeline {
    pub id: String,
    pub project_id: String,
    pub confidence: Confidence,
    /// The person confirmed it, so even a low turn counts.
    pub confirmed: bool,
    /// Sorted, non-overlapping.
    pub states: Vec<StateSpan>,
    /// Time the person had the pane in front of them, normalized. Waiting is
    /// the part of needs-you and ready that was not spent with the person.
    pub attended: Vec<Span>,
}

impl JobTimeline {
    fn spans_in(&self, wanted: impl Fn(&StateSpan) -> bool) -> Vec<Span> {
        spans::normalize(
            &self
                .states
                .iter()
                .filter(|state| wanted(state))
                .map(|state| state.span)
                .collect::<Vec<_>>(),
        )
    }

    fn counts(&self, guardrails: &Guardrails) -> bool {
        !guardrails.require_supervision || self.confidence >= Confidence::Medium || self.confirmed
    }
}

#[derive(Debug, Clone)]
pub struct ProjectInput {
    pub id: String,
    /// The person's time on this project, normalized.
    pub you: Vec<Span>,
    pub policy: Policy,
}

#[derive(Debug, Clone)]
pub struct Input {
    pub window: Span,
    pub projects: Vec<ProjectInput>,
    /// The person's time on no project ("other"): work, never billable.
    pub other_you: Vec<Span>,
    /// G3: when the person was in an active session, normalized.
    pub session: Vec<Span>,
    pub jobs: Vec<JobTimeline>,
    pub guardrails: Guardrails,
}

/// The agent time that was counted on one job, for turning into entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSpan {
    pub job_id: String,
    pub started_at: u64,
    pub ended_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLedger {
    pub project_id: String,
    /// The person's own time.
    pub you_ms: u64,
    /// Counted agent time that is not already the person's own.
    pub agent_ms: u64,
    pub billable_ms: u64,
    /// Time agents waited on the person before they came, never billed.
    pub waiting_ms: u64,
    /// The same, for waits that have not ended yet.
    pub still_waiting_ms: u64,
    /// Working time of low-confidence turns, pending until confirmed.
    pub pending_ms: u64,
    /// Counted working time dropped by the parallel cap.
    pub over_cap_ms: u64,
    pub stalled_ms: u64,
    /// Working time dropped by a "You only" policy.
    pub policy_excluded_ms: u64,
    /// Working time outside an active session (G3).
    pub outside_session_ms: u64,
    pub agent_spans: Vec<JobSpan>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    /// Sorted by project id.
    pub projects: Vec<ProjectLedger>,
    /// The person's work time: every project and "other", counted once.
    pub work_ms: u64,
    /// Across clients this can exceed the work time.
    pub billable_ms: u64,
    pub agent_ms: u64,
    pub waiting_ms: u64,
    pub still_waiting_ms: u64,
    pub pending_ms: u64,
    pub over_cap_ms: u64,
}

struct Demand<'a> {
    spans: Vec<Span>,
    /// Jobs of the project that counted, oldest first.
    jobs: Vec<(&'a JobTimeline, Vec<Span>)>,
}

pub fn account(input: &Input) -> Ledger {
    let window = input.window;
    let session = spans::clip(&input.session, window.start, window.end);

    let mut ledgers: BTreeMap<String, ProjectLedger> = BTreeMap::new();
    let mut you: BTreeMap<String, Vec<Span>> = BTreeMap::new();
    let mut policy: BTreeMap<String, Policy> = BTreeMap::new();
    for project in &input.projects {
        let clipped = spans::clip(&spans::normalize(&project.you), window.start, window.end);
        let ledger = ledgers.entry(project.id.clone()).or_default();
        ledger.project_id = project.id.clone();
        ledger.you_ms = spans::total(&clipped);
        you.insert(project.id.clone(), clipped);
        policy.insert(project.id.clone(), project.policy);
    }
    for job in &input.jobs {
        let ledger = ledgers.entry(job.project_id.clone()).or_default();
        ledger.project_id = job.project_id.clone();
        you.entry(job.project_id.clone()).or_default();
    }

    // Per job: what it did, and which of its working time can count.
    let mut jobs: Vec<&JobTimeline> = input.jobs.iter().collect();
    jobs.sort_by_key(|job| {
        (
            job.states.first().map_or(0, |state| state.span.start),
            job.id.clone(),
        )
    });
    let mut demand: BTreeMap<String, Demand<'_>> = BTreeMap::new();
    for job in jobs {
        let ledger = ledgers
            .get_mut(&job.project_id)
            .expect("a ledger exists for every job's project");
        let running = spans::clip(
            &job.spans_in(|state| state.state == JobState::Running),
            window.start,
            window.end,
        );
        let attended = spans::normalize(&job.attended);
        for open in [false, true] {
            let waiting = spans::clip(
                &spans::subtract(
                    &job.spans_in(|state| state.state.is_waiting() && state.open == open),
                    &attended,
                ),
                window.start,
                window.end,
            );
            if open {
                ledger.still_waiting_ms += spans::total(&waiting);
            } else {
                ledger.waiting_ms += spans::total(&waiting);
            }
        }
        ledger.stalled_ms += spans::total(&spans::clip(
            &job.spans_in(|state| state.state == JobState::Stalled),
            window.start,
            window.end,
        ));

        let in_session = spans::intersect(&running, &session);
        ledger.outside_session_ms += spans::total(&running) - spans::total(&in_session);
        if policy.get(&job.project_id) == Some(&Policy::YouOnly) {
            ledger.policy_excluded_ms += spans::total(&in_session);
            continue;
        }
        if !job.counts(&input.guardrails) {
            ledger.pending_ms += spans::total(&in_session);
            continue;
        }
        let slot = demand.entry(job.project_id.clone()).or_insert(Demand {
            spans: Vec::new(),
            jobs: Vec::new(),
        });
        slot.spans = spans::union(&slot.spans, &in_session);
        slot.jobs.push((job, in_session));
    }

    allocate(input, &you, &demand, &mut ledgers);

    let mut all_you: Vec<Span> = spans::clip(
        &spans::normalize(&input.other_you),
        window.start,
        window.end,
    );
    for spans_of in you.values() {
        all_you = spans::union(&all_you, spans_of);
    }

    let projects: Vec<ProjectLedger> = ledgers.into_values().collect();
    Ledger {
        work_ms: spans::total(&all_you),
        billable_ms: projects.iter().map(|p| p.billable_ms).sum(),
        agent_ms: projects.iter().map(|p| p.agent_ms).sum(),
        waiting_ms: projects.iter().map(|p| p.waiting_ms).sum(),
        still_waiting_ms: projects.iter().map(|p| p.still_waiting_ms).sum(),
        pending_ms: projects.iter().map(|p| p.pending_ms).sum(),
        over_cap_ms: projects.iter().map(|p| p.over_cap_ms).sum(),
        projects,
    }
}

/// G2 and G5: walks the day in stretches where nothing changes and decides
/// which projects bill in each. A contested stretch is cut into whole minutes
/// and its free slots go to the projects that have been given the least so far.
fn allocate(
    input: &Input,
    you: &BTreeMap<String, Vec<Span>>,
    demand: &BTreeMap<String, Demand<'_>>,
    ledgers: &mut BTreeMap<String, ProjectLedger>,
) {
    let mut edges: Vec<u64> = vec![input.window.start, input.window.end];
    for spans_of in you.values() {
        edges.extend(spans_of.iter().flat_map(|s| [s.start, s.end]));
    }
    for project in demand.values() {
        edges.extend(project.spans.iter().flat_map(|s| [s.start, s.end]));
    }
    edges.sort_unstable();
    edges.dedup();

    let cap = input.guardrails.parallel_cap.max(1);
    let mut given: BTreeMap<&str, u64> = BTreeMap::new();
    let mut counted: BTreeMap<String, Vec<JobSpan>> = BTreeMap::new();
    for pair in edges.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if end <= start {
            continue;
        }
        let owners: Vec<&str> = you
            .iter()
            .filter(|(_, spans_of)| spans::covers(spans_of, start))
            .map(|(id, _)| id.as_str())
            .collect();
        for owner in &owners {
            if let Some(ledger) = ledgers.get_mut(*owner) {
                ledger.billable_ms += end - start;
            }
        }
        let wanting: Vec<&str> = demand
            .iter()
            .filter(|(id, project)| {
                spans::covers(&project.spans, start) && !owners.contains(&id.as_str())
            })
            .map(|(id, _)| id.as_str())
            .collect();
        if wanting.is_empty() {
            continue;
        }
        let slots = cap.saturating_sub(owners.len());
        // Whole minutes when contested, so a share is always a real span.
        let contested = wanting.len() > slots;
        let mut cursor = start;
        while cursor < end {
            let stop = if contested {
                ((cursor / MINUTE_MS + 1) * MINUTE_MS).min(end)
            } else {
                end
            };
            let mut ranked = wanting.clone();
            ranked.sort_by_key(|id| (given.get(id).copied().unwrap_or(0), *id));
            for (rank, id) in ranked.into_iter().enumerate() {
                let len = stop - cursor;
                let ledger = ledgers.get_mut(id).expect("ledger for a wanting project");
                if rank < slots {
                    ledger.agent_ms += len;
                    ledger.billable_ms += len;
                    *given.entry(id).or_insert(0) += len;
                    let owner = demand[id]
                        .jobs
                        .iter()
                        .find(|(_, working)| spans::covers(working, cursor))
                        .map(|(job, _)| job.id.clone());
                    if let Some(job_id) = owner {
                        counted.entry(job_id.clone()).or_default().push(JobSpan {
                            job_id,
                            started_at: cursor,
                            ended_at: stop,
                        });
                    }
                } else {
                    ledger.over_cap_ms += len;
                }
            }
            cursor = stop;
        }
    }

    for ledger in ledgers.values_mut() {
        ledger.agent_spans.clear();
    }
    for (job_id, pieces) in counted {
        let merged = spans::normalize(
            &pieces
                .iter()
                .map(|piece| Span::new(piece.started_at, piece.ended_at))
                .collect::<Vec<_>>(),
        );
        let project = input
            .jobs
            .iter()
            .find(|job| job.id == job_id)
            .map(|job| job.project_id.clone());
        if let Some(ledger) = project.and_then(|id| ledgers.get_mut(&id)) {
            ledger
                .agent_spans
                .extend(merged.into_iter().map(|span| JobSpan {
                    job_id: job_id.clone(),
                    started_at: span.start,
                    ended_at: span.end,
                }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 9:00 on the board's test morning, minute-aligned.
    const BASE: u64 = 29_833_320 * MINUTE_MS;

    fn m(minutes: u64) -> u64 {
        BASE + minutes * MINUTE_MS
    }

    /// `h:mm` on the morning's clock: `at(10, 24)` is 10:24.
    fn at(hour: u64, minute: u64) -> u64 {
        m((hour - 9) * 60 + minute)
    }

    fn span(from: (u64, u64), to: (u64, u64)) -> Span {
        Span::new(at(from.0, from.1), at(to.0, to.1))
    }

    fn state(from: (u64, u64), to: (u64, u64), state: JobState, open: bool) -> StateSpan {
        StateSpan {
            span: span(from, to),
            state,
            open,
        }
    }

    fn job(
        id: &str,
        project: &str,
        confidence: Confidence,
        states: Vec<StateSpan>,
        attended: Vec<Span>,
    ) -> JobTimeline {
        JobTimeline {
            id: id.to_string(),
            project_id: project.to_string(),
            confidence,
            confirmed: false,
            states,
            attended: spans::normalize(&attended),
        }
    }

    fn project(id: &str, you: Vec<Span>) -> ProjectInput {
        ProjectInput {
            id: id.to_string(),
            you: spans::normalize(&you),
            policy: Policy::YouAndAgents,
        }
    }

    /// The design board's test morning (section 01): A is OpenRize, B the
    /// Acme website, C Ledgerly. You 150 min, agents A1 72, A2 27, C1 25.
    fn morning(guardrails: Guardrails) -> Input {
        let a_you = vec![
            span((9, 0), (9, 8)),
            span((9, 52), (9, 55)),
            span((10, 24), (10, 58)),
        ];
        let b_you = vec![span((9, 8), (9, 52)), span((10, 58), (11, 20))];
        let c_you = vec![span((9, 55), (10, 15)), span((11, 20), (11, 30))];
        let a1 = job(
            "a1",
            "a",
            Confidence::High,
            vec![
                state((9, 8), (9, 50), JobState::Running, false),
                state((9, 50), (9, 55), JobState::NeedsYou, false),
                state((9, 55), (10, 20), JobState::Running, false),
                state((10, 20), (10, 24), JobState::Ready, false),
            ],
            // Answering the question (9:52 to 9:55), then the review.
            vec![span((9, 52), (9, 55)), span((10, 24), (10, 30))],
        );
        let a2 = job(
            "a2",
            "a",
            Confidence::Low,
            vec![
                state((10, 58), (11, 25), JobState::Running, false),
                state((11, 25), (11, 30), JobState::Ready, true),
            ],
            vec![],
        );
        let c1 = job(
            "c1",
            "c",
            Confidence::Medium,
            vec![
                state((10, 15), (10, 40), JobState::Running, false),
                state((10, 40), (11, 20), JobState::Ready, false),
            ],
            vec![span((11, 20), (11, 30))],
        );
        Input {
            window: span((9, 0), (11, 30)),
            projects: vec![
                project("a", a_you),
                project("b", b_you),
                project("c", c_you),
            ],
            other_you: vec![span((10, 15), (10, 24))],
            session: vec![span((9, 0), (11, 30))],
            jobs: vec![a1, a2, c1],
            guardrails,
        }
    }

    fn of<'a>(ledger: &'a Ledger, id: &str) -> &'a ProjectLedger {
        ledger
            .projects
            .iter()
            .find(|project| project.project_id == id)
            .expect("project in the ledger")
    }

    fn minutes(ms: u64) -> u64 {
        assert_eq!(ms % MINUTE_MS, 0, "{ms} is not whole minutes");
        ms / MINUTE_MS
    }

    #[test]
    fn standard_bills_the_test_morning_to_3h53m_for_2h30m_of_work() {
        let ledger = account(&morning(Guardrails::STANDARD));

        assert_eq!(minutes(ledger.work_ms), 150);
        let (a, b, c) = (of(&ledger, "a"), of(&ledger, "b"), of(&ledger, "c"));
        assert_eq!(minutes(a.you_ms), 45);
        assert_eq!(minutes(a.agent_ms), 67); // A1: 9:08-9:50 and 9:55-10:20
        assert_eq!(minutes(a.billable_ms), 112); // 1h52m
        assert_eq!(minutes(b.billable_ms), 66); // 1h06m
        assert_eq!(minutes(c.you_ms), 30);
        assert_eq!(minutes(c.agent_ms), 25); // C1
        assert_eq!(minutes(c.billable_ms), 55);
        assert_eq!(minutes(ledger.billable_ms), 233); // 3h53m
        assert_eq!(minutes(ledger.agent_ms), 92);
        assert_eq!(ledger.over_cap_ms, 0);
    }

    #[test]
    fn waiting_is_never_billed_and_a2_stays_pending() {
        let ledger = account(&morning(Guardrails::STANDARD));
        let (a, c) = (of(&ledger, "a"), of(&ledger, "c"));

        // A1 waited 2 min before the question was answered and 4 min for
        // its review, C1 waited 40 min: 46 min never billed.
        assert_eq!(minutes(a.waiting_ms), 6);
        assert_eq!(minutes(c.waiting_ms), 40);
        assert_eq!(minutes(ledger.waiting_ms), 46);
        // A2's wait has not ended, and its 27 min of work wait for review.
        assert_eq!(minutes(a.still_waiting_ms), 5);
        assert_eq!(minutes(a.pending_ms), 27);
        assert_eq!(minutes(ledger.pending_ms), 27);
    }

    #[test]
    fn reviewing_a2_moves_its_27_minutes_to_billable() {
        let mut input = morning(Guardrails::STANDARD);
        input.jobs[1].confirmed = true;
        let ledger = account(&input);

        assert_eq!(minutes(ledger.billable_ms), 260); // 4h20m
        assert_eq!(minutes(ledger.pending_ms), 0);
    }

    #[test]
    fn the_other_presets_land_on_the_boards_numbers() {
        // Lenient: no supervision gate, cap 4.
        let lenient = account(&morning(Guardrails {
            parallel_cap: 4,
            require_supervision: false,
        }));
        assert_eq!(minutes(lenient.billable_ms), 260);

        // You only: every project bills just the person's own time.
        let mut input = morning(Guardrails::STANDARD);
        for project in &mut input.projects {
            project.policy = Policy::YouOnly;
        }
        let you_only = account(&input);
        assert_eq!(minutes(you_only.billable_ms), 141); // 2h21m
        assert_eq!(minutes(you_only.work_ms), 150);
        assert_eq!(minutes(you_only.agent_ms), 0);
    }

    #[test]
    fn counted_agent_time_never_overlaps_the_persons_own_time_on_that_project() {
        let input = morning(Guardrails::STANDARD);
        let ledger = account(&input);
        for project_ledger in &ledger.projects {
            let own = &input
                .projects
                .iter()
                .find(|p| p.id == project_ledger.project_id)
                .expect("input project")
                .you;
            let agent: Vec<Span> = project_ledger
                .agent_spans
                .iter()
                .map(|s| Span::new(s.started_at, s.ended_at))
                .collect();
            assert!(spans::intersect(own, &spans::normalize(&agent)).is_empty());
            assert_eq!(spans::total(&agent), project_ledger.agent_ms);
        }
    }

    #[test]
    fn three_projects_in_an_hour_bill_an_hour_and_a_half_for_an_hour_of_work() {
        // The captain's example: prompt A, B, C for 5 min each, B docs for
        // 10 min while A and B run, then review A, B, C, then email.
        let w = |from: u64, to: u64| Span::new(m(from), m(to));
        let working = |from: u64, to: u64, open: bool| StateSpan {
            span: w(from, to),
            state: JobState::Running,
            open,
        };
        let input = Input {
            window: w(0, 60),
            projects: vec![
                project("a", vec![w(0, 5), w(25, 30)]),
                project("b", vec![w(5, 10), w(15, 25), w(30, 35)]),
                project("c", vec![w(10, 15), w(35, 40)]),
            ],
            other_you: vec![w(40, 60)],
            session: vec![w(0, 60)],
            jobs: vec![
                job(
                    "a",
                    "a",
                    Confidence::High,
                    vec![working(5, 25, false)],
                    vec![],
                ),
                job(
                    "b",
                    "b",
                    Confidence::High,
                    vec![working(10, 30, false)],
                    vec![],
                ),
                job(
                    "c",
                    "c",
                    Confidence::High,
                    vec![working(15, 35, false)],
                    vec![],
                ),
            ],
            guardrails: Guardrails::STANDARD,
        };
        let ledger = account(&input);

        assert_eq!(minutes(ledger.work_ms), 60);
        for id in ["a", "b", "c"] {
            assert_eq!(minutes(of(&ledger, id).billable_ms), 30, "project {id}");
        }
        assert_eq!(minutes(ledger.billable_ms), 90);
        assert_eq!(ledger.over_cap_ms, 0);
    }

    #[test]
    fn one_project_all_hour_bills_the_hour_not_the_hour_plus_agent_time() {
        let w = |from: u64, to: u64| Span::new(m(from), m(to));
        let input = Input {
            window: w(0, 60),
            // Prompting 5 min, waiting out the agent on the same project is
            // not "you", then reviewing 10 min.
            projects: vec![project("a", vec![w(0, 5), w(5, 50), w(50, 60)])],
            other_you: vec![],
            session: vec![w(0, 60)],
            jobs: vec![job(
                "a1",
                "a",
                Confidence::High,
                vec![StateSpan {
                    span: w(5, 50),
                    state: JobState::Running,
                    open: false,
                }],
                vec![],
            )],
            guardrails: Guardrails::STANDARD,
        };
        let ledger = account(&input);
        assert_eq!(minutes(of(&ledger, "a").billable_ms), 60);
        assert_eq!(of(&ledger, "a").agent_ms, 0);
    }

    #[test]
    fn a_loop_billed_alone_is_an_hour_even_when_the_agent_ran_while_you_did_b() {
        // 5 min prompting, 45 min of agent, 10 min reviewing; B worked in
        // the gap.
        let w = |from: u64, to: u64| Span::new(m(from), m(to));
        let input = Input {
            window: w(0, 60),
            projects: vec![
                project("a", vec![w(0, 5), w(50, 60)]),
                project("b", vec![w(5, 50)]),
            ],
            other_you: vec![],
            session: vec![w(0, 60)],
            jobs: vec![job(
                "a1",
                "a",
                Confidence::High,
                vec![StateSpan {
                    span: w(5, 50),
                    state: JobState::Running,
                    open: false,
                }],
                vec![],
            )],
            guardrails: Guardrails::STANDARD,
        };
        let ledger = account(&input);
        assert_eq!(minutes(of(&ledger, "a").billable_ms), 60);
        assert_eq!(minutes(of(&ledger, "b").billable_ms), 45);
        assert_eq!(minutes(ledger.work_ms), 60);
        assert_eq!(minutes(ledger.billable_ms), 105);
    }

    /// 20 agents on 20 projects in one hour: the person prompts each for
    /// 1.5 min, then reviews six of them. Only the six they engaged with are
    /// counted, and the parallel cap holds the hour to about 2h12m.
    #[test]
    fn twenty_agents_in_an_hour_stay_within_three_times_the_work() {
        let half = |at: u64| m(0) + at * (MINUTE_MS / 2);
        let w = |from: u64, to: u64| Span::new(half(from), half(to));
        let mut projects = Vec::new();
        let mut jobs = Vec::new();
        for i in 0..20u64 {
            let id = format!("p{i:02}");
            let prompt = w(i * 3, i * 3 + 3);
            let mut you = vec![prompt];
            let run = w(i * 3 + 3, i * 3 + 63);
            // The first six are reviewed in the second half hour, 5 min each.
            let reviewed = i < 6;
            if reviewed {
                you.push(w(60 + i * 10, 60 + i * 10 + 10));
            }
            projects.push(project(&id, you));
            jobs.push(job(
                &id,
                &id,
                if reviewed {
                    Confidence::High
                } else {
                    Confidence::Low
                },
                vec![StateSpan {
                    span: run,
                    state: JobState::Running,
                    open: false,
                }],
                vec![],
            ));
        }
        let input = Input {
            window: Span::new(m(0), m(60)),
            projects,
            other_you: vec![],
            session: vec![Span::new(m(0), m(60))],
            jobs,
            guardrails: Guardrails::STANDARD,
        };
        let ledger = account(&input);

        assert_eq!(minutes(ledger.work_ms), 60);
        assert!(ledger.billable_ms <= 3 * ledger.work_ms);
        assert_eq!(minutes(ledger.billable_ms), 132); // about 2h12m
        assert!(ledger.over_cap_ms > 0);
        // The 14 projects nobody supervised keep only the person's prompt.
        for i in 6..20 {
            let id = format!("p{i:02}");
            assert_eq!(of(&ledger, &id).agent_ms, 0);
            assert!(of(&ledger, &id).pending_ms > 0);
        }
    }

    #[test]
    fn at_most_the_cap_of_projects_bill_in_any_minute() {
        // Six projects each run an agent for the same 30 minutes while the
        // person works elsewhere: only three bill, rotating fairly.
        let w = |from: u64, to: u64| Span::new(m(from), m(to));
        let mut jobs = Vec::new();
        let mut projects = vec![];
        for id in ["a", "b", "c", "d", "e", "f"] {
            projects.push(project(id, vec![]));
            jobs.push(job(
                id,
                id,
                Confidence::High,
                vec![StateSpan {
                    span: w(0, 30),
                    state: JobState::Running,
                    open: false,
                }],
                vec![],
            ));
        }
        let input = Input {
            window: w(0, 30),
            projects,
            other_you: vec![w(0, 30)],
            session: vec![w(0, 30)],
            jobs,
            guardrails: Guardrails::STANDARD,
        };
        let ledger = account(&input);

        assert_eq!(minutes(ledger.billable_ms), 90);
        assert_eq!(minutes(ledger.over_cap_ms), 90);
        for project_ledger in &ledger.projects {
            assert_eq!(minutes(project_ledger.billable_ms), 15, "even shares");
        }
    }

    #[test]
    fn the_persons_own_project_keeps_its_minute_under_the_cap() {
        let w = |from: u64, to: u64| Span::new(m(from), m(to));
        let mut jobs = Vec::new();
        let mut projects = vec![project("own", vec![w(0, 10)])];
        for id in ["b", "c", "d"] {
            projects.push(project(id, vec![]));
            jobs.push(job(
                id,
                id,
                Confidence::High,
                vec![StateSpan {
                    span: w(0, 10),
                    state: JobState::Running,
                    open: false,
                }],
                vec![],
            ));
        }
        let input = Input {
            window: w(0, 10),
            projects,
            other_you: vec![],
            session: vec![w(0, 10)],
            jobs,
            guardrails: Guardrails::STANDARD,
        };
        let ledger = account(&input);

        assert_eq!(minutes(of(&ledger, "own").billable_ms), 10);
        // Two slots remain for three agents: 20 of 30 agent minutes bill.
        assert_eq!(minutes(ledger.agent_ms), 20);
        assert_eq!(minutes(ledger.over_cap_ms), 10);
        assert_eq!(minutes(ledger.billable_ms), 30);
    }

    #[test]
    fn stalled_time_and_runs_outside_a_session_do_not_bill() {
        let w = |from: u64, to: u64| Span::new(m(from), m(to));
        let input = Input {
            window: w(0, 120),
            projects: vec![project("a", vec![w(0, 5)])],
            other_you: vec![],
            // The person was around for the first hour only.
            session: vec![w(0, 60)],
            jobs: vec![job(
                "a1",
                "a",
                Confidence::High,
                vec![
                    StateSpan {
                        span: w(5, 25),
                        state: JobState::Running,
                        open: false,
                    },
                    // No output for the stall cut: the tracker marks it.
                    StateSpan {
                        span: w(25, 45),
                        state: JobState::Stalled,
                        open: false,
                    },
                    StateSpan {
                        span: w(45, 100),
                        state: JobState::Running,
                        open: false,
                    },
                ],
                vec![],
            )],
            guardrails: Guardrails::STANDARD,
        };
        let a = account(&input).projects.remove(0);

        // 5-25 and 45-60 count (35 min); 60-100 is outside the session.
        assert_eq!(minutes(a.agent_ms), 35);
        assert_eq!(minutes(a.stalled_ms), 20);
        assert_eq!(minutes(a.outside_session_ms), 40);
    }

    #[test]
    fn confidence_follows_how_the_person_engaged() {
        let w = |from: u64, to: u64| Span::new(m(from), m(to));
        let turn = [w(0, 30)];
        let facts = |focus: &[Span], stopped: Option<u64>| {
            confidence(&TurnFacts {
                started_at: m(0),
                turn: &turn,
                stopped_at: stopped,
                focus,
                day_end: m(600),
            })
        };

        // In the pane for a minute while it ran: high.
        assert_eq!(facts(&[w(10, 12)], Some(m(30))), Confidence::High);
        // Only the dispatch itself (the first seconds) is not supervision.
        assert_eq!(
            facts(&[Span::new(m(0), m(0) + 20_000)], Some(m(30))),
            Confidence::Low
        );
        // Opening the pane within 30 s of the stop: high.
        assert_eq!(
            facts(&[Span::new(m(30) + 20_000, m(32))], Some(m(30))),
            Confidence::High
        );
        // Within 5 min: medium. Hours later the same day: medium.
        assert_eq!(facts(&[w(34, 36)], Some(m(30))), Confidence::Medium);
        assert_eq!(facts(&[w(300, 301)], Some(m(30))), Confidence::Medium);
        // The next day, or never: low. Still working with no one: low.
        assert_eq!(facts(&[w(700, 701)], Some(m(30))), Confidence::Low);
        assert_eq!(facts(&[], Some(m(30))), Confidence::Low);
        assert_eq!(facts(&[], None), Confidence::Low);
    }
}
