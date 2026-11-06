//! Erases private-window titles, URLs and domains once their entry is classified or
//! [`BACKSTOP_MS`] has passed. `secure_delete` and [`scrub_wal`] stop old bytes lingering;
//! private segments skip the title search index.

use rusqlite::Connection;

use super::REBUILD_WINDOW_MS;
use crate::capture::PRIVATE_TITLE;

pub const BACKSTOP_MS: u64 = REBUILD_WINDOW_MS;

/// Erases the detail of every private segment that is due; truncates the WAL if any row changed.
pub fn redact_private_detail(conn: &Connection, now: u64) -> Result<usize, String> {
    // The literals below are the ones in the `idx_segments_private_detail`
    // partial index, which is what keeps this cheap when nothing is due:
    // SQLite only uses a partial index for a query that repeats its condition.
    let changed = conn
        .execute(
            &format!(
                "UPDATE segments SET title = '{PRIVATE_TITLE}', url = NULL, domain = NULL
                 WHERE private = 1
                   AND (url IS NOT NULL OR domain IS NOT NULL OR title <> '{PRIVATE_TITLE}')
                   AND ended_at IS NOT NULL
                   AND (
                     ended_at < ?1
                     OR entry_id IN (
                       SELECT e.id FROM time_entries e
                       WHERE e.deleted_at IS NOT NULL
                          OR e.status = 'approved'
                          OR (e.status = 'pending' AND (
                                e.category_id IS NOT NULL
                                OR e.project_id IS NOT NULL
                                OR EXISTS (SELECT 1 FROM classify_jobs j
                                           WHERE j.entry_id = e.id AND j.kind = 'classify'
                                             AND j.state = 'done')))
                     )
                   );"
            ),
            [now.saturating_sub(BACKSTOP_MS) as i64],
        )
        .map_err(|error| error.to_string())?;
    if changed > 0 {
        scrub_wal(conn);
    }
    Ok(changed)
}

/// Truncates the write-ahead log, which holds the pre-erase page. It never waits: if a reader holds
/// the log it is skipped and a later checkpoint rewrites it.
pub fn scrub_wal(conn: &Connection) {
    let wait = std::time::Duration::from_secs(5);
    if conn.busy_timeout(std::time::Duration::ZERO).is_err() {
        return;
    }
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |_| Ok(()));
    let _ = conn.busy_timeout(wait);
}

#[cfg(test)]
mod tests {
    use super::super::{ActivityStore, KIND_ACTIVITY};
    use super::*;
    use crate::capture::WindowSample;
    use rusqlite::params;

    const MIN: u64 = 60_000;
    const HOUR: u64 = 60 * MIN;

    fn store() -> ActivityStore {
        let mut store = ActivityStore::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        store.tracking_hours.enabled = false;
        store
    }

    fn sample(app: &str, title: &str, url: Option<&str>, private: bool) -> Option<WindowSample> {
        Some(WindowSample {
            app: app.to_string(),
            title: title.to_string(),
            bundle_id: Some(format!("{app}.exe")),
            url: url.map(str::to_string),
            domain: url.and_then(|url| {
                url.split("://")
                    .nth(1)
                    .and_then(|rest| rest.split('/').next())
                    .map(str::to_string)
            }),
            private,
            keeps_display_awake: false,
        })
    }

    fn track(store: &mut ActivityStore, sample: &Option<WindowSample>, from: u64, to: u64) {
        for now in (from..=to).step_by(5_000) {
            store.tick(sample.clone(), 0, now).unwrap();
        }
        store.close_active_segment(to).unwrap();
    }

    fn segment_rows(store: &ActivityStore) -> Vec<(String, Option<String>, Option<String>)> {
        let mut statement = store
            .conn
            .prepare("SELECT title, url, domain FROM segments WHERE private = 1 ORDER BY id")
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn total_ms(store: &ActivityStore) -> i64 {
        store
            .conn
            .query_row(
                "SELECT COALESCE(SUM(ended_at - started_at), 0) FROM segments WHERE kind = ?1",
                [KIND_ACTIVITY],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_private_window_is_stored_with_its_address_trimmed_and_out_of_search() {
        let mut store = store();
        let private = sample(
            "browser",
            "secret page",
            Some("https://example.com/a/b?q=hunter2#top"),
            true,
        );
        track(&mut store, &private, 0, 10 * MIN);

        let rows = segment_rows(&store);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "secret page");
        assert_eq!(rows[0].1.as_deref(), Some("https://example.com/a/b"));
        let indexed: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM segments_fts WHERE segments_fts MATCH 'secret'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 0);
        let sites: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM apps WHERE kind = 'site'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(sites, 0);

        let normal = sample("browser", "open page", Some("https://docs.rs/x?y=1"), false);
        track(&mut store, &normal, 20 * MIN, 30 * MIN);
        let indexed: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM segments_fts WHERE segments_fts MATCH 'open'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 1);
    }

    #[test]
    fn switching_between_private_and_normal_in_one_browser_makes_new_segments() {
        let mut store = store();
        let normal = sample("browser", "same page", Some("https://a.example/p"), false);
        let private = sample("browser", "same page", Some("https://a.example/p"), true);
        for now in (0..=30_000).step_by(5_000) {
            store.tick(normal.clone(), 0, now).unwrap();
        }
        for now in (35_000..=120_000).step_by(5_000) {
            store.tick(private.clone(), 0, now).unwrap();
        }
        store.close_active_segment(120_000).unwrap();
        let flags: Vec<i64> = store
            .conn
            .prepare("SELECT private FROM segments ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(flags.contains(&0) && flags.contains(&1));
    }

    #[test]
    fn a_window_reported_private_late_is_marked_instead_of_split() {
        let mut store = store();
        let late = sample("browser", "page", Some("https://a.example/p?x=1"), false);
        let known = sample("browser", "page", Some("https://a.example/p"), true);
        store.tick(late, 0, 0).unwrap();
        store.tick(known.clone(), 0, 2_000).unwrap();
        for now in (4_000..=60_000).step_by(2_000) {
            store.tick(known.clone(), 0, now).unwrap();
        }
        store.close_active_segment(60_000).unwrap();
        let rows: Vec<(i64, Option<String>)> = store
            .conn
            .prepare("SELECT private, url FROM segments ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows, vec![(1, Some("https://a.example/p".to_string()))]);
    }

    fn private_entry(store: &mut ActivityStore) -> String {
        let private = sample(
            "browser",
            "secret page",
            Some("https://example.com/secret"),
            true,
        );
        track(store, &private, 0, 10 * MIN);
        store.rebuild_range(0, 3 * HOUR, 3 * HOUR).unwrap();
        store.list_time_entries(0, u64::MAX / 2).unwrap()[0]
            .id
            .clone()
    }

    #[test]
    fn nothing_is_erased_while_the_entry_is_unclassified() {
        let mut store = store();
        let id = private_entry(&mut store);
        assert_eq!(store.time_entry(&id).unwrap().status, "processing");
        assert_eq!(redact_private_detail(&store.conn, 3 * HOUR).unwrap(), 0);
        assert_eq!(segment_rows(&store)[0].0, "secret page");
    }

    #[test]
    fn a_finished_classification_erases_the_detail_and_nothing_else() {
        let mut store = store();
        let id = private_entry(&mut store);
        let before = total_ms(&store);
        let entry_before = store.time_entry(&id).unwrap();

        store
            .conn
            .execute(
                "UPDATE time_entries SET status = 'pending', category_id = 'c' WHERE id = ?1",
                [&id],
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE classify_jobs SET state = 'done' WHERE entry_id = ?1",
                [&id],
            )
            .unwrap();
        assert_eq!(redact_private_detail(&store.conn, 3 * HOUR).unwrap(), 1);

        let rows = segment_rows(&store);
        assert_eq!(rows, vec![(PRIVATE_TITLE.to_string(), None, None)]);
        assert_eq!(total_ms(&store), before);
        let owned: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM segments WHERE entry_id = ?1 AND app = 'browser'",
                [&id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owned, 1);
        let entry_after = store.time_entry(&id).unwrap();
        assert_eq!(
            (entry_before.started_at, entry_before.ended_at),
            (entry_after.started_at, entry_after.ended_at)
        );
        assert_eq!(entry_after.description, entry_before.description);
        assert!(!entry_after.description.contains("secret"));
        assert_eq!(redact_private_detail(&store.conn, 3 * HOUR).unwrap(), 0);
    }

    #[test]
    fn an_approved_entry_is_erased_and_a_failed_classification_is_not() {
        let mut store = store();
        let id = private_entry(&mut store);
        store
            .conn
            .execute(
                "UPDATE time_entries SET status = 'pending' WHERE id = ?1",
                [&id],
            )
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE classify_jobs SET state = 'failed' WHERE entry_id = ?1",
                [&id],
            )
            .unwrap();
        // "Retry" must still be able to read the titles.
        assert_eq!(redact_private_detail(&store.conn, 3 * HOUR).unwrap(), 0);

        store
            .conn
            .execute(
                "UPDATE time_entries SET status = 'approved' WHERE id = ?1",
                [&id],
            )
            .unwrap();
        assert_eq!(redact_private_detail(&store.conn, 3 * HOUR).unwrap(), 1);
    }

    #[test]
    fn the_backstop_erases_private_time_that_never_became_a_classified_entry() {
        let mut store = store();
        let private = sample("browser", "quick look", Some("https://a.example/x"), true);
        track(&mut store, &private, 0, 3 * MIN);
        store.rebuild_range(0, 3 * HOUR, 3 * HOUR).unwrap();
        assert!(store.list_time_entries(0, u64::MAX / 2).unwrap().is_empty());

        assert_eq!(
            redact_private_detail(&store.conn, 3 * MIN + BACKSTOP_MS - 1).unwrap(),
            0
        );
        let before = total_ms(&store);
        assert_eq!(
            redact_private_detail(&store.conn, 3 * MIN + BACKSTOP_MS + 1).unwrap(),
            1
        );
        assert_eq!(segment_rows(&store)[0].0, PRIVATE_TITLE);
        assert_eq!(total_ms(&store), before);
    }

    #[test]
    fn the_open_segment_and_normal_windows_are_never_touched() {
        let mut store = store();
        let normal = sample("browser", "open page", Some("https://docs.rs/x?y=1"), false);
        track(&mut store, &normal, 0, 10 * MIN);
        let private = sample("browser", "live page", Some("https://a.example/z"), true);
        store.tick(private, 0, 11 * MIN).unwrap();
        assert_eq!(
            redact_private_detail(&store.conn, 11 * MIN + 10 * BACKSTOP_MS).unwrap(),
            0
        );
        let normal_title: String = store
            .conn
            .query_row("SELECT title FROM segments WHERE private = 0", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(normal_title, "open page");
    }

    #[test]
    fn a_private_title_never_reaches_the_entry_description() {
        let mut store = store();
        let id = private_entry(&mut store);
        assert!(!store
            .time_entry(&id)
            .unwrap()
            .description
            .contains("secret"));
    }

    #[test]
    fn the_sweep_runs_with_the_store_and_the_index_matches_the_statement() {
        let dir = std::env::temp_dir().join(format!("openrize-redact-{}", uuid::Uuid::now_v7()));
        {
            let mut store = ActivityStore::load(&dir).unwrap();
            store.tracking_hours.enabled = false;
            let id = private_entry(&mut store);
            store
                .conn
                .execute(
                    "UPDATE time_entries SET status = 'approved' WHERE id = ?1",
                    params![id],
                )
                .unwrap();
        }
        let store = ActivityStore::load(&dir).unwrap();
        assert_eq!(segment_rows(&store)[0].0, PRIVATE_TITLE);
        let index: String = store
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'idx_segments_private_detail'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(index.contains(&format!("'{PRIVATE_TITLE}'")));
        let secure: i64 = store
            .conn
            .query_row("PRAGMA secure_delete", [], |row| row.get(0))
            .unwrap();
        assert_eq!(secure, 1);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }
}
