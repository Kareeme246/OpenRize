//! Privacy-minimized read models, shared by the headless service and app core.
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::{query_entries, EntryFilter};

pub const MAX_LIST: u32 = 500;

/// Never creates a database, migrates it, or starts capture workers.
pub fn open_database(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA query_only = ON;")
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub entries: u64,
    pub pending: u64,
    pub tracked_ms: u64,
    pub billable_ms: u64,
}

/// Stored entry totals, not a claim about the GUI's live capture state.
pub fn status(conn: &Connection) -> Result<Status, String> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(status != 'approved'), 0),
                COALESCE(SUM(MAX(0, ended_at - started_at)), 0),
                COALESCE(SUM(CASE WHEN billable THEN MAX(0, ended_at - started_at) ELSE 0 END), 0)
         FROM time_entries WHERE deleted_at IS NULL;",
        [],
        |row| {
            Ok(Status {
                entries: row.get::<_, i64>(0)?.max(0) as u64,
                pending: row.get::<_, i64>(1)?.max(0) as u64,
                tracked_ms: row.get::<_, i64>(2)?.max(0) as u64,
                billable_ms: row.get::<_, i64>(3)?.max(0) as u64,
            })
        },
    )
    .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySummary {
    pub id: String,
    pub started_at: u64,
    pub duration_ms: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<EntryFields>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryFields {
    pub description: String,
    pub description_truncated: bool,
    pub project_id: Option<String>,
    pub billable: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPage {
    pub entries: Vec<EntrySummary>,
    pub count: usize,
    pub truncated: bool,
}

/// Inclusive CLI bounds translated to the same half-open domain filter the
/// GUI uses. Millisecond precision is explicit; the adapter rejects finer input.
pub fn entries(
    conn: &Connection,
    from: u64,
    to: u64,
    status: Option<String>,
    limit: u32,
    full: bool,
) -> Result<EntryPage, String> {
    if from > to || to >= i64::MAX as u64 || !(1..=MAX_LIST).contains(&limit) {
        return Err("invalid range or limit".into());
    }
    let filter = EntryFilter {
        start_ms: from,
        end_ms: to + 1,
        status,
        ..EntryFilter::default()
    };
    let mut rows = query_entries(conn, &filter, limit + 1)?;
    let truncated = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let entries: Vec<_> = rows
        .into_iter()
        .map(|entry| {
            let detail = full.then(|| {
                let description: String = entry.description.chars().take(4096).collect();
                let description_truncated = description.len() < entry.description.len();
                EntryFields {
                    description,
                    description_truncated,
                    project_id: entry.project_id,
                    billable: entry.billable,
                }
            });
            EntrySummary {
                id: entry.id,
                started_at: entry.started_at,
                duration_ms: entry.ended_at.saturating_sub(entry.started_at),
                status: entry.status,
                detail,
            }
        })
        .collect();
    Ok(EntryPage {
        count: entries.len(),
        entries,
        truncated,
    })
}
