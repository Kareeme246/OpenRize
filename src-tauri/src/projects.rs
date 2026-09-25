//! The Projects page (P3): AI hints as T0 rules with a match preview,
//! per-project totals, project discovery from captured activity, and CSV
//! import.
//!
//! A project's `ai_hints` is free text: keywords, folders, repos, domains.
//! Each hint becomes one `origin = 'hint'` rule for the project, which is how
//! most project assignments resolve before the model is asked (design board
//! C§3b). Saving the hints replaces that project's hint rules.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::activity::{segment_from_row, ActivitySegment, ActivityStore, KIND_BREAK};
use crate::ai::rules::Rule;
use crate::models::{NewClient, NewProject};

pub const ORIGIN_HINT: &str = "hint";
/// Keywords shorter than this would match nearly every window title.
const MIN_KEYWORD_CHARS: usize = 3;
/// Discovery ignores anything seen for less than this in its window.
const DISCOVERY_MIN_MS: u64 = 30 * 60 * 1000;
const DISCOVERY_LIMIT: usize = 5;
const DISMISSED_KEY: &str = "project_suggestions_dismissed";

/// The muted palette (design board, theme T), for projects created without a
/// colour picker: imports and discovery.
pub const PALETTE: [&str; 10] = [
    "#75a4e5", "#56c2b1", "#e5995c", "#df84b5", "#e4817d", "#9aa6b4", "#bfa181", "#e7b447",
    "#66b1df", "#9b87df",
];

fn err(error: rusqlite::Error) -> String {
    error.to_string()
}

// --- Hints ------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// The text the user wrote.
    pub text: String,
    /// The rule it becomes, or `None` when it is too short to match on.
    pub rule: Option<(&'static str, String)>,
}

static DOMAIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[a-z0-9-]+(\.[a-z0-9-]+)*\.[a-z]{2,}$").expect("domain regex")
});

/// Classifies one hint: `~/Code/OpenRize` is a path, `github.com/a/b` a URL
/// prefix, `figma.com` a domain, and anything else a title keyword.
fn classify(text: &str) -> Option<(&'static str, String)> {
    if text.starts_with("~/") || text.starts_with('/') {
        return Some(("path_prefix", text.trim_end_matches('/').to_string()));
    }
    let bare = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .unwrap_or(text);
    if let Some((host, _)) = bare.split_once('/') {
        if DOMAIN.is_match(host) {
            return Some(("url_prefix", bare.trim_end_matches('/').to_string()));
        }
    }
    if DOMAIN.is_match(bare) {
        return Some(("domain", bare.to_ascii_lowercase()));
    }
    if text.chars().count() < MIN_KEYWORD_CHARS {
        return None;
    }
    Some(("title_contains", text.to_string()))
}

/// Splits hint text on commas, semicolons, and newlines, dropping blanks and
/// case-insensitive repeats.
pub fn parse_hints(text: &str) -> Vec<Hint> {
    let mut seen = HashSet::new();
    text.split([',', ';', '\n'])
        .map(str::trim)
        .filter(|hint| !hint.is_empty())
        .filter(|hint| seen.insert(hint.to_lowercase()))
        .map(|hint| Hint {
            text: hint.to_string(),
            rule: classify(hint),
        })
        .collect()
}

/// Replaces a project's hint rules with the ones its hints describe. `None`
/// (a cleared field, or a deleted project) just removes them.
pub fn sync_hint_rules(
    conn: &Connection,
    project_id: &str,
    hints: Option<&str>,
    now: u64,
) -> Result<(), String> {
    conn.execute(
        "UPDATE rules SET deleted_at = ?1, updated_at = ?1
         WHERE project_id = ?2 AND origin = ?3 AND deleted_at IS NULL;",
        params![now as i64, project_id, ORIGIN_HINT],
    )
    .map_err(err)?;
    for hint in parse_hints(hints.unwrap_or("")) {
        if let Some((kind, pattern)) = hint.rule {
            crate::ai::store::create_rule(
                conn,
                kind,
                &pattern,
                None,
                Some(project_id),
                ORIGIN_HINT,
                true,
                now,
            )?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HintMatch {
    pub hint: String,
    /// The rule kind the hint becomes; `None` when it is too short to use.
    pub match_kind: Option<String>,
    pub pattern: Option<String>,
    pub matched_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HintPreview {
    pub hints: Vec<HintMatch>,
    /// Time any hint matched, counted once ("would have matched 14h").
    pub total_ms: u64,
}

fn segments_since(conn: &Connection, since_ms: u64) -> Result<Vec<ActivitySegment>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id
             FROM segments
             WHERE started_at >= ?1 AND kind != ?2
             ORDER BY started_at ASC;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![since_ms as i64, KIND_BREAK], segment_from_row)
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

fn segment_ms(segment: &ActivitySegment) -> u64 {
    segment
        .ended_at
        .unwrap_or(segment.started_at)
        .saturating_sub(segment.started_at)
}

/// How much captured time since `since_ms` each hint would have matched.
pub fn preview_hints(conn: &Connection, hints: &str, since_ms: u64) -> Result<HintPreview, String> {
    let parsed = parse_hints(hints);
    let rules: Vec<Option<Rule>> = parsed
        .iter()
        .enumerate()
        .map(|(index, hint)| {
            hint.rule.as_ref().map(|(kind, pattern)| Rule {
                id: index.to_string(),
                match_kind: kind.to_string(),
                pattern: pattern.clone(),
                category_id: None,
                project_id: None,
                priority: 0,
                origin: ORIGIN_HINT.to_string(),
            })
        })
        .collect();

    let mut matched = vec![0u64; parsed.len()];
    let mut total_ms = 0u64;
    if rules.iter().any(Option::is_some) {
        for segment in segments_since(conn, since_ms)? {
            let ms = segment_ms(&segment);
            let mut any = false;
            for (index, rule) in rules.iter().enumerate() {
                if rule.as_ref().is_some_and(|rule| rule.matches(&segment)) {
                    matched[index] += ms;
                    any = true;
                }
            }
            if any {
                total_ms += ms;
            }
        }
    }

    Ok(HintPreview {
        hints: parsed
            .into_iter()
            .zip(matched)
            .map(|(hint, matched_ms)| HintMatch {
                hint: hint.text,
                match_kind: hint.rule.as_ref().map(|(kind, _)| kind.to_string()),
                pattern: hint.rule.map(|(_, pattern)| pattern),
                matched_ms,
            })
            .collect(),
        total_ms,
    })
}

// --- Stats and links ----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStats {
    pub project_id: String,
    pub entries: u32,
    pub total_ms: u64,
    /// Time in the page's selected range (the Time column).
    pub range_ms: u64,
    /// Time since the start of this month, for monthly budgets.
    pub month_ms: u64,
    pub billable_ms: u64,
    pub billable_month_ms: u64,
    pub last_activity: Option<u64>,
}

/// Totals for every project with at least one entry. Budgets are derived in
/// the view from these and the project's rate.
pub fn project_stats(
    conn: &Connection,
    range_start: u64,
    range_end: u64,
    month_start: u64,
) -> Result<Vec<ProjectStats>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT project_id,
                    COUNT(*),
                    SUM(ended_at - started_at),
                    SUM(CASE WHEN started_at >= ?1 AND started_at < ?2 THEN ended_at - started_at ELSE 0 END),
                    SUM(CASE WHEN started_at >= ?3 THEN ended_at - started_at ELSE 0 END),
                    SUM(CASE WHEN billable = 1 THEN ended_at - started_at ELSE 0 END),
                    SUM(CASE WHEN billable = 1 AND started_at >= ?3 THEN ended_at - started_at ELSE 0 END),
                    MAX(ended_at)
             FROM time_entries
             WHERE deleted_at IS NULL AND project_id IS NOT NULL
             GROUP BY project_id;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(
            params![range_start as i64, range_end as i64, month_start as i64],
            |row| {
                let ms = |index: usize| -> rusqlite::Result<u64> {
                    Ok(row.get::<_, i64>(index)?.max(0) as u64)
                };
                Ok(ProjectStats {
                    project_id: row.get(0)?,
                    entries: row.get::<_, i64>(1)? as u32,
                    total_ms: ms(2)?,
                    range_ms: ms(3)?,
                    month_ms: ms(4)?,
                    billable_ms: ms(5)?,
                    billable_month_ms: ms(6)?,
                    last_activity: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
                })
            },
        )
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

/// A rule (or an Apps-page default) that assigns time to a project.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRule {
    pub id: String,
    pub match_kind: String,
    pub pattern: String,
    /// manual | suggested | hint | app
    pub origin: String,
}

pub fn project_rules(conn: &Connection, project_id: &str) -> Result<Vec<ProjectRule>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, match_kind, pattern, origin FROM rules
             WHERE project_id = ?1 AND deleted_at IS NULL AND enabled = 1
             UNION ALL
             SELECT 'app:' || id, CASE WHEN kind = 'site' THEN 'domain' ELSE 'app' END, display_name, 'app'
             FROM apps
             WHERE default_project_id = ?1 AND deleted_at IS NULL;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(ProjectRule {
                id: row.get(0)?,
                match_kind: row.get(1)?,
                pattern: row.get(2)?,
                origin: row.get(3)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

// --- Discovery ------------------------------------------------------------------

/// "Create project pantry-tracker? (6h 10m, matched by ~/Code/pantry-tracker,
/// github.com/…/pantry-tracker)".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSuggestion {
    /// Lowercased name, the identity a dismissal remembers.
    pub key: String,
    pub name: String,
    pub ms: u64,
    /// The strongest matches, ready to become the project's AI hints.
    pub evidence: Vec<String>,
}

/// Folders people keep repositories in.
const CODE_ROOTS: &str = "Code|code|Projects|projects|src|dev|Developer|developer|repos|git|workspace|Workspace|Sites|work";

static TITLE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?:~|/Users/[^/\s]+|/home/[^/\s]+)/({CODE_ROOTS})/([A-Za-z0-9][A-Za-z0-9._-]*)"
    ))
    .expect("title path regex")
});

const FORGES: [&str; 4] = ["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];
/// First path segments on a forge that are pages, not owners.
const FORGE_PAGES: [&str; 18] = [
    "settings",
    "notifications",
    "pulls",
    "issues",
    "marketplace",
    "explore",
    "orgs",
    "topics",
    "login",
    "new",
    "search",
    "sponsors",
    "features",
    "about",
    "trending",
    "collections",
    "codespaces",
    "dashboard",
];

/// Editors whose window titles name the open folder.
const EDITORS: [&str; 12] = [
    "Code",
    "Visual Studio Code",
    "Cursor",
    "Zed",
    "Xcode",
    "Sublime Text",
    "Nova",
    "IntelliJ IDEA",
    "WebStorm",
    "RustRover",
    "PyCharm",
    "Android Studio",
];

static FOLDER_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_-]{1,59}$").expect("folder regex"));

/// Title parts an editor shows that are never a project.
const GENERIC_TITLES: [&str; 8] = [
    "welcome",
    "untitled",
    "settings",
    "preferences",
    "extensions",
    "terminal",
    "output",
    "search",
];

/// `(name, evidence)` pairs one segment points at.
fn candidates(segment: &ActivitySegment) -> Vec<(String, String)> {
    let mut out = Vec::new();

    if let Some(url) = segment.url.as_deref() {
        let bare = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .unwrap_or(url);
        let bare = bare.strip_prefix("www.").unwrap_or(bare);
        let mut parts = bare.split(['/', '?', '#']);
        if let (Some(host), Some(owner), Some(repo)) = (parts.next(), parts.next(), parts.next()) {
            let repo = repo.trim_end_matches(".git");
            if FORGES.contains(&host)
                && !owner.is_empty()
                && !repo.is_empty()
                && !FORGE_PAGES.contains(&owner.to_ascii_lowercase().as_str())
            {
                out.push((repo.to_string(), format!("{host}/{owner}/{repo}")));
            }
        }
    }

    for capture in TITLE_PATH.captures_iter(&segment.title) {
        let name = capture[2].trim_end_matches(['.', '-', '_']);
        if !name.is_empty() {
            out.push((name.to_string(), format!("~/{}/{name}", &capture[1])));
        }
    }

    // Editors title windows `file — folder` or `folder — file`; the piece
    // with no file extension is the folder.
    if EDITORS.contains(&segment.app.as_str()) {
        let title = segment.title.replace(" — ", " - ").replace(" – ", " - ");
        for piece in title.split(" - ") {
            let piece = piece.trim();
            if FOLDER_NAME.is_match(piece)
                && !piece.eq_ignore_ascii_case(&segment.app)
                && !GENERIC_TITLES.contains(&piece.to_ascii_lowercase().as_str())
            {
                out.push((piece.to_string(), piece.to_string()));
            }
        }
    }
    out
}

fn dismissed(conn: &Connection) -> Result<HashSet<String>, String> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1;",
            params![DISMISSED_KEY],
            |row| row.get(0),
        )
        .ok();
    Ok(raw
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .collect())
}

/// Remembers a dismissed suggestion so it never comes back.
pub fn dismiss_suggestion(conn: &Connection, key: &str) -> Result<(), String> {
    let mut keys: Vec<String> = dismissed(conn)?.into_iter().collect();
    let key = key.to_lowercase();
    if !keys.contains(&key) {
        keys.push(key);
    }
    keys.sort();
    let value = serde_json::to_string(&keys).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
        params![DISMISSED_KEY, value],
    )
    .map_err(err)?;
    Ok(())
}

#[derive(Default)]
struct Cluster {
    ms: u64,
    names: HashMap<String, u64>,
    evidence: HashMap<String, u64>,
}

/// Proposes projects from recurring repo, folder, and forge names in the
/// capture since `since_ms`, skipping names that already are a project (or a
/// project's hint) and ones the user dismissed. Runs over local data only.
/// How discovery compares names: "Acme Web", "acme-web", and "acme_web" are
/// one project.
fn name_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn discover(conn: &Connection, since_ms: u64) -> Result<Vec<ProjectSuggestion>, String> {
    let mut known: HashSet<String> = HashSet::new();
    {
        let mut stmt = conn
            .prepare("SELECT name FROM projects WHERE deleted_at IS NULL;")
            .map_err(err)?;
        for name in stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(err)?
        {
            known.insert(name_key(&name.map_err(err)?));
        }
        let mut stmt = conn
            .prepare(
                "SELECT pattern FROM rules WHERE deleted_at IS NULL AND project_id IS NOT NULL;",
            )
            .map_err(err)?;
        for pattern in stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(err)?
        {
            let pattern = pattern.map_err(err)?;
            if let Some(last) = pattern.trim_end_matches('/').rsplit('/').next() {
                known.insert(name_key(last));
            }
            known.insert(name_key(&pattern));
        }
    }
    let skipped = dismissed(conn)?;

    let mut clusters: HashMap<String, Cluster> = HashMap::new();
    for segment in segments_since(conn, since_ms)? {
        let ms = segment_ms(&segment);
        if ms == 0 {
            continue;
        }
        let mut counted = HashSet::new();
        for (name, evidence) in candidates(&segment) {
            let key = name.to_lowercase();
            let cluster = clusters.entry(key.clone()).or_default();
            if counted.insert(key) {
                cluster.ms += ms;
                *cluster.names.entry(name).or_default() += ms;
            }
            *cluster.evidence.entry(evidence).or_default() += ms;
        }
    }

    let mut out: Vec<ProjectSuggestion> = clusters
        .into_iter()
        .filter(|(key, cluster)| {
            cluster.ms >= DISCOVERY_MIN_MS
                && !known.contains(&name_key(key))
                && !skipped.contains(key)
        })
        .map(|(key, cluster)| {
            let name = cluster
                .names
                .into_iter()
                .max_by_key(|(name, ms)| (*ms, name.clone()))
                .map(|(name, _)| name)
                .unwrap_or_else(|| key.clone());
            let mut evidence: Vec<(String, u64)> = cluster.evidence.into_iter().collect();
            evidence.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            ProjectSuggestion {
                key,
                name,
                ms: cluster.ms,
                evidence: evidence.into_iter().take(3).map(|(e, _)| e).collect(),
            }
        })
        .collect();
    out.sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.key.cmp(&b.key)));
    out.truncate(DISCOVERY_LIMIT);
    Ok(out)
}

// --- CSV import -------------------------------------------------------------------

/// Minimal RFC 4180: quoted fields, doubled quotes, CRLF or LF line ends.
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|row| row.iter().any(|field| !field.trim().is_empty()));
    rows
}

/// `50h` / `50 hours` / `50` are hours; `$5,000` or `5000 USD` an amount.
fn parse_budget(text: &str) -> Option<(&'static str, f64)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lower = text.to_lowercase();
    let is_amount = text.starts_with(['$', '€', '£', '¥'])
        || text.ends_with(['$', '€', '£', '¥'])
        || lower.ends_with("usd")
        || lower.ends_with("eur")
        || lower.ends_with("gbp");
    let number: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let value: f64 = number.parse().ok().filter(|v: &f64| *v > 0.0)?;
    Some((if is_amount { "amount" } else { "hours" }, value))
}

/// `YYYY-MM-DD` as local midnight.
fn parse_due(text: &str) -> Option<u64> {
    use chrono::{NaiveDate, TimeZone};
    let date = NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()?;
    let midnight = date.and_hms_opt(0, 0, 0)?;
    let local = chrono::Local.from_local_datetime(&midnight).earliest()?;
    u64::try_from(local.timestamp_millis()).ok()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub created: u32,
    pub clients_created: u32,
    /// "Row 3: OpenRize already exists", one line per row not imported.
    pub skipped: Vec<String>,
}

/// Imports `name, client, budget, due date` rows (header required, columns in
/// any order). Clients are matched by name or created; existing project names
/// are skipped rather than duplicated.
pub fn import_csv(
    store: &mut ActivityStore,
    text: &str,
    now: u64,
) -> Result<ImportSummary, String> {
    let rows = parse_csv(text);
    let Some((header, body)) = rows.split_first() else {
        return Err("The file is empty.".to_string());
    };
    let column = |names: &[&str]| {
        header.iter().position(|cell| {
            let cell = cell.trim().to_lowercase().replace(['_', '-'], " ");
            names.contains(&cell.as_str())
        })
    };
    let name_col = column(&["name", "project", "project name"])
        .ok_or("The first row needs a \"name\" column.")?;
    let client_col = column(&["client", "client name", "customer"]);
    let budget_col = column(&["budget"]);
    let due_col = column(&["due date", "due", "deadline"]);

    let mut projects: HashSet<String> = store
        .list_projects()?
        .into_iter()
        .map(|project| project.name.to_lowercase())
        .collect();
    let mut clients: HashMap<String, String> = store
        .list_clients()?
        .into_iter()
        .map(|client| (client.name.to_lowercase(), client.id))
        .collect();

    let mut summary = ImportSummary {
        created: 0,
        clients_created: 0,
        skipped: Vec::new(),
    };
    for (index, row) in body.iter().enumerate() {
        let line = index + 2;
        let cell = |col: Option<usize>| {
            col.and_then(|col| row.get(col))
                .map(|value| value.trim().to_string())
                .unwrap_or_default()
        };
        let name = cell(Some(name_col));
        if name.is_empty() {
            summary.skipped.push(format!("Row {line}: no project name"));
            continue;
        }
        if !projects.insert(name.to_lowercase()) {
            summary
                .skipped
                .push(format!("Row {line}: {name} already exists"));
            continue;
        }

        let client_name = cell(client_col);
        let client_id = if client_name.is_empty() {
            None
        } else if let Some(id) = clients.get(&client_name.to_lowercase()) {
            Some(id.clone())
        } else {
            let client = store.create_client(
                NewClient {
                    name: client_name.clone(),
                    email: None,
                    address: None,
                    default_rate: None,
                    currency: None,
                },
                now,
            )?;
            summary.clients_created += 1;
            clients.insert(client_name.to_lowercase(), client.id.clone());
            Some(client.id)
        };

        let budget_text = cell(budget_col);
        let budget = parse_budget(&budget_text);
        if budget.is_none() && !budget_text.is_empty() {
            summary.skipped.push(format!(
                "Row {line}: imported {name} without its budget \"{budget_text}\""
            ));
        }
        let due_text = cell(due_col);
        let due_date = parse_due(&due_text);
        if due_date.is_none() && !due_text.is_empty() {
            summary.skipped.push(format!(
                "Row {line}: imported {name} without its due date \"{due_text}\" (use YYYY-MM-DD)"
            ));
        }

        store.create_project(
            NewProject {
                client_id,
                name,
                color: PALETTE[(summary.created as usize) % PALETTE.len()].to_string(),
                description: None,
                ai_hints: None,
                status: None,
                due_date,
                budget_kind: Some(budget.map_or("none", |(kind, _)| kind).to_string()),
                budget_value: budget.map(|(_, value)| value),
                budget_period: None,
                billable_default: None,
                hourly_rate: None,
            },
            now,
        )?;
        summary.created += 1;
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    fn store() -> ActivityStore {
        ActivityStore::from_conn(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn segment(
        conn: &Connection,
        app: &str,
        title: &str,
        url: Option<&str>,
        start: u64,
        minutes: u64,
    ) {
        conn.execute(
            "INSERT INTO segments (app, title, kind, started_at, ended_at, url) VALUES (?1, ?2, 'activity', ?3, ?4, ?5);",
            params![app, title, start as i64, (start + minutes * MIN) as i64, url],
        )
        .unwrap();
    }

    fn new_project(name: &str, hints: Option<&str>) -> NewProject {
        NewProject {
            client_id: None,
            name: name.to_string(),
            color: PALETTE[0].to_string(),
            description: None,
            ai_hints: hints.map(str::to_string),
            status: None,
            due_date: None,
            budget_kind: None,
            budget_value: None,
            budget_period: None,
            billable_default: None,
            hourly_rate: None,
        }
    }

    fn hint_rules(conn: &Connection, project: &str) -> Vec<(String, String)> {
        let mut stmt = conn
            .prepare(
                "SELECT match_kind, pattern FROM rules
                 WHERE project_id = ?1 AND origin = 'hint' AND deleted_at IS NULL ORDER BY pattern;",
            )
            .unwrap();
        stmt.query_map([project], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn hints_are_classified_into_rule_kinds() {
        let hints = parse_hints(
            "~/Code/OpenRize/, https://github.com/Kareeme246/OpenRize; figma.com\nOpenRize, ui, openrize",
        );
        let rules: Vec<_> = hints.iter().map(|h| h.rule.clone()).collect();
        assert_eq!(
            rules,
            vec![
                Some(("path_prefix", "~/Code/OpenRize".to_string())),
                Some(("url_prefix", "github.com/Kareeme246/OpenRize".to_string())),
                Some(("domain", "figma.com".to_string())),
                Some(("title_contains", "OpenRize".to_string())),
                // Too short to match on; the case-insensitive repeat is gone.
                None,
            ]
        );
    }

    #[test]
    fn saving_hints_replaces_the_projects_rules() {
        let mut store = store();
        let project = store
            .create_project(
                new_project("OpenRize", Some("~/Code/OpenRize, figma.com")),
                1,
            )
            .unwrap();
        assert_eq!(
            hint_rules(store.conn(), &project.id),
            vec![
                ("path_prefix".to_string(), "~/Code/OpenRize".to_string()),
                ("domain".to_string(), "figma.com".to_string()),
            ]
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
        );

        store
            .update_project(
                &project.id,
                crate::models::UpdateProject {
                    ai_hints: Some(Some("OpenRize".to_string())),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        assert_eq!(
            hint_rules(store.conn(), &project.id),
            vec![("title_contains".to_string(), "OpenRize".to_string())]
        );

        // Hint rules feed T0 only while the project is active.
        let active = crate::ai::store::load_rules(store.conn()).unwrap();
        assert!(active
            .iter()
            .any(|r| r.project_id.as_deref() == Some(project.id.as_str())));
        store
            .update_project(
                &project.id,
                crate::models::UpdateProject {
                    status: Some("archived".to_string()),
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        let archived = crate::ai::store::load_rules(store.conn()).unwrap();
        assert!(!archived
            .iter()
            .any(|r| r.project_id.as_deref() == Some(project.id.as_str())));
    }

    #[test]
    fn preview_counts_matched_time_once() {
        let store = store();
        let conn = store.conn();
        segment(conn, "WezTerm", "~/Code/OpenRize: cargo test", None, 0, 30);
        segment(conn, "Zed", "activity.rs - OpenRize", None, 30 * MIN, 20);
        segment(
            conn,
            "Safari",
            "Figma",
            Some("https://www.figma.com/file/x"),
            50 * MIN,
            10,
        );

        let preview = preview_hints(conn, "OpenRize, ~/Code/OpenRize, ui", 0).unwrap();
        let ms: Vec<u64> = preview.hints.iter().map(|h| h.matched_ms).collect();
        assert_eq!(ms, vec![50 * MIN, 30 * MIN, 0]);
        assert_eq!(preview.hints[2].match_kind, None);
        // The terminal segment matches both hints but counts once.
        assert_eq!(preview.total_ms, 50 * MIN);
    }

    #[test]
    fn discovery_clusters_repo_folder_and_forge_names() {
        let mut store = store();
        let conn = store.conn();
        segment(
            conn,
            "WezTerm",
            "kareem@mac: ~/Code/pantry-tracker",
            None,
            0,
            40,
        );
        segment(
            conn,
            "Safari",
            "pantry-tracker",
            Some("https://github.com/kareem/pantry-tracker/pulls"),
            40 * MIN,
            20,
        );
        segment(conn, "Zed", "main.rs — pantry-tracker", None, 60 * MIN, 10);
        // Too little time to suggest.
        segment(conn, "WezTerm", "~/Code/scratch", None, 70 * MIN, 5);
        // A forge page, not a repository.
        segment(
            conn,
            "Safari",
            "Notifications",
            Some("https://github.com/notifications"),
            75 * MIN,
            60,
        );

        let found = discover(conn, 0).unwrap();
        assert_eq!(found.len(), 1);
        let suggestion = &found[0];
        assert_eq!(suggestion.name, "pantry-tracker");
        assert_eq!(suggestion.ms, 70 * MIN);
        assert_eq!(
            suggestion.evidence,
            vec![
                "~/Code/pantry-tracker".to_string(),
                "github.com/kareem/pantry-tracker".to_string(),
                "pantry-tracker".to_string(),
            ]
        );

        // Accepting it (a project with those hints) or dismissing it ends it.
        dismiss_suggestion(conn, "Pantry-Tracker").unwrap();
        assert!(discover(conn, 0).unwrap().is_empty());
        conn.execute("DELETE FROM settings WHERE key = ?1;", [DISMISSED_KEY])
            .unwrap();
        store
            .create_project(new_project("Pantry", Some("~/Code/pantry-tracker")), 1)
            .unwrap();
        assert!(discover(store.conn(), 0).unwrap().is_empty());
    }

    #[test]
    fn discovery_skips_a_project_that_exists_under_another_spelling() {
        let mut store = store();
        segment(store.conn(), "Zed", "main.rs - pantry-tracker", None, 0, 40);
        assert_eq!(discover(store.conn(), 0).unwrap().len(), 1);
        store
            .create_project(new_project("Pantry Tracker", None), 1)
            .unwrap();
        assert!(discover(store.conn(), 0).unwrap().is_empty());
    }

    #[test]
    fn csv_parses_quotes_and_line_endings() {
        let rows = parse_csv("name,client\r\n\"Acme, Web\",\"The \"\"Big\"\" Co\"\n\nSolo,\n");
        assert_eq!(
            rows,
            vec![
                vec!["name".to_string(), "client".to_string()],
                vec!["Acme, Web".to_string(), "The \"Big\" Co".to_string()],
                vec!["Solo".to_string(), String::new()],
            ]
        );
    }

    #[test]
    fn budgets_parse_as_hours_or_amounts() {
        assert_eq!(parse_budget("50h"), Some(("hours", 50.0)));
        assert_eq!(parse_budget("12.5 hours"), Some(("hours", 12.5)));
        assert_eq!(parse_budget("$5,000"), Some(("amount", 5000.0)));
        assert_eq!(parse_budget("800 EUR"), Some(("amount", 800.0)));
        assert_eq!(parse_budget("lots"), None);
    }

    #[test]
    fn import_creates_projects_and_clients() {
        let mut store = store();
        store
            .create_project(new_project("OpenRize", None), 1)
            .unwrap();
        let summary = import_csv(
            &mut store,
            "Name,Client,Budget,Due date\nAcme Web,Acme Corp,50h,2026-10-15\nAcme Brand,acme corp,$900,soon\nopenrize,,,\n,Nobody,,\n",
            2,
        )
        .unwrap();
        assert_eq!(summary.created, 2);
        assert_eq!(summary.clients_created, 1);
        assert_eq!(summary.skipped.len(), 3);

        let projects = store.list_projects().unwrap();
        let web = projects.iter().find(|p| p.name == "Acme Web").unwrap();
        assert_eq!(web.budget_kind, "hours");
        assert_eq!(web.budget_value, Some(50.0));
        assert!(web.due_date.is_some());
        let brand = projects.iter().find(|p| p.name == "Acme Brand").unwrap();
        assert_eq!(brand.budget_kind, "amount");
        assert_eq!(brand.client_id, web.client_id);
        assert!(brand.due_date.is_none());

        assert!(import_csv(&mut store, "client\nAcme\n", 3).is_err());
    }

    #[test]
    fn stats_sum_range_month_and_billable_time() {
        let store = store();
        let conn = store.conn();
        for (id, start, minutes, billable) in [("a", 0u64, 60u64, 1), ("b", 100 * MIN, 30, 0)] {
            conn.execute(
                "INSERT INTO time_entries (id, started_at, ended_at, description, project_id, status, source, billable, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'x', 'p1', 'approved', 'auto', ?4, 0, 0);",
                params![id, start as i64, (start + minutes * MIN) as i64, billable],
            )
            .unwrap();
        }
        let stats = project_stats(conn, 90 * MIN, 200 * MIN, 50 * MIN).unwrap();
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.entries, 2);
        assert_eq!(s.total_ms, 90 * MIN);
        assert_eq!(s.range_ms, 30 * MIN);
        assert_eq!(s.month_ms, 30 * MIN);
        assert_eq!(s.billable_ms, 60 * MIN);
        assert_eq!(s.billable_month_ms, 0);
        assert_eq!(s.last_activity, Some(130 * MIN));
    }

    #[test]
    fn a_project_with_entries_cannot_be_deleted() {
        let mut store = store();
        let project = store
            .create_project(new_project("OpenRize", None), 1)
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO time_entries (id, started_at, ended_at, description, project_id, status, source, billable, created_at, updated_at)
                 VALUES ('e', 0, 1, 'x', ?1, 'approved', 'auto', 0, 0, 0);",
                [&project.id],
            )
            .unwrap();
        assert!(store.delete_project(&project.id, 2).is_err());
        let empty = store
            .create_project(new_project("Empty", Some("figma.com")), 1)
            .unwrap();
        store.delete_project(&empty.id, 2).unwrap();
        assert!(hint_rules(store.conn(), &empty.id).is_empty());
    }

    #[test]
    fn invoiced_time_pins_the_client() {
        let mut store = store();
        let project = store
            .create_project(new_project("OpenRize", None), 1)
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO time_entries (id, started_at, ended_at, description, project_id, status, source, billable, invoice_id, created_at, updated_at)
                 VALUES ('e', 0, 1, 'x', ?1, 'approved', 'auto', 1, 'inv', 0, 0);",
                [&project.id],
            )
            .unwrap();
        let moved = store.update_project(
            &project.id,
            crate::models::UpdateProject {
                client_id: Some(Some("other".to_string())),
                ..Default::default()
            },
            2,
        );
        assert!(moved.is_err());
        // Other edits still go through.
        store
            .update_project(
                &project.id,
                crate::models::UpdateProject {
                    name: Some("OpenRize 2".to_string()),
                    ..Default::default()
                },
                3,
            )
            .unwrap();
    }

    #[test]
    fn a_null_patch_clears_optional_fields() {
        let patch: crate::models::UpdateProject =
            serde_json::from_str(r#"{"clientId": null, "hourlyRate": 90}"#).unwrap();
        assert_eq!(patch.client_id, Some(None));
        assert_eq!(patch.hourly_rate, Some(Some(90.0)));
        assert_eq!(patch.due_date, None);
    }
}
