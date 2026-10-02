//! Joins the three records the billing rules need (the person's own entries,
//! the agent jobs the bridge saw, and the time spent in agents' panes), runs
//! them through `accounting`, and writes what counts back as reviewable
//! `time_entries` rows with `source = 'agent'`.
//!
//! Agent entries are *derived*: while one is pending (or building) it is
//! recomputed on every refresh, so a changed human entry or a newly seen
//! review moves it. An approved one is frozen like any approved entry, and a
//! deleted one is a tombstone that the same time is never recreated over. They
//! never overlap the person's own entries on the same project (the union), and
//! every work total reads `source != 'agent'`.

use std::collections::{BTreeMap, HashMap};

use chrono::{Duration, Local, NaiveTime, TimeZone};
use rusqlite::{params, Connection};
use serde::Serialize;

use super::accounting::{
    self, Confidence, Guardrails, Input, JobSpan, JobState, JobTimeline, Ledger, Policy,
    ProjectInput, StateSpan, TurnFacts,
};
use super::spans::{self, Span};
use super::store::{self, JobRow};

fn err(error: rusqlite::Error) -> String {
    error.to_string()
}

/// The marker on `time_entries.source`.
pub const SOURCE: &str = "agent";

/// The end of the calendar day (it turns at 5 AM) that `ms` falls in.
pub fn calendar_day_end(ms: u64) -> u64 {
    let local = Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .unwrap_or_else(Local::now);
    let day = (local - Duration::hours(5)).date_naive();
    let next =
        (day + Duration::days(1)).and_time(NaiveTime::from_hms_opt(5, 0, 0).unwrap_or_default());
    Local
        .from_local_datetime(&next)
        .earliest()
        .map_or(ms + 24 * 60 * 60 * 1000, |end| {
            end.timestamp_millis().max(0) as u64
        })
}

/// The calendar day `[start, end)` that contains `ms`.
pub fn calendar_day(ms: u64) -> (u64, u64) {
    let end = calendar_day_end(ms);
    let local = Local
        .timestamp_millis_opt(end as i64)
        .single()
        .unwrap_or_else(Local::now);
    let day = (local - Duration::hours(5)).date_naive() - Duration::days(1);
    let start = Local
        .from_local_datetime(&day.and_time(NaiveTime::from_hms_opt(5, 0, 0).unwrap_or_default()))
        .earliest()
        .map_or(end.saturating_sub(24 * 60 * 60 * 1000), |start| {
            start.timestamp_millis().max(0) as u64
        });
    (start, end)
}

/// The person's own time, from entries that are not agent entries.
struct Human {
    by_project: BTreeMap<String, Vec<Span>>,
    other: Vec<Span>,
    session: Vec<Span>,
}

fn load_human(conn: &Connection, window: Span, now: u64) -> Result<Human, String> {
    let mut stmt = conn
        .prepare(
            "SELECT project_id, started_at, ended_at, status FROM time_entries
             WHERE deleted_at IS NULL AND source != 'agent'
               AND ended_at > ?1 AND started_at < ?2;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![window.start as i64, window.end as i64], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(err)?;
    let mut projects: BTreeMap<String, Vec<Span>> = BTreeMap::new();
    let mut other = Vec::new();
    let mut all = Vec::new();
    for row in rows {
        let (project, started, ended, status) = row.map_err(err)?;
        let ended = if status == "building" {
            ended.max(now.min(window.end))
        } else {
            ended
        };
        let span = Span::new(started, ended);
        all.push(span);
        match project {
            Some(id) => projects.entry(id).or_default().push(span),
            None => other.push(span),
        }
    }
    let breaks = {
        let mut stmt = conn
            .prepare(
                "SELECT started_at, COALESCE(ended_at, ?3) FROM segments
                 WHERE kind = 'break' AND COALESCE(ended_at, ?3) > ?1 AND started_at < ?2;",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(
                params![window.start as i64, window.end as i64, now as i64],
                |row| {
                    Ok(Span::new(
                        row.get::<_, i64>(0)? as u64,
                        row.get::<_, i64>(1)? as u64,
                    ))
                },
            )
            .map_err(err)?;
        let found: Vec<Span> = rows.collect::<rusqlite::Result<_>>().map_err(err)?;
        spans::normalize(&found)
    };
    Ok(Human {
        by_project: projects
            .into_iter()
            .map(|(id, found)| (id, spans::normalize(&found)))
            .collect(),
        other: spans::normalize(&other),
        // G3: in a tracked session, and not on a break.
        session: spans::subtract(&spans::normalize(&all), &breaks),
    })
}

/// A job with its timeline and confidence, ready for the rules.
pub struct JobCtx {
    pub row: JobRow,
    pub timeline: JobTimeline,
}

/// The state timeline of a job, from its transitions.
fn states_of(row: &JobRow, transitions: &[(u64, JobState)], now: u64) -> Vec<StateSpan> {
    let mut states = Vec::new();
    for (index, (at, state)) in transitions.iter().enumerate() {
        let next = transitions.get(index + 1).map(|(next, _)| *next);
        let (end, open) = match next {
            Some(next) => (next, false),
            None if row.stopped_at.is_none() => (now.max(*at), true),
            // Ready waits for the person until they look.
            None if *state == JobState::Ready => match row.reviewed_at {
                Some(reviewed) => (reviewed, false),
                None => (now.max(*at), true),
            },
            None => (*at, false),
        };
        // A wait ends when the person reviews it, even if another state
        // follows (the agent was asked again before anyone looked).
        let end = if *state == JobState::Ready {
            row.reviewed_at
                .map_or(end, |reviewed| end.min(reviewed.max(*at)))
        } else {
            end
        };
        if end > *at {
            states.push(StateSpan {
                span: Span::new(*at, end),
                state: *state,
                open,
            });
        }
    }
    states
}

fn load_jobs(conn: &Connection, window: Span, now: u64) -> Result<Vec<JobCtx>, String> {
    let mut out = Vec::new();
    for row in store::jobs_in(conn, window.start, window.end)? {
        let transitions = store::transitions(conn, &row.id)?;
        let states = states_of(&row, &transitions, now);
        let focus = store::pane_focus(conn, &row.pane_key, row.started_at, u64::MAX / 2)?;
        let turn = spans::normalize(
            &states
                .iter()
                .filter(|s| {
                    matches!(
                        s.state,
                        JobState::Running | JobState::NeedsYou | JobState::Stalled
                    )
                })
                .map(|s| s.span)
                .collect::<Vec<_>>(),
        );
        let confidence = accounting::confidence(&TurnFacts {
            started_at: row.started_at,
            turn: &turn,
            stopped_at: row.stopped_at,
            focus: &focus,
            day_end: calendar_day_end(row.stopped_at.unwrap_or(row.started_at)),
        });
        out.push(JobCtx {
            timeline: JobTimeline {
                id: row.id.clone(),
                project_id: row.project_id.clone(),
                confidence,
                confirmed: row.confirmed,
                states,
                attended: focus,
            },
            row,
        });
    }
    Ok(out)
}

/// Everything one window's accounting produced.
pub struct Computed {
    pub window: Span,
    pub ledger: Ledger,
    pub jobs: Vec<JobCtx>,
}

pub fn compute(
    conn: &Connection,
    start: u64,
    end: u64,
    now: u64,
    guardrails: Guardrails,
) -> Result<Computed, String> {
    let window = Span::new(start, end);
    let human = load_human(conn, window, now)?;
    let jobs = load_jobs(conn, window, now)?;
    let input = Input {
        window,
        projects: human
            .by_project
            .iter()
            .map(|(id, you)| ProjectInput {
                id: id.clone(),
                you: you.clone(),
                policy: Policy::YouAndAgents,
            })
            .collect(),
        other_you: human.other,
        session: human.session,
        jobs: jobs.iter().map(|job| job.timeline.clone()).collect(),
        guardrails,
    };
    let ledger = accounting::account(&input);
    Ok(Computed {
        window,
        ledger,
        jobs,
    })
}

/// How a counted job's entries should read.
fn entry_status(job: &JobCtx, auto_accept: bool) -> (&'static str, Option<&'static str>) {
    if job.row.stopped_at.is_none() {
        return ("building", None);
    }
    if job.row.confirmed {
        return ("approved", Some("user"));
    }
    if job.timeline.confidence == Confidence::High && auto_accept {
        ("approved", Some("auto"))
    } else {
        ("pending", None)
    }
}

struct Existing {
    id: String,
    span: Span,
    status: String,
    deleted: bool,
    job_id: String,
}

/// Brings the window's agent entries in line with the ledger. Returns whether
/// any entry was created, moved, approved or removed.
pub fn materialize(
    conn: &mut Connection,
    computed: &Computed,
    auto_accept: bool,
    now: u64,
) -> Result<bool, String> {
    let window = computed.window;
    let tx = conn.transaction().map_err(err)?;
    let existing: Vec<Existing> = {
        let mut stmt = tx
            .prepare(
                "SELECT id, started_at, ended_at, status, deleted_at IS NOT NULL, agent_job_id
                 FROM time_entries
                 WHERE source = 'agent' AND started_at >= ?1 AND started_at < ?2
                   AND agent_job_id IS NOT NULL
                 ORDER BY started_at, id;",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![window.start as i64, window.end as i64], |row| {
                Ok(Existing {
                    id: row.get(0)?,
                    span: Span::new(row.get::<_, i64>(1)? as u64, row.get::<_, i64>(2)? as u64),
                    status: row.get(3)?,
                    deleted: row.get::<_, i64>(4)? != 0,
                    job_id: row.get(5)?,
                })
            })
            .map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)?
    };
    let mut by_job: HashMap<&str, Vec<&Existing>> = HashMap::new();
    for entry in &existing {
        by_job.entry(entry.job_id.as_str()).or_default().push(entry);
    }
    let counted: HashMap<&str, Vec<&JobSpan>> = {
        let mut map: HashMap<&str, Vec<&JobSpan>> = HashMap::new();
        for project in &computed.ledger.projects {
            for span in &project.agent_spans {
                map.entry(span.job_id.as_str()).or_default().push(span);
            }
        }
        map
    };
    let projects: HashMap<String, (bool, String)> = {
        let mut stmt = tx
            .prepare("SELECT id, billable_default, name FROM projects WHERE deleted_at IS NULL;")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get::<_, i64>(1)? != 0, row.get::<_, String>(2)?),
                ))
            })
            .map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)?
    };

    let mut changed = false;
    for job in &computed.jobs {
        let id = job.row.id.as_str();
        let desired = spans::normalize(
            &counted
                .get(id)
                .map(|found| {
                    found
                        .iter()
                        .map(|s| Span::new(s.started_at, s.ended_at))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        );
        let mine = by_job.get(id).cloned().unwrap_or_default();
        let frozen: Vec<Span> = spans::normalize(
            &mine
                .iter()
                .filter(|e| e.deleted || e.status == "approved")
                .map(|e| e.span)
                .collect::<Vec<_>>(),
        );
        let wanted = spans::subtract(&desired, &frozen);
        let mut live: Vec<&&Existing> = mine
            .iter()
            .filter(|e| !e.deleted && e.status != "approved")
            .collect();
        live.sort_by_key(|e| e.span);

        let (status, approved_by) = entry_status(job, auto_accept);
        let (billable, project_name) = projects
            .get(&job.row.project_id)
            .cloned()
            .unwrap_or((false, String::new()));
        let description = if project_name.is_empty() {
            format!("{} agent", job.row.agent)
        } else {
            format!("{} agent on {project_name}", job.row.agent)
        };

        for (index, span) in wanted.iter().enumerate() {
            match live.get(index) {
                Some(current) => {
                    let moved = current.span != *span;
                    let finalizes = current.status == "building" && status != "building";
                    if moved || finalizes {
                        tx.execute(
                            "UPDATE time_entries SET started_at = ?2, ended_at = ?3, status = ?4,
                                    approved_by = ?5, updated_at = ?6, billable = ?7
                             WHERE id = ?1;",
                            params![
                                current.id,
                                span.start as i64,
                                span.end as i64,
                                if current.status == "building" {
                                    status
                                } else {
                                    current.status.as_str()
                                },
                                if current.status == "building" {
                                    approved_by
                                } else {
                                    None
                                },
                                now as i64,
                                billable as i64,
                            ],
                        )
                        .map_err(err)?;
                        if finalizes && status == "approved" {
                            log_event(
                                &tx,
                                &current.id,
                                "auto_approved",
                                approved_by.unwrap_or("auto"),
                                now,
                            )?;
                        }
                        changed = true;
                    }
                }
                None => {
                    let entry_id = uuid::Uuid::now_v7().to_string();
                    tx.execute(
                        "INSERT INTO time_entries (id, started_at, ended_at, description, category_id,
                                project_id, status, approved_by, source, billable, created_at,
                                updated_at, description_origin, agent_job_id)
                         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, 'agent', ?8, ?9, ?9, 'template', ?10);",
                        params![
                            entry_id,
                            span.start as i64,
                            span.end as i64,
                            description,
                            job.row.project_id,
                            status,
                            approved_by,
                            billable as i64,
                            now as i64,
                            id,
                        ],
                    )
                    .map_err(err)?;
                    log_event(&tx, &entry_id, "created", "agent", now)?;
                    if status == "approved" {
                        log_event(
                            &tx,
                            &entry_id,
                            "auto_approved",
                            approved_by.unwrap_or("auto"),
                            now,
                        )?;
                    }
                    changed = true;
                }
            }
        }
        // Entries the ledger no longer asks for (a human entry now covers the
        // time, the turn was reconfigured) are derived data: drop them.
        for stale in live.iter().skip(wanted.len()) {
            tx.execute("DELETE FROM time_entries WHERE id = ?1;", params![stale.id])
                .map_err(err)?;
            changed = true;
        }
    }
    // Pending entries of a job that no longer exists in the window.
    let known: std::collections::HashSet<&str> = computed
        .jobs
        .iter()
        .map(|job| job.row.id.as_str())
        .collect();
    for entry in &existing {
        if !entry.deleted && entry.status != "approved" && !known.contains(entry.job_id.as_str()) {
            tx.execute("DELETE FROM time_entries WHERE id = ?1;", params![entry.id])
                .map_err(err)?;
            changed = true;
        }
    }
    for job in &computed.jobs {
        store::set_confidence(&tx, &job.row.id, job.timeline.confidence)?;
    }
    tx.commit().map_err(err)?;
    Ok(changed)
}

fn log_event(
    conn: &Connection,
    entry_id: &str,
    kind: &str,
    actor: &str,
    now: u64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO entry_events (id, entry_id, kind, actor, payload, at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5);",
        params![
            uuid::Uuid::now_v7().to_string(),
            entry_id,
            kind,
            actor,
            now as i64
        ],
    )
    .map(|_| ())
    .map_err(err)
}

/// Recomputes one window and writes its agent entries. Returns whether any
/// entry changed.
pub fn refresh(
    conn: &mut Connection,
    start: u64,
    end: u64,
    now: u64,
    auto_accept: bool,
) -> Result<bool, String> {
    let computed = compute(conn, start, end, now, Guardrails::STANDARD)?;
    materialize(conn, &computed, auto_accept, now)
}

// --- What the app shows ----------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String,
    pub agent: String,
    pub source: String,
    pub project_id: String,
    pub started_at: u64,
    pub stopped_at: Option<u64>,
    pub reviewed_at: Option<u64>,
    /// `running`, `needsYou`, `ready`, `reviewed` or `gone`.
    pub phase: &'static str,
    pub confidence: Confidence,
    pub confirmed: bool,
    pub segments: Vec<store::AgentSegment>,
    pub counted: Vec<JobSpan>,
    pub counted_ms: u64,
    pub pending_ms: u64,
    pub waiting_ms: u64,
}

fn phase_of(row: &JobRow) -> &'static str {
    match row.state {
        JobState::Running | JobState::Stalled => "running",
        JobState::NeedsYou => "needsYou",
        JobState::Ready => {
            if row.reviewed_at.is_some() {
                "reviewed"
            } else {
                "ready"
            }
        }
        JobState::Idle => "reviewed",
        JobState::Gone => "gone",
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub start: u64,
    pub end: u64,
    pub ledger: Ledger,
    pub jobs: Vec<JobView>,
}

pub fn report(conn: &Connection, start: u64, end: u64, now: u64) -> Result<Report, String> {
    let computed = compute(conn, start, end, now, Guardrails::STANDARD)?;
    let window = computed.window;
    let mut counted: HashMap<&str, Vec<JobSpan>> = HashMap::new();
    for project in &computed.ledger.projects {
        for span in &project.agent_spans {
            counted
                .entry(span.job_id.as_str())
                .or_default()
                .push(span.clone());
        }
    }
    let jobs = computed
        .jobs
        .iter()
        .map(|job| {
            let spans_of = counted
                .get(job.row.id.as_str())
                .cloned()
                .unwrap_or_default();
            let running = spans::clip(
                &spans::normalize(
                    &job.timeline
                        .states
                        .iter()
                        .filter(|s| s.state == JobState::Running)
                        .map(|s| s.span)
                        .collect::<Vec<_>>(),
                ),
                window.start,
                window.end,
            );
            let counted_ms = spans_of.iter().map(|s| s.ended_at - s.started_at).sum();
            let counts = job.timeline.confirmed || job.timeline.confidence >= Confidence::Medium;
            let waiting: u64 = spans::total(&spans::subtract(
                &spans::clip(
                    &spans::normalize(
                        &job.timeline
                            .states
                            .iter()
                            .filter(|s| s.state.is_waiting())
                            .map(|s| s.span)
                            .collect::<Vec<_>>(),
                    ),
                    window.start,
                    window.end,
                ),
                &job.timeline.attended,
            ));
            JobView {
                id: job.row.id.clone(),
                agent: job.row.agent.clone(),
                source: job.row.source.clone(),
                project_id: job.row.project_id.clone(),
                started_at: job.row.started_at,
                stopped_at: job.row.stopped_at,
                reviewed_at: job.row.reviewed_at,
                phase: phase_of(&job.row),
                confidence: job.timeline.confidence,
                confirmed: job.timeline.confirmed,
                segments: job
                    .timeline
                    .states
                    .iter()
                    .filter(|s| s.span.end > window.start && s.span.start < window.end)
                    .map(|s| store::AgentSegment {
                        started_at: s.span.start.max(window.start),
                        ended_at: s.span.end.min(window.end),
                        state: s.state,
                    })
                    .collect(),
                counted: spans_of,
                counted_ms,
                pending_ms: if counts { 0 } else { spans::total(&running) },
                waiting_ms: waiting,
            }
        })
        .collect();
    Ok(Report {
        start,
        end,
        ledger: computed.ledger,
        jobs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::store::NewJob;

    const MIN: u64 = 60_000;

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::run_migrations(&mut conn).unwrap();
        for (id, name) in [("a", "OpenRize"), ("b", "Acme"), ("c", "Ledgerly")] {
            conn.execute(
                "INSERT INTO projects (id, name, color, status, billable_default, created_at, updated_at)
                 VALUES (?1, ?2, '#fff', 'active', 1, 0, 0);",
                params![id, name],
            )
            .unwrap();
        }
        conn
    }

    fn human(conn: &Connection, id: &str, project: Option<&str>, from: u64, to: u64) {
        conn.execute(
            "INSERT INTO time_entries (id, started_at, ended_at, description, project_id, status,
                    source, billable, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'work', ?4, 'approved', 'auto', 1, 0, 0);",
            params![id, (from * MIN) as i64, (to * MIN) as i64, project],
        )
        .unwrap();
    }

    fn job(conn: &Connection, project: &str, pane: &str, run: (u64, u64)) -> String {
        let id = store::insert_job(
            conn,
            &NewJob {
                source: "herdr",
                agent: "claude",
                pane_key: pane,
                project_id: project,
                cwd: "/x",
                output_known: false,
            },
            run.0 * MIN,
        )
        .unwrap();
        store::set_state(conn, &id, JobState::Ready, run.1 * MIN, true, false).unwrap();
        id
    }

    fn agent_entries(conn: &Connection) -> Vec<(String, u64, u64, String, Option<String>)> {
        let mut stmt = conn
            .prepare(
                "SELECT project_id, started_at, ended_at, status, approved_by FROM time_entries
                 WHERE source = 'agent' AND deleted_at IS NULL ORDER BY started_at;",
            )
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64 / MIN,
                row.get::<_, i64>(2)? as u64 / MIN,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    /// The window the tests refresh: 0 to 10 hours of minutes.
    fn refresh_all(conn: &mut Connection, now: u64) -> bool {
        refresh(conn, 0, 600 * MIN, now * MIN, true).unwrap()
    }

    #[test]
    fn a_supervised_loop_becomes_an_approved_agent_entry_that_never_overlaps_the_person() {
        let mut conn = db();
        // A: prompt 0-5, review 50-60. B: 5-50. The agent ran 5-50 on A.
        human(&conn, "h1", Some("a"), 0, 5);
        human(&conn, "h2", Some("b"), 5, 50);
        human(&conn, "h3", Some("a"), 50, 60);
        let id = job(&conn, "a", "p1", (5, 50));
        // The person was in the pane right as it finished.
        let row = store::open_focus(&conn, "p1", "a", "claude", 50 * MIN).unwrap();
        store::extend_focus(&conn, row, 55 * MIN).unwrap();

        assert!(refresh_all(&mut conn, 60));
        assert_eq!(
            agent_entries(&conn),
            vec![(
                "a".to_string(),
                5,
                50,
                "approved".to_string(),
                Some("auto".to_string())
            )]
        );
        assert_eq!(
            store::job(&conn, &id).unwrap().unwrap().confidence,
            Some(Confidence::High)
        );
        // Nothing changes on the next pass.
        assert!(!refresh_all(&mut conn, 61));
    }

    #[test]
    fn an_unsupervised_turn_is_pending_and_a_confirmation_brings_it_in() {
        let mut conn = db();
        human(&conn, "h1", Some("b"), 0, 60);
        let id = job(&conn, "a", "p1", (10, 40));

        refresh_all(&mut conn, 60);
        assert!(
            agent_entries(&conn).is_empty(),
            "low confidence is not counted"
        );
        let shown = report(&conn, 0, 600 * MIN, 60 * MIN).unwrap();
        assert_eq!(shown.jobs[0].pending_ms, 30 * MIN);
        assert_eq!(shown.jobs[0].confidence, Confidence::Low);

        store::set_confirmed(&conn, &id).unwrap();
        refresh_all(&mut conn, 61);
        assert_eq!(
            agent_entries(&conn),
            vec![(
                "a".to_string(),
                10,
                40,
                "approved".to_string(),
                Some("user".to_string())
            )]
        );
    }

    #[test]
    fn a_medium_turn_waits_for_a_one_click_confirm() {
        let mut conn = db();
        human(&conn, "h1", Some("b"), 0, 120);
        job(&conn, "a", "p1", (10, 40));
        // Opened the pane three minutes after it finished.
        let row = store::open_focus(&conn, "p1", "a", "claude", 43 * MIN).unwrap();
        store::extend_focus(&conn, row, 44 * MIN).unwrap();

        refresh_all(&mut conn, 120);
        assert_eq!(
            agent_entries(&conn),
            vec![("a".to_string(), 10, 40, "pending".to_string(), None)]
        );
    }

    #[test]
    fn approved_agent_time_is_frozen_and_deleted_time_is_not_recreated() {
        let mut conn = db();
        human(&conn, "h1", Some("b"), 0, 120);
        let id = job(&conn, "a", "p1", (10, 40));
        let row = store::open_focus(&conn, "p1", "a", "claude", 40 * MIN).unwrap();
        store::extend_focus(&conn, row, 41 * MIN).unwrap();
        refresh_all(&mut conn, 120);
        assert_eq!(agent_entries(&conn).len(), 1);

        // The person deletes the entry: the same minutes stay deleted.
        conn.execute(
            "UPDATE time_entries SET deleted_at = 1 WHERE source = 'agent';",
            [],
        )
        .unwrap();
        refresh_all(&mut conn, 121);
        assert!(agent_entries(&conn).is_empty());
        let _ = id;
    }

    #[test]
    fn a_running_turn_builds_live_and_settles_when_it_stops() {
        let mut conn = db();
        human(&conn, "h1", Some("b"), 0, 120);
        let id = store::insert_job(
            &conn,
            &NewJob {
                source: "herdr",
                agent: "claude",
                pane_key: "p1",
                project_id: "a",
                cwd: "/x",
                output_known: false,
            },
            10 * MIN,
        )
        .unwrap();
        let row = store::open_focus(&conn, "p1", "a", "claude", 12 * MIN).unwrap();
        store::extend_focus(&conn, row, 14 * MIN).unwrap();

        refresh_all(&mut conn, 30);
        assert_eq!(
            agent_entries(&conn),
            vec![("a".to_string(), 10, 30, "building".to_string(), None)]
        );
        // It grows while the job runs.
        refresh_all(&mut conn, 40);
        assert_eq!(agent_entries(&conn)[0].2, 40);

        store::set_state(&conn, &id, JobState::Ready, 50 * MIN, true, false).unwrap();
        refresh_all(&mut conn, 51);
        assert_eq!(
            agent_entries(&conn),
            vec![(
                "a".to_string(),
                10,
                50,
                "approved".to_string(),
                Some("auto".to_string())
            )]
        );
    }

    #[test]
    fn calendar_days_turn_at_five_in_the_morning() {
        let (start, end) = calendar_day(
            Local
                .with_ymd_and_hms(2026, 9, 30, 14, 0, 0)
                .unwrap()
                .timestamp_millis() as u64,
        );
        let s = Local.timestamp_millis_opt(start as i64).unwrap();
        let e = Local.timestamp_millis_opt(end as i64).unwrap();
        assert_eq!(s.format("%Y-%m-%d %H:%M").to_string(), "2026-09-30 05:00");
        assert_eq!(e.format("%Y-%m-%d %H:%M").to_string(), "2026-10-01 05:00");
        // 2 AM still belongs to the day before.
        let (start, _) = calendar_day(
            Local
                .with_ymd_and_hms(2026, 10, 1, 2, 0, 0)
                .unwrap()
                .timestamp_millis() as u64,
        );
        let s = Local.timestamp_millis_opt(start as i64).unwrap();
        assert_eq!(s.format("%Y-%m-%d %H:%M").to_string(), "2026-09-30 05:00");
    }
}
