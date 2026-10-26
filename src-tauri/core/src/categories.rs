//! The Categories page: per-category totals, which also tell its Delete
//! whether a category has history to keep (see
//! `ActivityStore::remove_category`).

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::err;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryStats {
    pub category_id: String,
    pub entries: u32,
    pub approved: u32,
    pub total_ms: u64,
    /// Time in the page's selected range (the Time column).
    pub range_ms: u64,
    pub last_activity: Option<u64>,
}

/// Totals for every category with at least one entry.
pub fn category_stats(
    conn: &Connection,
    range_start: u64,
    range_end: u64,
) -> Result<Vec<CategoryStats>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT category_id,
                    COUNT(*),
                    SUM(CASE WHEN status = 'approved' THEN 1 ELSE 0 END),
                    SUM(ended_at - started_at),
                    SUM(CASE WHEN started_at >= ?1 AND started_at < ?2 THEN ended_at - started_at ELSE 0 END),
                    MAX(ended_at)
             FROM time_entries
             WHERE deleted_at IS NULL AND category_id IS NOT NULL
             GROUP BY category_id;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![range_start as i64, range_end as i64], |row| {
            let ms = |index: usize| -> rusqlite::Result<u64> {
                Ok(row.get::<_, i64>(index)?.max(0) as u64)
            };
            Ok(CategoryStats {
                category_id: row.get(0)?,
                entries: row.get::<_, i64>(1)? as u32,
                approved: row.get::<_, i64>(2)? as u32,
                total_ms: ms(3)?,
                range_ms: ms(4)?,
                last_activity: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}
