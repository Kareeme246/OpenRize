use rusqlite::{Connection, Result};

/// Creates the database schema (`user_version` 1) on a new database and does
/// nothing on one that already has it. There is no upgrade path: the schema is
/// a single version, so an older database must be recreated, not upgraded.
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

            -- Invoices: editable drafts that Finalize turns into numbered, frozen
            -- PDFs. Each invoice carries its own From block (`from_*`), copied from
            -- `invoice_profile` when the draft is created and frozen at finalize
            -- (`issuer_json` is the snapshot that was printed). Money is integer
            -- USD cents; quantities are hundredths.
            CREATE TABLE IF NOT EXISTS invoice_profile (
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
            INSERT OR IGNORE INTO invoice_profile (id) VALUES (1);

            CREATE TABLE IF NOT EXISTS invoice_sequences (
              year        INTEGER PRIMARY KEY,
              next_number INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS invoices (
              id                   TEXT PRIMARY KEY,
              client_id            TEXT NOT NULL,
              client_name          TEXT NOT NULL,
              client_email         TEXT,
              client_address       TEXT,
              currency             TEXT NOT NULL,
              status               TEXT NOT NULL CHECK (status IN ('draft', 'open', 'paid', 'void')),
              number               TEXT UNIQUE,
              issue_date           TEXT,
              due_date             TEXT,
              terms_days           INTEGER,
              subject              TEXT,
              notes                TEXT,
              payment_instructions TEXT,
              from_name            TEXT NOT NULL DEFAULT '',
              from_address         TEXT NOT NULL DEFAULT '',
              from_email           TEXT,
              from_phone           TEXT,
              issuer_json          TEXT,
              total_cents          INTEGER NOT NULL DEFAULT 0,
              pdf                  BLOB,
              issued_at            INTEGER,
              paid_at              INTEGER,
              created_at           INTEGER NOT NULL,
              updated_at           INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_invoices_status ON invoices (status);

            CREATE TABLE IF NOT EXISTS invoice_lines (
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
            CREATE INDEX IF NOT EXISTS idx_invoice_lines_invoice ON invoice_lines (invoice_id, position);

            -- Time on an invoice is frozen: it cannot be edited, re-projected or
            -- deleted until its draft is deleted (or the invoice voided).
            CREATE TRIGGER IF NOT EXISTS protect_invoiced_time BEFORE UPDATE OF started_at, ended_at, description, project_id, status, billable, deleted_at ON time_entries
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

    Ok(())
}

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

    #[test]
    fn migrations_run_cleanly_on_fresh_db() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let v: i32 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);

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
    fn running_again_on_an_up_to_date_database_changes_nothing() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();
        conn.execute(
            "UPDATE invoice_profile SET name = 'Offline Studios' WHERE id = 1",
            [],
        )
        .unwrap();
        run_migrations(&mut conn).unwrap();

        let (name, categories): (String, i64) = conn
            .query_row(
                "SELECT (SELECT name FROM invoice_profile WHERE id = 1),
                        (SELECT COUNT(*) FROM categories)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), categories), ("Offline Studios", 12));
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
}
