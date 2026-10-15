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
fn manual_entry_creation_cannot_be_undone_until_unapproved() {
    let mut store = store();
    let id = entry(&mut store, 0, 30, false);
    let before = store.time_entry(&id).unwrap();
    assert_locked(store.undo(NOW));
    assert_eq!(store.time_entry(&id).unwrap(), before);
    // Another client unapproves without changing this client's history.
    unapprove_behind_its_back(&store, &id);
    assert_eq!(store.undo(NOW).unwrap(), Some("Add"));
    assert!(store.time_entry(&id).is_err());
    assert_eq!(store.redo(NOW).unwrap(), Some("Add"));
    assert_eq!(status(&store, &id), "pending");
}

#[test]
fn catalog_deletion_waits_for_all_affected_entries_to_be_unapproved() {
    for project in [false, true] {
        let mut store = store();
        store.conn.execute_batch("INSERT INTO projects (id,name,color,created_at,updated_at) VALUES ('p','Project','blue',1,1);").unwrap();
        let approved = entry(&mut store, 0, 30, false);
        let pending = entry(&mut store, 30, 60, true);
        if project {
            store
                .conn
                .execute("UPDATE time_entries SET project_id = 'p';", [])
                .unwrap();
        }
        let before = store.list_time_entries(0, NOW).unwrap();
        let category_count = store.list_categories().unwrap().len();
        let history = store.undo_stack.len();
        let result = if project {
            store.delete_project("p", NOW)
        } else {
            store.delete_category("c", NOW)
        };
        assert_eq!(result.unwrap_err(), "This deletion affects approved entries. Unapprove those entries before deleting their category or project.");
        assert_eq!(store.list_time_entries(0, NOW).unwrap(), before);
        assert_eq!(store.undo_stack.len(), history);
        assert_eq!(store.list_categories().unwrap().len(), category_count);
        assert_eq!(store.list_projects().unwrap().len(), 1);
        store.unapprove_time_entries(&[approved], NOW).unwrap();
        if project {
            store.delete_project("p", NOW).unwrap();
        } else {
            store.delete_category("c", NOW).unwrap();
        }
        let entry = store.time_entry(&pending).unwrap();
        assert!(if project {
            entry.project_id.is_none()
        } else {
            entry.category_id.is_none()
        });
        assert_eq!(store.undo(NOW).unwrap(), Some("Delete"));
    }
}

#[test]
fn catalog_undo_and_redo_cannot_change_entries_approved_since_deletion() {
    for project in [false, true] {
        let mut store = store();
        let id = entry(&mut store, 0, 30, true);
        if project {
            store.conn.execute_batch("INSERT INTO projects (id,name,color,created_at,updated_at) VALUES ('p','Project','blue',1,1); UPDATE time_entries SET project_id = 'p';").unwrap();
            store.delete_project("p", NOW).unwrap();
        } else {
            store.delete_category("c", NOW).unwrap();
        }
        approve_behind_its_back(&store, &id);
        let before = store.time_entry(&id).unwrap();
        assert_locked(store.undo(NOW));
        assert_eq!(store.time_entry(&id).unwrap(), before);
        unapprove_behind_its_back(&store, &id);
        assert_eq!(store.undo(NOW).unwrap(), Some("Delete"));
        approve_behind_its_back(&store, &id);
        let before = store.time_entry(&id).unwrap();
        assert_locked(store.redo(NOW));
        assert_eq!(store.time_entry(&id).unwrap(), before);
        unapprove_behind_its_back(&store, &id);
        assert_eq!(store.redo(NOW).unwrap(), Some("Delete"));
    }
}

fn manual(from: u64, to: u64, review: bool) -> NewTimeEntry {
    NewTimeEntry {
        started_at: from * MIN,
        ended_at: to * MIN,
        description: "Manual work".to_string(),
        category_id: Some("c".to_string()),
        project_id: None,
        billable: None,
        review,
    }
}

#[test]
fn manual_creation_refuses_every_overlap_with_approved_time_atomically() {
    let mut store = store();
    let id = entry(&mut store, 10, 30, false);
    let before = store.time_entry(&id).unwrap();
    let history = store.undo_stack.len();
    let jobs: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM classify_jobs;", [], |row| row.get(0))
        .unwrap();
    for (from, to) in [(0, 20), (20, 40), (15, 25), (0, 40), (10, 30)] {
        for review in [false, true] {
            assert_eq!(
                store
                    .create_manual_entry(manual(from, to, review), NOW)
                    .unwrap_err(),
                APPROVED_OVERLAP
            );
            assert_eq!(
                store.list_time_entries(0, NOW).unwrap(),
                vec![before.clone()]
            );
            assert_eq!(store.undo_stack.len(), history);
            assert_eq!(
                store
                    .conn
                    .query_row("SELECT COUNT(*) FROM classify_jobs;", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                jobs
            );
        }
    }
    // Sharing an edge does not overlap.
    entry(&mut store, 0, 10, false);
    entry(&mut store, 30, 40, false);
}

#[test]
fn manual_creation_allows_unapproved_and_deleted_time() {
    let mut store = store();
    let id = entry(&mut store, 0, 30, false);
    store.unapprove_time_entries(&[id], NOW).unwrap();
    let overlap = entry(&mut store, 10, 20, false);
    store
        .conn
        .execute(
            "UPDATE time_entries SET deleted_at = ?1 WHERE id = ?2;",
            params![NOW as i64, overlap],
        )
        .unwrap();
    entry(&mut store, 10, 20, false);
}

#[test]
fn redoing_manual_creation_checks_new_approved_time_and_keeps_the_history() {
    let mut store = store();
    let id = entry(&mut store, 0, 30, true);
    store.undo(NOW).unwrap();
    // An approved entry arrives from another client while Add is undone.
    store.conn.execute_batch("INSERT INTO time_entries (id,started_at,ended_at,description,status,source,created_at,updated_at) VALUES ('other',600000,1200000,'Other','approved','manual',1,1);").unwrap();
    assert_eq!(store.redo(NOW).unwrap_err(), APPROVED_OVERLAP);
    assert!(store.time_entry(&id).is_err());
    unapprove_behind_its_back(&store, "other");
    assert_eq!(store.redo(NOW).unwrap(), Some("Add"));
    assert_eq!(status(&store, &id), "pending");
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

#[test]
fn deleting_pending_time_leaves_a_catalog_shared_with_approved_time_alone() {
    let mut store = store();
    let approved = entry(&mut store, 0, 30, false);
    let pending = entry(&mut store, 30, 60, true);
    store.delete_time_entry(&pending, NOW).unwrap();
    assert!(store.time_entry(&pending).unwrap().deleted_at.is_some());
    let approved = store.time_entry(&approved).unwrap();
    assert_eq!(approved.category_id.as_deref(), Some("c"));
    assert_eq!(approved.status, "approved");
    assert_eq!(store.undo(NOW).unwrap(), Some("Delete"));
    assert!(store.time_entry(&pending).unwrap().deleted_at.is_none());
}

#[test]
fn redoing_catalog_deletion_checks_new_approved_links() {
    let mut store = store();
    let id = entry(&mut store, 0, 30, true);
    store.delete_category("c", NOW).unwrap();
    store.undo(NOW).unwrap();
    store.conn.execute_batch("INSERT INTO time_entries (id,started_at,ended_at,description,category_id,status,source,created_at,updated_at) VALUES ('other',2400000,3000000,'Other','c','approved','manual',1,1);").unwrap();
    assert_locked(store.redo(NOW));
    assert_eq!(
        store.time_entry(&id).unwrap().category_id.as_deref(),
        Some("c")
    );
    unapprove_behind_its_back(&store, "other");
    assert_eq!(store.redo(NOW).unwrap(), Some("Delete"));
}

#[test]
fn a_pending_entry_deletes_whatever_it_shares_with_approved_entries() {
    let mut store = store();
    store.conn.execute_batch("INSERT INTO projects (id,name,color,created_at,updated_at) VALUES ('p','Project','blue',1,1);
         INSERT INTO rules (id,match_kind,pattern,category_id,project_id,created_at,updated_at) VALUES ('r','app','Editor','c','p',1,1);").unwrap();
    let add =
        |store: &mut ActivityStore, from: u64, category: Option<&str>, project: Option<&str>| {
            store
                .create_manual_entry(
                    NewTimeEntry {
                        started_at: from * MIN,
                        ended_at: (from + 30) * MIN,
                        description: "Work".to_string(),
                        category_id: category.map(str::to_string),
                        project_id: project.map(str::to_string),
                        billable: None,
                        review: true,
                    },
                    NOW,
                )
                .unwrap()
                .id
        };
    let categorized = add(&mut store, 0, Some("c"), None);
    let projected = add(&mut store, 30, Some("c"), Some("p"));
    let bare = add(&mut store, 60, None, None);
    let approved = entry(&mut store, 90, 120, false);
    store
        .conn
        .execute(
            "UPDATE time_entries SET project_id = 'p' WHERE id = ?1;",
            [&approved],
        )
        .unwrap();

    // The approved entry shares the category and project, and stays locked.
    assert_locked(store.delete_time_entry(&approved, NOW));
    store
        .delete_time_entries(&[categorized, projected, bare], NOW)
        .unwrap();
    let live = store.list_time_entries(0, u64::MAX / 2).unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].id, approved);
    assert_eq!(live[0].category_id.as_deref(), Some("c"));
    assert_eq!(live[0].project_id.as_deref(), Some("p"));
    assert!(store.list_categories().unwrap().iter().any(|c| c.id == "c"));
    assert_eq!(store.list_projects().unwrap().len(), 1);
    let rules: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM rules WHERE id = 'r' AND deleted_at IS NULL;",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rules, 1);
}

#[test]
fn deleting_an_entry_removes_exactly_its_time_from_project_totals() {
    let mut store = store();
    store.conn.execute_batch("INSERT INTO projects (id,name,color,created_at,updated_at) VALUES ('p','Project','blue',1,1);").unwrap();
    let add = |store: &mut ActivityStore, from: u64, review: bool| {
        store
            .create_manual_entry(
                NewTimeEntry {
                    started_at: from * MIN,
                    ended_at: (from + 30) * MIN,
                    description: "Work".to_string(),
                    category_id: Some("c".to_string()),
                    project_id: Some("p".to_string()),
                    billable: Some(true),
                    review,
                },
                NOW,
            )
            .unwrap()
            .id
    };
    let totals = |store: &ActivityStore| {
        crate::projects::project_stats(&store.conn, 0, u64::MAX / 2, 0)
            .unwrap()
            .into_iter()
            .map(|stats| (stats.entries, stats.total_ms, stats.unbilled_ms))
            .collect::<Vec<_>>()
    };
    let kept = add(&mut store, 0, false);
    let pending = add(&mut store, 30, true);
    // A hand-added entry bills at once; a pending one waits for approval.
    assert_eq!(totals(&store), vec![(2, 60 * MIN, 30 * MIN)]);
    store.delete_time_entry(&pending, NOW).unwrap();
    assert_eq!(totals(&store), vec![(1, 30 * MIN, 30 * MIN)]);
    assert_eq!(status(&store, &kept), "approved");
    let approved = add(&mut store, 60, false);
    assert_locked(store.delete_time_entry(&approved, NOW));
    store
        .unapprove_time_entries(std::slice::from_ref(&approved), NOW)
        .unwrap();
    store.delete_time_entry(&approved, NOW).unwrap();
    assert_eq!(totals(&store), vec![(1, 30 * MIN, 30 * MIN)]);
}
