//! A toast's Undo names its change by sequence number, so it can only ever
//! take back that change, never one made after it.
use super::history::SUPERSEDED;
use super::*;
use crate::models::NewTimeEntry;

const MIN: u64 = 60_000;
const NOW: u64 = 500 * MIN;

fn store() -> ActivityStore {
    let mut store = ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
        .expect("schema");
    store.tracking_hours.enabled = false;
    store
}

/// A pending hand-made entry from `from` to `to` minutes.
fn entry(store: &mut ActivityStore, from: u64, to: u64) -> String {
    store
        .create_manual_entry(
            NewTimeEntry {
                started_at: from * MIN,
                ended_at: to * MIN,
                description: "Work".to_string(),
                category_id: None,
                project_id: None,
                billable: None,
                review: true,
            },
            NOW,
        )
        .unwrap()
        .id
}

#[test]
fn undo_change_takes_back_only_the_latest_change_it_names() {
    let mut store = store();
    assert_eq!(store.undo_head(), None);
    let first = entry(&mut store, 0, 30);
    let (first_seq, label) = store.undo_head().unwrap();
    assert_eq!(label, "Add");
    let second = entry(&mut store, 30, 60);
    let (second_seq, _) = store.undo_head().unwrap();
    assert_ne!(first_seq, second_seq);

    // The first change is no longer the latest: nothing moves.
    assert_eq!(store.undo_change(first_seq, NOW).unwrap_err(), SUPERSEDED);
    assert!(store.time_entry(&first).is_ok());
    assert!(store.time_entry(&second).is_ok());

    assert_eq!(store.undo_change(second_seq, NOW).unwrap(), "Add");
    assert!(store.time_entry(&second).is_err());
    assert_eq!(store.undo_head(), Some((first_seq, "Add")));

    // A redone change keeps its number.
    assert_eq!(store.redo(NOW).unwrap(), Some("Add"));
    assert_eq!(store.undo_head(), Some((second_seq, "Add")));
}

#[test]
fn a_change_that_records_nothing_leaves_the_head_alone() {
    let mut store = store();
    let id = entry(&mut store, 0, 30);
    let head = store.undo_head();
    // Unapproving a pending entry changes nothing, so nothing is recorded.
    store.unapprove_time_entries(&[id], NOW).unwrap();
    assert_eq!(store.undo_head(), head);
}
