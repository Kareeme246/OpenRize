//! What the "Keep activity until" sweep, deleting an entry and the address migration leave
//! behind, down to the bytes of the database file.
use super::*;

const NOW: u64 = 100 * ONE_DAY_MS;
const OLD: u64 = NOW - 40 * ONE_DAY_MS;
const RECENT: u64 = NOW - ONE_DAY_MS;
const HOUR: u64 = 60 * 60 * 1000;

fn store() -> ActivityStore {
    let mut store = ActivityStore::from_conn(Connection::open_in_memory().unwrap()).unwrap();
    store.tracking_hours.enabled = false;
    store
}

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("openrize-{name}-{}", uuid::Uuid::now_v7()))
}

fn count(store: &ActivityStore, sql: &str) -> i64 {
    store.conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn text(store: &ActivityStore, sql: &str) -> Option<String> {
    store.conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

/// An old and a recent entry, each with everything the app derives from one.
fn seed(store: &ActivityStore) {
    for (id, at) in [("old", OLD), ("recent", RECENT)] {
        let end = at + HOUR;
        store.conn.execute_batch(&format!(
            "INSERT INTO time_entries (id, started_at, ended_at, description, status, source, created_at, updated_at)
               VALUES ('{id}', {at}, {end}, 'Zqxwvj plan', 'approved', 'auto', {at}, {at});
             INSERT INTO segments (app, title, kind, started_at, ended_at, entry_id, url, domain)
               VALUES ('Browser', 'Zqxwvj Kplmnb {id}', 'activity', {at}, {end}, '{id}', 'https://bank.example/{id}', 'bank.example');
             INSERT INTO entry_embeddings (entry_id, vec, text_hash, features, model, created_at)
               VALUES ('{id}', x'00', 'hash', 'titles: Zqxwvj Kplmnb; domains: bank.example', 'model', {at});
             INSERT INTO suggestions (id, entry_id, field, value_id, confidence, raw_confidence, signals, rationale, alternatives, engine, outcome, created_at, updated_at)
               VALUES ('s-{id}', '{id}', 'category', 'c', 0.9, 0.8, '{{\"items\":[{{\"kind\":\"title\",\"text\":\"Zqxwvj Kplmnb\"}}]}}', 'Zqxwvj Kplmnb', '[]', 'full', 'accepted', {at}, {at});
             INSERT INTO entry_events (id, entry_id, kind, actor, payload, at) VALUES
               ('edit-{id}', '{id}', 'edited', 'user', '{{\"draft\":\"Zqxwvj\",\"final\":\"Plan\"}}', {at}),
               ('undo-{id}', '{id}', 'undone', 'user', '{{\"action\":\"Edit\"}}', {at}),
               ('ok-{id}', '{id}', 'accepted', 'user', NULL, {at});
             INSERT INTO classify_jobs (id, entry_id, kind, state, next_at, created_at, updated_at)
               VALUES ('j-{id}', '{id}', 'classify', 'done', {at}, {at}, {at});
             INSERT INTO breaks (id, source, planned_ms, status, created_at, updated_at)
               VALUES ('b-{id}', 'manual', 1, 'done', {at}, {at});
             INSERT INTO apps (id, kind, identifier, display_name, first_seen, last_seen, created_at, updated_at)
               VALUES ('site-{id}', 'site', '{id}.example', 'Site', {at}, {at}, {at}, {at});"
        )).unwrap();
    }
    store.conn.execute_batch(&format!(
        "INSERT INTO apps (id, kind, identifier, display_name, excluded, default_category_id, first_seen, last_seen, created_at, updated_at) VALUES
           ('excluded', 'site', 'excluded.example', 'Excluded', 1, NULL, {OLD}, {OLD}, {OLD}, {OLD}),
           ('defaulted', 'app', 'com.example.editor', 'Editor', 0, 'c', {OLD}, {OLD}, {OLD}, {OLD});"
    )).unwrap();
}

#[test]
fn the_sweep_removes_what_was_derived_from_old_activity_and_keeps_the_timesheet() {
    let mut store = store();
    seed(&store);
    let verdicts = crate::ai::store::outcome_count(&store.conn, NOW).unwrap();
    assert_eq!(verdicts, 2);

    assert_eq!(store.purge_older_than(30, NOW).unwrap(), 1);

    // The timeline and everything read off it are gone for the old entry only.
    assert_eq!(count(&store, "SELECT COUNT(*) FROM segments"), 1);
    assert_eq!(
        text(
            &store,
            "SELECT group_concat(entry_id) FROM entry_embeddings"
        )
        .as_deref(),
        Some("recent")
    );
    assert_eq!(
        text(&store, "SELECT group_concat(id) FROM breaks").as_deref(),
        Some("b-recent")
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM suggestions WHERE entry_id = 'old'
               AND signals IS NULL AND rationale IS NULL AND confidence = 0.9 AND outcome = 'accepted'"
        ),
        1
    );
    assert_eq!(
        text(
            &store,
            "SELECT rationale FROM suggestions WHERE entry_id = 'recent'"
        )
        .as_deref(),
        Some("Zqxwvj Kplmnb")
    );
    // Events stay as the entry's history; only the text they carried goes. An undo's payload
    // names the action and is what its label is made of.
    assert_eq!(
        text(
            &store,
            "SELECT group_concat(id || '=' || COALESCE(payload, '-'), ' ') FROM
               (SELECT id, payload FROM entry_events WHERE entry_id = 'old' ORDER BY id)"
        )
        .as_deref(),
        Some("edit-old=- ok-old=- undo-old={\"action\":\"Edit\"}")
    );
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM entry_events WHERE entry_id = 'recent' AND payload LIKE '%Zqxwvj%'"),
        1
    );
    // A site not seen since goes; one the user set something on stays.
    assert_eq!(
        text(
            &store,
            "SELECT group_concat(id) FROM (SELECT id FROM apps ORDER BY id)"
        )
        .as_deref(),
        Some("defaulted,excluded,site-recent")
    );

    // The entries are the timesheet: both stay, described, and no job comes back for them.
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM time_entries WHERE description = 'Zqxwvj plan' AND deleted_at IS NULL"),
        2
    );
    assert_eq!(count(&store, "SELECT COUNT(*) FROM classify_jobs"), 2);
    assert_eq!(
        crate::ai::store::enqueue_unclassified(&store.conn, 0, NOW, NOW).unwrap(),
        0
    );
    assert_eq!(
        crate::ai::store::outcome_count(&store.conn, NOW).unwrap(),
        verdicts
    );
    assert!(
        crate::ai::store::latest_suggestions(&store.conn, "old").unwrap()[0]
            .rationale
            .is_empty()
    );

    let changes = store.conn.total_changes();
    assert_eq!(store.purge_older_than(30, NOW).unwrap(), 0);
    assert_eq!(
        store.conn.total_changes(),
        changes,
        "the sweep is not idempotent"
    );
}

#[test]
fn keeping_activity_forever_removes_nothing_derived() {
    let mut store = store();
    seed(&store);
    let changes = store.conn.total_changes();
    assert_eq!(store.purge_older_than(0, NOW).unwrap(), 0);
    assert_eq!(store.conn.total_changes(), changes);
}

fn deletable(store: &ActivityStore) {
    store.conn.execute_batch(
        "INSERT INTO time_entries (id, started_at, ended_at, description, created_at, updated_at, source) VALUES
           ('a', 100, 200, 'Zqxwvj call', 1, 1, 'manual'), ('b', 200, 300, 'Kept', 1, 1, 'manual');
         INSERT INTO segments (id, app, title, kind, started_at, ended_at, entry_id) VALUES (1, 'Editor', 'Zqxwvj', 'activity', 100, 200, 'a');
         INSERT INTO entry_embeddings (entry_id, vec, text_hash, features, model, created_at) VALUES
           ('a', x'00', 'h', 'Zqxwvj', 'm', 1), ('b', x'00', 'h', 'kept', 'm', 1);
         INSERT INTO suggestions (id, entry_id, field, confidence, rationale, created_at, updated_at) VALUES
           ('sa', 'a', 'category', 0.5, 'Zqxwvj', 1, 1), ('sb', 'b', 'category', 0.5, 'kept', 1, 1);
         INSERT INTO entry_events (id, entry_id, kind, actor, payload, at) VALUES
           ('ea', 'a', 'edited', 'user', 'Zqxwvj', 1), ('eb', 'b', 'edited', 'user', 'kept', 1);
         INSERT INTO classify_jobs (id, entry_id, kind, state, next_at, created_at, updated_at) VALUES
           ('ja', 'a', 'classify', 'done', 1, 1, 1), ('jb', 'b', 'classify', 'done', 1, 1, 1);",
    ).unwrap();
}

fn rows_of(store: &ActivityStore, entry: &str) -> i64 {
    [
        "suggestions",
        "classify_jobs",
        "entry_embeddings",
        "entry_events",
    ]
    .iter()
    .map(|table| {
        count(
            store,
            &format!("SELECT COUNT(*) FROM {table} WHERE entry_id = '{entry}'"),
        )
    })
    .sum::<i64>()
        + count(
            store,
            &format!("SELECT COUNT(*) FROM time_entries WHERE id = '{entry}'"),
        )
}

#[test]
fn a_deleted_entry_can_be_undone_for_a_day_and_is_then_gone_with_all_it_left() {
    let mut store = store();
    deletable(&store);
    store.delete_time_entry("a", NOW).unwrap();
    assert_eq!(rows_of(&store, "a"), 5, "deleting only hides the entry");

    // Inside the window the sweep leaves it, and undo brings it back whole.
    store
        .purge_older_than(0, NOW + DELETED_ENTRY_GRACE_MS - 1)
        .unwrap();
    assert_eq!(rows_of(&store, "a"), 5);
    store.undo(NOW).unwrap();
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM time_entries WHERE id = 'a' AND deleted_at IS NULL"
        ),
        1
    );
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM segments WHERE entry_id = 'a'"),
        1
    );

    store.delete_time_entry("a", NOW).unwrap();
    store
        .purge_older_than(0, NOW + DELETED_ENTRY_GRACE_MS + 1)
        .unwrap();
    assert_eq!(rows_of(&store, "a"), 0);
    assert_eq!(rows_of(&store, "b"), 5, "another entry was touched");

    // The window has closed: undo says so instead of half-restoring.
    let error = store.undo(NOW + DELETED_ENTRY_GRACE_MS + 2).unwrap_err();
    assert!(error.contains("changed since"), "{error}");
    assert_eq!(rows_of(&store, "a"), 0);
}

#[test]
fn an_invoiced_entry_is_never_removed_by_the_sweep() {
    let mut store = store();
    deletable(&store);
    store
        .conn
        .execute(
            "UPDATE time_entries SET deleted_at = 1, invoice_id = 'inv' WHERE id = 'a'",
            [],
        )
        .unwrap();
    store.purge_older_than(7, NOW).unwrap();
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM time_entries WHERE id = 'a'"),
        1
    );
}

fn file_bytes(dir: &Path) -> Vec<u8> {
    ["", "-wal", "-shm"]
        .iter()
        .filter_map(|suffix| std::fs::read(dir.join(format!("{DB_FILE}{suffix}"))).ok())
        .flatten()
        .collect()
}

fn holds(bytes: &[u8], needle: &str) -> bool {
    bytes
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// The title as stored, and three of the trigrams the search index makes of it.
const TRACES: [&str; 5] = ["Zqxwvj", "zqx", "xwv", "kpl", "bank.example"];

#[test]
fn restarting_ends_the_undo_window_and_the_file_keeps_nothing_of_the_entry() {
    let dir = temp_dir("retention-restart");
    {
        let mut store = ActivityStore::load(&dir).unwrap();
        deletable(&store);
        store.delete_time_entry("a", NOW).unwrap();
        assert_eq!(rows_of(&store, "a"), 5);
    }
    let store = ActivityStore::load(&dir).unwrap();
    assert_eq!(rows_of(&store, "a"), 0);
    assert_eq!(rows_of(&store, "b"), 5);
    drop(store);
    assert!(!holds(&file_bytes(&dir), "Zqxwvj"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_swept_title_is_not_in_the_database_file_in_any_form() {
    let dir = temp_dir("retention-bytes");
    let mut store = ActivityStore::load(&dir).unwrap();
    store.tracking_hours.enabled = false;
    seed(&store);
    store
        .conn
        .execute("DELETE FROM time_entries WHERE id = 'recent'", [])
        .unwrap();
    for table in [
        "segments",
        "entry_embeddings",
        "suggestions",
        "entry_events",
    ] {
        store
            .conn
            .execute(
                &format!("DELETE FROM {table} WHERE entry_id = 'recent'"),
                [],
            )
            .unwrap();
    }
    store
        .conn
        .execute("UPDATE time_entries SET description = 'Planning'", [])
        .unwrap();
    redact::scrub_wal(&store.conn);
    let before = file_bytes(&dir);
    for trace in TRACES {
        assert!(
            holds(&before, trace),
            "{trace} was never stored, so the test proves nothing"
        );
    }

    assert_eq!(store.purge_older_than(30, NOW).unwrap(), 1);
    let after = file_bytes(&dir);
    for trace in TRACES {
        assert!(
            !holds(&after, trace),
            "{trace} is still in the file after the sweep"
        );
    }
    // The kept entry is still there to read.
    assert!(holds(&after, "Planning"));
    drop(store);
    for trace in TRACES {
        assert!(
            !holds(&file_bytes(&dir), trace),
            "{trace} came back on close"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_title_indexed_before_its_window_was_known_private_leaves_no_trigrams() {
    let dir = temp_dir("retention-private");
    let mut store = ActivityStore::load(&dir).unwrap();
    store.tracking_hours.enabled = false;
    let sample = |private| {
        Some(WindowSample {
            app: "Browser".to_string(),
            title: "Zqxwvj Kplmnb".to_string(),
            bundle_id: Some("browser".to_string()),
            url: Some("https://bank.example/a?token=1".to_string()),
            domain: Some("bank.example".to_string()),
            private,
            keeps_display_awake: false,
        })
    };
    // The browser answers a moment late: the title is indexed as a normal window's first.
    store.tick(sample(false), 0, NOW).unwrap();
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM segments_fts WHERE segments_fts MATCH 'zqxwvj'"
        ),
        1
    );
    store.tick(sample(true), 0, NOW + 2_000).unwrap();
    store.close_active_segment(NOW + 60_000).unwrap();
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM segments WHERE private = 1"),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM segments_fts WHERE segments_fts MATCH 'zqxwvj'"
        ),
        0
    );

    let later = NOW + redact::BACKSTOP_MS + 120_000;
    assert_eq!(
        redact::redact_private_detail(&store.conn, later).unwrap(),
        1
    );
    // The site it listed while the window still read as normal went with the flag.
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM apps WHERE kind = 'site'"),
        0
    );
    let bytes = file_bytes(&dir);
    for trace in TRACES {
        assert!(!holds(&bytes, trace), "{trace} is still in the file");
    }
    drop(store);
    let _ = std::fs::remove_dir_all(dir);
}

/// id, url, domain, private, title.
type Stored = (i64, Option<String>, Option<String>, i64, String);

/// A database as version 15 left it: addresses stored whole, and an index that keeps the
/// trigrams of a deleted title until a merge.
fn v15_database(dir: &Path) {
    drop(ActivityStore::load(dir).unwrap());
    let conn = Connection::open(dir.join(DB_FILE)).unwrap();
    conn.execute_batch(&format!(
        "PRAGMA secure_delete=ON;
         INSERT INTO segments_fts (segments_fts, rank) VALUES ('secure-delete', 0);
         INSERT INTO segments (app, title, kind, started_at, ended_at) VALUES ('Editor', 'Zqxwvj Kplmnb', 'activity', 0, 1);
         DELETE FROM segments;
         INSERT INTO segments (id, app, title, kind, started_at, ended_at, url, domain, private) VALUES
           (1, 'Browser', 'Reset password', 'activity', {OLD}, {OLD} + 1, 'https://id.example/reset?token=hunter2secret#step-2', 'id.example', 0),
           (2, 'Browser', 'Docs', 'activity', {OLD}, {OLD} + 1, 'https://docs.example/page#hunter2anchor?not=query', 'docs.example', 0),
           (3, 'Browser', 'Plain', 'activity', {OLD}, {OLD} + 1, 'https://plain.example/a/b', 'plain.example', 0),
           (4, 'Browser', 'Hidden', 'activity', {NOW}, {NOW} + 1, 'https://private.example/x?q=hunter2search', 'private.example', 1),
           (5, 'Editor', 'main.rs', 'activity', {OLD}, {OLD} + 1, NULL, NULL, 0);
         PRAGMA user_version = 15;"
    ))
    .unwrap();
}

#[test]
fn upgrading_from_v15_strips_stored_addresses_and_leaves_no_trace_in_the_file() {
    let dir = temp_dir("retention-v15");
    v15_database(&dir);
    let before = file_bytes(&dir);
    for trace in ["hunter2secret", "hunter2anchor", "hunter2search", "zqx"] {
        assert!(
            holds(&before, trace),
            "{trace} was never stored, so the test proves nothing"
        );
    }
    // `rize` does not migrate: it refuses the older database and leaves it as it was.
    let error = crate::state::AppState::open_existing(&dir).err().unwrap();
    assert_eq!(error.code, crate::protocol::code::INCOMPATIBLE);
    assert!(holds(&file_bytes(&dir), "hunter2secret"));

    let store = ActivityStore::load(&dir).unwrap();
    assert_eq!(
        count(&store, "PRAGMA user_version"),
        i64::from(crate::migrations::SCHEMA_VERSION)
    );
    let rows: Vec<Stored> = store
        .conn
        .prepare("SELECT id, url, domain, private, title FROM segments ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let row = |id, url: Option<&str>, domain: Option<&str>, private, title: &str| {
        (
            id,
            url.map(str::to_string),
            domain.map(str::to_string),
            private,
            title.to_string(),
        )
    };
    assert_eq!(
        rows,
        vec![
            row(
                1,
                Some("https://id.example/reset"),
                Some("id.example"),
                0,
                "Reset password"
            ),
            row(
                2,
                Some("https://docs.example/page"),
                Some("docs.example"),
                0,
                "Docs"
            ),
            row(
                3,
                Some("https://plain.example/a/b"),
                Some("plain.example"),
                0,
                "Plain"
            ),
            // Stripped by the migration (`migrations::strip_urls_tests`), then erased whole:
            // a private window this long closed is past the backstop when the store loads.
            row(4, None, None, 1, crate::capture::PRIVATE_TITLE),
            row(5, None, None, 0, "main.rs"),
        ]
    );
    // Search still finds what it should, and only that.
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM segments_fts WHERE segments_fts MATCH 'password'"
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM segments_fts WHERE segments_fts MATCH 'hidden'"
        ),
        0
    );
    let after = file_bytes(&dir);
    for trace in ["hunter2", "zqx", "kpl"] {
        assert!(
            !holds(&after, trace),
            "{trace} is still in the file after the upgrade"
        );
    }
    drop(store);
    assert!(!holds(&file_bytes(&dir), "hunter2"));

    let _ = std::fs::remove_dir_all(dir);
}
