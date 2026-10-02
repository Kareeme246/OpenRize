//! Threads: the day as one row per project instead of one row per entry.
//!
//! The person's own time is split into per-project **visits**. An entry still
//! has one project, but its segments can each match a different project's
//! rules (a browser tab on another project's repo, a terminal in another
//! folder), and a visit is a stretch of one project's segments. Segments
//! without a project rule take the entry's own project. A stretch under two
//! minutes is absorbed into the one beside it, so a glance is not a visit.
//!
//! The visits of an entry partition exactly the entry's time, so the sum of
//! all visits is the work time, whatever the rules say. Everything else here
//! is drawn, never counted: agent rails (what each agent was doing) and the
//! in-flight band (a job was open on the project, whether or not the person
//! was there).
//!
//! Nothing is stored: visits are derived on demand from entries, segments and
//! rules, so a changed rule changes the picture at once.

use std::collections::BTreeMap;

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::activity::{segment_from_row, ActivitySegment};
use crate::agents::accounting::JobState;
use crate::agents::ledger;
use crate::ai::rules::{self, Rule};
use crate::entry_builder::SHORT_BREAK_THRESHOLD_MS;

/// A stretch on one project shorter than this is not a visit.
pub const MIN_VISIT_MS: u64 = 2 * 60 * 1000;
/// Most days one call returns (a week plus room).
pub const MAX_DAYS: usize = 32;

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// One segment's span and the project its rules gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub start: u64,
    pub project_id: Option<String>,
}

/// An entry reduced to what visits need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInput {
    pub id: String,
    pub project_id: Option<String>,
    pub start: u64,
    pub end: u64,
    /// The start of each activity segment and the project its rules gave it.
    pub pieces: Vec<Piece>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Visit {
    pub entry_id: String,
    pub project_id: Option<String>,
    pub started_at: u64,
    pub ended_at: u64,
}

impl Visit {
    fn ms(&self) -> u64 {
        self.ended_at - self.started_at
    }
}

/// Splits one entry into visits that together cover it exactly.
pub fn visits_of(entry: &EntryInput) -> Vec<Visit> {
    if entry.end <= entry.start {
        return Vec::new();
    }
    let mut pieces: Vec<&Piece> = entry.pieces.iter().collect();
    pieces.sort_by_key(|piece| piece.start);

    // Runs of one project, each running until the next one starts.
    let mut runs: Vec<Visit> = Vec::new();
    for piece in pieces {
        let project = piece
            .project_id
            .clone()
            .or_else(|| entry.project_id.clone());
        let start = piece.start.clamp(entry.start, entry.end);
        match runs.last() {
            Some(last) if last.project_id == project => {}
            _ => runs.push(Visit {
                entry_id: entry.id.clone(),
                project_id: project,
                started_at: if runs.is_empty() { entry.start } else { start },
                ended_at: entry.end,
            }),
        }
    }
    if runs.is_empty() {
        runs.push(Visit {
            entry_id: entry.id.clone(),
            project_id: entry.project_id.clone(),
            started_at: entry.start,
            ended_at: entry.end,
        });
    }
    for index in 1..runs.len() {
        let next = runs[index].started_at;
        runs[index - 1].ended_at = next;
    }

    // Absorb the shortest glance into its neighbor until none is left.
    while runs.len() > 1 {
        let Some(index) = runs
            .iter()
            .enumerate()
            .filter(|(_, run)| run.ms() < MIN_VISIT_MS)
            .min_by_key(|(_, run)| run.ms())
            .map(|(index, _)| index)
        else {
            break;
        };
        let glance = runs.remove(index);
        if index == 0 {
            runs[0].started_at = glance.started_at;
        } else {
            runs[index - 1].ended_at = glance.ended_at;
        }
        let mut merged: Vec<Visit> = Vec::with_capacity(runs.len());
        for run in runs {
            match merged.last_mut() {
                Some(last) if last.project_id == run.project_id => last.ended_at = run.ended_at,
                _ => merged.push(run),
            }
        }
        runs = merged;
    }
    runs
}

/// A run of the person's own time on one project with no real gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stretch {
    pub project_id: Option<String>,
    pub start: u64,
    pub end: u64,
}

/// Visits joined into stretches: same project, gap up to the short-break
/// threshold.
pub fn stretches_of(visits: &[Visit]) -> Vec<Stretch> {
    let mut sorted: Vec<&Visit> = visits.iter().collect();
    sorted.sort_by_key(|visit| visit.started_at);
    let mut stretches: Vec<Stretch> = Vec::new();
    for visit in sorted {
        match stretches.last_mut() {
            Some(last)
                if last.project_id == visit.project_id
                    && visit.started_at.saturating_sub(last.end) <= SHORT_BREAK_THRESHOLD_MS =>
            {
                last.end = last.end.max(visit.ended_at);
            }
            _ => stretches.push(Stretch {
                project_id: visit.project_id.clone(),
                start: visit.started_at,
                end: visit.ended_at,
            }),
        }
    }
    stretches
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    /// Times the person moved from one project to another without stopping.
    pub switches: u32,
    pub longest_ms: u64,
    pub longest_project_id: Option<String>,
}

pub fn focus_of(visits: &[Visit]) -> Focus {
    let stretches = stretches_of(visits);
    let mut focus = Focus::default();
    for (index, stretch) in stretches.iter().enumerate() {
        let ms = stretch.end - stretch.start;
        if ms > focus.longest_ms {
            focus.longest_ms = ms;
            focus.longest_project_id = stretch.project_id.clone();
        }
        // A real break between two stretches is a stop, not a switch.
        if index > 0
            && stretch.start.saturating_sub(stretches[index - 1].end) <= SHORT_BREAK_THRESHOLD_MS
        {
            focus.switches += 1;
        }
    }
    focus
}

/// What one agent was doing on a thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rail {
    pub job_id: String,
    pub agent: String,
    pub started_at: u64,
    pub ended_at: u64,
    pub state: JobState,
}

/// A stretch during which a job was open on the thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Band {
    pub job_id: String,
    pub started_at: u64,
    pub ended_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// `None` is time on no project.
    pub project_id: Option<String>,
    /// The person's own time here: the sum of the visits.
    pub you_ms: u64,
    /// Counted agent time here, on top of the person's own. Not in `you_ms`.
    pub agents_ms: u64,
    pub visits: Vec<Visit>,
    pub rails: Vec<Rail>,
    pub bands: Vec<Band>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayThreads {
    pub start: u64,
    pub end: u64,
    /// The person's work time: every visit, counted once.
    pub work_ms: u64,
    pub agents_ms: u64,
    pub focus: Focus,
    pub threads: Vec<Thread>,
}

fn clip_visit(visit: &Visit, start: u64, end: u64) -> Option<Visit> {
    let (from, to) = (visit.started_at.max(start), visit.ended_at.min(end));
    (to > from).then(|| Visit {
        started_at: from,
        ended_at: to,
        ..visit.clone()
    })
}

/// The visits of every non-agent entry that touches `[start, end)`, clipped to it.
pub fn visits_in(
    conn: &Connection,
    rules: &[Rule],
    start: u64,
    end: u64,
    now: u64,
) -> Result<Vec<Visit>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, started_at, ended_at, status FROM time_entries
             WHERE deleted_at IS NULL AND source != 'agent'
               AND ended_at > ?1 AND started_at < ?2
             ORDER BY started_at, id;",
        )
        .map_err(err)?;
    let entries: Vec<EntryInput> = stmt
        .query_map(params![start as i64, end as i64], |row| {
            let status: String = row.get(4)?;
            let began = row.get::<_, i64>(2)? as u64;
            let mut ended = row.get::<_, i64>(3)? as u64;
            if status == "building" {
                ended = ended.max(now.min(end));
            }
            Ok(EntryInput {
                id: row.get(0)?,
                project_id: row.get(1)?,
                start: began,
                end: ended,
                pieces: Vec::new(),
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(err)?;

    let mut segments = conn
        .prepare(
            "SELECT id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id,
                    url, domain, entry_id
             FROM segments
             WHERE entry_id = ?1 AND kind != 'break'
             ORDER BY started_at;",
        )
        .map_err(err)?;
    let mut visits = Vec::new();
    for mut entry in entries {
        let found: Vec<ActivitySegment> = segments
            .query_map([&entry.id], segment_from_row)
            .map_err(err)?
            .collect::<rusqlite::Result<_>>()
            .map_err(err)?;
        entry.pieces = found
            .iter()
            .map(|segment| Piece {
                start: segment.started_at,
                project_id: rules::project_of(rules, segment),
            })
            .collect();
        visits.extend(
            visits_of(&entry)
                .iter()
                .filter_map(|visit| clip_visit(visit, start, end)),
        );
    }
    Ok(visits)
}

fn thread_of(
    threads: &mut BTreeMap<Option<String>, Thread>,
    project: Option<String>,
) -> &mut Thread {
    threads.entry(project.clone()).or_insert_with(|| Thread {
        project_id: project,
        you_ms: 0,
        agents_ms: 0,
        visits: Vec::new(),
        rails: Vec::new(),
        bands: Vec::new(),
    })
}

/// One day's threads: visits, agent rails and bands, and focus stats.
pub fn day(conn: &Connection, start: u64, end: u64, now: u64) -> Result<DayThreads, String> {
    let rules = crate::ai::store::load_rules(conn).map_err(err)?;
    let visits = visits_in(conn, &rules, start, end, now)?;
    let report = ledger::report(conn, start, end, now)?;

    let mut threads: BTreeMap<Option<String>, Thread> = BTreeMap::new();
    for visit in &visits {
        let slot = thread_of(&mut threads, visit.project_id.clone());
        slot.you_ms += visit.ms();
        slot.visits.push(visit.clone());
    }
    for project in &report.ledger.projects {
        if project.agent_ms > 0 {
            thread_of(&mut threads, Some(project.project_id.clone())).agents_ms = project.agent_ms;
        }
    }
    for job in &report.jobs {
        let project = Some(job.project_id.clone());
        let slot = thread_of(&mut threads, project);
        for segment in &job.segments {
            if matches!(segment.state, JobState::Running | JobState::NeedsYou) {
                let (from, to) = (segment.started_at.max(start), segment.ended_at.min(end));
                if to > from {
                    slot.rails.push(Rail {
                        job_id: job.id.clone(),
                        agent: job.agent.clone(),
                        started_at: from,
                        ended_at: to,
                        state: segment.state,
                    });
                }
            }
        }
        let open_until = job.stopped_at.unwrap_or(now);
        let (from, to) = (job.started_at.max(start), open_until.min(end));
        if to > from {
            slot.bands.push(Band {
                job_id: job.id.clone(),
                started_at: from,
                ended_at: to,
            });
        }
    }

    let mut threads: Vec<Thread> = threads.into_values().collect();
    threads.retain(|t| t.you_ms > 0 || t.agents_ms > 0 || !t.rails.is_empty());
    threads.sort_by(|a, b| {
        a.project_id
            .is_none()
            .cmp(&b.project_id.is_none())
            .then((b.you_ms + b.agents_ms).cmp(&(a.you_ms + a.agents_ms)))
            .then(a.project_id.cmp(&b.project_id))
    });
    Ok(DayThreads {
        start,
        end,
        work_ms: visits.iter().map(Visit::ms).sum(),
        agents_ms: report.ledger.agent_ms,
        focus: focus_of(&visits),
        threads,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    fn piece(at: u64, project: Option<&str>) -> Piece {
        Piece {
            start: at * MIN,
            project_id: project.map(str::to_string),
        }
    }

    fn entry(project: Option<&str>, start: u64, end: u64, pieces: Vec<Piece>) -> EntryInput {
        EntryInput {
            id: "e".into(),
            project_id: project.map(str::to_string),
            start: start * MIN,
            end: end * MIN,
            pieces,
        }
    }

    fn shape(visits: &[Visit]) -> Vec<(Option<&str>, u64, u64)> {
        visits
            .iter()
            .map(|v| {
                (
                    v.project_id.as_deref(),
                    v.started_at / MIN,
                    v.ended_at / MIN,
                )
            })
            .collect()
    }

    fn covered(visits: &[Visit]) -> u64 {
        visits.iter().map(Visit::ms).sum()
    }

    #[test]
    fn an_entry_without_rule_hits_is_one_visit_on_its_project() {
        let e = entry(
            Some("a"),
            0,
            30,
            vec![piece(0, None), piece(10, None), piece(20, None)],
        );
        assert_eq!(shape(&visits_of(&e)), vec![(Some("a"), 0, 30)]);
    }

    #[test]
    fn a_rule_hit_on_another_project_becomes_its_own_visit() {
        let e = entry(
            Some("a"),
            0,
            60,
            vec![piece(0, None), piece(20, Some("b")), piece(35, None)],
        );
        let visits = visits_of(&e);

        assert_eq!(
            shape(&visits),
            vec![(Some("a"), 0, 20), (Some("b"), 20, 35), (Some("a"), 35, 60)]
        );
        assert_eq!(covered(&visits), 60 * MIN, "visits partition the entry");
    }

    #[test]
    fn a_glance_under_two_minutes_is_absorbed() {
        let e = entry(
            Some("a"),
            0,
            30,
            vec![piece(0, None), piece(10, Some("b")), piece(11, None)],
        );
        let visits = visits_of(&e);
        assert_eq!(shape(&visits), vec![(Some("a"), 0, 30)]);
    }

    #[test]
    fn a_glance_at_the_start_joins_the_next_visit() {
        let e = entry(None, 0, 30, vec![piece(0, Some("b")), piece(1, Some("a"))]);
        let visits = visits_of(&e);
        assert_eq!(shape(&visits), vec![(Some("a"), 0, 30)]);
    }

    #[test]
    fn unmatched_time_with_no_entry_project_is_a_thread_of_its_own() {
        let e = entry(None, 0, 40, vec![piece(0, None), piece(20, Some("a"))]);
        assert_eq!(
            shape(&visits_of(&e)),
            vec![(None, 0, 20), (Some("a"), 20, 40)]
        );
    }

    #[test]
    fn an_entry_with_no_segments_is_one_visit() {
        let e = entry(Some("a"), 5, 25, vec![]);
        assert_eq!(shape(&visits_of(&e)), vec![(Some("a"), 5, 25)]);
        assert!(visits_of(&entry(Some("a"), 5, 5, vec![])).is_empty());
    }

    #[test]
    fn focus_counts_switches_and_the_longest_stretch() {
        let visits: Vec<Visit> = [
            ("a", 0, 44),
            ("b", 44, 60),
            ("a", 60, 70),
            // A real break: not a switch.
            ("c", 90, 100),
            ("c", 101, 110),
        ]
        .iter()
        .map(|(p, s, e)| Visit {
            entry_id: "e".into(),
            project_id: Some(p.to_string()),
            started_at: s * MIN,
            ended_at: e * MIN,
        })
        .collect();

        let focus = focus_of(&visits);

        assert_eq!(focus.switches, 2);
        assert_eq!(focus.longest_ms, 44 * MIN);
        assert_eq!(focus.longest_project_id.as_deref(), Some("a"));
    }

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        openrize_core::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch(
            "INSERT INTO projects (id, client_id, name, color, created_at, updated_at)
               VALUES ('a', NULL, 'Alpha', '#fff', 0, 0), ('b', NULL, 'Beta', '#fff', 0, 0);
             INSERT INTO rules (id, match_kind, pattern, project_id, priority, origin, enabled, created_at, updated_at)
               VALUES ('r1', 'title_contains', 'Beta', 'b', 0, 'manual', 1, 0, 0);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_day_derives_threads_from_segments_and_rules_without_changing_work_time() {
        let conn = db();
        let m = MIN as i64;
        conn.execute(
            "INSERT INTO time_entries (id, started_at, ended_at, description, project_id, status, source, billable, created_at, updated_at)
             VALUES ('e1', 0, ?1, 'Work', 'a', 'approved', 'auto', 1, 0, 0);",
            [60 * m],
        )
        .unwrap();
        for (title, start, end) in [
            ("main.rs", 0, 20),
            ("Beta board", 20, 35),
            ("main.rs", 35, 60),
        ] {
            conn.execute(
                "INSERT INTO segments (app, title, kind, started_at, ended_at, entry_id)
                 VALUES ('Zed', ?1, 'activity', ?2, ?3, 'e1');",
                params![title, start * m, end * m],
            )
            .unwrap();
        }

        let day = day(&conn, 0, 24 * 60 * MIN, 24 * 60 * MIN).unwrap();

        assert_eq!(day.work_ms, 60 * MIN);
        assert_eq!(day.threads.len(), 2);
        assert_eq!(day.threads[0].project_id.as_deref(), Some("a"));
        assert_eq!(day.threads[0].you_ms, 45 * MIN);
        assert_eq!(day.threads[1].project_id.as_deref(), Some("b"));
        assert_eq!(day.threads[1].you_ms, 15 * MIN);
        assert_eq!(day.focus.switches, 2);
        assert_eq!(day.focus.longest_ms, 25 * MIN);
        assert_eq!(day.agents_ms, 0);
    }
}
