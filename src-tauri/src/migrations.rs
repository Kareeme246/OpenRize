use rusqlite::{Connection, Result};

pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    let current_version: i32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;

    if current_version < 1 {
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
    }

    let version_after_v1: i32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;

    if version_after_v1 < 2 {
        let tx = conn.transaction()?;

        // Add new columns to segments table if not present
        let mut existing_cols = std::collections::HashSet::new();
        {
            let mut stmt = tx.prepare("PRAGMA table_info(segments);")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            for col in rows.flatten() {
                existing_cols.insert(col);
            }
        }

        if !existing_cols.contains("app_id") {
            tx.execute("ALTER TABLE segments ADD COLUMN app_id TEXT;", [])?;
        }
        if !existing_cols.contains("bundle_id") {
            tx.execute("ALTER TABLE segments ADD COLUMN bundle_id TEXT;", [])?;
        }
        if !existing_cols.contains("url") {
            tx.execute("ALTER TABLE segments ADD COLUMN url TEXT;", [])?;
        }
        if !existing_cols.contains("domain") {
            tx.execute("ALTER TABLE segments ADD COLUMN domain TEXT;", [])?;
        }
        if !existing_cols.contains("entry_id") {
            tx.execute("ALTER TABLE segments ADD COLUMN entry_id TEXT;", [])?;
        }

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

        // Seed 12 default categories if table is empty
        let cat_count: i64 = tx.query_row("SELECT COUNT(*) FROM categories;", [], |r| r.get(0))?;
        if cat_count == 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            let defaults = [
                ("Coding", "#75a4e5", "Writing, reviewing, debugging code", "Apply when writing, reviewing, or debugging code, using an IDE, terminal, or code editor.", 1, 1, 1),
                ("Research", "#56c2b1", "Documentation and technical research", "Reading documentation, investigating solutions, exploring tools and libraries.", 1, 1, 2),
                ("Communication", "#e5995c", "Email, chat, and messaging", "Email, Slack, Discord, async messaging, team coordination.", 0, 1, 3),
                ("Design", "#df84b5", "UI/UX, visual design, assets", "Working in Figma, design tools, creating mockups, UI components, styling.", 1, 1, 4),
                ("Writing", "#e4817d", "Docs, articles, specifications", "Writing prose, documentation, specifications, blog posts, proposals.", 1, 1, 5),
                ("Planning", "#9aa6b4", "Roadmaps, tasks, issue tracking", "Project planning, issue trackers, linear, github issues, backlog management.", 1, 1, 6),
                ("Administrative", "#bfa181", "Invoicing, accounts, setup", "Administrative tasks, billing, accounts, dev environment setup, file management.", 0, 1, 7),
                ("Meetings", "#e7b447", "Calls, video conferences, syncs", "Zoom, Google Meet, video calls, live client meetings, team syncs.", 1, 1, 8),
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
        }

        // Backfill apps table from existing segments
        {
            let mut insert_app = tx.prepare(
                "INSERT OR IGNORE INTO apps (id, kind, identifier, display_name, excluded, first_seen, last_seen, created_at, updated_at)
                 SELECT ?1, 'app', s.app, s.app, 0, MIN(s.started_at), MAX(COALESCE(s.ended_at, s.started_at)), MIN(s.started_at), MAX(COALESCE(s.ended_at, s.started_at))
                 FROM segments s
                 WHERE s.app != ''
                 GROUP BY s.app;",
            )?;
            let id = uuid::Uuid::now_v7().to_string();
            insert_app.execute([id])?;
        }

        tx.execute("PRAGMA user_version = 2;", [])?;
        tx.commit()?;
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
        assert_eq!(v, 2);

        let cat_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM categories;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cat_count, 12);
    }

    #[test]
    fn migrations_upgrade_from_v1() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE segments (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              app TEXT NOT NULL,
              title TEXT NOT NULL,
              kind TEXT NOT NULL,
              label TEXT,
              started_at INTEGER NOT NULL,
              ended_at INTEGER,
              reviewed INTEGER NOT NULL DEFAULT 0
            );
            PRAGMA user_version = 1;",
        )
        .unwrap();

        run_migrations(&mut conn).unwrap();

        let v: i32 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 2);

        // Check new column exists
        conn.execute("SELECT bundle_id, url, entry_id FROM segments LIMIT 1;", [])
            .unwrap();
    }
}
