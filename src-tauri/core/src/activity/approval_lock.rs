//! Approved entries are frozen: every change except unapprove is refused with
//! one message, by the store itself, so the app and the `rize` CLI agree.
use super::*;
use crate::models::{NewTimeEntry, UpdateTimeEntry};

const MIN: u64 = 60_000;
const NOW: u64 = 500 * MIN;

fn store() -> ActivityStore {
    let mut store = ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
        .expect("schema");
    store.tracking_hours.enabled = false;
    store
        .conn
        .execute_batch(
            "INSERT INTO categories (id,name,color,created_at,updated_at) VALUES ('c','Work','green',1,1);
             INSERT INTO categories (id,name,color,created_at,updated_at) VALUES ('d','Other','red',1,1);",
        )
        .unwrap();
    store
}

/// A hand-made entry from `from` to `to` minutes. Labeled entries are
/// approved on creation; `review` leaves them pending.
fn entry(store: &mut ActivityStore, from: u64, to: u64, review: bool) -> String {
    store
        .create_manual_entry(
            NewTimeEntry {
                started_at: from * MIN,
                ended_at: to * MIN,
                description: "Work".to_string(),
                category_id: Some("c".to_string()),
                project_id: None,
                billable: None,
                review,
            },
            NOW,
        )
        .unwrap()
        .id
}

fn status(store: &ActivityStore, id: &str) -> String {
    store.time_entry(id).unwrap().status
}

fn assert_locked<T: std::fmt::Debug>(result: Result<T, String>) {
    assert_eq!(result.unwrap_err(), LOCKED);
}

fn patch(update: impl FnOnce(&mut UpdateTimeEntry)) -> UpdateTimeEntry {
    let mut patch = UpdateTimeEntry::default();
    update(&mut patch);
    patch
}

#[test]
fn every_field_change_on_an_approved_entry_is_refused() {
    let mut store = store();
    let id = entry(&mut store, 0, 60, false);
    assert_eq!(status(&store, &id), "approved");
    let before = store.time_entry(&id).unwrap();
    let patches = [
        patch(|p| p.description = Some("Other".into())),
        patch(|p| p.category_id = Some("d".into())),
        patch(|p| p.category_id = Some(String::new())),
        patch(|p| p.project_id = Some(String::new())),
        patch(|p| p.started_at = Some(10 * MIN)),
        patch(|p| p.ended_at = Some(90 * MIN)),
        patch(|p| p.billable = Some(true)),
        patch(|p| p.status = Some("pending".into())),
    ];
    for patch in patches {
        assert_locked(store.update_time_entry(&id, patch, NOW));
    }
    assert_eq!(store.time_entry(&id).unwrap(), before);
}

#[test]
fn a_bulk_edit_touching_an_approved_entry_writes_nothing() {
    let mut store = store();
    let approved = entry(&mut store, 0, 30, false);
    let pending = entry(&mut store, 30, 60, true);
    let ids = vec![pending.clone(), approved.clone()];
    assert_locked(store.update_time_entries(&ids, patch(|p| p.billable = Some(true)), NOW));
    assert!(!store.time_entry(&pending).unwrap().billable);
    assert!(!store.time_entry(&approved).unwrap().billable);
}

#[test]
fn approved_entries_cannot_be_split_merged_deleted_or_rejected() {
    let mut store = store();
    let first = entry(&mut store, 0, 30, false);
    let second = entry(&mut store, 30, 60, false);
    let pending = entry(&mut store, 60, 90, true);

    assert_locked(store.split_time_entry(&first, 10 * MIN, NOW));
    assert_locked(store.merge_time_entries(&first, &second, NOW));
    assert_locked(store.merge_time_entries(&second, &first, NOW));
    // Either side being approved is enough.
    assert_locked(store.merge_time_entries(&pending, &second, NOW));
    assert_locked(store.merge_time_entries(&second, &pending, NOW));
    assert_locked(store.delete_time_entry(&first, NOW));
    assert_locked(store.delete_time_entries(&[pending.clone(), first.clone()], NOW));
    assert_locked(store.reject_time_entry(&first, NOW));
    assert_locked(store.queue_classification(&first, NOW));

    assert_eq!(store.list_time_entries(0, u64::MAX / 2).unwrap().len(), 3);
    assert_eq!(
        status(&store, &pending),
        "pending",
        "the bulk delete wrote nothing"
    );
    assert!(store.time_entry(&pending).unwrap().deleted_at.is_none());
}

#[test]
fn unapproving_is_the_way_out() {
    let mut store = store();
    let first = entry(&mut store, 0, 30, false);
    let second = entry(&mut store, 30, 60, false);
    store
        .unapprove_time_entries(&[first.clone(), second.clone()], NOW)
        .unwrap();
    store
        .update_time_entry(&first, patch(|p| p.billable = Some(true)), NOW)
        .unwrap();
    store.merge_time_entries(&first, &second, NOW).unwrap();
    let (a, _) = store.split_time_entry(&first, 20 * MIN, NOW).unwrap();
    assert_eq!(a.status, "pending");
    store.delete_time_entry(&first, NOW).unwrap();
}

#[test]
fn approving_stays_allowed_and_is_idempotent() {
    let mut store = store();
    let id = entry(&mut store, 0, 30, true);
    store
        .approve_time_entries(std::slice::from_ref(&id), "user", NOW)
        .unwrap();
    let approved = store.time_entry(&id).unwrap();
    assert_eq!(approved.status, "approved");
    store
        .approve_time_entries(std::slice::from_ref(&id), "user", NOW + MIN)
        .unwrap();
    assert_eq!(
        store.time_entry(&id).unwrap(),
        approved,
        "approving twice changes nothing"
    );
    store
        .unapprove_time_entries(std::slice::from_ref(&id), NOW)
        .unwrap();
    assert_eq!(status(&store, &id), "pending");
}

#[test]
fn manual_entries_are_created_approved_and_frozen_after() {
    let mut store = store();
    let id = entry(&mut store, 0, 30, false);
    assert_eq!(status(&store, &id), "approved");
    assert_locked(store.update_time_entry(&id, patch(|p| p.billable = Some(true)), NOW));
    // The creation itself can still be taken back.
    assert_eq!(store.undo(NOW).unwrap(), Some("Add"));
    assert!(store.time_entry(&id).is_err());
    assert_eq!(store.redo(NOW).unwrap(), Some("Add"));
    assert_eq!(status(&store, &id), "approved");
}

#[test]
fn an_approved_entry_a_catalog_delete_unlinks_stays_allowed() {
    let mut store = store();
    let id = entry(&mut store, 0, 30, false);
    store.delete_category("c", NOW).unwrap();
    let entry = store.time_entry(&id).unwrap();
    assert_eq!(entry.status, "approved");
    assert_eq!(entry.category_id, None);
    assert_eq!(store.undo(NOW).unwrap(), Some("Delete"));
    assert_eq!(
        store.time_entry(&id).unwrap().category_id.as_deref(),
        Some("c")
    );
}

/// What the AI worker or another client does behind the history's back.
fn approve_behind_its_back(store: &ActivityStore, id: &str) {
    store
        .conn
        .execute(
            "UPDATE time_entries SET status = 'approved', approved_by = 'auto' WHERE id = ?1;",
            [id],
        )
        .unwrap();
}

fn unapprove_behind_its_back(store: &ActivityStore, id: &str) {
    store
        .conn
        .execute(
            "UPDATE time_entries SET status = 'pending', approved_by = NULL WHERE id = ?1;",
            [id],
        )
        .unwrap();
}

#[test]
fn undoing_an_accept_is_an_unapprove_and_stays_allowed() {
    let mut store = store();
    let id = entry(&mut store, 0, 30, true);
    store
        .approve_time_entries(std::slice::from_ref(&id), "user", NOW)
        .unwrap();
    assert_eq!(store.undo(NOW).unwrap(), Some("Accept"));
    assert_eq!(status(&store, &id), "pending");
    assert_eq!(store.redo(NOW).unwrap(), Some("Accept"));
    assert_eq!(status(&store, &id), "approved");
    // Unapprove and its undo (back to approved) work too.
    store
        .unapprove_time_entries(std::slice::from_ref(&id), NOW)
        .unwrap();
    assert_eq!(store.undo(NOW).unwrap(), Some("Unapprove"));
    assert_eq!(status(&store, &id), "approved");
}

#[test]
fn undo_and_redo_refuse_to_change_an_approved_entry_and_stay_on_the_stack() {
    // Edit
    let mut store = store();
    let id = entry(&mut store, 0, 30, true);
    store
        .update_time_entry(&id, patch(|p| p.billable = Some(true)), NOW)
        .unwrap();
    approve_behind_its_back(&store, &id);
    assert_locked(store.undo(NOW));
    assert!(
        store.time_entry(&id).unwrap().billable,
        "nothing was written"
    );
    unapprove_behind_its_back(&store, &id);
    assert_eq!(store.undo(NOW).unwrap(), Some("Edit"), "still on the stack");
    approve_behind_its_back(&store, &id);
    assert_locked(store.redo(NOW));
    unapprove_behind_its_back(&store, &id);
    assert_eq!(store.redo(NOW).unwrap(), Some("Edit"));

    // Split: undo and redo
    let mut store = self::store();
    let id = entry(&mut store, 0, 60, true);
    let (_, second) = store.split_time_entry(&id, 20 * MIN, NOW).unwrap();
    approve_behind_its_back(&store, &second.id);
    assert_locked(store.undo(NOW));
    unapprove_behind_its_back(&store, &second.id);
    assert_eq!(store.undo(NOW).unwrap(), Some("Split"));
    approve_behind_its_back(&store, &id);
    assert_locked(store.redo(NOW));
    assert_eq!(store.list_time_entries(0, u64::MAX / 2).unwrap().len(), 1);
    unapprove_behind_its_back(&store, &id);
    assert_eq!(store.redo(NOW).unwrap(), Some("Split"));

    // Merge: undo and redo
    let mut store = self::store();
    let a = entry(&mut store, 0, 30, true);
    let b = entry(&mut store, 30, 60, true);
    store.merge_time_entries(&a, &b, NOW).unwrap();
    approve_behind_its_back(&store, &a);
    assert_locked(store.undo(NOW));
    unapprove_behind_its_back(&store, &a);
    assert_eq!(store.undo(NOW).unwrap(), Some("Merge"));
    approve_behind_its_back(&store, &b);
    assert_locked(store.redo(NOW));
    unapprove_behind_its_back(&store, &b);
    assert_eq!(store.redo(NOW).unwrap(), Some("Merge"));

    // Delete: undo and redo
    let mut store = self::store();
    let id = entry(&mut store, 0, 30, true);
    store.delete_time_entry(&id, NOW).unwrap();
    approve_behind_its_back(&store, &id);
    assert_locked(store.undo(NOW));
    unapprove_behind_its_back(&store, &id);
    assert_eq!(store.undo(NOW).unwrap(), Some("Delete"));
    approve_behind_its_back(&store, &id);
    assert_locked(store.redo(NOW));
    unapprove_behind_its_back(&store, &id);
    assert_eq!(store.redo(NOW).unwrap(), Some("Delete"));
}

#[test]
fn tracking_never_extends_or_rebuilds_an_approved_entry() {
    let mut store = store();
    let sample = |title: &str| {
        Some(WindowSample {
            app: "Code".to_string(),
            title: title.to_string(),
            bundle_id: None,
            url: None,
            domain: None,
            keeps_display_awake: false,
        })
    };
    for now in (10 * MIN..=40 * MIN).step_by(5_000) {
        store.tick(sample("a"), 0, now).unwrap();
        if now % MIN == 0 {
            store.rebuild_range(0, now, now).unwrap();
        }
    }
    let live = store.list_time_entries(0, u64::MAX / 2).unwrap();
    let id = live[0].id.clone();
    store
        .conn
        .execute(
            "UPDATE time_entries SET category_id = 'c' WHERE id = ?1;",
            [&id],
        )
        .unwrap();
    store
        .approve_time_entries(std::slice::from_ref(&id), "user", 40 * MIN + 2_000)
        .unwrap();
    let approved = store.time_entry(&id).unwrap();
    for now in (40 * MIN + 5_000..=60 * MIN).step_by(5_000) {
        store.tick(sample("b"), 0, now).unwrap();
        if now % MIN == 0 {
            store.rebuild_range(0, now, now).unwrap();
        }
    }
    let after = store.time_entry(&id).unwrap();
    assert_eq!(
        (after.started_at, after.ended_at),
        (approved.started_at, approved.ended_at)
    );
    assert_eq!(after.description, approved.description);
    assert_eq!(after.status, "approved");
    assert_eq!(store.list_time_entries(0, u64::MAX / 2).unwrap().len(), 2);
}
