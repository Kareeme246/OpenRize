use super::ActivityStore;
use rusqlite::{params, Connection};

fn store() -> ActivityStore {
    let store = ActivityStore::from_conn(Connection::open_in_memory().unwrap()).unwrap();
    store.conn().execute_batch(
        "INSERT INTO categories (id,name,color,created_at,updated_at) VALUES ('c','Work','green',1,1);
         INSERT INTO projects (id,name,color,created_at,updated_at) VALUES ('p','Project','blue',1,1);
         INSERT INTO time_entries (id,started_at,ended_at,description,category_id,project_id,created_at,updated_at,source) VALUES
           ('a',100,200,'First','c','p',1,1,'manual'), ('b',200,300,'Shared','c','p',1,1,'manual');
         INSERT INTO segments (id,app,title,kind,started_at,ended_at,entry_id) VALUES (1,'Editor','First','activity',100,200,'a');
         INSERT INTO rules (id,match_kind,pattern,category_id,project_id,created_at,updated_at) VALUES ('r','app','Editor','c','p',1,1);
         INSERT INTO apps (id,kind,identifier,display_name,default_category_id,default_project_id,first_seen,last_seen,created_at,updated_at) VALUES ('app','native','editor','Editor','c','p',1,1,1,1);"
    ).unwrap();
    store
}

fn count(store: &ActivityStore, sql: &str) -> i64 {
    store.conn().query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn entry_cleanup_and_captured_activity_are_one_undoable_operation() {
    let mut store = store();
    store
        .delete_time_entries(&["a".into(), "a".into()], 400)
        .unwrap();
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM time_entries WHERE deleted_at IS NULL"
        ),
        1
    );
    assert_eq!(count(&store, "SELECT count(*) FROM time_entries WHERE category_id IS NOT NULL OR project_id IS NOT NULL"), 0);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM categories WHERE id='c' AND deleted_at IS NULL"
        ),
        0
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM projects WHERE id='p' AND deleted_at IS NULL"
        ),
        0
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM rules WHERE id='r' AND deleted_at IS NULL"
        ),
        0
    );
    assert_eq!(
        count(&store, "SELECT count(*) FROM segments WHERE entry_id='a'"),
        0
    );
    assert_eq!(count(&store, "SELECT count(*) FROM apps WHERE default_category_id IS NOT NULL OR default_project_id IS NOT NULL"), 0);
    assert!(store.undo_deletion(500).unwrap());
    assert_eq!(count(&store, "SELECT count(*) FROM time_entries WHERE deleted_at IS NULL AND category_id='c' AND project_id='p'"), 2);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM segments WHERE entry_id='a' AND ended_at=200"
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM categories WHERE id='c' AND archived=0 AND deleted_at IS NULL"
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM rules WHERE id='r' AND deleted_at IS NULL"
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM apps WHERE default_category_id='c' AND default_project_id='p'"
        ),
        1
    );
    assert!(!store.undo_deletion(501).unwrap());
    assert!(store.redo_deletion(600).unwrap());
    assert_eq!(
        count(&store, "SELECT count(*) FROM segments WHERE entry_id='a'"),
        0
    );
    assert!(store.undo_deletion(700).unwrap());
    assert_eq!(
        count(&store, "SELECT count(*) FROM segments WHERE entry_id='a'"),
        1
    );
}

#[test]
fn categories_and_projects_delete_independently_and_undo_in_order() {
    let mut store = store();
    store.delete_category("c", 400).unwrap();
    assert_eq!(count(&store, "SELECT count(*) FROM time_entries WHERE deleted_at IS NULL AND category_id IS NULL AND project_id='p'"), 2);
    store.delete_project("p", 500).unwrap();
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM time_entries WHERE deleted_at IS NULL AND project_id IS NULL"
        ),
        2
    );
    store.undo_deletion(600).unwrap();
    store.undo_deletion(700).unwrap();
    assert_eq!(count(&store, "SELECT count(*) FROM time_entries WHERE deleted_at IS NULL AND category_id='c' AND project_id='p'"), 2);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM rules WHERE id='r' AND deleted_at IS NULL"
        ),
        1
    );
}

#[test]
fn invoiced_shared_time_blocks_the_entire_cleanup_atomically() {
    let mut store = store();
    store.conn().execute_batch("INSERT INTO invoices (id,client_id,client_name,currency,status,created_at,updated_at) VALUES ('invoice','client','Client','USD','draft',1,1); UPDATE time_entries SET invoice_id='invoice' WHERE id='b';").unwrap();
    assert!(store.delete_time_entry("a", 400).is_err());
    assert_eq!(count(&store, "SELECT count(*) FROM time_entries WHERE deleted_at IS NULL AND category_id='c' AND project_id='p'"), 2);
    assert_eq!(
        count(&store, "SELECT count(*) FROM segments WHERE entry_id='a'"),
        1
    );
    assert!(!store.undo_deletion(500).unwrap());
}

#[test]
fn undo_preserves_description_edits_and_new_deletion_clears_redo() {
    let mut store = store();
    store.delete_category("c", 400).unwrap();
    store
        .conn()
        .execute(
            "UPDATE time_entries SET description=?1 WHERE id='b';",
            ["Edited later"],
        )
        .unwrap();
    store.undo_deletion(500).unwrap();
    let text: String = store
        .conn()
        .query_row(
            "SELECT description FROM time_entries WHERE id='b'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(text, "Edited later");
    store.delete_project("p", 600).unwrap();
    assert!(!store.redo_deletion(700).unwrap());
}

#[test]
fn conflicting_link_edit_rolls_back_undo_without_losing_history() {
    let mut store = store();
    store.delete_category("c", 400).unwrap();
    store
        .conn()
        .execute(
            "UPDATE time_entries SET category_id=?1 WHERE id='b';",
            ["another"],
        )
        .unwrap();
    assert!(store.undo_deletion(500).is_err());
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM time_entries WHERE id='a' AND category_id IS NULL"
        ),
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM categories WHERE id='c' AND deleted_at IS NULL"
        ),
        0
    );
    store
        .conn()
        .execute("UPDATE time_entries SET category_id=NULL WHERE id='b';", [])
        .unwrap();
    assert!(store.undo_deletion(600).unwrap());
}

#[test]
fn redo_refuses_new_or_edited_activity_without_losing_history() {
    for change in [
        "INSERT INTO segments (id,app,title,kind,started_at,ended_at,entry_id) VALUES (2,'Editor','Later','activity',500,600,'a');",
        "UPDATE segments SET title='Edited later' WHERE id=1;",
    ] {
        let mut store = store();
        store.delete_time_entry("a", 400).unwrap();
        store.undo_deletion(500).unwrap();
        store.conn().execute_batch(change).unwrap();
        assert!(store.redo_deletion(600).is_err());
        assert_eq!(count(&store, "SELECT count(*) FROM time_entries WHERE deleted_at IS NULL AND category_id='c' AND project_id='p'"), 2);
        store.conn().execute_batch("DELETE FROM segments WHERE id=2; UPDATE segments SET title='First' WHERE id=1;").unwrap();
        assert!(store.redo_deletion(700).unwrap());
    }
}

#[test]
fn restored_live_activity_is_closed_and_does_not_recreate_deleted_time() {
    let mut store = store();
    store
        .conn()
        .execute("UPDATE segments SET ended_at=NULL WHERE id=?1", params![1])
        .unwrap();
    store.delete_time_entry("a", 400).unwrap();
    store.rebuild_time_entries_in_range(0, 1000, 450).unwrap();
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM time_entries WHERE deleted_at IS NULL"
        ),
        1
    );
    store.undo_deletion(500).unwrap();
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM segments WHERE id=1 AND ended_at=400"
        ),
        1
    );
}
