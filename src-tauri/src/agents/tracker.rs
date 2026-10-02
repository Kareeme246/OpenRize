//! Turns what the bridges see (which panes hold agents, what state each is in,
//! which one the person is looking at) into jobs, state transitions and focus
//! time. Pure of any I/O except the database connection it is handed, so the
//! whole lifecycle is tested against synthetic observations.
//!
//! A job is one agent turn: it opens when an agent starts working and closes
//! when it stops (ready, idle or gone). A turn can pass through *needs you*
//! (the agent paused for an answer) and back; it is still one turn. After it
//! stops, the job waits to be reviewed: the person looks at the pane, or the
//! agent starts its next turn.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::Serialize;

use super::accounting::JobState;
use super::store::{self, NewJob};
use super::Source;
use crate::ai::rules::Rule;

/// G6: working with no output for this long stops counting.
pub const STALL_CUT_MS: u64 = 10 * 60 * 1000;
/// A pane the person is looking at is written to disk at most this often.
const FOCUS_WRITE_MS: u64 = 5_000;
/// A job's last-seen time is refreshed at most this often.
const TOUCH_MS: u64 = 15_000;

/// What a source reports about one pane, in the shared vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PaneState {
    Running,
    /// Blocked on an approval, question or permission.
    NeedsYou,
    /// Finished, and nobody has looked yet.
    Ready,
    Idle,
    /// No output lately. Only a source that cannot tell *why* reports this
    /// (tmux): the tracker waits out a short pause before calling it done.
    Quiet,
    Unknown,
}

/// One agent pane at one moment. Metadata only, never terminal content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneObservation {
    /// Unique across sources and servers: `herdr:<socket>:<pane>`.
    pub key: String,
    pub agent: String,
    pub state: PaneState,
    pub cwd: Option<String>,
    /// The pane is the one in front within its multiplexer.
    pub focused: bool,
    /// When the pane last produced output, if the source can say.
    pub output_at: Option<u64>,
}

/// A `path_prefix` rule that names a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRule {
    pub path: String,
    pub project_id: String,
    pub priority: i64,
}

pub fn path_rules(rules: &[Rule]) -> Vec<PathRule> {
    rules
        .iter()
        .filter(|rule| rule.match_kind == "path_prefix")
        .filter_map(|rule| {
            let project_id = rule.project_id.clone()?;
            let path = expand_home(rule.pattern.trim())
                .trim_end_matches('/')
                .to_string();
            (!path.is_empty()).then_some(PathRule {
                path,
                project_id,
                priority: rule.priority,
            })
        })
        .collect()
}

fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => path.to_string(),
    }
}

/// The project whose folder contains `cwd`. The longest folder wins, then the
/// higher priority, so a sub-project beats the repository around it.
pub fn project_for_cwd<'a>(rules: &'a [PathRule], cwd: &str) -> Option<&'a str> {
    let cwd = cwd.trim_end_matches('/');
    rules
        .iter()
        .filter(|rule| cwd == rule.path || cwd.starts_with(&format!("{}/", rule.path)))
        .max_by_key(|rule| (rule.path.len(), rule.priority))
        .map(|rule| rule.project_id.as_str())
}

/// How one source behaves, as far as the tracker cares.
#[derive(Debug, Clone, Copy)]
pub struct SourceTraits {
    /// The source reports when output last happened, so a stall can be seen.
    pub output_known: bool,
    /// Quiet this long after output means the turn is over.
    pub settle_ms: u64,
}

impl SourceTraits {
    pub fn of(source: Source) -> Self {
        match source {
            Source::Herdr => Self {
                output_known: false,
                settle_ms: 0,
            },
            Source::Tmux => Self {
                output_known: true,
                settle_ms: 60_000,
            },
        }
    }
}

pub struct Context<'a> {
    pub now: u64,
    /// The person is at the keyboard with a terminal in front: a tracked
    /// session, not idle, not away, not on a break.
    pub attending: bool,
    pub rules: &'a [PathRule],
}

/// What the live board shows for one pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveAgent {
    pub key: String,
    pub source: String,
    pub agent: String,
    pub state: PaneState,
    pub project_id: Option<String>,
    pub job_id: Option<String>,
    pub since: u64,
    pub focused: bool,
}

struct OpenJob {
    id: String,
    started_at: u64,
    state: JobState,
    stalled_at: Option<u64>,
    last_output_at: Option<u64>,
}

struct OpenFocus {
    row: i64,
    last_write: u64,
}

struct Track {
    source: Source,
    source_id: String,
    agent: String,
    state: PaneState,
    since: u64,
    project_id: Option<String>,
    focused: bool,
    /// The turn in progress, if any.
    job: Option<OpenJob>,
    /// The last turn that stopped and has not been reviewed.
    unreviewed: Option<String>,
    quiet_since: Option<u64>,
    focus: Option<OpenFocus>,
    touched: u64,
}

#[derive(Default)]
pub struct Tracker {
    panes: HashMap<String, Track>,
}

/// What a round of observations changed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// A job opened, stopped, changed state or was reviewed.
    pub jobs: bool,
    /// The person's time in an agent's pane changed.
    pub focus: bool,
    /// Anything the live board shows.
    pub live: bool,
}

impl Outcome {
    pub fn any(self) -> bool {
        self.jobs || self.focus || self.live
    }
}

impl Tracker {
    pub fn live(&self) -> Vec<LiveAgent> {
        let mut agents: Vec<LiveAgent> = self
            .panes
            .iter()
            .map(|(key, track)| LiveAgent {
                key: key.clone(),
                source: track.source.as_str().to_string(),
                agent: track.agent.clone(),
                // A finished turn nobody has looked at is ready for review,
                // whatever the source calls its quiet pane.
                state: match (&track.job, &track.unreviewed, track.state) {
                    (None, Some(_), _) => PaneState::Ready,
                    (None, None, PaneState::Quiet) => PaneState::Idle,
                    (_, _, state) => state,
                },
                project_id: track.project_id.clone(),
                job_id: track.job.as_ref().map(|job| job.id.clone()),
                since: track.since,
                focused: track.focused,
            })
            .collect();
        agents.sort_by(|a, b| a.key.cmp(&b.key));
        agents
    }

    /// Drops every pane of a source (its extension was turned off, or its
    /// connection went away) and closes whatever was open.
    pub fn forget_source(
        &mut self,
        conn: &Connection,
        source_id: &str,
        now: u64,
    ) -> Result<Outcome, String> {
        let mut outcome = Outcome::default();
        self.close_missing(conn, source_id, &HashSet::new(), now, &mut outcome)?;
        Ok(outcome)
    }

    fn close_missing(
        &mut self,
        conn: &Connection,
        source_id: &str,
        seen: &HashSet<&str>,
        now: u64,
        outcome: &mut Outcome,
    ) -> Result<(), String> {
        let gone: Vec<String> = self
            .panes
            .iter()
            .filter(|(key, track)| track.source_id == source_id && !seen.contains(key.as_str()))
            .map(|(key, _)| key.clone())
            .collect();
        for key in gone {
            if let Some(mut track) = self.panes.remove(&key) {
                close_pane(conn, &mut track, now, outcome)?;
            }
        }
        Ok(())
    }

    /// Applies one full snapshot of a source's agent panes.
    pub fn observe(
        &mut self,
        conn: &Connection,
        source_id: &str,
        source: Source,
        panes: &[PaneObservation],
        ctx: &Context<'_>,
    ) -> Result<Outcome, String> {
        let mut outcome = Outcome::default();
        let seen: HashSet<&str> = panes.iter().map(|pane| pane.key.as_str()).collect();

        for pane in panes {
            let track = self.panes.entry(pane.key.clone()).or_insert_with(|| {
                outcome.live = true;
                Track {
                    source,
                    source_id: source_id.to_string(),
                    agent: pane.agent.clone(),
                    state: PaneState::Unknown,
                    since: ctx.now,
                    project_id: None,
                    focused: false,
                    job: None,
                    unreviewed: None,
                    quiet_since: None,
                    focus: None,
                    touched: ctx.now,
                }
            });
            if track.agent != pane.agent {
                track.agent = pane.agent.clone();
                outcome.live = true;
            }
            let project = pane
                .cwd
                .as_deref()
                .and_then(|cwd| project_for_cwd(ctx.rules, cwd))
                .map(str::to_string);
            if track.job.is_none() && track.project_id != project {
                track.project_id = project.clone();
                outcome.live = true;
            }
            if track.focused != pane.focused {
                track.focused = pane.focused;
                outcome.live = true;
            }
            step_state(conn, track, pane, project.as_deref(), ctx, &mut outcome)?;
            step_focus(conn, track, pane, ctx, &mut outcome)?;
        }

        self.close_missing(conn, source_id, &seen, ctx.now, &mut outcome)?;
        Ok(outcome)
    }
}

fn close_pane(
    conn: &Connection,
    track: &mut Track,
    now: u64,
    outcome: &mut Outcome,
) -> Result<(), String> {
    outcome.live = true;
    if let Some(job) = track.job.take() {
        store::set_state(conn, &job.id, JobState::Gone, now, true, true)?;
        outcome.jobs = true;
    } else if let Some(id) = track.unreviewed.take() {
        store::mark_reviewed(conn, &id, now)?;
        outcome.jobs = true;
    }
    if let Some(focus) = track.focus.take() {
        store::extend_focus(conn, focus.row, now)?;
        outcome.focus = true;
    }
    Ok(())
}

fn step_state(
    conn: &Connection,
    track: &mut Track,
    pane: &PaneObservation,
    project: Option<&str>,
    ctx: &Context<'_>,
    outcome: &mut Outcome,
) -> Result<(), String> {
    let now = ctx.now;
    let traits = SourceTraits::of(track.source);
    if pane.state != PaneState::Quiet {
        track.quiet_since = None;
    }
    match pane.state {
        PaneState::Unknown => return Ok(()),
        PaneState::Running => {
            if track.job.is_none() {
                // Only a project's own agents are tracked: a cwd that matches
                // no project is never stored.
                if let Some(project) = project {
                    if let Some(previous) = track.unreviewed.take() {
                        store::mark_reviewed(conn, &previous, now)?;
                    }
                    let cwd = pane.cwd.as_deref().unwrap_or_default();
                    let id = store::insert_job(
                        conn,
                        &NewJob {
                            source: track.source.as_str(),
                            agent: &pane.agent,
                            pane_key: &pane.key,
                            project_id: project,
                            cwd,
                            output_known: traits.output_known,
                        },
                        now,
                    )?;
                    track.project_id = Some(project.to_string());
                    track.job = Some(OpenJob {
                        id,
                        started_at: now,
                        state: JobState::Running,
                        stalled_at: None,
                        last_output_at: traits.output_known.then_some(now),
                    });
                    track.touched = now;
                    outcome.jobs = true;
                }
            } else if let Some(job) = track.job.as_mut() {
                if job.state == JobState::NeedsYou {
                    store::set_state(conn, &job.id, JobState::Running, now, false, false)?;
                    job.state = JobState::Running;
                    outcome.jobs = true;
                }
            }
            if let Some(job) = track.job.as_mut() {
                step_output(conn, job, pane, traits, now, outcome)?;
            }
        }
        PaneState::NeedsYou => {
            if let Some(job) = track.job.as_mut() {
                if job.state != JobState::NeedsYou {
                    store::set_state(conn, &job.id, JobState::NeedsYou, now, false, false)?;
                    job.state = JobState::NeedsYou;
                    outcome.jobs = true;
                }
            }
        }
        PaneState::Ready | PaneState::Idle | PaneState::Quiet => {
            if let Some(job) = track.job.as_ref() {
                let (state, at) = if pane.state == PaneState::Quiet {
                    let since = *track.quiet_since.get_or_insert(now);
                    if now.saturating_sub(since) < traits.settle_ms {
                        // A pause, not an end: the agent may be thinking.
                        track.state = PaneState::Running;
                        return Ok(());
                    }
                    let stopped = pane.output_at.unwrap_or(since).min(now).max(job.started_at);
                    (JobState::Ready, stopped)
                } else if pane.state == PaneState::Idle {
                    (JobState::Idle, now)
                } else {
                    (JobState::Ready, now)
                };
                let id = job.id.clone();
                let seen = state == JobState::Idle;
                store::set_state(conn, &id, state, at, true, seen)?;
                track.job = None;
                if !seen {
                    track.unreviewed = Some(id);
                }
                outcome.jobs = true;
            } else if pane.state == PaneState::Idle {
                if let Some(id) = track.unreviewed.take() {
                    store::mark_reviewed(conn, &id, now)?;
                    outcome.jobs = true;
                }
            }
        }
    }

    if track.state != pane.state {
        track.state = if pane.state == PaneState::Quiet && track.job.is_some() {
            PaneState::Running
        } else {
            pane.state
        };
        track.since = now;
        outcome.live = true;
    }
    if let Some(job) = track.job.as_ref() {
        if now.saturating_sub(track.touched) >= TOUCH_MS {
            store::touch(conn, &job.id, now)?;
            track.touched = now;
        }
    }
    Ok(())
}

/// G6: a working stretch with no output for the stall cut stops counting. Only
/// a source that reports output can tell, and the cut is applied back to when
/// the silence began.
fn step_output(
    conn: &Connection,
    job: &mut OpenJob,
    pane: &PaneObservation,
    traits: SourceTraits,
    now: u64,
    outcome: &mut Outcome,
) -> Result<(), String> {
    if !traits.output_known {
        return Ok(());
    }
    if let Some(at) = pane.output_at {
        if job.last_output_at.is_none_or(|last| at > last) {
            job.last_output_at = Some(at);
            store::set_last_output(conn, &job.id, at)?;
        }
    }
    let last = job.last_output_at.unwrap_or(job.started_at);
    match job.stalled_at {
        None if now.saturating_sub(last) >= STALL_CUT_MS => {
            let at = last + STALL_CUT_MS;
            store::set_state(conn, &job.id, JobState::Stalled, at, false, false)?;
            job.stalled_at = Some(at);
            job.state = JobState::Stalled;
            outcome.jobs = true;
        }
        Some(stalled) if last > stalled => {
            store::set_state(conn, &job.id, JobState::Running, last, false, false)?;
            job.stalled_at = None;
            job.state = JobState::Running;
            outcome.jobs = true;
        }
        _ => {}
    }
    Ok(())
}

/// Time the person has an agent's pane in front of them becomes its own
/// record: it attributes their minutes to the agent's project, and tells how
/// closely they supervised a turn.
fn step_focus(
    conn: &Connection,
    track: &mut Track,
    pane: &PaneObservation,
    ctx: &Context<'_>,
    outcome: &mut Outcome,
) -> Result<(), String> {
    let now = ctx.now;
    let project = track.project_id.clone();
    let attending = ctx.attending && pane.focused;
    match (attending, project) {
        (true, Some(project)) => {
            match track.focus.as_mut() {
                None => {
                    let row = store::open_focus(conn, &pane.key, &project, &pane.agent, now)?;
                    track.focus = Some(OpenFocus {
                        row,
                        last_write: now,
                    });
                    outcome.focus = true;
                }
                Some(open) if now.saturating_sub(open.last_write) >= FOCUS_WRITE_MS => {
                    store::extend_focus(conn, open.row, now)?;
                    open.last_write = now;
                    outcome.focus = true;
                }
                Some(_) => {}
            }
            // Looking at a finished agent is reviewing it.
            if let Some(id) = track.unreviewed.take() {
                store::mark_reviewed(conn, &id, now)?;
                outcome.jobs = true;
            }
        }
        _ => {
            if let Some(open) = track.focus.take() {
                store::extend_focus(conn, open.row, now)?;
                outcome.focus = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000;
    const MIN: u64 = 60 * S;

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        openrize_core::migrations::run_migrations(&mut conn).unwrap();
        conn
    }

    fn rules() -> Vec<PathRule> {
        vec![
            PathRule {
                path: "/Users/me/Code/OpenRize".into(),
                project_id: "openrize".into(),
                priority: 0,
            },
            PathRule {
                path: "/Users/me/Code/OpenRize/cli".into(),
                project_id: "cli".into(),
                priority: 0,
            },
        ]
    }

    fn pane(state: PaneState) -> PaneObservation {
        PaneObservation {
            key: "herdr:default:p1".into(),
            agent: "claude".into(),
            state,
            cwd: Some("/Users/me/Code/OpenRize/src".into()),
            focused: false,
            output_at: None,
        }
    }

    fn observe(
        tracker: &mut Tracker,
        conn: &Connection,
        source: Source,
        panes: &[PaneObservation],
        now: u64,
        attending: bool,
    ) -> Outcome {
        let rules = rules();
        tracker
            .observe(
                conn,
                &format!("{}:default", source.as_str()),
                source,
                panes,
                &Context {
                    now,
                    attending,
                    rules: &rules,
                },
            )
            .unwrap()
    }

    fn only_job(conn: &Connection) -> store::JobRow {
        let jobs = store::jobs_in(conn, 0, u64::MAX / 2).unwrap();
        assert_eq!(jobs.len(), 1, "{jobs:?}");
        jobs.into_iter().next().unwrap()
    }

    #[test]
    fn the_longest_folder_names_the_project() {
        let rules = rules();
        assert_eq!(
            project_for_cwd(&rules, "/Users/me/Code/OpenRize/src"),
            Some("openrize")
        );
        assert_eq!(
            project_for_cwd(&rules, "/Users/me/Code/OpenRize/cli/src"),
            Some("cli")
        );
        // A folder that only shares a prefix of letters is not inside it.
        assert_eq!(project_for_cwd(&rules, "/Users/me/Code/OpenRize2"), None);
        assert_eq!(project_for_cwd(&rules, "/tmp/scratch"), None);
    }

    #[test]
    fn a_turn_runs_asks_a_question_finishes_and_is_reviewed() {
        let conn = db();
        let mut tracker = Tracker::default();
        let t0 = 1_000 * MIN;
        let step = |tracker: &mut Tracker, state, at, attending| {
            observe(tracker, &conn, Source::Herdr, &[pane(state)], at, attending)
        };

        assert!(step(&mut tracker, PaneState::Running, t0, false).jobs);
        step(&mut tracker, PaneState::NeedsYou, t0 + 40 * MIN, false);
        step(&mut tracker, PaneState::Running, t0 + 45 * MIN, false);
        step(&mut tracker, PaneState::Ready, t0 + 72 * MIN, false);

        let job = only_job(&conn);
        assert_eq!(job.project_id, "openrize");
        assert_eq!(job.stopped_at, Some(t0 + 72 * MIN));
        assert_eq!(job.reviewed_at, None);
        assert_eq!(job.state, JobState::Ready);

        // Looking at the pane is the review.
        let mut seen = pane(PaneState::Ready);
        seen.focused = true;
        let outcome = observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[seen],
            t0 + 76 * MIN,
            true,
        );
        assert!(outcome.jobs && outcome.focus);
        assert_eq!(only_job(&conn).reviewed_at, Some(t0 + 76 * MIN));
        assert_eq!(
            store::transitions(&conn, &job.id).unwrap(),
            vec![
                (t0, JobState::Running),
                (t0 + 40 * MIN, JobState::NeedsYou),
                (t0 + 45 * MIN, JobState::Running),
                (t0 + 72 * MIN, JobState::Ready),
            ]
        );
    }

    #[test]
    fn a_new_turn_reviews_the_last_one_and_opens_its_own_job() {
        let conn = db();
        let mut tracker = Tracker::default();
        let t0 = 1_000 * MIN;
        for (state, at) in [
            (PaneState::Running, t0),
            (PaneState::Ready, t0 + 10 * MIN),
            (PaneState::Running, t0 + 30 * MIN),
        ] {
            observe(
                &mut tracker,
                &conn,
                Source::Herdr,
                &[pane(state)],
                at,
                false,
            );
        }
        let jobs = store::jobs_in(&conn, 0, u64::MAX / 2).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].reviewed_at, Some(t0 + 30 * MIN));
        assert_eq!(jobs[1].started_at, t0 + 30 * MIN);
        assert_eq!(jobs[1].stopped_at, None);
    }

    #[test]
    fn working_straight_to_idle_means_it_was_watched() {
        let conn = db();
        let mut tracker = Tracker::default();
        observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[pane(PaneState::Running)],
            10 * MIN,
            false,
        );
        observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[pane(PaneState::Idle)],
            20 * MIN,
            false,
        );

        let job = only_job(&conn);
        assert_eq!(job.stopped_at, Some(20 * MIN));
        assert_eq!(job.reviewed_at, Some(20 * MIN));
    }

    #[test]
    fn a_folder_outside_every_project_is_never_stored() {
        let conn = db();
        let mut tracker = Tracker::default();
        let mut outsider = pane(PaneState::Running);
        outsider.cwd = Some("/tmp/scratch".into());
        outsider.focused = true;
        let outcome = observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[outsider],
            10 * MIN,
            true,
        );

        assert!(!outcome.jobs);
        assert!(store::jobs_in(&conn, 0, u64::MAX / 2).unwrap().is_empty());
        assert!(store::focus_in(&conn, 0, u64::MAX / 2).unwrap().is_empty());
        // The live board still knows an agent is there, with no project.
        let live = tracker.live();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].project_id, None);
        assert_eq!(live[0].state, PaneState::Running);
    }

    #[test]
    fn a_closed_pane_ends_its_job() {
        let conn = db();
        let mut tracker = Tracker::default();
        observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[pane(PaneState::Running)],
            10 * MIN,
            false,
        );
        let outcome = observe(&mut tracker, &conn, Source::Herdr, &[], 15 * MIN, false);

        assert!(outcome.jobs);
        let job = only_job(&conn);
        assert_eq!(job.state, JobState::Gone);
        assert_eq!(job.stopped_at, Some(15 * MIN));
        assert!(tracker.live().is_empty());
    }

    #[test]
    fn time_in_the_pane_is_recorded_only_while_attending() {
        let conn = db();
        let mut tracker = Tracker::default();
        let mut focused = pane(PaneState::Running);
        focused.focused = true;
        let t0 = 100 * MIN;

        observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[focused.clone()],
            t0,
            true,
        );
        observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[focused.clone()],
            t0 + 8 * S,
            true,
        );
        observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[focused.clone()],
            t0 + 20 * S,
            true,
        );
        // The terminal loses the front: the open stretch closes where it ended.
        observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[focused.clone()],
            t0 + 30 * S,
            false,
        );
        observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[focused.clone()],
            t0 + 5 * MIN,
            false,
        );
        // Back again: a second stretch.
        observe(
            &mut tracker,
            &conn,
            Source::Herdr,
            &[focused],
            t0 + 6 * MIN,
            true,
        );

        let rows = store::focus_in(&conn, 0, u64::MAX / 2).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].started_at, rows[0].ended_at), (t0, t0 + 30 * S));
        assert_eq!(rows[0].project_id, "openrize");
        assert_eq!(rows[1].started_at, t0 + 6 * MIN);
    }

    #[test]
    fn tmux_waits_out_a_pause_then_ends_the_turn_when_output_stopped() {
        let conn = db();
        let mut tracker = Tracker::default();
        let t0 = 500 * MIN;
        let tmux = |state, output_at| PaneObservation {
            key: "tmux:default:%3".into(),
            agent: "claude".into(),
            state,
            cwd: Some("/Users/me/Code/OpenRize".into()),
            focused: false,
            output_at: Some(output_at),
        };
        let at = |tracker: &mut Tracker, pane: PaneObservation, now| {
            observe(tracker, &conn, Source::Tmux, &[pane], now, false)
        };

        at(&mut tracker, tmux(PaneState::Running, t0), t0);
        // 20 s of silence: still the same turn.
        at(
            &mut tracker,
            tmux(PaneState::Quiet, t0 + 5 * S),
            t0 + 20 * S,
        );
        assert_eq!(only_job(&conn).stopped_at, None);
        // Output again, then silence that outlasts the settle time.
        at(
            &mut tracker,
            tmux(PaneState::Running, t0 + 40 * S),
            t0 + 40 * S,
        );
        at(
            &mut tracker,
            tmux(PaneState::Quiet, t0 + 50 * S),
            t0 + 70 * S,
        );
        at(
            &mut tracker,
            tmux(PaneState::Quiet, t0 + 50 * S),
            t0 + 140 * S,
        );

        let job = only_job(&conn);
        // It ended where the output did, not where the silence was noticed.
        assert_eq!(job.stopped_at, Some(t0 + 50 * S));
        assert_eq!(job.state, JobState::Ready);
        assert_eq!(job.reviewed_at, None);
        assert!(job.output_known);
    }

    #[test]
    fn a_working_stretch_with_no_output_is_cut_after_ten_minutes() {
        let conn = db();
        let mut tracker = Tracker::default();
        let t0 = 700 * MIN;
        let tmux = |output_at| PaneObservation {
            key: "tmux:default:%3".into(),
            agent: "claude".into(),
            // A source that keeps saying "working" without output, such as a
            // hook reporting the turn is open.
            state: PaneState::Running,
            cwd: Some("/Users/me/Code/OpenRize".into()),
            focused: false,
            output_at,
        };
        observe(
            &mut tracker,
            &conn,
            Source::Tmux,
            &[tmux(Some(t0))],
            t0,
            false,
        );
        observe(
            &mut tracker,
            &conn,
            Source::Tmux,
            &[tmux(Some(t0 + 3 * MIN))],
            t0 + 5 * MIN,
            false,
        );
        observe(
            &mut tracker,
            &conn,
            Source::Tmux,
            &[tmux(None)],
            t0 + 20 * MIN,
            false,
        );

        let job = only_job(&conn);
        assert_eq!(job.state, JobState::Stalled);
        let transitions = store::transitions(&conn, &job.id).unwrap();
        // Cut ten minutes after the last output, not when it was noticed.
        assert_eq!(
            transitions.last(),
            Some(&(t0 + 13 * MIN, JobState::Stalled))
        );

        // Output resumes: counting resumes from that output.
        observe(
            &mut tracker,
            &conn,
            Source::Tmux,
            &[tmux(Some(t0 + 25 * MIN))],
            t0 + 25 * MIN,
            false,
        );
        let transitions = store::transitions(&conn, &job.id).unwrap();
        assert_eq!(
            transitions.last(),
            Some(&(t0 + 25 * MIN, JobState::Running))
        );
    }
}
