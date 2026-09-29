use rusqlite::{Connection, Result};

pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    let current_version: i32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;

    if current_version < 1 {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS segments (
              id         INTEGER PRIMARY KEY AUTOINCREMENT,
              app        TEXT    NOT NULL,
              title      TEXT    NOT NULL,
              kind       TEXT    NOT NULL,
              label      TEXT,
              started_at INTEGER NOT NULL,
              ended_at   INTEGER,
              reviewed   INTEGER NOT NULL DEFAULT 0,
              app_id     TEXT,
              bundle_id  TEXT,
              url        TEXT,
              domain     TEXT,
              entry_id   TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_segments_started_at ON segments (started_at);
            CREATE INDEX IF NOT EXISTS idx_segments_ended_at ON segments (ended_at);
            CREATE INDEX IF NOT EXISTS idx_segments_entry_id ON segments (entry_id);

            CREATE TABLE IF NOT EXISTS settings (
              key   TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS daily_rollups (
              day_epoch INTEGER NOT NULL,
              kind      TEXT    NOT NULL,
              total_ms  INTEGER NOT NULL,
              PRIMARY KEY (day_epoch, kind)
            );

            CREATE TABLE IF NOT EXISTS apps (
              id                  TEXT PRIMARY KEY,
              kind                TEXT NOT NULL,
              identifier          TEXT NOT NULL UNIQUE,
              display_name        TEXT NOT NULL,
              icon_png            BLOB,
              default_category_id TEXT,
              default_project_id  TEXT,
              excluded            INTEGER NOT NULL DEFAULT 0,
              first_seen          INTEGER NOT NULL,
              last_seen           INTEGER NOT NULL,
              created_at          INTEGER NOT NULL,
              updated_at          INTEGER NOT NULL,
              deleted_at          INTEGER
            );

            CREATE TABLE IF NOT EXISTS categories (
              id               TEXT PRIMARY KEY,
              name             TEXT NOT NULL,
              color            TEXT NOT NULL,
              description      TEXT,
              ai_prompt        TEXT,
              billable_default INTEGER NOT NULL DEFAULT 0,
              counts_as_work   INTEGER NOT NULL DEFAULT 1,
              archived         INTEGER NOT NULL DEFAULT 0,
              sort             INTEGER NOT NULL DEFAULT 0,
              created_at       INTEGER NOT NULL,
              updated_at       INTEGER NOT NULL,
              deleted_at       INTEGER
            );

            CREATE TABLE IF NOT EXISTS projects (
              id               TEXT PRIMARY KEY,
              client_id        TEXT,
              name             TEXT NOT NULL,
              color            TEXT NOT NULL,
              description      TEXT,
              ai_hints         TEXT,
              status           TEXT NOT NULL DEFAULT 'active',
              due_date         INTEGER,
              budget_kind      TEXT NOT NULL DEFAULT 'none',
              budget_value     REAL,
              budget_period    TEXT NOT NULL DEFAULT 'total',
              billable_default INTEGER NOT NULL DEFAULT 0,
              hourly_rate      REAL,
              created_at       INTEGER NOT NULL,
              updated_at       INTEGER NOT NULL,
              deleted_at       INTEGER
            );

            CREATE TABLE IF NOT EXISTS clients (
              id           TEXT PRIMARY KEY,
              name         TEXT NOT NULL,
              email        TEXT,
              address      TEXT,
              default_rate REAL,
              currency     TEXT,
              created_at   INTEGER NOT NULL,
              updated_at   INTEGER NOT NULL,
              deleted_at   INTEGER
            );

            CREATE TABLE IF NOT EXISTS rules (
              id          TEXT PRIMARY KEY,
              match_kind  TEXT NOT NULL,
              pattern     TEXT NOT NULL,
              category_id TEXT,
              project_id  TEXT,
              priority    INTEGER NOT NULL DEFAULT 0,
              enabled     INTEGER NOT NULL DEFAULT 1,
              origin      TEXT NOT NULL DEFAULT 'manual',
              created_at  INTEGER NOT NULL,
              updated_at  INTEGER NOT NULL,
              deleted_at  INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_rules_project ON rules (project_id);

            CREATE TABLE IF NOT EXISTS time_entries (
              id                 TEXT PRIMARY KEY,
              started_at         INTEGER NOT NULL,
              ended_at           INTEGER NOT NULL,
              description        TEXT NOT NULL,
              description_origin TEXT NOT NULL DEFAULT 'template',
              category_id        TEXT,
              project_id         TEXT,
              status             TEXT NOT NULL DEFAULT 'pending',
              approved_by        TEXT,
              source             TEXT NOT NULL DEFAULT 'auto',
              billable           INTEGER NOT NULL DEFAULT 0,
              invoice_id         TEXT,
              created_at         INTEGER NOT NULL,
              updated_at         INTEGER NOT NULL,
              deleted_at         INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_time_entries_started_at ON time_entries (started_at);
            CREATE INDEX IF NOT EXISTS idx_time_entries_status ON time_entries (status);
            CREATE INDEX IF NOT EXISTS idx_time_entries_project ON time_entries (project_id, started_at);

            CREATE TABLE IF NOT EXISTS suggestions (
              id            TEXT PRIMARY KEY,
              entry_id      TEXT NOT NULL,
              field         TEXT NOT NULL,
              value_id      TEXT,
              confidence    REAL NOT NULL,
              raw_confidence REAL,
              tier          TEXT,
              signals       TEXT,
              rationale     TEXT,
              alternatives  TEXT,
              engine        TEXT NOT NULL DEFAULT 'full',
              model_version TEXT,
              outcome       TEXT,
              created_at    INTEGER NOT NULL,
              updated_at    INTEGER NOT NULL,
              deleted_at    INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_suggestions_entry ON suggestions (entry_id, field, created_at);
            CREATE INDEX IF NOT EXISTS idx_suggestions_created_at ON suggestions (created_at);

            CREATE TABLE IF NOT EXISTS entry_events (
              id       TEXT PRIMARY KEY,
              entry_id TEXT NOT NULL,
              kind     TEXT NOT NULL,
              actor    TEXT NOT NULL,
              payload  TEXT,
              at       INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_entry_events_entry_id ON entry_events (entry_id);

            -- One NLEmbedding vector per entry (512 x f32, little-endian) plus the
            -- feature text it was computed from, reused as few-shot examples.
            CREATE TABLE IF NOT EXISTS entry_embeddings (
              entry_id   TEXT PRIMARY KEY,
              vec        BLOB NOT NULL,
              text_hash  TEXT NOT NULL,
              features   TEXT NOT NULL,
              model      TEXT NOT NULL,
              created_at INTEGER NOT NULL
            );

            -- Durable work queue for the classification worker. `kind` is
            -- classify (full pipeline) or embed (vector only, for entries that
            -- were approved without ever being classified).
            CREATE TABLE IF NOT EXISTS classify_jobs (
              id         TEXT PRIMARY KEY,
              entry_id   TEXT NOT NULL,
              kind       TEXT NOT NULL DEFAULT 'classify',
              state      TEXT NOT NULL DEFAULT 'queued',
              attempts   INTEGER NOT NULL DEFAULT 0,
              next_at    INTEGER NOT NULL,
              last_error TEXT,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL,
              UNIQUE (entry_id, kind)
            );
            CREATE INDEX IF NOT EXISTS idx_classify_jobs_state ON classify_jobs (state, next_at);

            -- Trained personal models (Create ML). Only one per kind is active.
            CREATE TABLE IF NOT EXISTS model_artifacts (
              id          TEXT PRIMARY KEY,
              kind        TEXT NOT NULL,
              path        TEXT NOT NULL,
              trained_at  INTEGER NOT NULL,
              n_examples  INTEGER NOT NULL,
              holdout_acc REAL,
              calibration TEXT,
              active      INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_model_artifacts_kind ON model_artifacts (kind, active, trained_at);

            CREATE TABLE IF NOT EXISTS energy_samples (
              id            INTEGER PRIMARY KEY AUTOINCREMENT,
              sampled_at    INTEGER NOT NULL,
              duration_ms   INTEGER NOT NULL,
              cpu_time_ms   INTEGER NOT NULL,
              energy_nj     INTEGER NOT NULL,
              power_watts   REAL NOT NULL,
              impact_level  TEXT NOT NULL,
              ai_active     INTEGER NOT NULL DEFAULT 0,
              on_battery    INTEGER NOT NULL DEFAULT 0,
              battery_level REAL
            );
            CREATE INDEX IF NOT EXISTS idx_energy_samples_sampled_at ON energy_samples (sampled_at);

            -- Full-text search over window titles for Time Entries (\"where did I
            -- work on invoices.rs?\"). A trigram index answers substring `LIKE`
            -- queries, so \"Rize\" finds \"OpenRize\". It mirrors `segments`
            -- through triggers; `segments.id` is an INTEGER PRIMARY KEY, so the
            -- external-content rowid is stable across VACUUM.
            CREATE VIRTUAL TABLE IF NOT EXISTS segments_fts USING fts5(
              title, content='segments', content_rowid='id', tokenize='trigram'
            );
            CREATE TRIGGER IF NOT EXISTS segments_fts_insert AFTER INSERT ON segments BEGIN
              INSERT INTO segments_fts (rowid, title) VALUES (new.id, new.title);
            END;
            CREATE TRIGGER IF NOT EXISTS segments_fts_delete AFTER DELETE ON segments BEGIN
              INSERT INTO segments_fts (segments_fts, rowid, title) VALUES ('delete', old.id, old.title);
            END;
            CREATE TRIGGER IF NOT EXISTS segments_fts_update AFTER UPDATE OF title ON segments BEGIN
              INSERT INTO segments_fts (segments_fts, rowid, title) VALUES ('delete', old.id, old.title);
              INSERT INTO segments_fts (rowid, title) VALUES (new.id, new.title);
            END;
            ",
        )?;

        seed_default_categories(&tx)?;

        tx.execute("PRAGMA user_version = 1;", [])?;
        tx.commit()?;
    }

    if current_version < 2 {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "CREATE TABLE invoices (
               id TEXT PRIMARY KEY,
               client_id TEXT NOT NULL,
               client_name TEXT NOT NULL,
               client_email TEXT,
               client_address TEXT,
               currency TEXT NOT NULL,
               status TEXT NOT NULL CHECK (status IN ('draft', 'sent', 'paid')),
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE TABLE invoice_lines (
               entry_id TEXT PRIMARY KEY,
               invoice_id TEXT NOT NULL REFERENCES invoices(id),
               project_name TEXT NOT NULL,
               description TEXT NOT NULL,
               started_at INTEGER NOT NULL,
               ended_at INTEGER NOT NULL,
               rate REAL NOT NULL,
               amount_cents INTEGER NOT NULL
             );
             CREATE INDEX idx_invoice_lines_invoice ON invoice_lines(invoice_id);
             CREATE TRIGGER protect_invoiced_time BEFORE UPDATE OF started_at, ended_at, description, project_id, status, billable, deleted_at ON time_entries
             WHEN OLD.invoice_id IS NOT NULL AND (
               NEW.started_at IS NOT OLD.started_at OR NEW.ended_at IS NOT OLD.ended_at OR
               NEW.description IS NOT OLD.description OR NEW.project_id IS NOT OLD.project_id OR
               NEW.status IS NOT OLD.status OR NEW.billable IS NOT OLD.billable OR NEW.deleted_at IS NOT OLD.deleted_at
             ) BEGIN SELECT RAISE(ABORT, 'Invoiced time cannot be edited; delete its draft invoice first'); END;
             PRAGMA user_version = 2;",
        )?;
        tx.commit()?;
    }

    if current_version < 3 {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "DROP TRIGGER protect_invoiced_time;
             CREATE TRIGGER protect_invoiced_time BEFORE UPDATE OF started_at, ended_at, description, project_id, status, billable, deleted_at ON time_entries
             WHEN OLD.invoice_id IS NOT NULL AND (
               NEW.started_at IS NOT OLD.started_at OR NEW.ended_at IS NOT OLD.ended_at OR
               NEW.description IS NOT OLD.description OR
               (NEW.project_id IS NOT OLD.project_id AND NOT (
                 NEW.project_id IS NULL AND EXISTS (
                   SELECT 1 FROM projects WHERE id = OLD.project_id AND deleted_at IS NOT NULL
                 )
               )) OR
               NEW.status IS NOT OLD.status OR NEW.billable IS NOT OLD.billable OR NEW.deleted_at IS NOT OLD.deleted_at
             ) BEGIN SELECT RAISE(ABORT, 'Invoiced time cannot be edited; delete its draft invoice first'); END;
             PRAGMA user_version = 3;",
        )?;
        tx.commit()?;
    }

    if current_version < 4 {
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE categories SET color = '#52b788' WHERE color = '#e5995c';",
            [],
        )?;
        tx.execute(
            "UPDATE categories SET color = '#707bf0' WHERE color = '#e7b447';",
            [],
        )?;
        tx.execute(
            "UPDATE projects SET color = '#52b788' WHERE color = '#e5995c';",
            [],
        )?;
        tx.execute(
            "UPDATE projects SET color = '#707bf0' WHERE color = '#e7b447';",
            [],
        )?;
        tx.execute("PRAGMA user_version = 4;", [])?;
        tx.commit()?;
    }

    if current_version < 5 {
        let tx = conn.transaction()?;
        tx.execute_batch(INVOICE_DOCUMENTS_V5)?;
        tx.execute("PRAGMA user_version = 5;", [])?;
        tx.commit()?;
    }

    Ok(())
}

/// Real invoice documents. Rebuilds `invoices` and `invoice_lines` (their
/// status CHECK and the one-row-per-entry primary key cannot be altered in
/// place) and adds the issuer profile and per-year number sequences.
///
/// Existing invoices are preserved, not reinterpreted. A USD draft becomes an
/// editable draft (issue date from its creation day, Net 30); everything else
/// is marked `legacy`: read-only history with no number or PDF. `sent` maps to
/// `open`. Stored `amount_cents` are kept for legacy rows; rates become cents
/// and hours become hundredths for display only.
const INVOICE_DOCUMENTS_V5: &str = "
ALTER TABLE invoices RENAME TO invoices_v4;

CREATE TABLE invoice_profile (
  id                   INTEGER PRIMARY KEY CHECK (id = 1),
  name                 TEXT NOT NULL DEFAULT '',
  address              TEXT NOT NULL DEFAULT '',
  email                TEXT,
  phone                TEXT,
  payment_instructions TEXT,
  default_notes        TEXT,
  default_terms_days   INTEGER NOT NULL DEFAULT 30,
  logo                 BLOB,
  logo_mime            TEXT,
  updated_at           INTEGER NOT NULL DEFAULT 0
);
INSERT INTO invoice_profile (id) VALUES (1);

CREATE TABLE invoice_sequences (
  year        INTEGER PRIMARY KEY,
  next_number INTEGER NOT NULL
);

CREATE TABLE invoices (
  id                   TEXT PRIMARY KEY,
  client_id            TEXT NOT NULL,
  client_name          TEXT NOT NULL,
  client_email         TEXT,
  client_address       TEXT,
  currency             TEXT NOT NULL,
  status               TEXT NOT NULL CHECK (status IN ('draft', 'open', 'paid', 'void')),
  legacy               INTEGER NOT NULL DEFAULT 0,
  number               TEXT UNIQUE,
  issue_date           TEXT,
  due_date             TEXT,
  terms_days           INTEGER,
  subject              TEXT,
  notes                TEXT,
  payment_instructions TEXT,
  issuer_json          TEXT,
  total_cents          INTEGER NOT NULL DEFAULT 0,
  pdf                  BLOB,
  issued_at            INTEGER,
  paid_at              INTEGER,
  created_at           INTEGER NOT NULL,
  updated_at           INTEGER NOT NULL
);

INSERT INTO invoices (
  id, client_id, client_name, client_email, client_address, currency, status, legacy,
  issue_date, due_date, terms_days, total_cents, created_at, updated_at
)
SELECT
  id, client_id, client_name, client_email, client_address, currency,
  CASE status WHEN 'sent' THEN 'open' ELSE status END,
  CASE WHEN status = 'draft' AND currency = 'USD' THEN 0 ELSE 1 END,
  CASE WHEN status = 'draft' AND currency = 'USD'
       THEN date(created_at / 1000, 'unixepoch', 'localtime') END,
  CASE WHEN status = 'draft' AND currency = 'USD'
       THEN date(created_at / 1000, 'unixepoch', 'localtime', '+30 days') END,
  CASE WHEN status = 'draft' AND currency = 'USD' THEN 30 END,
  COALESCE((SELECT SUM(amount_cents) FROM invoice_lines WHERE invoice_id = invoices_v4.id), 0),
  created_at, updated_at
FROM invoices_v4;

CREATE TABLE invoice_lines_v5 (
  id                  TEXT PRIMARY KEY,
  invoice_id          TEXT NOT NULL REFERENCES invoices(id),
  position            INTEGER NOT NULL,
  kind                TEXT NOT NULL CHECK (kind IN ('time', 'retainer', 'manual')),
  entry_id            TEXT UNIQUE,
  project_name        TEXT NOT NULL DEFAULT '',
  description         TEXT NOT NULL,
  started_at          INTEGER,
  ended_at            INTEGER,
  quantity_hundredths INTEGER NOT NULL,
  unit                TEXT NOT NULL DEFAULT '',
  rate_cents          INTEGER NOT NULL,
  amount_cents        INTEGER NOT NULL
);

INSERT INTO invoice_lines_v5 (
  id, invoice_id, position, kind, entry_id, project_name, description, started_at, ended_at,
  quantity_hundredths, unit, rate_cents, amount_cents
)
SELECT
  lower(hex(randomblob(16))),
  invoice_id,
  row_number() OVER (PARTITION BY invoice_id ORDER BY started_at, entry_id) - 1,
  'time', entry_id, project_name, description, started_at, ended_at,
  MAX(1, CAST(((ended_at - started_at) * 100 + 1800000) / 3600000 AS INTEGER)),
  'hrs',
  CAST(round(rate * 100) AS INTEGER),
  amount_cents
FROM invoice_lines;

-- Editable drafts must satisfy quantity x rate = amount on paper.
UPDATE invoice_lines_v5
SET amount_cents = (quantity_hundredths * rate_cents + 50) / 100
WHERE invoice_id IN (SELECT id FROM invoices WHERE status = 'draft' AND legacy = 0);
UPDATE invoices
SET total_cents = COALESCE((SELECT SUM(amount_cents) FROM invoice_lines_v5 WHERE invoice_id = invoices.id), 0)
WHERE status = 'draft' AND legacy = 0;

DROP TABLE invoice_lines;
DROP TABLE invoices_v4;
ALTER TABLE invoice_lines_v5 RENAME TO invoice_lines;
CREATE INDEX idx_invoice_lines_invoice ON invoice_lines(invoice_id, position);
CREATE INDEX idx_invoices_status ON invoices(status);
";

fn seed_default_categories(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let defaults = [
        ("Coding", "#75a4e5", "Writing, reviewing, debugging code", "Apply when writing, reviewing, or debugging code, using an IDE, terminal, or code editor.", 1, 1, 1),
        ("Research", "#56c2b1", "Documentation and technical research", "Reading documentation, investigating solutions, exploring tools and libraries.", 1, 1, 2),
        ("Communication", "#52b788", "Email, chat, and messaging", "Email, Slack, Discord, async messaging, team coordination.", 0, 1, 3),
        ("Design", "#df84b5", "UI/UX, visual design, assets", "Working in Figma, design tools, creating mockups, UI components, styling.", 1, 1, 4),
        ("Writing", "#e4817d", "Docs, articles, specifications", "Writing prose, documentation, specifications, blog posts, proposals.", 1, 1, 5),
        ("Planning", "#9aa6b4", "Roadmaps, tasks, issue tracking", "Project planning, issue trackers, linear, github issues, backlog management.", 1, 1, 6),
        ("Administrative", "#bfa181", "Invoicing, accounts, setup", "Administrative tasks, billing, accounts, dev environment setup, file management.", 0, 1, 7),
        ("Meetings", "#707bf0", "Calls, video conferences, syncs", "Zoom, Google Meet, video calls, live client meetings, team syncs.", 1, 1, 8),
        ("Learning", "#66b1df", "Courses, tutorials, reading", "Tutorials, educational courses, learning new frameworks and concepts.", 0, 1, 9),
        ("Review", "#9b87df", "Pull requests, code review", "Reviewing pull requests, inspecting diffs, giving feedback on changes.", 1, 1, 10),
        ("Debugging", "#e97e7b", "Bug reproduction, profiling, fixing", "Tracking bugs, stepping through debuggers, profiling performance bottlenecks.", 1, 1, 11),
        ("Break", "#64748b", "Rest, lunch, away from screen", "Away from computer, resting, lunch breaks.", 0, 0, 12),
    ];

    let mut insert_cat = tx.prepare(
        "INSERT INTO categories (id, name, color, description, ai_prompt, billable_default, counts_as_work, archived, sort, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?9);",
    )?;

    for (name, color, desc, prompt, billable, counts, sort) in defaults {
        let id = uuid::Uuid::now_v7().to_string();
        insert_cat.execute((
            id, name, color, desc, prompt, billable, counts, sort, now as i64,
        ))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A structural fingerprint of tables, columns, indexes, and triggers.
    /// Columns are unordered because the old ladder's `ALTER TABLE ADD COLUMN`
    /// statements append them; `sqlite_master.sql` would report that harmless
    /// difference despite equivalent table definitions.
    fn schema_snapshot(conn: &Connection) -> Vec<String> {
        let mut lines = Vec::new();

        let mut tables_stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%'
                 ORDER BY name;",
            )
            .unwrap();
        let table_names: Vec<String> = tables_stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();

        for table in &table_names {
            let mut cols_stmt = conn
                .prepare(&format!("PRAGMA table_info({table});"))
                .unwrap();
            let mut cols: Vec<String> = cols_stmt
                .query_map([], |row| {
                    let name: String = row.get(1)?;
                    let ty: String = row.get(2)?;
                    let notnull: i64 = row.get(3)?;
                    let dflt: Option<String> = row.get(4)?;
                    let pk: i64 = row.get(5)?;
                    Ok(format!("{name}:{ty}:{notnull}:{dflt:?}:{pk}"))
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            cols.sort();
            lines.push(format!("TABLE {table} COLUMNS [{}]", cols.join(", ")));

            let mut idx_stmt = conn
                .prepare(&format!("PRAGMA index_list({table});"))
                .unwrap();
            let indexes: Vec<(String, i64, i64)> = idx_stmt
                .query_map([], |row| Ok((row.get(1)?, row.get(2)?, row.get(4)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            let mut idx_lines: Vec<String> = indexes
                .into_iter()
                .map(|(idx_name, unique, partial)| {
                    let mut info_stmt = conn
                        .prepare(&format!("PRAGMA index_info({idx_name});"))
                        .unwrap();
                    let cols: Vec<String> = info_stmt
                        .query_map([], |row| row.get::<_, String>(2))
                        .unwrap()
                        .collect::<rusqlite::Result<_>>()
                        .unwrap();
                    format!(
                        "{idx_name}: unique={unique} partial={partial} cols=[{}]",
                        cols.join(",")
                    )
                })
                .collect();
            idx_lines.sort();
            lines.push(format!("TABLE {table} INDEXES [{}]", idx_lines.join(", ")));
        }

        let mut triggers_stmt = conn
            .prepare(
                "SELECT name, sql FROM sqlite_master
                 WHERE type = 'trigger'
                 ORDER BY name;",
            )
            .unwrap();
        let mut triggers: Vec<String> = triggers_stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let sql: String = row.get(1)?;
                let normalized = sql.split_whitespace().collect::<Vec<_>>().join(" ");
                Ok(format!("{name}: {normalized}"))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        triggers.sort();
        lines.extend(triggers);

        let mut virtual_tables_stmt = conn
            .prepare(
                "SELECT name, sql FROM sqlite_master
                 WHERE type = 'table' AND sql LIKE 'CREATE VIRTUAL TABLE%'
                 ORDER BY name;",
            )
            .unwrap();
        let mut virtual_tables: Vec<String> = virtual_tables_stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let sql: String = row.get(1)?;
                let normalized = sql.split_whitespace().collect::<Vec<_>>().join(" ");
                Ok(format!("{name}: {normalized}"))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        virtual_tables.sort();
        lines.extend(virtual_tables);

        lines
    }

    #[test]
    fn migrations_run_cleanly_on_fresh_db() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let v: i32 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 5);

        let cat_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM categories;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cat_count, 12);

        conn.execute(
            "SELECT id, sampled_at, power_watts FROM energy_samples LIMIT 1;",
            [],
        )
        .unwrap();
    }

    #[test]
    fn fresh_schema_matches_legacy_schema_upgraded_to_latest() {
        let mut fresh = Connection::open_in_memory().unwrap();
        run_migrations(&mut fresh).unwrap();

        let mut legacy = Connection::open_in_memory().unwrap();
        legacy_v1_through_v7::run(&mut legacy).unwrap();

        let v: i32 = legacy
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 7);
        // The shipped pre-invoice schema is equivalent to legacy v7, but its
        // user_version was reset to 1. Reproduce an upgrade of that install.
        legacy.execute_batch("PRAGMA user_version = 1;").unwrap();
        run_migrations(&mut legacy).unwrap();
        assert_eq!(schema_snapshot(&fresh), schema_snapshot(&legacy));
    }

    /// The v2 invoice tables as shipped, with one row of every kind.
    #[test]
    fn v5_keeps_legacy_invoices_and_upgrades_usd_drafts() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE invoices (
               id TEXT PRIMARY KEY, client_id TEXT NOT NULL, client_name TEXT NOT NULL,
               client_email TEXT, client_address TEXT, currency TEXT NOT NULL,
               status TEXT NOT NULL CHECK (status IN ('draft', 'sent', 'paid')),
               created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
             CREATE TABLE invoice_lines (
               entry_id TEXT PRIMARY KEY, invoice_id TEXT NOT NULL REFERENCES invoices(id),
               project_name TEXT NOT NULL, description TEXT NOT NULL, started_at INTEGER NOT NULL,
               ended_at INTEGER NOT NULL, rate REAL NOT NULL, amount_cents INTEGER NOT NULL);
             INSERT INTO invoices VALUES ('draft-usd', 'c', 'Acme', 'a@x.test', '1 Main', 'USD', 'draft', 86400000, 86400000);
             INSERT INTO invoices VALUES ('draft-eur', 'c', 'Acme', NULL, NULL, 'EUR', 'draft', 86400000, 86400000);
             INSERT INTO invoices VALUES ('sent', 'c', 'Acme', NULL, NULL, 'USD', 'sent', 86400000, 86400000);
             INSERT INTO invoices VALUES ('paid', 'c', 'Acme', NULL, NULL, 'USD', 'paid', 86400000, 86400000);
             INSERT INTO invoice_lines VALUES ('e1', 'draft-usd', 'P', 'Work', 0, 1000000, 100.0, 2778);
             INSERT INTO invoice_lines VALUES ('e2', 'sent', 'P', 'Work', 0, 3600000, 100.5, 10050);
             PRAGMA user_version = 4;",
        )
        .unwrap();
        run_migrations(&mut conn).unwrap();

        let row = |id: &str| -> (String, i64, Option<String>, i64) {
            conn.query_row(
                "SELECT status, legacy, number, total_cents FROM invoices WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
        };
        // A USD draft stays editable; its amount now satisfies quantity x rate:
        // 0.28 h (16m40s) x $100.00 = $28.00 rather than the old exact $27.78.
        assert_eq!(row("draft-usd"), ("draft".into(), 0, None, 2800));
        let (issue, due, terms): (String, String, i64) = conn
            .query_row(
                "SELECT issue_date, due_date, terms_days FROM invoices WHERE id = 'draft-usd'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(terms, 30);
        assert!(issue < due);
        // Everything else is preserved untouched as read-only history.
        assert_eq!(row("draft-eur"), ("draft".into(), 1, None, 0));
        assert_eq!(row("sent"), ("open".into(), 1, None, 10050));
        assert_eq!(row("paid"), ("paid".into(), 1, None, 0));
        let (qty, rate, amount, kind): (i64, i64, i64, String) = conn
            .query_row(
                "SELECT quantity_hundredths, rate_cents, amount_cents, kind FROM invoice_lines WHERE entry_id = 'e2'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (qty, rate, amount, kind.as_str()),
            (100, 10_050, 10_050, "time")
        );
        let profile_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM invoice_profile", [], |r| r.get(0))
            .unwrap();
        assert_eq!(profile_rows, 1);
        let v: i32 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 5);
    }

    #[test]
    fn zero_length_segments_do_not_create_entries() {
        let segment = crate::entry_builder::SegmentInput {
            id: 1,
            app: "Code".to_string(),
            title: "main.rs".to_string(),
            kind: "activity".to_string(),
            label: None,
            started_at: 10,
            ended_at: Some(10),
            entry_id: None,
        };
        let entries = crate::entry_builder::build_entries(
            &[segment],
            &[],
            &crate::entry_builder::EntrySettings::default(),
            1000,
        );

        assert!(entries.is_empty());
    }

    #[test]
    fn window_titles_are_searchable_by_substring() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO segments (app, title, kind, started_at) VALUES ('Zed', 'invoices.rs - OpenRize', 'activity', 1);",
            [],
        )
        .unwrap();

        let hits = |conn: &Connection, pattern: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM segments_fts WHERE title LIKE ?1;",
                [pattern],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(hits(&conn, "%invoices.rs%"), 1);
        assert_eq!(hits(&conn, "%rize%"), 1);

        conn.execute(
            "INSERT INTO segments (app, title, kind, started_at) VALUES ('Safari', 'docs.rs serde', 'activity', 2);",
            [],
        )
        .unwrap();
        assert_eq!(hits(&conn, "%serde%"), 1);

        conn.execute(
            "UPDATE segments SET title = 'renamed' WHERE app = 'Safari';",
            [],
        )
        .unwrap();
        assert_eq!(hits(&conn, "%serde%"), 0);
        assert_eq!(hits(&conn, "%renamed%"), 1);

        conn.execute("DELETE FROM segments WHERE app = 'Zed';", [])
            .unwrap();
        assert_eq!(hits(&conn, "%invoices%"), 0);
    }

    /// The old v1-v7 ladder exists only here so the schema-equivalence test
    /// compares introspected results rather than relying on visual inspection.
    mod legacy_v1_through_v7 {
        use rusqlite::{Connection, Result};

        pub fn run(conn: &mut Connection) -> Result<()> {
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS segments (
                  id         INTEGER PRIMARY KEY AUTOINCREMENT,
                  app        TEXT    NOT NULL,
                  title      TEXT    NOT NULL,
                  kind       TEXT    NOT NULL,
                  label      TEXT,
                  started_at INTEGER NOT NULL,
                  ended_at   INTEGER,
                  reviewed   INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX IF NOT EXISTS idx_segments_started_at ON segments (started_at);
                CREATE INDEX IF NOT EXISTS idx_segments_ended_at ON segments (ended_at);

                CREATE TABLE IF NOT EXISTS settings (
                  key   TEXT PRIMARY KEY,
                  value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS daily_rollups (
                  day_epoch INTEGER NOT NULL,
                  kind      TEXT    NOT NULL,
                  total_ms  INTEGER NOT NULL,
                  PRIMARY KEY (day_epoch, kind)
                );
                PRAGMA user_version = 1;
                ",
            )?;

            let tx = conn.transaction()?;
            tx.execute("ALTER TABLE segments ADD COLUMN app_id TEXT;", [])?;
            tx.execute("ALTER TABLE segments ADD COLUMN bundle_id TEXT;", [])?;
            tx.execute("ALTER TABLE segments ADD COLUMN url TEXT;", [])?;
            tx.execute("ALTER TABLE segments ADD COLUMN domain TEXT;", [])?;
            tx.execute("ALTER TABLE segments ADD COLUMN entry_id TEXT;", [])?;
            tx.execute_batch(
                "
                CREATE INDEX IF NOT EXISTS idx_segments_entry_id ON segments (entry_id);

                CREATE TABLE IF NOT EXISTS apps (
                  id                  TEXT PRIMARY KEY,
                  kind                TEXT NOT NULL,
                  identifier          TEXT NOT NULL UNIQUE,
                  display_name        TEXT NOT NULL,
                  icon_png            BLOB,
                  default_category_id TEXT,
                  default_project_id  TEXT,
                  excluded            INTEGER NOT NULL DEFAULT 0,
                  first_seen          INTEGER NOT NULL,
                  last_seen           INTEGER NOT NULL,
                  created_at          INTEGER NOT NULL,
                  updated_at          INTEGER NOT NULL,
                  deleted_at          INTEGER
                );

                CREATE TABLE IF NOT EXISTS categories (
                  id               TEXT PRIMARY KEY,
                  name             TEXT NOT NULL,
                  color            TEXT NOT NULL,
                  description      TEXT,
                  ai_prompt        TEXT,
                  billable_default INTEGER NOT NULL DEFAULT 0,
                  counts_as_work   INTEGER NOT NULL DEFAULT 1,
                  archived         INTEGER NOT NULL DEFAULT 0,
                  sort             INTEGER NOT NULL DEFAULT 0,
                  created_at       INTEGER NOT NULL,
                  updated_at       INTEGER NOT NULL,
                  deleted_at       INTEGER
                );

                CREATE TABLE IF NOT EXISTS projects (
                  id               TEXT PRIMARY KEY,
                  client_id        TEXT,
                  name             TEXT NOT NULL,
                  color            TEXT NOT NULL,
                  description      TEXT,
                  ai_hints         TEXT,
                  status           TEXT NOT NULL DEFAULT 'active',
                  due_date         INTEGER,
                  budget_kind      TEXT NOT NULL DEFAULT 'none',
                  budget_value     REAL,
                  budget_period    TEXT NOT NULL DEFAULT 'total',
                  billable_default INTEGER NOT NULL DEFAULT 0,
                  hourly_rate      REAL,
                  created_at       INTEGER NOT NULL,
                  updated_at       INTEGER NOT NULL,
                  deleted_at       INTEGER
                );

                CREATE TABLE IF NOT EXISTS clients (
                  id           TEXT PRIMARY KEY,
                  name         TEXT NOT NULL,
                  email        TEXT,
                  address      TEXT,
                  default_rate REAL,
                  currency     TEXT,
                  created_at   INTEGER NOT NULL,
                  updated_at   INTEGER NOT NULL,
                  deleted_at   INTEGER
                );

                CREATE TABLE IF NOT EXISTS rules (
                  id          TEXT PRIMARY KEY,
                  match_kind  TEXT NOT NULL,
                  pattern     TEXT NOT NULL,
                  category_id TEXT,
                  project_id  TEXT,
                  priority    INTEGER NOT NULL DEFAULT 0,
                  enabled     INTEGER NOT NULL DEFAULT 1,
                  origin      TEXT NOT NULL DEFAULT 'manual',
                  created_at  INTEGER NOT NULL,
                  updated_at  INTEGER NOT NULL,
                  deleted_at  INTEGER
                );

                CREATE TABLE IF NOT EXISTS time_entries (
                  id          TEXT PRIMARY KEY,
                  started_at  INTEGER NOT NULL,
                  ended_at    INTEGER NOT NULL,
                  description TEXT NOT NULL,
                  category_id TEXT,
                  project_id  TEXT,
                  status      TEXT NOT NULL DEFAULT 'pending',
                  approved_by TEXT,
                  source      TEXT NOT NULL DEFAULT 'auto',
                  billable    INTEGER NOT NULL DEFAULT 0,
                  invoice_id  TEXT,
                  created_at  INTEGER NOT NULL,
                  updated_at  INTEGER NOT NULL,
                  deleted_at  INTEGER
                );
                CREATE INDEX IF NOT EXISTS idx_time_entries_started_at ON time_entries (started_at);
                CREATE INDEX IF NOT EXISTS idx_time_entries_status ON time_entries (status);

                CREATE TABLE IF NOT EXISTS suggestions (
                  id            TEXT PRIMARY KEY,
                  entry_id      TEXT NOT NULL,
                  field         TEXT NOT NULL,
                  value_id      TEXT,
                  confidence    REAL NOT NULL,
                  signals       TEXT,
                  rationale     TEXT,
                  alternatives  TEXT,
                  engine        TEXT NOT NULL DEFAULT 'full',
                  model_version TEXT,
                  outcome       TEXT,
                  created_at    INTEGER NOT NULL,
                  updated_at    INTEGER NOT NULL,
                  deleted_at    INTEGER
                );

                CREATE TABLE IF NOT EXISTS entry_events (
                  id       TEXT PRIMARY KEY,
                  entry_id TEXT NOT NULL,
                  kind     TEXT NOT NULL,
                  actor    TEXT NOT NULL,
                  payload  TEXT,
                  at       INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_entry_events_entry_id ON entry_events (entry_id);
                ",
            )?;

            super::super::seed_default_categories(&tx)?;
            tx.execute("PRAGMA user_version = 2;", [])?;
            tx.commit()?;

            let tx = conn.transaction()?;
            tx.execute(
                "ALTER TABLE time_entries ADD COLUMN description_origin TEXT NOT NULL DEFAULT 'template';",
                [],
            )?;
            tx.execute_batch(
                "
                CREATE INDEX IF NOT EXISTS idx_suggestions_entry ON suggestions (entry_id, field, created_at);

                CREATE TABLE IF NOT EXISTS entry_embeddings (
                  entry_id   TEXT PRIMARY KEY,
                  vec        BLOB NOT NULL,
                  text_hash  TEXT NOT NULL,
                  features   TEXT NOT NULL,
                  model      TEXT NOT NULL,
                  created_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS classify_jobs (
                  id         TEXT PRIMARY KEY,
                  entry_id   TEXT NOT NULL,
                  kind       TEXT NOT NULL DEFAULT 'classify',
                  state      TEXT NOT NULL DEFAULT 'queued',
                  attempts   INTEGER NOT NULL DEFAULT 0,
                  next_at    INTEGER NOT NULL,
                  last_error TEXT,
                  created_at INTEGER NOT NULL,
                  updated_at INTEGER NOT NULL,
                  UNIQUE (entry_id, kind)
                );
                CREATE INDEX IF NOT EXISTS idx_classify_jobs_state ON classify_jobs (state, next_at);

                CREATE TABLE IF NOT EXISTS model_artifacts (
                  id          TEXT PRIMARY KEY,
                  kind        TEXT NOT NULL,
                  path        TEXT NOT NULL,
                  trained_at  INTEGER NOT NULL,
                  n_examples  INTEGER NOT NULL,
                  holdout_acc REAL,
                  calibration TEXT,
                  active      INTEGER NOT NULL DEFAULT 0
                );
                ",
            )?;
            tx.execute("PRAGMA user_version = 3;", [])?;
            tx.commit()?;

            let tx = conn.transaction()?;
            tx.execute(
                "ALTER TABLE suggestions ADD COLUMN raw_confidence REAL;",
                [],
            )?;
            tx.execute("ALTER TABLE suggestions ADD COLUMN tier TEXT;", [])?;
            tx.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_suggestions_created_at ON suggestions (created_at);
                 CREATE INDEX IF NOT EXISTS idx_model_artifacts_kind ON model_artifacts (kind, active, trained_at);",
            )?;
            tx.execute("PRAGMA user_version = 4;", [])?;
            tx.commit()?;

            let tx = conn.transaction()?;
            tx.execute_batch(
                "
                CREATE VIRTUAL TABLE IF NOT EXISTS segments_fts USING fts5(
                  title, content='segments', content_rowid='id', tokenize='trigram'
                );
                CREATE TRIGGER IF NOT EXISTS segments_fts_insert AFTER INSERT ON segments BEGIN
                  INSERT INTO segments_fts (rowid, title) VALUES (new.id, new.title);
                END;
                CREATE TRIGGER IF NOT EXISTS segments_fts_delete AFTER DELETE ON segments BEGIN
                  INSERT INTO segments_fts (segments_fts, rowid, title) VALUES ('delete', old.id, old.title);
                END;
                CREATE TRIGGER IF NOT EXISTS segments_fts_update AFTER UPDATE OF title ON segments BEGIN
                  INSERT INTO segments_fts (segments_fts, rowid, title) VALUES ('delete', old.id, old.title);
                  INSERT INTO segments_fts (rowid, title) VALUES (new.id, new.title);
                END;
                INSERT INTO segments_fts (segments_fts) VALUES ('rebuild');

                CREATE INDEX IF NOT EXISTS idx_time_entries_project ON time_entries (project_id, started_at);
                CREATE INDEX IF NOT EXISTS idx_rules_project ON rules (project_id);
                ",
            )?;
            tx.execute("PRAGMA user_version = 5;", [])?;
            tx.commit()?;

            let tx = conn.transaction()?;
            tx.execute("PRAGMA user_version = 6;", [])?;
            tx.commit()?;

            let tx = conn.transaction()?;
            tx.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS energy_samples (
                  id           INTEGER PRIMARY KEY AUTOINCREMENT,
                  sampled_at   INTEGER NOT NULL,
                  duration_ms  INTEGER NOT NULL,
                  cpu_time_ms  INTEGER NOT NULL,
                  energy_nj    INTEGER NOT NULL,
                  power_watts  REAL NOT NULL,
                  impact_level TEXT NOT NULL,
                  ai_active    INTEGER NOT NULL DEFAULT 0,
                  on_battery   INTEGER NOT NULL DEFAULT 0,
                  battery_level REAL
                );
                CREATE INDEX IF NOT EXISTS idx_energy_samples_sampled_at ON energy_samples (sampled_at);
                ",
            )?;
            tx.execute("PRAGMA user_version = 7;", [])?;
            tx.commit()?;

            Ok(())
        }
    }
}
