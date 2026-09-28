use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceLine {
    pub entry_id: String,
    pub project_name: String,
    pub description: String,
    pub started_at: u64,
    pub ended_at: u64,
    pub rate: f64,
    pub amount_cents: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invoice {
    pub id: String,
    pub client_id: String,
    pub client_name: String,
    pub client_email: Option<String>,
    pub client_address: Option<String>,
    pub currency: String,
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub lines: Vec<InvoiceLine>,
}

pub fn list(conn: &Connection) -> Result<Vec<Invoice>, String> {
    let mut stmt = conn
        .prepare("SELECT id, client_id, client_name, client_email, client_address, currency, status, created_at, updated_at FROM invoices ORDER BY created_at DESC")
        .map_err(|e| e.to_string())?;
    let mut invoices: Vec<Invoice> = stmt
        .query_map([], |row| {
            Ok(Invoice {
                id: row.get(0)?,
                client_id: row.get(1)?,
                client_name: row.get(2)?,
                client_email: row.get(3)?,
                client_address: row.get(4)?,
                currency: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get::<_, i64>(7)? as u64,
                updated_at: row.get::<_, i64>(8)? as u64,
                lines: Vec::new(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())?;
    let mut lines = conn
        .prepare("SELECT entry_id, project_name, description, started_at, ended_at, rate, amount_cents FROM invoice_lines WHERE invoice_id = ?1 ORDER BY started_at, entry_id")
        .map_err(|e| e.to_string())?;
    for invoice in &mut invoices {
        invoice.lines = lines
            .query_map([&invoice.id], |row| {
                Ok(InvoiceLine {
                    entry_id: row.get(0)?,
                    project_name: row.get(1)?,
                    description: row.get(2)?,
                    started_at: row.get::<_, i64>(3)? as u64,
                    ended_at: row.get::<_, i64>(4)? as u64,
                    rate: row.get(5)?,
                    amount_cents: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;
    }
    Ok(invoices)
}

pub fn create(
    conn: &mut Connection,
    client_id: &str,
    start_ms: u64,
    end_ms: u64,
    now: u64,
) -> Result<Invoice, String> {
    if start_ms >= end_ms || end_ms > i64::MAX as u64 {
        return Err("Choose a valid date range".into());
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let (name, email, address, currency): (String, Option<String>, Option<String>, Option<String>) = tx
        .query_row(
            "SELECT name, email, address, currency FROM clients WHERE id = ?1 AND deleted_at IS NULL",
            [client_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| "Select an existing client".to_string())?;
    let currency = currency.ok_or("Set the client's currency before invoicing")?;
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err("Set a three-letter uppercase currency on the client".into());
    }
    let candidates: Vec<(String, String, String, u64, u64, Option<f64>)> = {
        let mut stmt = tx.prepare(
            "SELECT e.id, p.name, e.description, e.started_at, e.ended_at, COALESCE(p.hourly_rate, c.default_rate)
             FROM time_entries e JOIN projects p ON p.id = e.project_id
             JOIN clients c ON c.id = p.client_id
             WHERE c.id = ?1 AND e.status = 'approved' AND e.billable = 1
               AND e.invoice_id IS NULL AND e.deleted_at IS NULL
               AND e.started_at >= ?2 AND e.started_at < ?3
             ORDER BY e.started_at, e.id",
        ).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![client_id, start_ms as i64, end_ms as i64], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, i64>(3)? as u64,
                    row.get::<_, i64>(4)? as u64,
                    row.get(5)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?
    };
    if candidates.is_empty() {
        return Err("No uninvoiced approved billable time for this client and range".into());
    }
    let id = uuid::Uuid::now_v7().to_string();
    let mut lines = Vec::with_capacity(candidates.len());
    for (entry_id, project_name, description, started_at, ended_at, rate) in candidates {
        let rate =
            rate.ok_or_else(|| format!("Set an hourly rate for {project_name} or its client"))?;
        if !rate.is_finite() || rate < 0.0 || ended_at <= started_at {
            return Err("Invoice time or hourly rate is invalid".into());
        }
        let cents = rate * (ended_at - started_at) as f64 / 3_600_000.0 * 100.0;
        if !cents.is_finite() || cents > i64::MAX as f64 {
            return Err("Invoice amount is too large".into());
        }
        lines.push(InvoiceLine {
            entry_id,
            project_name,
            description,
            started_at,
            ended_at,
            rate,
            amount_cents: cents.round() as i64,
        });
    }
    tx.execute(
        "INSERT INTO invoices (id, client_id, client_name, client_email, client_address, currency, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'draft', ?7, ?7)",
        params![id, client_id, name, email, address, currency, now as i64],
    ).map_err(|e| e.to_string())?;
    for line in &lines {
        tx.execute(
            "INSERT INTO invoice_lines (entry_id, invoice_id, project_name, description, started_at, ended_at, rate, amount_cents) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![line.entry_id, id, line.project_name, line.description, line.started_at as i64, line.ended_at as i64, line.rate, line.amount_cents],
        ).map_err(|e| e.to_string())?;
        let updated = tx.execute(
            "UPDATE time_entries SET invoice_id = ?1 WHERE id = ?2 AND invoice_id IS NULL AND status = 'approved' AND billable = 1 AND deleted_at IS NULL",
            params![id, line.entry_id],
        ).map_err(|e| e.to_string())?;
        if updated != 1 {
            return Err("Time changed while creating the invoice".into());
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(Invoice {
        id,
        client_id: client_id.into(),
        client_name: name,
        client_email: email,
        client_address: address,
        currency,
        status: "draft".into(),
        created_at: now,
        updated_at: now,
        lines,
    })
}

pub fn set_status(conn: &Connection, id: &str, status: &str, now: u64) -> Result<(), String> {
    let previous = match status {
        "sent" => "draft",
        "paid" => "sent",
        _ => return Err("Invoices advance from draft to sent to paid".into()),
    };
    let changed = conn
        .execute(
            "UPDATE invoices SET status = ?1, updated_at = ?2 WHERE id = ?3 AND status = ?4",
            params![status, now as i64, id, previous],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err("Invoice not found or not in the preceding state".into());
    }
    Ok(())
}

pub fn delete_draft(conn: &mut Connection, id: &str) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM invoices WHERE id = ?1 AND status = 'draft')",
            [id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("Only draft invoices can be deleted".into());
    }
    tx.execute(
        "UPDATE time_entries SET invoice_id = NULL WHERE invoice_id = ?1",
        [id],
    )
    .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM invoice_lines WHERE invoice_id = ?1", [id])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM invoices WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_snapshots_rates_prevents_duplicates_and_releases_time_on_delete() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::run_migrations(&mut conn).unwrap();
        conn.execute("INSERT INTO clients (id, name, default_rate, currency, created_at, updated_at) VALUES ('c', 'Client', 100, 'USD', 0, 0)", []).unwrap();
        conn.execute("INSERT INTO projects (id, client_id, name, color, created_at, updated_at) VALUES ('p', 'c', 'Project', '#fff', 0, 0)", []).unwrap();
        conn.execute("INSERT INTO time_entries (id, started_at, ended_at, description, project_id, status, billable, created_at, updated_at) VALUES ('e', 1000, 3601000, 'Work', 'p', 'approved', 1, 0, 0)", []).unwrap();
        let invoice = create(&mut conn, "c", 0, 4_000_000, 5).unwrap();
        assert_eq!(invoice.lines[0].amount_cents, 10_000);
        assert!(create(&mut conn, "c", 0, 4_000_000, 6).is_err());
        assert!(conn
            .execute(
                "UPDATE time_entries SET ended_at = 10000 WHERE id = 'e'",
                []
            )
            .is_err());
        conn.execute("UPDATE clients SET default_rate = 200 WHERE id = 'c'", [])
            .unwrap();
        assert_eq!(list(&conn).unwrap()[0].lines[0].rate, 100.0);
        assert!(set_status(&conn, &invoice.id, "paid", 6).is_err());
        delete_draft(&mut conn, &invoice.id).unwrap();
        let next = create(&mut conn, "c", 0, 4_000_000, 7).unwrap();
        assert_eq!(next.lines[0].amount_cents, 20_000);
        set_status(&conn, &next.id, "sent", 8).unwrap();
        assert!(delete_draft(&mut conn, &next.id).is_err());
        set_status(&conn, &next.id, "paid", 9).unwrap();
        assert_eq!(list(&conn).unwrap()[0].status, "paid");
    }
}
