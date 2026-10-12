//! Splitting, merging and undoing entries while capture keeps running: the
//! same tick and rebuild calls the app makes, end to end.
use super::*;

const MIN: u64 = 60_000;
const TICK: u64 = 5_000;

fn store() -> ActivityStore {
    let mut store = ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
        .expect("schema");
    store.tracking_hours.enabled = false;
    store
}

fn sample(title: &str) -> Option<WindowSample> {
    Some(WindowSample {
        app: "Code".to_string(),
        title: title.to_string(),
        bundle_id: None,
        url: None,
        domain: None,
        keeps_display_awake: false,
    })
}

/// Works from `from` to `to`, rebuilding every minute like the AI worker.
/// `switch_every` changes the window title (a new segment) that often;
/// `None` keeps one segment open the whole time.
fn work(store: &mut ActivityStore, from: u64, to: u64, switch_every: Option<u64>) {
    let mut now = from;
    while now <= to {
        let title = switch_every.map_or(0, |every| now / every);
        store
            .tick(sample(&format!("file {title}")), 0, now)
            .unwrap();
        if now.is_multiple_of(MIN) {
            store.rebuild_range(0, now, now).unwrap();
        }
        now += TICK;
    }
}

/// Live entries in start order: (id, start, end, status).
fn entries(store: &ActivityStore) -> Vec<(String, u64, u64, String)> {
    store
        .list_time_entries(0, u64::MAX / 2)
        .unwrap()
        .into_iter()
        .map(|e| (e.id, e.started_at, e.ended_at, e.status))
        .collect()
}

/// Every millisecond from `start` to `end` belongs to exactly one entry.
fn assert_covered(store: &ActivityStore, start: u64, end: u64) {
    let list = entries(store);
    assert!(!list.is_empty(), "no entries at all");
    assert_eq!(list[0].1, start, "coverage starts late: {list:?}");
    for pair in list.windows(2) {
        assert_eq!(pair[0].2, pair[1].1, "gap or overlap: {list:?}");
    }
    assert!(
        list.last().unwrap().2 >= end,
        "coverage ends early: {list:?}"
    );
}

#[test]
fn splitting_the_live_entry_keeps_recording_into_the_second_half() {
    for switch_every in [Some(2 * MIN), None] {
        let mut store = store();
        let start = 10 * MIN;
        work(&mut store, start, start + 60 * MIN, switch_every);
        let live = entries(&store);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].3, "building");

        let at = start + 30 * MIN + 7_000;
        let now = start + 60 * MIN + 2_000;
        let (first, second) = store.split_time_entry(&live[0].0, at, now).unwrap();
        assert_eq!(first.ended_at, at);
        assert_eq!(second.started_at, at);
        assert_eq!(first.status, "pending");
        assert_eq!(second.status, "building");

        // Capture and the worker's rebuilds carry on.
        work(
            &mut store,
            start + 60 * MIN + TICK,
            start + 75 * MIN,
            switch_every,
        );
        let list = entries(&store);
        assert_eq!(list.len(), 2, "{switch_every:?}: {list:?}");
        assert_eq!(list[0].0, first.id);
        assert_eq!((list[0].1, list[0].2), (start, at), "{list:?}");
        assert_eq!(list[1].0, second.id, "the live half kept its id");
        assert_eq!(list[1].3, "building");
        assert_covered(&store, start, start + 75 * MIN);
    }
}

/// One live session from 10 min, an hour long, title changing every 2 min.
fn live_hour() -> (ActivityStore, String) {
    let mut store = store();
    work(&mut store, 10 * MIN, 70 * MIN, Some(2 * MIN));
    let id = entries(&store)[0].0.clone();
    (store, id)
}

#[test]
fn undoing_a_live_split_rejoins_everything_recorded_since() {
    let (mut store, id) = live_hour();
    let (_, second) = store.split_time_entry(&id, 40 * MIN, 70 * MIN).unwrap();
    work(&mut store, 70 * MIN + TICK, 80 * MIN, Some(2 * MIN));

    assert_eq!(store.undo(80 * MIN).unwrap(), Some("Split"));
    work(&mut store, 80 * MIN + TICK, 85 * MIN, Some(2 * MIN));
    let list = entries(&store);
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].0, id);
    assert_eq!(list[0].3, "building");
    assert_covered(&store, 10 * MIN, 85 * MIN);

    assert_eq!(store.redo(85 * MIN).unwrap(), Some("Split"));
    work(&mut store, 85 * MIN + TICK, 90 * MIN, Some(2 * MIN));
    let list = entries(&store);
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!((list[0].0.as_str(), list[0].2), (id.as_str(), 40 * MIN));
    assert_eq!(
        list[1].0, second.id,
        "redo brings the same second half back"
    );
    assert_eq!(list[1].3, "building");
    assert_covered(&store, 10 * MIN, 90 * MIN);
}

#[test]
fn the_live_entry_splits_anywhere_up_to_now() {
    // The stored end trails the clock until the next rebuild.
    let (mut store, id) = live_hour();
    work(
        &mut store,
        70 * MIN + TICK,
        70 * MIN + 50_000,
        Some(2 * MIN),
    );
    let stored_end = entries(&store)[0].2;
    assert!(stored_end < 70 * MIN + 50_000);
    let (first, second) = store
        .split_time_entry(&id, stored_end + 20_000, 70 * MIN + 50_000)
        .unwrap();
    assert_eq!(first.ended_at, stored_end + 20_000);
    assert_eq!(second.status, "building");
    work(&mut store, 70 * MIN + 55_000, 75 * MIN, Some(2 * MIN));
    assert_eq!(entries(&store).len(), 2);
    assert_covered(&store, 10 * MIN, 75 * MIN);
}

#[test]
fn approving_the_live_entry_closes_it_and_recording_carries_on() {
    let (mut store, id) = live_hour();
    store
        .conn
        .execute(
            "UPDATE time_entries SET category_id = 'c' WHERE id = ?1;",
            [&id],
        )
        .unwrap();
    store
        .approve_time_entries(std::slice::from_ref(&id), "user", 70 * MIN + 2_000)
        .unwrap();
    work(&mut store, 70 * MIN + TICK, 80 * MIN, Some(2 * MIN));
    let list = entries(&store);
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!(
        (list[0].0.as_str(), list[0].3.as_str()),
        (id.as_str(), "approved")
    );
    assert_eq!(list[0].2, 70 * MIN + 2_000);
    assert_eq!(list[1].3, "building");
    assert_covered(&store, 10 * MIN, 80 * MIN);

    // Undo reopens it, and the session it spawned folds back in.
    assert_eq!(store.undo(80 * MIN).unwrap(), Some("Accept"));
    work(&mut store, 80 * MIN + TICK, 82 * MIN, Some(2 * MIN));
    let list = entries(&store);
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(
        (list[0].0.as_str(), list[0].3.as_str()),
        (id.as_str(), "building")
    );
    assert_covered(&store, 10 * MIN, 82 * MIN);
}

#[test]
fn merging_a_live_split_back_keeps_it_recording_and_undo_splits_it_again() {
    let (mut store, id) = live_hour();
    let (_, second) = store.split_time_entry(&id, 40 * MIN, 70 * MIN).unwrap();
    work(&mut store, 70 * MIN + TICK, 75 * MIN, Some(2 * MIN));

    store.merge_time_entries(&id, &second.id, 75 * MIN).unwrap();
    work(&mut store, 75 * MIN + TICK, 80 * MIN, Some(2 * MIN));
    let list = entries(&store);
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(
        (list[0].0.as_str(), list[0].3.as_str()),
        (id.as_str(), "building")
    );
    assert_covered(&store, 10 * MIN, 80 * MIN);

    assert_eq!(store.undo(80 * MIN).unwrap(), Some("Merge"));
    work(&mut store, 80 * MIN + TICK, 85 * MIN, Some(2 * MIN));
    let list = entries(&store);
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!(list[1].0, second.id);
    assert_eq!(list[1].3, "building");
    assert_covered(&store, 10 * MIN, 85 * MIN);

    assert_eq!(store.redo(85 * MIN).unwrap(), Some("Merge"));
    work(&mut store, 85 * MIN + TICK, 90 * MIN, Some(2 * MIN));
    assert_eq!(entries(&store).len(), 1);
    assert_covered(&store, 10 * MIN, 90 * MIN);
}

#[test]
fn undoing_an_entry_added_over_tracked_time_leaves_no_hole() {
    let (mut store, _) = live_hour();
    let added = store
        .create_manual_entry(
            crate::models::NewTimeEntry {
                started_at: 0,
                ended_at: 10 * MIN,
                description: "Meeting".to_string(),
                category_id: None,
                project_id: None,
                billable: None,
                review: false,
            },
            70 * MIN,
        )
        .unwrap();
    assert_eq!(store.undo(70 * MIN).unwrap(), Some("Add"));
    assert!(
        store.time_entry(&added.id).is_err(),
        "removed, not tombstoned"
    );
    work(&mut store, 70 * MIN + TICK, 75 * MIN, Some(2 * MIN));
    assert_covered(&store, 10 * MIN, 75 * MIN);
    assert_eq!(store.redo(75 * MIN).unwrap(), Some("Add"));
    assert_eq!(store.time_entry(&added.id).unwrap().description, "Meeting");
}

/// A finished, classified session from 10 to 40 min.
fn classified_session() -> (ActivityStore, String) {
    let mut store = store();
    work(&mut store, 10 * MIN, 40 * MIN, Some(2 * MIN));
    store.rebuild_range(0, 50 * MIN, 50 * MIN).unwrap();
    let id = entries(&store)[0].0.clone();
    store
        .conn
        .execute_batch(&format!(
            "INSERT INTO categories (id,name,color,created_at,updated_at) VALUES ('c','Work','green',1,1);
             INSERT INTO suggestions (id,entry_id,field,value_id,confidence,created_at,updated_at)
               VALUES ('s','{id}','category','c',0.9,1,1);
             UPDATE time_entries SET status = 'pending', category_id = 'c' WHERE id = '{id}';
             DELETE FROM classify_jobs;"
        ))
        .unwrap();
    (store, id)
}

#[test]
fn splitting_never_reclassifies_and_names_each_half_after_its_own_activity() {
    let (mut store, id) = classified_session();
    let (first, second) = store.split_time_entry(&id, 25 * MIN, 50 * MIN).unwrap();
    assert_eq!(second.category_id.as_deref(), Some("c"));
    assert_eq!(second.status, "pending");
    assert_ne!(first.description, second.description);
    assert!(first.description.starts_with("Code: file"));
    store.rebuild_range(0, 51 * MIN, 51 * MIN).unwrap();
    let queued: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM classify_jobs;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(queued, 0, "both halves keep the suggestion they had");
    let inherited = crate::ai::store::latest_suggestions(&store.conn, &second.id).unwrap();
    assert_eq!(inherited.len(), 1);
}

#[test]
fn invoiced_time_cannot_be_split_or_merged() {
    let (mut store, id) = classified_session();
    store
        .conn
        .execute(
            "UPDATE time_entries SET invoice_id = 'i' WHERE id = ?1;",
            [&id],
        )
        .unwrap();
    assert!(store.split_time_entry(&id, 25 * MIN, 50 * MIN).is_err());
    assert_eq!(store.undo(50 * MIN).unwrap(), None);
}

#[test]
fn accept_and_edits_undo_and_redo_with_their_ai_feedback() {
    let (mut store, id) = classified_session();
    store
        .approve_time_entries(std::slice::from_ref(&id), "user", 50 * MIN)
        .unwrap();
    let outcome = |store: &ActivityStore| -> Option<String> {
        store
            .conn
            .query_row(
                "SELECT outcome FROM suggestions WHERE id = 's';",
                [],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert_eq!(outcome(&store).as_deref(), Some("accepted"));

    assert_eq!(store.undo(51 * MIN).unwrap(), Some("Accept"));
    assert_eq!(store.time_entry(&id).unwrap().status, "pending");
    assert_eq!(
        outcome(&store),
        None,
        "the learning signal is withdrawn too"
    );
    assert_eq!(store.redo(52 * MIN).unwrap(), Some("Accept"));
    assert_eq!(store.time_entry(&id).unwrap().status, "approved");
    assert_eq!(outcome(&store).as_deref(), Some("accepted"));

    store
        .unapprove_time_entries(std::slice::from_ref(&id), 53 * MIN)
        .unwrap();
    store
        .update_time_entry(
            &id,
            crate::models::UpdateTimeEntry {
                description: Some("Writing".to_string()),
                billable: Some(true),
                ..Default::default()
            },
            54 * MIN,
        )
        .unwrap();
    assert_eq!(store.undo(55 * MIN).unwrap(), Some("Edit"));
    let entry = store.time_entry(&id).unwrap();
    assert!(!entry.billable);
    assert_ne!(entry.description, "Writing");
    assert_eq!(store.undo(55 * MIN).unwrap(), Some("Unapprove"));
    assert_eq!(store.time_entry(&id).unwrap().status, "approved");
}

#[test]
fn a_bulk_edit_is_one_undo_step() {
    let mut store = store();
    work(&mut store, 0, 10 * MIN, None);
    store.tick_away(10 * MIN + TICK).unwrap();
    work(&mut store, 20 * MIN, 30 * MIN, None);
    store.rebuild_range(0, 40 * MIN, 40 * MIN).unwrap();
    let ids: Vec<String> = entries(&store).into_iter().map(|e| e.0).collect();
    assert_eq!(ids.len(), 2);
    store
        .update_time_entries(
            &ids,
            crate::models::UpdateTimeEntry {
                billable: Some(true),
                ..Default::default()
            },
            40 * MIN,
        )
        .unwrap();
    assert_eq!(store.undo(41 * MIN).unwrap(), Some("Edit"));
    assert!(store
        .time_entries(&ids)
        .unwrap()
        .iter()
        .all(|e| !e.billable));
    assert_eq!(store.undo(41 * MIN).unwrap(), None);
}

#[test]
fn an_undo_that_would_overwrite_a_later_change_is_refused_and_kept() {
    let (mut store, id) = classified_session();
    store.split_time_entry(&id, 25 * MIN, 50 * MIN).unwrap();
    // Changed outside the undo history (another client, say).
    store
        .conn
        .execute(
            "UPDATE time_entries SET ended_at = ?1 WHERE id = ?2;",
            params![(20 * MIN) as i64, id],
        )
        .unwrap();
    assert!(store.undo(51 * MIN).is_err());
    assert_eq!(entries(&store).len(), 2, "nothing half-applied");
    store
        .conn
        .execute(
            "UPDATE time_entries SET ended_at = ?1 WHERE id = ?2;",
            params![(25 * MIN) as i64, id],
        )
        .unwrap();
    assert_eq!(
        store.undo(52 * MIN).unwrap(),
        Some("Split"),
        "still on the stack"
    );
    assert_eq!(entries(&store).len(), 1);
}

#[test]
fn deleting_the_live_entry_and_undoing_it_keeps_capture_honest() {
    let (mut store, id) = live_hour();
    store.delete_time_entry(&id, 70 * MIN + 1_000).unwrap();
    work(&mut store, 70 * MIN + TICK, 80 * MIN, Some(2 * MIN));
    let list = entries(&store);
    assert_eq!(
        list.len(),
        1,
        "capture carried on in a new session: {list:?}"
    );
    assert!(list[0].1 >= 70 * MIN);
    assert_eq!(store.undo(80 * MIN).unwrap(), Some("Delete"));
    assert_eq!(entries(&store)[0].0, id);
}
