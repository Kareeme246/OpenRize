//! Filtered entry queries and SQL aggregations behind My Timesheet, Time
//! Entries, Calendar Month, and the Projects page (P3).
//!
//! Totals and pivots are computed in SQL, following `compute_totals`, so a
//! year of entries never crosses IPC just to be summed. Day, week, and month
//! boundaries are local-time concepts: the caller passes bucket boundaries as
//! epoch milliseconds instead of Rust guessing a timezone, so DST days come
//! out as 23 or 25 hours on their own.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use rusqlite::{params, params_from_iter, types::Value, Connection};
use serde::{Deserialize, Serialize};

use crate::activity::time_entry_from_row;
use crate::models::TimeEntry;

/// The filter value meaning "no category / project / client".
pub const NONE: &str = "none";
/// Enough for a year by day, plus the closing boundary.
pub const MAX_BUCKETS: usize = 400;
/// The Log tab's ceiling. Anything bigger is a range to aggregate, not list.
pub const MAX_QUERY_ROWS: u32 = 5_000;
/// Search terms past this are ignored rather than growing the query.
const MAX_SEARCH_TERMS: usize = 8;

/// An app or a website: sites are keyed by domain, apps by name, matching how
/// the Apps page lists them.
const APP_KEY: &str = "COALESCE(NULLIF(s.domain, ''), s.app)";
/// A segment's share of its entry, clamped to the entry's bounds so app
/// totals add up to the same time as every other grouping (the open segment
/// ends with its entry).
const SEGMENT_MS: &str = "MAX(0, MIN(COALESCE(s.ended_at, te.ended_at), te.ended_at) - MAX(s.started_at, te.started_at))";

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
}

struct Clause {
    sql: String,
    params: Vec<Value>,
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
    fn clause(&self) -> Result<Clause, String> {
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

/// Entries matching a filter, newest first, with the AI summary and the
/// dominant app the list views show.
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
             ORDER BY te.started_at DESC
             LIMIT ?;",
            clause.sql
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(params_from_iter(values), time_entry_from_row)
        .map_err(err)?;
    let mut entries: Vec<TimeEntry> = rows.collect::<rusqlite::Result<_>>().map_err(err)?;

    let mut ai = crate::ai::store::summaries(conn, filter.start_ms, filter.end_ms)?;
    let mut apps = dominant_apps(conn, filter.start_ms, filter.end_ms)?;
    for entry in &mut entries {
        entry.ai = ai.remove(&entry.id);
        entry.dominant_app = apps.remove(&entry.id);
    }
    Ok(entries)
}

/// Entry id -> the app or site with most of its active time, for every entry
/// overlapping the range.
pub fn dominant_apps(
    conn: &Connection,
    start_ms: u64,
    end_ms: u64,
) -> Result<HashMap<String, String>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT s.entry_id, {APP_KEY} AS app, SUM({SEGMENT_MS}) AS ms
             FROM segments s JOIN time_entries te ON te.id = s.entry_id
             WHERE te.deleted_at IS NULL AND te.ended_at >= ?1 AND te.started_at <= ?2
               AND s.kind != 'break'
             GROUP BY s.entry_id, app;"
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(params![start_ms as i64, end_ms as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(err)?;
    let mut best: HashMap<String, (String, i64)> = HashMap::new();
    for row in rows {
        let (entry, app, ms) = row.map_err(err)?;
        let slot = best.entry(entry).or_insert_with(|| (app.clone(), ms));
        if ms > slot.1 {
            *slot = (app, ms);
        }
    }
    Ok(best.into_iter().map(|(id, (app, _))| (id, app)).collect())
}

/// What a pivot row is keyed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    Project,
    Client,
    Category,
    App,
    Status,
    None,
}

impl GroupBy {
    pub fn parse(value: &str) -> Result<Self, String> {
        Ok(match value {
            "project" => Self::Project,
            "client" => Self::Client,
            "category" => Self::Category,
            "app" => Self::App,
            "status" => Self::Status,
            "none" => Self::None,
            other => return Err(format!("unknown grouping {other}")),
        })
    }

    fn key_sql(self) -> &'static str {
        match self {
            Self::Project => "te.project_id",
            Self::Client => "p.client_id",
            Self::Category => "te.category_id",
            Self::App => APP_KEY,
            Self::Status => "CASE WHEN te.status = 'approved' THEN 'approved' ELSE 'pending' END",
            Self::None => "NULL",
        }
    }
}

/// One cell of a pivot: a group key in one bucket.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollupCell {
    /// The group's id (or app/status name); `None` is "No project",
    /// "Uncategorized", "No client", or the single group of `none`.
    pub key: Option<String>,
    /// Index into the buckets the caller passed.
    pub bucket: u32,
    pub ms: u64,
    pub entries: u32,
    pub approved_ms: u64,
    /// Entries waiting for review (status `pending`).
    pub pending: u32,
    pub billable_ms: u64,
}

/// Sums the filtered entries per group per bucket. `boundaries` holds n + 1
/// ascending epoch-ms values for n buckets; an entry lands in the bucket its
/// start falls in. Grouping by app sums segment time instead, so one entry
/// can contribute to several apps.
pub fn rollup(
    conn: &Connection,
    filter: &EntryFilter,
    boundaries: &[u64],
    group_by: GroupBy,
) -> Result<Vec<RollupCell>, String> {
    if boundaries.len() < 2 || boundaries.len() > MAX_BUCKETS + 1 {
        return Err(format!(
            "expected 2 to {} bucket boundaries, got {}",
            MAX_BUCKETS + 1,
            boundaries.len()
        ));
    }
    if boundaries.windows(2).any(|pair| pair[1] <= pair[0]) {
        return Err("bucket boundaries must ascend".to_string());
    }

    let clause = filter.clause()?;
    let buckets = serde_json::to_string(boundaries).map_err(|e| e.to_string())?;
    let mut values: Vec<Value> = vec![buckets.into()];
    values.extend(clause.params);

    let buckets_cte = "WITH b AS (
        SELECT CAST(key AS INTEGER) AS i,
               CAST(value AS INTEGER) AS lo,
               CAST(LEAD(value) OVER (ORDER BY key) AS INTEGER) AS hi
        FROM json_each(?)
    )";
    let key = group_by.key_sql();
    let sql = if group_by == GroupBy::App {
        format!(
            "{buckets_cte}
             SELECT {key} AS k, b.i,
                    SUM({SEGMENT_MS}),
                    COUNT(DISTINCT te.id),
                    SUM(CASE WHEN te.status = 'approved' THEN {SEGMENT_MS} ELSE 0 END),
                    COUNT(DISTINCT CASE WHEN te.status = 'pending' THEN te.id END),
                    SUM(CASE WHEN te.billable = 1 THEN {SEGMENT_MS} ELSE 0 END)
             FROM segments s
             JOIN time_entries te ON te.id = s.entry_id
             LEFT JOIN projects p ON p.id = te.project_id
             JOIN b ON te.started_at >= b.lo AND te.started_at < b.hi
             WHERE s.kind != 'break' AND {}
             GROUP BY k, b.i
             ORDER BY b.i;",
            clause.sql
        )
    } else {
        format!(
            "{buckets_cte}
             SELECT {key} AS k, b.i,
                    SUM(te.ended_at - te.started_at),
                    COUNT(*),
                    SUM(CASE WHEN te.status = 'approved' THEN te.ended_at - te.started_at ELSE 0 END),
                    SUM(CASE WHEN te.status = 'pending' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN te.billable = 1 THEN te.ended_at - te.started_at ELSE 0 END)
             FROM time_entries te
             LEFT JOIN projects p ON p.id = te.project_id
             JOIN b ON te.started_at >= b.lo AND te.started_at < b.hi
             WHERE {}
             GROUP BY k, b.i
             ORDER BY b.i;",
            clause.sql
        )
    };

    let mut stmt = conn.prepare(&sql).map_err(err)?;
    let rows = stmt
        .query_map(params_from_iter(values), |row| {
            Ok(RollupCell {
                key: row.get(0)?,
                bucket: row.get::<_, i64>(1)? as u32,
                ms: row.get::<_, i64>(2)?.max(0) as u64,
                entries: row.get::<_, i64>(3)? as u32,
                approved_ms: row.get::<_, i64>(4)?.max(0) as u64,
                pending: row.get::<_, i64>(5)? as u32,
                billable_ms: row.get::<_, i64>(6)?.max(0) as u64,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

// --- Export -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
}

impl ExportFormat {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            other => Err(format!("unknown export format {other}")),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

/// One exported row. Window titles and URLs are never included: an export
/// leaves the machine, and titles are the private part of the capture.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportRow {
    date: String,
    start: String,
    end: String,
    hours: f64,
    description: String,
    category: String,
    project: String,
    client: String,
    status: String,
    billable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub count: u32,
}

fn local(ms: u64) -> DateTime<Local> {
    DateTime::from_timestamp_millis(ms as i64)
        .unwrap_or_default()
        .with_timezone(&Local)
}

fn export_rows(conn: &Connection, filter: &EntryFilter) -> Result<Vec<ExportRow>, String> {
    let clause = filter.clause()?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT te.started_at, te.ended_at, te.description, c.name, p.name, cl.name,
                    te.status, te.billable
             FROM time_entries te
             LEFT JOIN projects p ON p.id = te.project_id
             LEFT JOIN categories c ON c.id = te.category_id
             LEFT JOIN clients cl ON cl.id = p.client_id
             WHERE {}
             ORDER BY te.started_at ASC;",
            clause.sql
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(params_from_iter(clause.params), |row| {
            let started = row.get::<_, i64>(0)? as u64;
            let ended = row.get::<_, i64>(1)? as u64;
            let status: String = row.get(6)?;
            Ok(ExportRow {
                date: local(started).format("%Y-%m-%d").to_string(),
                start: local(started).format("%H:%M").to_string(),
                end: local(ended).format("%H:%M").to_string(),
                hours: (ended.saturating_sub(started) as f64 / 3_600_000.0 * 100.0).round() / 100.0,
                description: row.get(2)?,
                category: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                project: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                client: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                status: if status == "approved" {
                    status
                } else {
                    "pending".to_string()
                },
                billable: row.get::<_, i64>(7)? != 0,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

/// Quotes a CSV field when it needs it, and defuses spreadsheet formulas: a
/// description starting with `=` would otherwise run when the file opens.
fn csv_field(value: &str) -> String {
    let guarded = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if guarded.contains([',', '"', '\n', '\r']) || guarded != guarded.trim() {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn render(rows: &[ExportRow], format: ExportFormat) -> Result<String, String> {
    match format {
        ExportFormat::Json => serde_json::to_string_pretty(rows).map_err(|e| e.to_string()),
        ExportFormat::Csv => {
            let mut out = String::from(
                "date,start,end,hours,description,category,project,client,status,billable\n",
            );
            for row in rows {
                let fields = [
                    row.date.clone(),
                    row.start.clone(),
                    row.end.clone(),
                    format!("{:.2}", row.hours),
                    row.description.clone(),
                    row.category.clone(),
                    row.project.clone(),
                    row.client.clone(),
                    row.status.clone(),
                    row.billable.to_string(),
                ];
                let line: Vec<String> = fields.iter().map(|field| csv_field(field)).collect();
                out.push_str(&line.join(","));
                out.push('\n');
            }
            Ok(out)
        }
    }
}

/// `openrize-entries-2026-09-21-to-2026-09-27.csv`, with ` (2)` and so on
/// rather than overwriting an earlier export.
fn export_path(dir: &Path, filter: &EntryFilter, format: ExportFormat) -> PathBuf {
    let first = local(filter.start_ms).format("%Y-%m-%d").to_string();
    let last = local(filter.end_ms.saturating_sub(1))
        .format("%Y-%m-%d")
        .to_string();
    let stem = if first == last {
        format!("openrize-entries-{first}")
    } else {
        format!("openrize-entries-{first}-to-{last}")
    };
    let extension = format.extension();
    let mut path = dir.join(format!("{stem}.{extension}"));
    let mut copy = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({copy}).{extension}"));
        copy += 1;
    }
    path
}

/// Writes the filtered entries into `dir` and returns where they went.
pub fn export_entries(
    conn: &Connection,
    filter: &EntryFilter,
    format: ExportFormat,
    dir: &Path,
) -> Result<ExportResult, String> {
    let rows = export_rows(conn, filter)?;
    let body = render(&rows, format)?;
    fs::create_dir_all(dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let path = export_path(dir, filter, format);
    fs::write(&path, body).map_err(|error| format!("could not write the export: {error}"))?;
    Ok(ExportResult {
        path: path.display().to_string(),
        count: rows.len() as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::ActivityStore;

    const MIN: u64 = 60_000;
    const HOUR: u64 = 60 * MIN;

    fn store() -> ActivityStore {
        ActivityStore::from_conn(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn category_id(conn: &Connection, name: &str) -> String {
        conn.query_row(
            "SELECT id FROM categories WHERE name = ?1;",
            [name],
            |row| row.get(0),
        )
        .unwrap()
    }

    /// Inserts an entry with one segment per `(app, domain, title, minutes)`.
    #[allow(clippy::too_many_arguments)]
    fn entry(
        conn: &Connection,
        id: &str,
        start: u64,
        minutes: u64,
        description: &str,
        category: Option<&str>,
        project: Option<&str>,
        status: &str,
        billable: bool,
        segments: &[(&str, Option<&str>, &str, u64)],
    ) {
        conn.execute(
            "INSERT INTO time_entries (id, started_at, ended_at, description, category_id, project_id, status, source, billable, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'auto', ?8, 0, 0);",
            params![
                id,
                start as i64,
                (start + minutes * MIN) as i64,
                description,
                category,
                project,
                status,
                billable as i64
            ],
        )
        .unwrap();
        let mut at = start;
        for (app, domain, title, mins) in segments {
            conn.execute(
                "INSERT INTO segments (app, title, kind, started_at, ended_at, domain, entry_id)
                 VALUES (?1, ?2, 'activity', ?3, ?4, ?5, ?6);",
                params![app, title, at as i64, (at + mins * MIN) as i64, domain, id],
            )
            .unwrap();
            at += mins * MIN;
        }
    }

    fn range(start: u64, end: u64) -> EntryFilter {
        EntryFilter {
            start_ms: start,
            end_ms: end,
            ..EntryFilter::default()
        }
    }

    /// Two days: day 0 has coding (OpenRize, approved, billable) and a
    /// pending Slack block; day 1 has an uncategorized browsing block.
    fn seeded() -> (ActivityStore, String, String) {
        let store = store();
        let conn = store.conn();
        let coding = category_id(conn, "Coding");
        let comms = category_id(conn, "Communication");
        conn.execute(
            "INSERT INTO clients (id, name, created_at, updated_at) VALUES ('acme', 'Acme', 0, 0);",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, client_id, name, color, created_at, updated_at)
             VALUES ('openrize', 'acme', 'OpenRize', '#75a4e5', 0, 0);",
            [],
        )
        .unwrap();
        entry(
            conn,
            "e1",
            HOUR,
            30,
            "Wired tray pause into the timer store",
            Some(&coding),
            Some("openrize"),
            "approved",
            true,
            &[
                ("Zed", None, "invoices.rs - OpenRize", 20),
                ("Safari", Some("docs.rs"), "serde - Rust", 10),
            ],
        );
        entry(
            conn,
            "e2",
            2 * HOUR,
            15,
            "Slack triage",
            Some(&comms),
            None,
            "pending",
            false,
            &[("Slack", None, "general", 15)],
        );
        entry(
            conn,
            "e3",
            25 * HOUR,
            45,
            "Mixed browsing",
            None,
            None,
            "pending",
            false,
            &[("Zen", Some("youtube.com"), "Music", 45)],
        );
        (store, coding, comms)
    }

    #[test]
    fn rollup_groups_by_category_per_day() {
        let (store, coding, comms) = seeded();
        let cells = rollup(
            store.conn(),
            &range(0, 48 * HOUR),
            &[0, 24 * HOUR, 48 * HOUR],
            GroupBy::Category,
        )
        .unwrap();
        let find = |key: Option<&str>, bucket: u32| {
            cells
                .iter()
                .find(|c| c.key.as_deref() == key && c.bucket == bucket)
                .cloned()
        };
        let code = find(Some(&coding), 0).unwrap();
        assert_eq!(code.ms, 30 * MIN);
        assert_eq!(code.approved_ms, 30 * MIN);
        assert_eq!(code.billable_ms, 30 * MIN);
        let slack = find(Some(&comms), 0).unwrap();
        assert_eq!(
            (slack.ms, slack.pending, slack.approved_ms),
            (15 * MIN, 1, 0)
        );
        let uncategorized = find(None, 1).unwrap();
        assert_eq!(uncategorized.ms, 45 * MIN);
        assert_eq!(cells.len(), 3);
    }

    #[test]
    fn rollup_by_app_splits_an_entry_across_its_apps() {
        let (store, _, _) = seeded();
        let cells = rollup(
            store.conn(),
            &range(0, 24 * HOUR),
            &[0, 24 * HOUR],
            GroupBy::App,
        )
        .unwrap();
        let ms = |key: &str| {
            cells
                .iter()
                .find(|c| c.key.as_deref() == Some(key))
                .unwrap()
                .ms
        };
        assert_eq!(ms("Zed"), 20 * MIN);
        // Websites are keyed by domain, not by the browser.
        assert_eq!(ms("docs.rs"), 10 * MIN);
        assert_eq!(ms("Slack"), 15 * MIN);
    }

    #[test]
    fn rollup_by_client_and_status() {
        let (store, _, _) = seeded();
        let by_client = rollup(
            store.conn(),
            &range(0, 48 * HOUR),
            &[0, 48 * HOUR],
            GroupBy::Client,
        )
        .unwrap();
        let acme = by_client
            .iter()
            .find(|c| c.key.as_deref() == Some("acme"))
            .unwrap();
        assert_eq!(acme.ms, 30 * MIN);
        let none = by_client.iter().find(|c| c.key.is_none()).unwrap();
        assert_eq!(none.ms, 60 * MIN);

        let by_status = rollup(
            store.conn(),
            &range(0, 48 * HOUR),
            &[0, 48 * HOUR],
            GroupBy::Status,
        )
        .unwrap();
        let pending = by_status
            .iter()
            .find(|c| c.key.as_deref() == Some("pending"))
            .unwrap();
        assert_eq!((pending.ms, pending.entries), (60 * MIN, 2));
    }

    #[test]
    fn filters_narrow_every_query() {
        let (store, coding, _) = seeded();
        let conn = store.conn();
        let ids = |filter: EntryFilter| -> Vec<String> {
            query_entries(conn, &filter, 100)
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect()
        };
        let all = range(0, 48 * HOUR);
        assert_eq!(ids(all.clone()), vec!["e3", "e2", "e1"]);
        assert_eq!(
            ids(EntryFilter {
                category_id: Some(coding),
                ..all.clone()
            }),
            vec!["e1"]
        );
        assert_eq!(
            ids(EntryFilter {
                category_id: Some(NONE.to_string()),
                ..all.clone()
            }),
            vec!["e3"]
        );
        assert_eq!(
            ids(EntryFilter {
                project_id: Some(NONE.to_string()),
                ..all.clone()
            }),
            vec!["e3", "e2"]
        );
        assert_eq!(
            ids(EntryFilter {
                client_id: Some("acme".to_string()),
                ..all.clone()
            }),
            vec!["e1"]
        );
        assert_eq!(
            ids(EntryFilter {
                app: Some("docs.rs".to_string()),
                ..all.clone()
            }),
            vec!["e1"]
        );
        assert_eq!(
            ids(EntryFilter {
                status: Some("pending".to_string()),
                ..all.clone()
            }),
            vec!["e3", "e2"]
        );
        assert_eq!(
            ids(EntryFilter {
                billable: Some(true),
                ..all.clone()
            }),
            vec!["e1"]
        );
        assert!(query_entries(
            conn,
            &EntryFilter {
                status: Some("bogus".to_string()),
                ..all
            },
            10
        )
        .is_err());
    }

    #[test]
    fn search_finds_window_titles_and_descriptions() {
        let (store, _, _) = seeded();
        let conn = store.conn();
        let search = |text: &str| -> Vec<String> {
            query_entries(
                conn,
                &EntryFilter {
                    search: Some(text.to_string()),
                    ..range(0, 48 * HOUR)
                },
                100,
            )
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect()
        };
        // "where did I work on invoices.rs?": only a window title says so.
        assert_eq!(search("invoices.rs"), vec!["e1"]);
        // Substring, case-insensitive.
        assert_eq!(search("rize"), vec!["e1"]);
        assert_eq!(search("slack"), vec!["e2"]);
        // Every word must match somewhere.
        assert_eq!(search("tray serde"), vec!["e1"]);
        assert!(search("tray youtube").is_empty());
        // Short terms still work, through a scan.
        assert_eq!(search("rs"), vec!["e1"]);
        assert_eq!(search("100%").len(), 0);
    }

    #[test]
    fn entries_carry_their_dominant_app() {
        let (store, _, _) = seeded();
        let entries = query_entries(store.conn(), &range(0, 48 * HOUR), 100).unwrap();
        let app = |id: &str| {
            entries
                .iter()
                .find(|e| e.id == id)
                .and_then(|e| e.dominant_app.clone())
        };
        assert_eq!(app("e1").as_deref(), Some("Zed"));
        assert_eq!(app("e3").as_deref(), Some("youtube.com"));
    }

    #[test]
    fn bad_buckets_are_rejected() {
        let (store, _, _) = seeded();
        let conn = store.conn();
        let all = range(0, 48 * HOUR);
        assert!(rollup(conn, &all, &[0], GroupBy::None).is_err());
        assert!(rollup(conn, &all, &[10, 5], GroupBy::None).is_err());
        assert!(rollup(conn, &range(5, 5), &[0, 1], GroupBy::None).is_err());
    }

    #[test]
    fn csv_fields_are_quoted_and_defused() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a, b"), "\"a, b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
    }

    #[test]
    fn export_writes_the_filtered_rows_without_titles() {
        let (store, _, _) = seeded();
        let dir =
            std::env::temp_dir().join(format!("openrize-export-{}", crate::timers::now_epoch_ms()));
        let filter = EntryFilter {
            billable: Some(true),
            ..range(0, 48 * HOUR)
        };
        let result = export_entries(store.conn(), &filter, ExportFormat::Csv, &dir).unwrap();
        assert_eq!(result.count, 1);
        let body = fs::read_to_string(&result.path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("Wired tray pause into the timer store"));
        assert!(lines[1].contains("OpenRize"));
        assert!(lines[1].contains("Acme"));
        assert!(!body.contains("invoices.rs"));

        // A second export never overwrites the first.
        let again = export_entries(store.conn(), &filter, ExportFormat::Json, &dir).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&again.path).unwrap()).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_ne!(result.path, again.path);
        let _ = fs::remove_dir_all(&dir);
    }
}
