//! What the desktop app and `rize` share without Tauri: the domain models,
//! entry queries, where the app keeps its files, and the protocol and local
//! endpoint `rize` reaches the app through.

pub mod ipc;
pub mod models;
pub mod paths;
pub mod protocol;

/// Canonical installed app identity, also used by independent clients.
pub const APP_IDENTIFIER: &str = "com.offlinestudios.openrize";

use models::TimeEntry;
use rusqlite::{params_from_iter, types::Value, Connection, Row};
use serde::{Deserialize, Serialize};

/// The filter value meaning "no category / project / client".
pub const NONE: &str = "none";
/// The Log tab's ceiling. Anything bigger is a range to aggregate, not list.
pub const MAX_QUERY_ROWS: u32 = 5_000;
/// Search terms past this are ignored rather than growing the query.
const MAX_SEARCH_TERMS: usize = 8;

/// An app or a website: sites are keyed by domain, apps by name, matching how
/// the Apps page lists them.
pub const APP_KEY: &str = "COALESCE(NULLIF(s.domain, ''), s.app)";
fn err(error: rusqlite::Error) -> String {
    error.to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EntryFilter {
    /// Entries that start in `[start_ms, end_ms)`. The builder splits entries
    /// at local midnight, so a day range never cuts one in half.
    pub start_ms: u64,
    pub end_ms: u64,
    /// A category id, or `none` for uncategorized.
    pub category_id: Option<String>,
    /// A project id, or `none` for "No project".
    pub project_id: Option<String>,
    /// A client id, or `none` for time with no client.
    pub client_id: Option<String>,
    /// An app name or a website domain.
    pub app: Option<String>,
    /// `pending` (anything not approved yet) or `approved`.
    pub status: Option<String>,
    pub billable: Option<bool>,
    /// Words that must each appear in the description or a window title.
    pub search: Option<String>,
    /// `work` (the default: the person's own entries), `agent` (counted agent
    /// time only) or `all`. Work totals must never include agent entries, so
    /// leaving this out can only ever give work.
    pub scope: Option<String>,
}

pub struct Clause {
    pub sql: String,
    pub params: Vec<Value>,
}

/// `column IS NULL` for the `none` sentinel, else an equality.
fn id_condition(column: &str, value: &Option<String>, sql: &mut Vec<String>, out: &mut Vec<Value>) {
    match value.as_deref() {
        None | Some("") => {}
        Some(NONE) => sql.push(format!("{column} IS NULL")),
        Some(id) => {
            sql.push(format!("{column} = ?"));
            out.push(id.to_string().into());
        }
    }
}

/// Search terms, each a substring. `%` is dropped because LIKE would treat
/// it as a wildcard; `_` is kept, since matching any one character still
/// matches the literal underscore the user meant.
fn search_terms(search: &str) -> Vec<String> {
    search
        .split_whitespace()
        .map(|term| term.replace('%', ""))
        .filter(|term| !term.is_empty())
        .take(MAX_SEARCH_TERMS)
        .collect()
}

impl EntryFilter {
    /// WHERE conditions over `te` (time_entries) and `p`, its project, which
    /// every query LEFT JOINs.
    pub fn clause(&self) -> Result<Clause, String> {
        if self.end_ms > i64::MAX as u64 {
            return Err("range exceeds the supported timestamp".to_string());
        }
        if self.end_ms <= self.start_ms {
            return Err("the range must end after it starts".to_string());
        }
        let mut sql = vec![
            "te.deleted_at IS NULL".to_string(),
            "te.started_at >= ?".to_string(),
            "te.started_at < ?".to_string(),
        ];
        let mut values: Vec<Value> =
            vec![(self.start_ms as i64).into(), (self.end_ms as i64).into()];

        match self.scope.as_deref() {
            None | Some("") | Some("work") => sql.push("te.source != 'agent'".to_string()),
            Some("agent") => sql.push("te.source = 'agent'".to_string()),
            Some("all") => {}
            Some(other) => return Err(format!("unknown scope {other}")),
        }
        id_condition("te.category_id", &self.category_id, &mut sql, &mut values);
        id_condition("te.project_id", &self.project_id, &mut sql, &mut values);
        id_condition("p.client_id", &self.client_id, &mut sql, &mut values);

        if let Some(app) = self.app.as_deref().filter(|app| !app.is_empty()) {
            sql.push(format!(
                "EXISTS (SELECT 1 FROM segments s WHERE s.entry_id = te.id AND s.kind != 'break' AND {APP_KEY} = ?)"
            ));
            values.push(app.to_string().into());
        }
        match self.status.as_deref() {
            None | Some("") => {}
            Some("approved") => sql.push("te.status = 'approved'".to_string()),
            Some("pending") => sql.push("te.status != 'approved'".to_string()),
            Some(other) => return Err(format!("unknown status filter {other}")),
        }
        if let Some(billable) = self.billable {
            sql.push("te.billable = ?".to_string());
            values.push((billable as i64).into());
        }
        // The trigram index on window titles answers `LIKE '%term%'`.
        for term in search_terms(self.search.as_deref().unwrap_or("")) {
            let pattern = format!("%{term}%");
            sql.push(
                "(te.description LIKE ? OR te.id IN (
                   SELECT s.entry_id FROM segments s
                   WHERE s.entry_id IS NOT NULL
                     AND s.id IN (SELECT rowid FROM segments_fts WHERE title LIKE ?)))"
                    .to_string(),
            );
            values.push(pattern.clone().into());
            values.push(pattern.into());
        }

        Ok(Clause {
            sql: sql.join(" AND "),
            params: values,
        })
    }
}

pub fn time_entry_from_row(row: &Row<'_>) -> rusqlite::Result<TimeEntry> {
    Ok(TimeEntry {
        id: row.get(0)?,
        started_at: row.get::<_, i64>(1)? as u64,
        ended_at: row.get::<_, i64>(2)? as u64,
        description: row.get(3)?,
        category_id: row.get(4)?,
        project_id: row.get(5)?,
        status: row.get(6)?,
        approved_by: row.get(7)?,
        source: row.get(8)?,
        billable: row.get::<_, i64>(9)? != 0,
        invoice_id: row.get(10)?,
        created_at: row.get::<_, i64>(11)? as u64,
        updated_at: row.get::<_, i64>(12)? as u64,
        deleted_at: row.get::<_, Option<i64>>(13)?.map(|v| v as u64),
        description_origin: row.get(14)?,
        ai: None,
        dominant_app: None,
    })
}

pub fn query_entries(
    conn: &Connection,
    filter: &EntryFilter,
    limit: u32,
) -> Result<Vec<TimeEntry>, String> {
    let clause = filter.clause()?;
    let mut values = clause.params;
    values.push((limit.clamp(1, MAX_QUERY_ROWS) as i64).into());
    let mut stmt = conn
        .prepare(&format!(
            "SELECT te.id, te.started_at, te.ended_at, te.description, te.category_id, te.project_id,
                    te.status, te.approved_by, te.source, te.billable, te.invoice_id, te.created_at,
                    te.updated_at, te.deleted_at, te.description_origin
             FROM time_entries te LEFT JOIN projects p ON p.id = te.project_id
             WHERE {}
             ORDER BY te.started_at DESC, te.id ASC
             LIMIT ?;",
            clause.sql
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(params_from_iter(values), time_entry_from_row)
        .map_err(err)?;
    let entries: Vec<TimeEntry> = rows.collect::<rusqlite::Result<_>>().map_err(err)?;

    Ok(entries)
}
