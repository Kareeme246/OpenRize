//! Merging neighbours: two entries may miss their shared border by up to
//! `MERGE_GAP_MS` and still merge, and the merged entry spans the gap. Entries
//! that overlap or sit further apart never merge.
use super::*;

const MIN: u64 = 60_000;
/// The first entry every test starts from: 10:00 to 10:20.
const FIRST: (u64, u64) = (10 * MIN, 20 * MIN);

/// Two captured sessions: `FIRST`, and a second one from `second_start` on.
fn store(second_start: u64) -> ActivityStore {
    let second_end = second_start + 6 * MIN;
    let store = ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
        .expect("schema");
    store
        .conn()
        .execute_batch(&format!(
            "INSERT INTO time_entries (id,started_at,ended_at,description,created_at,updated_at,source) VALUES
               ('a',{},{},'First',1,1,'auto'),
               ('b',{}, {},'Second',1,1,'auto');",
            FIRST.0, FIRST.1, second_start, second_end
        ))
        .expect("entries");
    store
}

/// Live entries in start order: (id, start, end).
fn entries(store: &ActivityStore) -> Vec<(String, u64, u64)> {
    store
        .list_time_entries(0, u64::MAX / 2)
        .unwrap()
        .into_iter()
        .map(|entry| (entry.id, entry.started_at, entry.ended_at))
        .collect()
}

#[test]
fn abutting_entries_still_merge() {
    let mut store = store(FIRST.1);
    let merged = store.merge_time_entries("a", "b", 40 * MIN).unwrap();
    assert_eq!((merged.started_at, merged.ended_at), (FIRST.0, 26 * MIN));
    assert_eq!(entries(&store).len(), 1);
}

#[test]
fn a_gap_inside_the_tolerance_merges_and_claims_the_gap() {
    let mut store = store(FIRST.1 + 4 * MIN);
    let merged = store.merge_time_entries("a", "b", 40 * MIN).unwrap();
    // The merged entry spans the gap, so the gap counts as tracked time.
    assert_eq!(merged.started_at, FIRST.0);
    assert_eq!(merged.ended_at, 20 * MIN + 4 * MIN + 6 * MIN);
    assert_eq!(merged.ended_at - merged.started_at, 20 * MIN);
    assert_eq!(entries(&store).len(), 1);
}

#[test]
fn the_tolerance_itself_is_enough() {
    let mut store = store(FIRST.1 + MERGE_GAP_MS);
    let merged = store.merge_time_entries("a", "b", 40 * MIN).unwrap();
    assert_eq!(
        merged.ended_at - merged.started_at,
        10 * MIN + MERGE_GAP_MS + 6 * MIN
    );
}

#[test]
fn a_gap_beyond_the_tolerance_is_refused() {
    let mut store = store(FIRST.1 + MERGE_GAP_MS + 1);
    assert!(store.merge_time_entries("a", "b", 40 * MIN).is_err());
    // A refused merge leaves both entries exactly as they were.
    let list = entries(&store);
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!((list[0].1, list[0].2), FIRST);
    assert_eq!(store.undo(40 * MIN).unwrap(), None);
}

#[test]
fn overlapping_entries_are_refused() {
    let mut store = store(FIRST.1 - 5 * MIN);
    assert!(store.merge_time_entries("a", "b", 40 * MIN).is_err());
    assert_eq!(entries(&store).len(), 2);
    assert_eq!(store.undo(40 * MIN).unwrap(), None);
}

#[test]
fn undoing_a_gap_merge_puts_the_gap_back() {
    let mut store = store(FIRST.1 + 4 * MIN);
    store.merge_time_entries("a", "b", 40 * MIN).unwrap();
    assert_eq!(store.undo(40 * MIN).unwrap(), Some("Merge"));
    let list = entries(&store);
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!((list[0].1, list[0].2), FIRST);
    assert_eq!(list[1].1, 24 * MIN, "the gap is untracked again");
}
