//! The `breaks` table: one row per break decision.

use rusqlite::{params, Connection};
use serde::Serialize;

use super::engine::{streak_from_segments, BreakRecord};

/// A row as the Calendar shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakEntry {
    pub id: String,
    pub source: String,
    pub schedule_id: Option<String>,
    pub due_at: Option<u64>,
    pub planned_ms: u64,
    pub status: String,
    pub snoozes: u32,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub segment_id: Option<i64>,
}

pub fn insert(conn: &Connection, record: &BreakRecord, now: u64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO breaks
           (id, source, schedule_id, due_at, planned_ms, status, snoozes,
            started_at, ended_at, segment_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            record.id,
            record.source.as_str(),
            record.schedule_id,
            record.due_at.map(|value| value as i64),
            record.planned_ms as i64,
            record.status,
            record.snoozes,
            record.started_at.map(|value| value as i64),
            record.ended_at.map(|value| value as i64),
            record.segment_id,
            now as i64,
        ],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

pub fn set_ended(conn: &Connection, id: &str, ended_at: u64, now: u64) -> Result<(), String> {
    conn.execute(
        "UPDATE breaks SET ended_at = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, ended_at as i64, now as i64],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

pub fn set_planned(conn: &Connection, id: &str, planned_ms: u64, now: u64) -> Result<(), String> {
    conn.execute(
        "UPDATE breaks SET planned_ms = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, planned_ms as i64, now as i64],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// Keep the Calendar's segment and break decision in agreement, atomically.
pub(super) fn complete_sleep(
    conn: &Connection,
    id: &str,
    ended_at: u64,
    now: u64,
) -> Result<(), String> {
    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE segments SET ended_at = ?2
         WHERE id = (SELECT segment_id FROM breaks WHERE id = ?1) AND kind = 'break'",
            params![id, ended_at as i64],
        )
        .map_err(|error| error.to_string())?;
    set_ended(&transaction, id, ended_at, now)?;
    transaction.commit().map_err(|error| error.to_string())
}

/// The break now runs in this segment (a sleep closed the one it had).
pub(super) fn set_segment(
    conn: &Connection,
    id: &str,
    segment_id: Option<i64>,
    now: u64,
) -> Result<(), String> {
    conn.execute(
        "UPDATE breaks SET segment_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, segment_id, now as i64],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// Normal quit stamps ended_at in on_exit. A still-open taken row means the
/// app died without that callback: honor the active break's planned duration,
/// but never end it after `now`, so a quick relaunch cannot leave a future end.
pub fn close_orphans(conn: &Connection, now: u64) -> Result<(), String> {
    let mut statement = conn
        .prepare(
            "SELECT id, started_at, started_at + planned_ms FROM breaks
         WHERE status = 'taken' AND started_at IS NOT NULL AND ended_at IS NULL",
        )
        .map_err(|error| error.to_string())?;
    let unfinished = statement
        .query_map([], |row| {
            let started_at = row.get::<_, i64>(1)? as u64;
            let planned_end = row.get::<_, i64>(2)? as u64;
            Ok((
                row.get::<_, String>(0)?,
                planned_end.min(now).max(started_at),
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for (id, ended_at) in unfinished {
        complete_sleep(conn, &id, ended_at, now)?;
    }
    Ok(())
}

/// Breaks that started (or, if never taken, came due) in `[since, until)`.
pub fn list(conn: &Connection, since: u64, until: u64) -> Result<Vec<BreakEntry>, String> {
    let mut statement = conn
        .prepare(
            "SELECT id, source, schedule_id, due_at, planned_ms, status, snoozes,
                    started_at, ended_at, segment_id
             FROM breaks
             WHERE COALESCE(started_at, due_at, created_at) >= ?1
               AND COALESCE(started_at, due_at, created_at) < ?2
             ORDER BY COALESCE(started_at, due_at, created_at) ASC, id ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![since as i64, until as i64], |row| {
            Ok(BreakEntry {
                id: row.get(0)?,
                source: row.get(1)?,
                schedule_id: row.get(2)?,
                due_at: row.get::<_, Option<i64>>(3)?.map(|value| value as u64),
                planned_ms: row.get::<_, i64>(4)? as u64,
                status: row.get(5)?,
                snoozes: row.get(6)?,
                started_at: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
                ended_at: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
                segment_id: row.get(9)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

/// The work clock at launch, from today's segments (see `streak_from_segments`).
pub fn rebuild_streak(
    conn: &Connection,
    day_start: u64,
    now: u64,
    break_ms: u64,
) -> Result<u64, String> {
    let mut statement = conn
        .prepare(
            "SELECT kind, label, started_at, COALESCE(ended_at, ?2)
             FROM segments
             WHERE COALESCE(ended_at, ?2) > ?1
             ORDER BY started_at ASC, id ASC",
        )
        .map_err(|error| error.to_string())?;
    let segments = statement
        .query_map(params![day_start as i64, now as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?.max(day_start as i64) as u64,
                row.get::<_, i64>(3)? as u64,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(streak_from_segments(&segments, now, break_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::breaks::engine::{Source, STATUS_SKIPPED, STATUS_TAKEN};

    fn conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        openrize_core::migrations::run_migrations(&mut conn).unwrap();
        conn
    }

    fn record(id: &str, status: &'static str, started_at: Option<u64>) -> BreakRecord {
        BreakRecord {
            id: id.to_string(),
            source: Source::Interval,
            schedule_id: None,
            due_at: Some(1_000),
            planned_ms: 300_000,
            status,
            snoozes: 2,
            started_at,
            ended_at: None,
            segment_id: None,
        }
    }

    #[test]
    fn decisions_round_trip_through_the_table() {
        let conn = conn();
        insert(&conn, &record("a", STATUS_TAKEN, Some(2_000)), 5).unwrap();
        insert(&conn, &record("b", STATUS_SKIPPED, None), 6).unwrap();
        set_ended(&conn, "a", 302_000, 7).unwrap();
        set_planned(&conn, "a", 600_000, 8).unwrap();

        let all = list(&conn, 0, 1_000_000).unwrap();
        assert_eq!(all.len(), 2);
        // Ordered by when the decision happened: the skipped one came due at
        // 1000, the taken one started at 2000.
        assert_eq!(all[0].id, "b");
        assert_eq!(all[0].status, "skipped");
        assert_eq!(all[1].id, "a");
        assert_eq!(all[1].ended_at, Some(302_000));
        assert_eq!(all[1].planned_ms, 600_000);
        assert_eq!(all[1].snoozes, 2);
        assert_eq!(list(&conn, 1_500, 1_000_000).unwrap().len(), 1);
    }

    #[test]
    fn an_unclean_shutdown_completes_the_break_at_its_planned_end() {
        let conn = conn();
        conn.execute(
            "INSERT INTO segments (id, app, title, kind, started_at, ended_at)
             VALUES (7, 'Break', 'Break', 'break', 2000, 90000)",
            [],
        )
        .unwrap();
        insert(&conn, &record("a", STATUS_TAKEN, Some(2_000)), 5).unwrap();
        conn.execute("UPDATE breaks SET segment_id = 7 WHERE id = 'a'", [])
            .unwrap();
        close_orphans(&conn, 400_000).unwrap();
        close_orphans(&conn, 500_000).unwrap(); // Reconciliation is idempotent.
        assert_eq!(list(&conn, 0, 10_000).unwrap()[0].ended_at, Some(302_000));
        assert_eq!(
            conn.query_row("SELECT ended_at FROM segments WHERE id = 7", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            302_000
        );
    }

    #[test]
    fn a_quick_relaunch_ends_the_break_at_the_restart_not_its_planned_end() {
        let conn = conn();
        conn.execute(
            "INSERT INTO segments (id, app, title, kind, started_at, ended_at)
             VALUES (7, 'Break', 'Break', 'break', 2000, 2000)",
            [],
        )
        .unwrap();
        insert(&conn, &record("a", STATUS_TAKEN, Some(2_000)), 5).unwrap();
        conn.execute("UPDATE breaks SET segment_id = 7 WHERE id = 'a'", [])
            .unwrap();
        close_orphans(&conn, 100_000).unwrap();
        close_orphans(&conn, 400_000).unwrap(); // Later launches leave it be.
        assert_eq!(list(&conn, 0, 10_000).unwrap()[0].ended_at, Some(100_000));
        assert_eq!(
            conn.query_row("SELECT ended_at FROM segments WHERE id = 7", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            100_000
        );
    }

    #[test]
    fn a_break_moves_to_a_new_segment() {
        let conn = conn();
        conn.execute(
            "INSERT INTO segments (id, app, title, kind, started_at)
             VALUES (9, 'Break', 'Break', 'break', 50000)",
            [],
        )
        .unwrap();
        insert(&conn, &record("a", STATUS_TAKEN, Some(2_000)), 5).unwrap();
        set_segment(&conn, "a", Some(9), 6).unwrap();
        assert_eq!(list(&conn, 0, 10_000).unwrap()[0].segment_id, Some(9));
    }

    #[test]
    fn startup_repairs_an_open_segment_but_leaves_normal_quit_and_stop_alone() {
        let conn = conn();
        conn.execute(
            "INSERT INTO segments (id, app, title, kind, started_at)
             VALUES (7, 'Break', 'Break', 'break', 2000)",
            [],
        )
        .unwrap();
        let mut active = record("crash", STATUS_TAKEN, Some(2_000));
        active.segment_id = Some(7);
        insert(&conn, &active, 5).unwrap();
        insert(&conn, &record("quit-or-stop", STATUS_TAKEN, Some(3_000)), 6).unwrap();
        set_ended(&conn, "quit-or-stop", 90_000, 90_000).unwrap();
        insert(&conn, &record("skipped", STATUS_SKIPPED, None), 7).unwrap();
        // Exercise the actual startup order: ActivityStore first closes open
        // segments at their start; breaks::init then reconciles decisions.
        let activity = crate::activity::ActivityStore::from_conn(conn).unwrap();
        close_orphans(activity.conn(), 600_000).unwrap();
        let all = list(activity.conn(), 0, 1_000_000).unwrap();
        assert_eq!(
            all.iter().find(|row| row.id == "crash").unwrap().ended_at,
            Some(302_000)
        );
        assert_eq!(
            all.iter()
                .find(|row| row.id == "quit-or-stop")
                .unwrap()
                .ended_at,
            Some(90_000)
        );
        assert_eq!(
            all.iter().find(|row| row.id == "skipped").unwrap().ended_at,
            None
        );
        assert_eq!(
            activity
                .conn()
                .query_row("SELECT ended_at FROM segments WHERE id = 7", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            302_000
        );
    }

    #[test]
    fn ordinary_sleep_leaves_the_gap_alone() {
        let conn = conn();
        conn.execute(
            "INSERT INTO segments (app, title, kind, started_at, ended_at)
             VALUES ('Code', 'Editor', 'activity', 2000, 90000)",
            [],
        )
        .unwrap();
        close_orphans(&conn, 600_000).unwrap();
        assert!(list(&conn, 0, 1_000_000).unwrap().is_empty());
        assert_eq!(
            conn.query_row("SELECT ended_at FROM segments", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            90_000
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM segments", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
    }

    #[test]
    fn the_work_clock_is_rebuilt_from_stored_segments() {
        let conn = conn();
        conn.execute_batch(
            "INSERT INTO segments (app, title, kind, label, started_at, ended_at)
               VALUES ('Code', 'a', 'activity', NULL, 1000000, 2200000);
             INSERT INTO segments (app, title, kind, label, started_at, ended_at)
               VALUES ('Break', 'Break', 'break', 'Break', 2200000, 2260000);
             INSERT INTO segments (app, title, kind, label, started_at, ended_at)
               VALUES ('Code', 'b', 'activity', NULL, 2260000, NULL);",
        )
        .unwrap();
        // 10 minutes since the break; the open segment runs to now.
        let streak = rebuild_streak(&conn, 0, 2_860_000, 300_000).unwrap();
        assert_eq!(streak, 600_000);
    }
}
