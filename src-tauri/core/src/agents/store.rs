//! Persistence for agent jobs: one row per turn, its state transitions, and
//! the time the person spent in an agent's pane. Everything here is metadata
//! (state, working directory of a known project, agent name, pane id); the
//! bridge never reads or stores terminal content.

use rusqlite::{params, Connection};
use serde::Serialize;

use super::accounting::{Confidence, JobState};
use super::spans::{self, Span};

fn err(error: rusqlite::Error) -> String {
    error.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    pub id: String,
    pub source: String,
    pub agent: String,
    pub pane_key: String,
    pub project_id: String,
    pub cwd: String,
    pub started_at: u64,
    pub stopped_at: Option<u64>,
    pub reviewed_at: Option<u64>,
    pub state: JobState,
    pub state_since: u64,
    pub output_known: bool,
    pub last_output_at: Option<u64>,
    pub last_seen_at: u64,
    pub confirmed: bool,
    pub confidence: Option<Confidence>,
}

const JOB_COLUMNS: &str = "id, source, agent, pane_key, project_id, cwd, started_at, stopped_at, \
     reviewed_at, state, state_since, output_known, last_output_at, last_seen_at, confirmed, confidence";

fn job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRow> {
    let state: String = row.get(9)?;
    let confidence: Option<String> = row.get(15)?;
    Ok(JobRow {
        id: row.get(0)?,
        source: row.get(1)?,
        agent: row.get(2)?,
        pane_key: row.get(3)?,
        project_id: row.get(4)?,
        cwd: row.get(5)?,
        started_at: row.get::<_, i64>(6)? as u64,
        stopped_at: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        reviewed_at: row.get::<_, Option<i64>>(8)?.map(|v| v as u64),
        state: JobState::parse(&state).unwrap_or(JobState::Gone),
        state_since: row.get::<_, i64>(10)? as u64,
        output_known: row.get::<_, i64>(11)? != 0,
        last_output_at: row.get::<_, Option<i64>>(12)?.map(|v| v as u64),
        last_seen_at: row.get::<_, i64>(13)? as u64,
        confirmed: row.get::<_, i64>(14)? != 0,
        confidence: confidence.as_deref().and_then(Confidence::parse),
    })
}

pub struct NewJob<'a> {
    pub source: &'a str,
    pub agent: &'a str,
    pub pane_key: &'a str,
    pub project_id: &'a str,
    pub cwd: &'a str,
    pub output_known: bool,
}

/// Opens a job that starts working at `now`.
pub fn insert_job(conn: &Connection, new: &NewJob<'_>, now: u64) -> Result<String, String> {
    let id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO agent_jobs (id, source, agent, pane_key, project_id, cwd, started_at, state,
                                 state_since, output_known, last_output_at, last_seen_at,
                                 created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?7, ?8, ?9, ?7, ?7, ?7);",
        params![
            id,
            new.source,
            new.agent,
            new.pane_key,
            new.project_id,
            new.cwd,
            now as i64,
            new.output_known as i64,
            new.output_known.then_some(now as i64),
        ],
    )
    .map_err(err)?;
    add_transition(conn, &id, now, JobState::Running)?;
    Ok(id)
}

pub fn add_transition(
    conn: &Connection,
    job_id: &str,
    at: u64,
    state: JobState,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO agent_transitions (job_id, at, state) VALUES (?1, ?2, ?3);",
        params![job_id, at as i64, state.as_str()],
    )
    .map(|_| ())
    .map_err(err)
}

/// Records a change of state. `stopped` ends the turn (the agent is no
/// longer working or blocked); `reviewed` closes the wait on the person.
pub fn set_state(
    conn: &Connection,
    job_id: &str,
    state: JobState,
    at: u64,
    stopped: bool,
    reviewed: bool,
) -> Result<(), String> {
    add_transition(conn, job_id, at, state)?;
    conn.execute(
        "UPDATE agent_jobs SET state = ?2, state_since = ?3, last_seen_at = ?3, updated_at = ?3,
                stopped_at = CASE WHEN ?4 THEN COALESCE(stopped_at, ?3) ELSE stopped_at END,
                reviewed_at = CASE WHEN ?5 THEN COALESCE(reviewed_at, ?3) ELSE reviewed_at END
         WHERE id = ?1;",
        params![job_id, state.as_str(), at as i64, stopped, reviewed],
    )
    .map(|_| ())
    .map_err(err)
}

/// Marks a stopped job as reviewed without changing its state.
pub fn mark_reviewed(conn: &Connection, job_id: &str, at: u64) -> Result<(), String> {
    conn.execute(
        "UPDATE agent_jobs SET reviewed_at = COALESCE(reviewed_at, ?2), updated_at = ?2
         WHERE id = ?1;",
        params![job_id, at as i64],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn touch(conn: &Connection, job_id: &str, now: u64) -> Result<(), String> {
    conn.execute(
        "UPDATE agent_jobs SET last_seen_at = ?2 WHERE id = ?1;",
        params![job_id, now as i64],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn set_last_output(conn: &Connection, job_id: &str, at: u64) -> Result<(), String> {
    conn.execute(
        "UPDATE agent_jobs SET last_output_at = ?2 WHERE id = ?1 AND COALESCE(last_output_at, 0) < ?2;",
        params![job_id, at as i64],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn set_confirmed(conn: &Connection, job_id: &str) -> Result<bool, String> {
    conn.execute(
        "UPDATE agent_jobs SET confirmed = 1 WHERE id = ?1;",
        params![job_id],
    )
    .map(|changed| changed > 0)
    .map_err(err)
}

pub fn set_confidence(
    conn: &Connection,
    job_id: &str,
    confidence: Confidence,
) -> Result<(), String> {
    conn.execute(
        "UPDATE agent_jobs SET confidence = ?2 WHERE id = ?1 AND COALESCE(confidence, '') != ?2;",
        params![job_id, confidence.as_str()],
    )
    .map(|_| ())
    .map_err(err)
}

#[cfg(test)]
pub fn job(conn: &Connection, id: &str) -> Result<Option<JobRow>, String> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        &format!("SELECT {JOB_COLUMNS} FROM agent_jobs WHERE id = ?1;"),
        params![id],
        job_from_row,
    )
    .optional()
    .map_err(err)
}

/// Jobs that never stopped: the app quit (or crashed) while they ran. They
/// end where the bridge last saw them, so a closed laptop never bills.
pub fn close_orphans(conn: &Connection) -> Result<usize, String> {
    let orphans: Vec<(String, i64)> = {
        let mut stmt = conn
            .prepare("SELECT id, last_seen_at FROM agent_jobs WHERE stopped_at IS NULL;")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)?
    };
    for (id, seen) in &orphans {
        set_state(conn, id, JobState::Gone, *seen as u64, true, true)?;
    }
    // Focus rows need no closing: each is extended by heartbeats, so it
    // already ends where the person was last seen in the pane.
    Ok(orphans.len())
}

/// Jobs whose life touches `[start, end)`: started inside it, still open, or
/// stopped (or reviewed) inside it.
pub fn jobs_in(conn: &Connection, start: u64, end: u64) -> Result<Vec<JobRow>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM agent_jobs
             WHERE started_at < ?2
               AND (stopped_at IS NULL OR stopped_at > ?1 OR COALESCE(reviewed_at, ?2) > ?1)
             ORDER BY started_at, id;"
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(params![start as i64, end as i64], job_from_row)
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

pub fn transitions(conn: &Connection, job_id: &str) -> Result<Vec<(u64, JobState)>, String> {
    let mut stmt = conn
        .prepare("SELECT at, state FROM agent_transitions WHERE job_id = ?1 ORDER BY at, rowid;")
        .map_err(err)?;
    let rows = stmt
        .query_map(params![job_id], |row| {
            let state: String = row.get(1)?;
            Ok((row.get::<_, i64>(0)? as u64, state))
        })
        .map_err(err)?;
    let mut out = Vec::new();
    for row in rows {
        let (at, state) = row.map_err(err)?;
        if let Some(state) = JobState::parse(&state) {
            out.push((at, state));
        }
    }
    Ok(out)
}

// --- Focus ------------------------------------------------------------------

pub fn open_focus(
    conn: &Connection,
    pane_key: &str,
    project_id: &str,
    agent: &str,
    now: u64,
) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO agent_focus (pane_key, project_id, agent, started_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?4);",
        params![pane_key, project_id, agent, now as i64],
    )
    .map_err(err)?;
    Ok(conn.last_insert_rowid())
}

pub fn extend_focus(conn: &Connection, id: i64, now: u64) -> Result<(), String> {
    conn.execute(
        "UPDATE agent_focus SET ended_at = ?2 WHERE id = ?1 AND ended_at < ?2;",
        params![id, now as i64],
    )
    .map(|_| ())
    .map_err(err)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRow {
    pub id: i64,
    pub pane_key: String,
    pub project_id: String,
    pub agent: String,
    pub started_at: u64,
    pub ended_at: u64,
}

pub fn focus_in(conn: &Connection, start: u64, end: u64) -> Result<Vec<FocusRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, pane_key, project_id, agent, started_at, ended_at FROM agent_focus
             WHERE ended_at > ?1 AND started_at < ?2 ORDER BY started_at, id;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![start as i64, end as i64], |row| {
            Ok(FocusRow {
                id: row.get(0)?,
                pane_key: row.get(1)?,
                project_id: row.get(2)?,
                agent: row.get(3)?,
                started_at: row.get::<_, i64>(4)? as u64,
                ended_at: row.get::<_, i64>(5)? as u64,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

/// Focus on one pane from `from` on, normalized.
pub fn pane_focus(
    conn: &Connection,
    pane_key: &str,
    from: u64,
    to: u64,
) -> Result<Vec<Span>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT started_at, ended_at FROM agent_focus
             WHERE pane_key = ?1 AND ended_at > ?2 AND started_at < ?3;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![pane_key, from as i64, to as i64], |row| {
            Ok(Span::new(
                row.get::<_, i64>(0)? as u64,
                row.get::<_, i64>(1)? as u64,
            ))
        })
        .map_err(err)?;
    let found: Vec<Span> = rows.collect::<rusqlite::Result<_>>().map_err(err)?;
    Ok(spans::normalize(&found))
}

/// Everything the board shows about one job, for the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSegment {
    pub started_at: u64,
    pub ended_at: u64,
    pub state: JobState,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::run_migrations(&mut conn).unwrap();
        conn
    }

    fn new_job(conn: &Connection, now: u64) -> String {
        insert_job(
            conn,
            &NewJob {
                source: "herdr",
                agent: "claude",
                pane_key: "herdr:default:p1",
                project_id: "p",
                cwd: "/Users/me/Code/OpenRize",
                output_known: false,
            },
            now,
        )
        .unwrap()
    }

    #[test]
    fn a_job_keeps_its_state_changes_in_order() {
        let conn = db();
        let id = new_job(&conn, 1_000);
        set_state(&conn, &id, JobState::NeedsYou, 5_000, false, false).unwrap();
        set_state(&conn, &id, JobState::Running, 9_000, false, false).unwrap();
        set_state(&conn, &id, JobState::Ready, 20_000, true, false).unwrap();

        let row = job(&conn, &id).unwrap().unwrap();
        assert_eq!(row.state, JobState::Ready);
        assert_eq!(row.stopped_at, Some(20_000));
        assert_eq!(row.reviewed_at, None);
        assert_eq!(
            transitions(&conn, &id).unwrap(),
            vec![
                (1_000, JobState::Running),
                (5_000, JobState::NeedsYou),
                (9_000, JobState::Running),
                (20_000, JobState::Ready),
            ]
        );

        set_state(&conn, &id, JobState::Idle, 30_000, true, true).unwrap();
        let row = job(&conn, &id).unwrap().unwrap();
        // The stop and the review keep the first time they were recorded.
        assert_eq!(row.stopped_at, Some(20_000));
        assert_eq!(row.reviewed_at, Some(30_000));
    }

    #[test]
    fn jobs_left_open_by_a_quit_end_where_the_bridge_last_saw_them() {
        let conn = db();
        let id = new_job(&conn, 1_000);
        touch(&conn, &id, 61_000).unwrap();
        assert_eq!(close_orphans(&conn).unwrap(), 1);

        let row = job(&conn, &id).unwrap().unwrap();
        assert_eq!(row.stopped_at, Some(61_000));
        assert_eq!(row.state, JobState::Gone);
        assert_eq!(close_orphans(&conn).unwrap(), 0);
    }

    #[test]
    fn focus_extends_and_reads_back_normalized() {
        let conn = db();
        let first = open_focus(&conn, "p1", "proj", "claude", 1_000).unwrap();
        extend_focus(&conn, first, 4_000).unwrap();
        let second = open_focus(&conn, "p1", "proj", "claude", 4_000).unwrap();
        extend_focus(&conn, second, 6_000).unwrap();

        assert_eq!(
            pane_focus(&conn, "p1", 0, 10_000).unwrap(),
            vec![Span::new(1_000, 6_000)]
        );
        assert!(pane_focus(&conn, "other", 0, 10_000).unwrap().is_empty());
        assert_eq!(focus_in(&conn, 0, 10_000).unwrap().len(), 2);
    }
}
