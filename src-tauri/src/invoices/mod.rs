//! Real invoices: editable drafts built from approved billable time, fixed
//! retainer fees and manual items, finalized into numbered, immutable PDFs.
//!
//! Lifecycle: `draft` (editable, time reserved, no number) -> `open` (finalized:
//! numbered, PDF archived, frozen) -> `paid`; an open invoice can instead be
//! `void`ed, which releases its time but keeps its number and PDF. Rust owns all
//! arithmetic (integer USD cents, see `money`) and the paper (`pdf`); the
//! frontend only ever sends line choices and displays what comes back.

mod money;
mod pdf;
pub mod profile;

use chrono::{Datelike, TimeZone};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use money::{due_date, format_date, hours_hundredths, line_amount_cents, parse_date, terms_label};
use pdf::{PaperInvoice, PaperLine};
use profile::{IssuerSnapshot, MAX_TERMS_DAYS};

const MAX_LINES: usize = 500;
const MAX_DESCRIPTION: usize = 2000;
const MAX_SUBJECT: usize = 200;
const MAX_ADDRESS: usize = 500;
const MAX_NOTES: usize = 2000;
const MAX_UNIT: usize = 16;

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceLine {
    pub id: String,
    /// `time` (one approved entry), `retainer` (fixed period fee) or `manual`.
    pub kind: String,
    pub entry_id: Option<String>,
    pub project_name: String,
    pub description: String,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub quantity_hundredths: i64,
    pub unit: String,
    pub rate_cents: i64,
    pub amount_cents: i64,
    /// A time line whose entry is agent time (counted by the agent bridge)
    /// rather than time the person spent.
    pub agent: bool,
}

/// How one project's time on an invoice divides between the person and
/// their agents. Only projects with agent time have one.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSplit {
    pub project_name: String,
    pub you_ms: u64,
    pub agents_ms: u64,
    /// The line as it prints, e.g. `OpenRize · 1h52m (you 45m · agents 1h07m)`.
    pub label: String,
}

/// `1h07m`, `1h`, `45m`: whole minutes, rounded to the nearest.
pub fn format_span(ms: u64) -> String {
    let minutes = (ms + 30_000) / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m:02}m"),
    }
}

/// The you/agents split per project over an invoice's time lines, in the order
/// the projects first appear. Rust owns the arithmetic; nothing downstream
/// recomputes it.
pub fn splits_of(lines: &[InvoiceLine]) -> Vec<ProjectSplit> {
    let mut order: Vec<&str> = Vec::new();
    let mut totals: std::collections::HashMap<&str, (u64, u64)> = std::collections::HashMap::new();
    for line in lines.iter().filter(|line| line.kind == "time") {
        let (Some(start), Some(end)) = (line.started_at, line.ended_at) else {
            continue;
        };
        let duration = end.saturating_sub(start);
        let slot = totals.entry(line.project_name.as_str()).or_insert_with(|| {
            order.push(line.project_name.as_str());
            (0, 0)
        });
        if line.agent {
            slot.1 += duration;
        } else {
            slot.0 += duration;
        }
    }
    order
        .into_iter()
        .filter_map(|name| {
            let (you_ms, agents_ms) = totals[name];
            (agents_ms > 0).then(|| ProjectSplit {
                project_name: name.to_owned(),
                you_ms,
                agents_ms,
                label: format!(
                    "{name} · {} (you {} · agents {})",
                    format_span(you_ms + agents_ms),
                    format_span(you_ms),
                    format_span(agents_ms)
                ),
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceSummary {
    pub id: String,
    pub number: Option<String>,
    pub client_id: String,
    pub client_name: String,
    /// `draft`, `open`, `paid` or `void`.
    pub status: String,
    pub currency: String,
    pub issue_date: Option<String>,
    pub due_date: Option<String>,
    pub total_cents: i64,
    pub created_at: u64,
    pub issued_at: Option<u64>,
    pub paid_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invoice {
    #[serde(flatten)]
    pub summary: InvoiceSummary,
    pub bill_to_email: Option<String>,
    pub bill_to_address: Option<String>,
    /// The From block: this invoice's own copy, defaulted from Invoice settings
    /// when the draft is created and frozen at finalize.
    pub from_name: String,
    pub from_address: String,
    pub from_email: Option<String>,
    pub from_phone: Option<String>,
    pub terms_days: Option<i64>,
    pub subject: Option<String>,
    pub notes: Option<String>,
    pub payment_instructions: Option<String>,
    pub lines: Vec<InvoiceLine>,
    /// Present only when agents contributed time: see `ProjectSplit`.
    pub splits: Vec<ProjectSplit>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineInput {
    pub kind: String,
    /// Required for `time` lines; the entry supplies project, hours and date.
    pub entry_id: Option<String>,
    pub description: String,
    /// Hundredths; ignored for `time` lines (derived from the entry).
    pub quantity_hundredths: Option<i64>,
    pub unit: Option<String>,
    /// Defaults to the project's, else the client's, hourly rate for `time` lines.
    pub rate_cents: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftInput {
    pub id: Option<String>,
    pub client_id: String,
    pub bill_to_name: String,
    pub bill_to_address: Option<String>,
    pub bill_to_email: Option<String>,
    pub from_name: String,
    pub from_address: String,
    pub from_email: Option<String>,
    pub from_phone: Option<String>,
    pub issue_date: String,
    pub terms_days: i64,
    pub subject: Option<String>,
    pub notes: Option<String>,
    pub payment_instructions: Option<String>,
    pub lines: Vec<LineInput>,
}

/// One approved, billable, not-yet-invoiced entry, with the rate it would bill at.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillableEntry {
    pub entry_id: String,
    pub project_id: String,
    pub project_name: String,
    pub description: String,
    pub started_at: u64,
    pub ended_at: u64,
    pub quantity_hundredths: i64,
    pub rate_cents: Option<i64>,
    pub amount_cents: Option<i64>,
    /// Already on the draft being edited.
    pub on_this_invoice: bool,
    /// Agent time rather than time the person spent.
    pub agent: bool,
}

const SUMMARY_COLUMNS: &str = "id, number, client_id, client_name, status, currency,
    issue_date, due_date, total_cents, created_at, issued_at, paid_at";

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InvoiceSummary> {
    Ok(InvoiceSummary {
        id: row.get(0)?,
        number: row.get(1)?,
        client_id: row.get(2)?,
        client_name: row.get(3)?,
        status: row.get(4)?,
        currency: row.get(5)?,
        issue_date: row.get(6)?,
        due_date: row.get(7)?,
        total_cents: row.get(8)?,
        created_at: row.get::<_, i64>(9)? as u64,
        issued_at: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
        paid_at: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
    })
}

pub fn list(conn: &Connection) -> Result<Vec<InvoiceSummary>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM invoices ORDER BY COALESCE(issued_at, created_at) DESC, id DESC"
        ))
        .map_err(err)?;
    let rows = stmt.query_map([], summary_from_row).map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

pub fn get(conn: &Connection, id: &str) -> Result<Invoice, String> {
    let mut invoice = conn
        .query_row(
            &format!(
                "SELECT {SUMMARY_COLUMNS}, client_email, client_address, terms_days, subject, notes,
                        payment_instructions, from_name, from_address, from_email, from_phone
                 FROM invoices WHERE id = ?1"
            ),
            [id],
            |row| {
                Ok(Invoice {
                    summary: summary_from_row(row)?,
                    bill_to_email: row.get(12)?,
                    bill_to_address: row.get(13)?,
                    terms_days: row.get(14)?,
                    subject: row.get(15)?,
                    notes: row.get(16)?,
                    payment_instructions: row.get(17)?,
                    from_name: row.get(18)?,
                    from_address: row.get(19)?,
                    from_email: row.get(20)?,
                    from_phone: row.get(21)?,
                    lines: Vec::new(),
                    splits: Vec::new(),
                })
            },
        )
        .map_err(|_| "Invoice not found".to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT l.id, l.kind, l.entry_id, l.project_name, l.description, l.started_at,
                    l.ended_at, l.quantity_hundredths, l.unit, l.rate_cents, l.amount_cents,
                    COALESCE(e.source = 'agent', 0)
             FROM invoice_lines l
             LEFT JOIN time_entries e ON e.id = l.entry_id
             WHERE l.invoice_id = ?1 ORDER BY l.position",
        )
        .map_err(err)?;
    invoice.lines = stmt
        .query_map([id], |row| {
            Ok(InvoiceLine {
                id: row.get(0)?,
                kind: row.get(1)?,
                entry_id: row.get(2)?,
                project_name: row.get(3)?,
                description: row.get(4)?,
                started_at: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
                ended_at: row.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                quantity_hundredths: row.get(7)?,
                unit: row.get(8)?,
                rate_cents: row.get(9)?,
                amount_cents: row.get(10)?,
                agent: row.get::<_, i64>(11)? != 0,
            })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    invoice.splits = splits_of(&invoice.lines);
    Ok(invoice)
}

/// Approved, billable, uninvoiced time for a client in `[start_ms, end_ms)`,
/// with the rate each entry would bill at. Entries already on `invoice_id`
/// (the draft being edited) are included and flagged. Agent time is offered
/// only when `with_agents` (advanced workflow tracking is on).
pub fn billable_entries(
    conn: &Connection,
    client_id: &str,
    start_ms: u64,
    end_ms: u64,
    invoice_id: Option<&str>,
    with_agents: bool,
) -> Result<Vec<BillableEntry>, String> {
    if start_ms >= end_ms || end_ms > i64::MAX as u64 {
        return Err("Choose a valid date range".into());
    }
    let mut stmt = conn
        .prepare(
            "SELECT e.id, p.id, p.name, e.description, e.started_at, e.ended_at,
                    COALESCE(p.hourly_rate, c.default_rate), e.invoice_id,
                    e.source = 'agent'
             FROM time_entries e
             JOIN projects p ON p.id = e.project_id
             JOIN clients c ON c.id = p.client_id
             WHERE c.id = ?1 AND c.deleted_at IS NULL AND p.deleted_at IS NULL
               AND e.status = 'approved' AND e.billable = 1 AND e.deleted_at IS NULL
               AND (e.invoice_id IS NULL OR e.invoice_id = ?4)
               AND (?5 OR e.source != 'agent')
               AND e.started_at >= ?2 AND e.started_at < ?3
             ORDER BY e.started_at, e.id",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(
            params![
                client_id,
                start_ms as i64,
                end_ms as i64,
                invoice_id,
                with_agents
            ],
            |row| {
                let started: i64 = row.get(4)?;
                let ended: i64 = row.get(5)?;
                let rate: Option<f64> = row.get(6)?;
                let linked: Option<String> = row.get(7)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    started,
                    ended,
                    rate,
                    linked.is_some(),
                    row.get::<_, bool>(8)?,
                ))
            },
        )
        .map_err(err)?;
    let mut entries = Vec::new();
    for row in rows {
        let (entry_id, project_id, project_name, description, started, ended, rate, linked, agent) =
            row.map_err(err)?;
        let quantity = hours_hundredths(ended - started);
        let rate_cents = rate.and_then(rate_to_cents);
        entries.push(BillableEntry {
            entry_id,
            project_id,
            project_name,
            description,
            started_at: started as u64,
            ended_at: ended as u64,
            quantity_hundredths: quantity,
            amount_cents: rate_cents.and_then(|r| line_amount_cents(quantity, r).ok()),
            rate_cents,
            on_this_invoice: linked,
            agent,
        });
    }
    Ok(entries)
}

/// A REAL dollar rate (from the projects/clients tables) as whole cents.
fn rate_to_cents(rate: f64) -> Option<i64> {
    let cents = (rate * 100.0).round();
    (cents.is_finite() && (0.0..=money::MAX_RATE_CENTS as f64).contains(&cents))
        .then_some(cents as i64)
}

/// A draft fully resolved against the database: validated, with every derived
/// value (hours, project names, rates, amounts, due date, total) computed.
/// Shared by preview, save and (via the stored rows) finalize.
fn resolve(conn: &Connection, input: &DraftInput) -> Result<Invoice, String> {
    let client_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM clients WHERE id = ?1 AND deleted_at IS NULL)",
            [&input.client_id],
            |row| row.get(0),
        )
        .map_err(err)?;
    if !client_exists {
        return Err("Select an existing client".into());
    }

    let bill_to_name = input.bill_to_name.trim().to_owned();
    if bill_to_name.is_empty() {
        return Err("Add a name to bill".into());
    }
    if bill_to_name.chars().count() > 120 {
        return Err("The bill-to name is too long".into());
    }
    let bill_to_address = text(&input.bill_to_address, MAX_ADDRESS, "The bill-to address")?;
    if bill_to_address
        .as_ref()
        .is_some_and(|a| a.lines().count() > 8)
    {
        return Err("The bill-to address can have at most 8 lines".into());
    }
    let bill_to_email = text(&input.bill_to_email, 120, "The bill-to email")?;
    let from_name = input.from_name.trim().to_owned();
    if from_name.chars().count() > 120 {
        return Err("The From name is too long".into());
    }
    let from_address = input.from_address.trim().to_owned();
    if from_address.chars().count() > MAX_ADDRESS || from_address.lines().count() > 8 {
        return Err("The From address can have at most 500 characters and 8 lines".into());
    }
    let from_email = text(&input.from_email, 120, "The From email")?;
    let from_phone = text(&input.from_phone, 120, "The From phone")?;
    parse_date(&input.issue_date)?;
    if !(0..=MAX_TERMS_DAYS).contains(&input.terms_days) {
        return Err("Payment terms must be between 0 and 365 days".into());
    }
    let due = due_date(&input.issue_date, input.terms_days)?;
    let subject = text(&input.subject, MAX_SUBJECT, "The subject")?;
    let notes = text(&input.notes, MAX_NOTES, "Notes")?;
    let payment = text(
        &input.payment_instructions,
        MAX_NOTES,
        "Payment instructions",
    )?;
    if input.lines.is_empty() {
        return Err("Add at least one line item".into());
    }
    if input.lines.len() > MAX_LINES {
        return Err("An invoice can have at most 500 lines".into());
    }

    let mut lines = Vec::with_capacity(input.lines.len());
    let mut seen = std::collections::HashSet::new();
    let mut total: i64 = 0;
    for (index, line) in input.lines.iter().enumerate() {
        let resolved = match line.kind.as_str() {
            "time" => resolve_time_line(conn, input, line, index, &mut seen)?,
            "retainer" | "manual" => resolve_fixed_line(line, index)?,
            _ => return Err("Unknown line type".into()),
        };
        total = total
            .checked_add(resolved.amount_cents)
            .ok_or("The invoice total is too large")?;
        lines.push(resolved);
    }

    Ok(Invoice {
        summary: InvoiceSummary {
            id: input.id.clone().unwrap_or_default(),
            number: None,
            client_id: input.client_id.clone(),
            client_name: bill_to_name,
            status: "draft".into(),
            currency: "USD".into(),
            issue_date: Some(input.issue_date.clone()),
            due_date: Some(due),
            total_cents: total,
            created_at: 0,
            issued_at: None,
            paid_at: None,
        },
        bill_to_email,
        bill_to_address,
        from_name,
        from_address,
        from_email,
        from_phone,
        terms_days: Some(input.terms_days),
        subject,
        notes,
        payment_instructions: payment,
        splits: splits_of(&lines),
        lines,
    })
}

fn text(value: &Option<String>, max: usize, label: &str) -> Result<Option<String>, String> {
    let value = value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned);
    if value.as_ref().is_some_and(|v| v.chars().count() > max) {
        return Err(format!("{label} is too long"));
    }
    Ok(value)
}

fn description(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.chars().count() > MAX_DESCRIPTION {
        return Err("A line description is too long".into());
    }
    Ok(value.to_owned())
}

fn resolve_fixed_line(line: &LineInput, index: usize) -> Result<InvoiceLine, String> {
    let description = description(&line.description)?;
    if description.is_empty() {
        return Err("Every line needs a description".into());
    }
    let quantity = line
        .quantity_hundredths
        .ok_or("Every line needs a quantity")?;
    let rate = line.rate_cents.ok_or("Every line needs a rate")?;
    let unit = line.unit.as_deref().unwrap_or("").trim().to_owned();
    if unit.chars().count() > MAX_UNIT {
        return Err("A unit can be at most 16 characters".into());
    }
    Ok(InvoiceLine {
        id: index.to_string(),
        kind: line.kind.clone(),
        entry_id: None,
        project_name: String::new(),
        description,
        started_at: None,
        ended_at: None,
        quantity_hundredths: quantity,
        unit,
        rate_cents: rate,
        amount_cents: line_amount_cents(quantity, rate)?,
        agent: false,
    })
}

fn resolve_time_line(
    conn: &Connection,
    input: &DraftInput,
    line: &LineInput,
    index: usize,
    seen: &mut std::collections::HashSet<String>,
) -> Result<InvoiceLine, String> {
    let entry_id = line
        .entry_id
        .as_deref()
        .ok_or("A time line needs a tracked entry")?;
    if !seen.insert(entry_id.to_owned()) {
        return Err("The same time entry is on this invoice twice".into());
    }
    let row: Option<(String, i64, i64, String, Option<f64>, bool)> = conn
        .query_row(
            "SELECT p.name, e.started_at, e.ended_at, e.description,
                    COALESCE(p.hourly_rate, c.default_rate), e.source = 'agent'
             FROM time_entries e
             JOIN projects p ON p.id = e.project_id
             JOIN clients c ON c.id = p.client_id
             WHERE e.id = ?1 AND c.id = ?2 AND e.status = 'approved' AND e.billable = 1
               AND e.deleted_at IS NULL AND p.deleted_at IS NULL
               AND (e.invoice_id IS NULL OR e.invoice_id = ?3)",
            params![entry_id, input.client_id, input.id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(err)?;
    let (project_name, started, ended, entry_description, default_rate, agent) = row.ok_or(
        "A time entry is no longer available to invoice (it may be on another invoice or changed)",
    )?;
    if ended <= started {
        return Err("A time entry has an invalid duration".into());
    }
    let rate = match line.rate_cents {
        Some(rate) => rate,
        None => default_rate.and_then(rate_to_cents).ok_or_else(|| {
            format!("Set an hourly rate for {project_name} or its client before invoicing")
        })?,
    };
    let quantity = hours_hundredths(ended - started);
    let description = match description(&line.description)? {
        d if d.is_empty() => description(&entry_description)?,
        d => d,
    };
    Ok(InvoiceLine {
        id: index.to_string(),
        kind: "time".into(),
        entry_id: Some(entry_id.to_owned()),
        project_name,
        description,
        started_at: Some(started as u64),
        ended_at: Some(ended as u64),
        quantity_hundredths: quantity,
        unit: "hrs".into(),
        rate_cents: rate,
        amount_cents: line_amount_cents(quantity, rate)?,
        agent,
    })
}

/// The draft as it would print, resolved but not stored.
pub fn quote(conn: &Connection, input: &DraftInput) -> Result<Invoice, String> {
    resolve(conn, input)
}

/// Creates or updates a draft, reserving exactly its time entries (entries
/// dropped from the draft are released) in one transaction.
pub fn save_draft(conn: &mut Connection, input: &DraftInput, now: u64) -> Result<Invoice, String> {
    let tx = conn.transaction().map_err(err)?;
    if let Some(id) = &input.id {
        let editable: bool = tx
            .query_row(
                "SELECT status = 'draft' FROM invoices WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|_| "Invoice not found".to_string())?;
        if !editable {
            return Err("Only drafts can be edited".into());
        }
    }
    let resolved = resolve(&tx, input)?;
    let id = input
        .id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());

    tx.execute(
        "UPDATE time_entries SET invoice_id = NULL WHERE invoice_id = ?1",
        [&id],
    )
    .map_err(err)?;
    tx.execute("DELETE FROM invoice_lines WHERE invoice_id = ?1", [&id])
        .map_err(err)?;
    let summary = &resolved.summary;
    tx.execute(
        "INSERT INTO invoices (id, client_id, client_name, client_email, client_address, currency,
                               status, issue_date, due_date, terms_days, subject, notes,
                               payment_instructions, total_cents, created_at, updated_at,
                               from_name, from_address, from_email, from_phone)
         VALUES (?1, ?2, ?3, ?4, ?5, 'USD', 'draft', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13,
                 ?14, ?15, ?16, ?17)
         ON CONFLICT(id) DO UPDATE SET
           client_id = excluded.client_id, client_name = excluded.client_name,
           client_email = excluded.client_email, client_address = excluded.client_address,
           from_name = excluded.from_name, from_address = excluded.from_address,
           from_email = excluded.from_email, from_phone = excluded.from_phone,
           issue_date = excluded.issue_date, due_date = excluded.due_date,
           terms_days = excluded.terms_days, subject = excluded.subject, notes = excluded.notes,
           payment_instructions = excluded.payment_instructions,
           total_cents = excluded.total_cents, updated_at = excluded.updated_at",
        params![
            id,
            summary.client_id,
            summary.client_name,
            resolved.bill_to_email,
            resolved.bill_to_address,
            summary.issue_date,
            summary.due_date,
            resolved.terms_days,
            resolved.subject,
            resolved.notes,
            resolved.payment_instructions,
            summary.total_cents,
            now as i64,
            resolved.from_name,
            resolved.from_address,
            resolved.from_email,
            resolved.from_phone,
        ],
    )
    .map_err(err)?;
    for (position, line) in resolved.lines.iter().enumerate() {
        tx.execute(
            "INSERT INTO invoice_lines (id, invoice_id, position, kind, entry_id, project_name,
                                        description, started_at, ended_at, quantity_hundredths,
                                        unit, rate_cents, amount_cents)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                uuid::Uuid::now_v7().to_string(),
                id,
                position as i64,
                line.kind,
                line.entry_id,
                line.project_name,
                line.description,
                line.started_at.map(|v| v as i64),
                line.ended_at.map(|v| v as i64),
                line.quantity_hundredths,
                line.unit,
                line.rate_cents,
                line.amount_cents,
            ],
        )
        .map_err(err)?;
        if let Some(entry_id) = &line.entry_id {
            let updated = tx
                .execute(
                    "UPDATE time_entries SET invoice_id = ?1
                     WHERE id = ?2 AND invoice_id IS NULL AND status = 'approved'
                       AND billable = 1 AND deleted_at IS NULL",
                    params![id, entry_id],
                )
                .map_err(err)?;
            if updated != 1 {
                return Err("Time changed while saving the invoice".into());
            }
        }
    }
    tx.commit().map_err(err)?;
    get(conn, &id)
}

/// The invoice as it prints. Its From block is the invoice's own; only the
/// logo comes from the shared profile. A draft with no From yet prints
/// placeholders so the preview still renders.
fn paper(
    invoice: &Invoice,
    logo: Option<Vec<u8>>,
    number: Option<String>,
) -> Result<PaperInvoice, String> {
    let issue = parse_date(
        invoice
            .summary
            .issue_date
            .as_deref()
            .ok_or("Set an issue date")?,
    )?;
    let due = parse_date(
        invoice
            .summary
            .due_date
            .as_deref()
            .ok_or("Set a due date")?,
    )?;
    let terms = terms_label(invoice.terms_days.unwrap_or(30));
    let mut bill_to_lines: Vec<String> = invoice
        .bill_to_address
        .iter()
        .flat_map(|a| a.lines())
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    if let Some(email) = &invoice.bill_to_email {
        bill_to_lines.push(email.clone());
    }
    let issuer_name = if invoice.from_name.is_empty() {
        "Your business name".to_owned()
    } else {
        invoice.from_name.clone()
    };
    let mut issuer_lines: Vec<String> = if invoice.from_address.is_empty() {
        vec!["Add your address in the From section".to_owned()]
    } else {
        invoice
            .from_address
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    };
    issuer_lines.extend(invoice.from_email.clone());
    issuer_lines.extend(invoice.from_phone.clone());
    let lines = invoice
        .lines
        .iter()
        .map(|line| PaperLine {
            description: line.description.clone(),
            detail: (line.kind == "time").then(|| {
                let day = line
                    .started_at
                    .and_then(|ms| chrono::Local.timestamp_millis_opt(ms as i64).single())
                    .map(|t| format_date(t.date_naive()))
                    .unwrap_or_default();
                let who = if line.agent { " · agents" } else { "" };
                format!("{} · {day}{who}", line.project_name)
            }),
            quantity_hundredths: line.quantity_hundredths,
            unit: line.unit.clone(),
            rate_cents: line.rate_cents,
            amount_cents: line.amount_cents,
        })
        .collect();
    Ok(PaperInvoice {
        number,
        issue_date: format_date(issue),
        due_date: format_date(due),
        terms,
        subject: invoice.subject.clone(),
        issuer_name,
        issuer_lines,
        logo,
        bill_to_name: invoice.summary.client_name.clone(),
        bill_to_lines,
        lines,
        subtotal_cents: invoice.summary.total_cents,
        splits: invoice
            .splits
            .iter()
            .map(|split| split.label.clone())
            .collect(),
        payment_instructions: invoice.payment_instructions.clone(),
        notes: invoice.notes.clone(),
    })
}

/// The DRAFT-watermarked PDF for an unsaved or saved draft.
pub fn render_draft(conn: &Connection, input: &DraftInput) -> Result<Vec<u8>, String> {
    let invoice = resolve(conn, input)?;
    pdf::render(&paper(&invoice, profile::logo_bytes(conn)?, None)?)
}

/// Freezes a draft: allocates the next number for its issue year, renders the
/// official PDF and stores number, snapshot and bytes, all or nothing.
pub fn finalize(conn: &mut Connection, id: &str, now: u64) -> Result<Invoice, String> {
    let tx = conn.transaction().map_err(err)?;
    let invoice = get(&tx, id)?;
    if invoice.summary.status != "draft" {
        return Err("Only drafts can be finalized".into());
    }
    if invoice.lines.is_empty() {
        return Err("Add at least one line item".into());
    }
    if invoice.summary.total_cents <= 0 {
        return Err("The invoice total must be greater than $0.00".into());
    }
    if invoice.from_name.is_empty() || invoice.from_address.is_empty() {
        return Err(
            "Add your business name and address in the From section before finalizing".into(),
        );
    }
    let issue = parse_date(
        invoice
            .summary
            .issue_date
            .as_deref()
            .ok_or("Set an issue date")?,
    )?;
    let number = allocate_number(&tx, issue.year())?;
    let bytes = pdf::render(&paper(
        &invoice,
        profile::logo_bytes(&tx)?,
        Some(number.clone()),
    )?)?;
    let snapshot = serde_json::to_string(&IssuerSnapshot {
        name: invoice.from_name.clone(),
        address: invoice.from_address.clone(),
        email: invoice.from_email.clone(),
        phone: invoice.from_phone.clone(),
    })
    .map_err(err)?;
    tx.execute(
        "UPDATE invoices SET status = 'open', number = ?1, issuer_json = ?2, pdf = ?3,
                issued_at = ?4, updated_at = ?4
         WHERE id = ?5 AND status = 'draft'",
        params![number, snapshot, bytes, now as i64, id],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    get(conn, id)
}

fn allocate_number(tx: &Transaction<'_>, year: i32) -> Result<String, String> {
    let mut next = profile::next_number_for(tx, year)?;
    let number = loop {
        let candidate = format!("INV-{year}-{next:04}");
        let taken: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM invoices WHERE number = ?1)",
                [&candidate],
                |row| row.get(0),
            )
            .map_err(err)?;
        if !taken {
            break candidate;
        }
        next += 1;
    };
    tx.execute(
        "INSERT INTO invoice_sequences (year, next_number) VALUES (?1, ?2)
         ON CONFLICT(year) DO UPDATE SET next_number = excluded.next_number",
        params![year, next + 1],
    )
    .map_err(err)?;
    Ok(number)
}

/// The archived PDF of a finalized invoice.
pub fn stored_pdf(conn: &Connection, id: &str) -> Result<Vec<u8>, String> {
    conn.query_row("SELECT pdf FROM invoices WHERE id = ?1", [id], |row| {
        row.get::<_, Option<Vec<u8>>>(0)
    })
    .map_err(|_| "Invoice not found".to_string())?
    .ok_or_else(|| "This invoice has not been finalized, so it has no PDF".into())
}

/// Suggested file name, e.g. `INV-2026-0001-Acme-Corp.pdf` or `Draft-Acme-Corp.pdf`.
pub fn file_name(number: Option<&str>, client_name: &str) -> String {
    let client: String = client_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let client = client
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let client: String = client.chars().take(40).collect();
    match (number, client.is_empty()) {
        (Some(n), false) => format!("{n}-{client}.pdf"),
        (Some(n), true) => format!("{n}.pdf"),
        (None, false) => format!("Draft-{client}.pdf"),
        (None, true) => "Draft-invoice.pdf".into(),
    }
}

/// `open` <-> `paid`.
pub fn set_paid(conn: &Connection, id: &str, paid: bool, now: u64) -> Result<(), String> {
    let (from, to) = if paid {
        ("open", "paid")
    } else {
        ("paid", "open")
    };
    let changed = conn
        .execute(
            "UPDATE invoices SET status = ?1, paid_at = ?2, updated_at = ?3
             WHERE id = ?4 AND status = ?5",
            params![to, paid.then_some(now as i64), now as i64, id, from],
        )
        .map_err(err)?;
    if changed == 0 {
        return Err(format!("Invoice is not {from}"));
    }
    Ok(())
}

/// Cancels an open invoice: its number and PDF stay on record, its time is
/// released so it can be invoiced again.
pub fn void(conn: &mut Connection, id: &str, now: u64) -> Result<(), String> {
    let tx = conn.transaction().map_err(err)?;
    let changed = tx
        .execute(
            "UPDATE invoices SET status = 'void', updated_at = ?1 WHERE id = ?2 AND status = 'open'",
            params![now as i64, id],
        )
        .map_err(err)?;
    if changed == 0 {
        return Err("Only open invoices can be voided; mark a paid invoice unpaid first".into());
    }
    tx.execute(
        "UPDATE time_entries SET invoice_id = NULL WHERE invoice_id = ?1",
        [id],
    )
    .map_err(err)?;
    tx.execute(
        "UPDATE invoice_lines SET entry_id = NULL WHERE invoice_id = ?1",
        [id],
    )
    .map_err(err)?;
    tx.commit().map_err(err)
}

pub fn delete_draft(conn: &mut Connection, id: &str) -> Result<(), String> {
    let tx = conn.transaction().map_err(err)?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM invoices WHERE id = ?1 AND status = 'draft')",
            [id],
            |row| row.get(0),
        )
        .map_err(err)?;
    if !exists {
        return Err("Only draft invoices can be deleted".into());
    }
    tx.execute(
        "UPDATE time_entries SET invoice_id = NULL WHERE invoice_id = ?1",
        [id],
    )
    .map_err(err)?;
    tx.execute("DELETE FROM invoice_lines WHERE invoice_id = ?1", [id])
        .map_err(err)?;
    tx.execute("DELETE FROM invoices WHERE id = ?1", [id])
        .map_err(err)?;
    tx.commit().map_err(err)
}

#[cfg(test)]
mod tests;
