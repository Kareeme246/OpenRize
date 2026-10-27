//! Merging neighbours: two entries may miss their shared border by up to
//! `MERGE_GAP_MS` and still merge, and the merged entry spans the gap.
//! Overlapping entries merge too, into the union of their spans. Entries on
//! different local days, or further apart than the tolerance, never merge.
use super::*;
use chrono::{Local, TimeZone};

const MIN: u64 = 60_000;

/// A local wall-clock time on an ordinary mid-month day, so the day-boundary
/// cases hold in any timezone.
fn at(hour: u32, minute: u32) -> u64 {
    day_at(15, hour, minute)
}

fn day_at(day: u32, hour: u32, minute: u32) -> u64 {
    Local
        .with_ymd_and_hms(2026, 6, day, hour, minute, 0)
        .earliest()
        .expect("local time")
        .timestamp_millis() as u64
}

const NOW: u64 = 1_800_000_000_000;

/// The first entry most tests start from: 10:00 to 10:20.
fn first() -> (u64, u64) {
    (at(10, 0), at(10, 20))
}

/// Two captured sessions: `first()`, and a second one from `second_start`
/// for six minutes.
fn store(second_start: u64) -> ActivityStore {
    store_of(first(), (second_start, second_start + 6 * MIN))
}

fn store_of(a: (u64, u64), b: (u64, u64)) -> ActivityStore {
    let store = ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
        .expect("schema");
    store
        .conn()
        .execute_batch(&format!(
            "INSERT INTO time_entries (id,started_at,ended_at,description,created_at,updated_at,source) VALUES
               ('a',{},{},'First',1,1,'auto'),
               ('b',{}, {},'Second',1,1,'auto');",
            a.0, a.1, b.0, b.1
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

fn assert_refused(store: &mut ActivityStore, original: &[(u64, u64)]) {
    assert!(store.merge_time_entries("a", "b", NOW).is_err());
    // A refused merge leaves both entries exactly as they were.
    let list = entries(store);
    let spans: Vec<_> = list.iter().map(|entry| (entry.1, entry.2)).collect();
    assert_eq!(spans, original, "{list:?}");
    assert_eq!(store.undo(NOW).unwrap(), None);
}

#[test]
fn abutting_entries_still_merge() {
    let mut store = store(first().1);
    let merged = store.merge_time_entries("a", "b", NOW).unwrap();
    assert_eq!(
        (merged.started_at, merged.ended_at),
        (at(10, 0), at(10, 26))
    );
    assert_eq!(entries(&store).len(), 1);
}

#[test]
fn a_gap_inside_the_tolerance_merges_and_claims_the_gap() {
    let mut store = store(first().1 + 4 * MIN);
    let merged = store.merge_time_entries("a", "b", NOW).unwrap();
    // The merged entry spans the gap, so the gap counts as tracked time.
    assert_eq!(merged.started_at, at(10, 0));
    assert_eq!(merged.ended_at, at(10, 30));
    assert_eq!(merged.ended_at - merged.started_at, 30 * MIN);
    assert_eq!(entries(&store).len(), 1);
}

#[test]
fn the_tolerance_itself_is_enough() {
    let mut store = store(first().1 + MERGE_GAP_MS);
    let merged = store.merge_time_entries("a", "b", NOW).unwrap();
    assert_eq!(
        merged.ended_at - merged.started_at,
        20 * MIN + MERGE_GAP_MS + 6 * MIN
    );
}

#[test]
fn a_gap_beyond_the_tolerance_is_refused() {
    let mut store = store(first().1 + MERGE_GAP_MS + 1);
    assert_refused(&mut store, &[first(), (at(10, 25) + 1, at(10, 31) + 1)]);
}

#[test]
fn entries_overlapping_by_seconds_merge_into_their_union() {
    let b = (first().1 - 5_000, first().1 + 6 * MIN);
    let mut store = store_of(first(), b);
    let merged = store.merge_time_entries("a", "b", NOW).unwrap();
    assert_eq!((merged.started_at, merged.ended_at), (first().0, b.1));
    assert_eq!(entries(&store).len(), 1);
}

#[test]
fn entries_overlapping_by_a_minute_merge_from_either_side() {
    let b = (first().1 - MIN, first().1 + 6 * MIN);
    for (primary, secondary) in [("a", "b"), ("b", "a")] {
        let mut store = store_of(first(), b);
        let merged = store.merge_time_entries(primary, secondary, NOW).unwrap();
        assert_eq!((merged.started_at, merged.ended_at), (first().0, b.1));
        assert_eq!(entries(&store).len(), 1);
    }
}

#[test]
fn an_entry_inside_another_is_absorbed() {
    let b = (at(10, 5), at(10, 10));
    let mut store = store_of(first(), b);
    let merged = store.merge_time_entries("a", "b", NOW).unwrap();
    assert_eq!((merged.started_at, merged.ended_at), first());
    let mut store = store_of(first(), b);
    let merged = store.merge_time_entries("b", "a", NOW).unwrap();
    assert_eq!((merged.started_at, merged.ended_at), first());
}

#[test]
fn an_overlap_does_not_widen_the_tolerance() {
    // b overlaps a by a minute and ends well past it: nothing about the
    // overlap makes a third entry further than the tolerance mergeable.
    let b = (first().1 - MIN, first().1 + 6 * MIN);
    let mut store = store_of(first(), b);
    store
        .conn()
        .execute(
            "INSERT INTO time_entries (id,started_at,ended_at,description,created_at,updated_at,source)
             VALUES ('c',?1,?2,'Third',1,1,'auto');",
            params![
                (b.1 + MERGE_GAP_MS + 1) as i64,
                (b.1 + MERGE_GAP_MS + 1 + 5 * MIN) as i64
            ],
        )
        .unwrap();
    assert!(store.merge_time_entries("b", "c", NOW).is_err());
    assert!(store.merge_time_entries("a", "c", NOW).is_err());
    assert_eq!(entries(&store).len(), 3);
}

#[test]
fn an_overlap_across_midnight_is_refused() {
    // 23:55 to 00:05 overlaps 00:03 to 00:10 on the next day, and the
    // earlier entry itself runs across midnight.
    let a = (day_at(15, 23, 55), day_at(16, 0, 5));
    let b = (day_at(16, 0, 3), day_at(16, 0, 10));
    let mut store = store_of(a, b);
    assert_refused(&mut store, &[a, b]);
}

#[test]
fn an_overlap_that_stops_at_midnight_is_one_days_work() {
    let a = (day_at(15, 23, 0), day_at(16, 0, 0));
    let b = (day_at(15, 23, 50), day_at(15, 23, 59));
    let mut store = store_of(a, b);
    let merged = store.merge_time_entries("a", "b", NOW).unwrap();
    assert_eq!((merged.started_at, merged.ended_at), a);
}

#[test]
fn a_gap_across_midnight_is_refused() {
    let a = (day_at(15, 23, 58), day_at(15, 23, 59));
    let b = (day_at(16, 0, 1), day_at(16, 0, 5));
    let mut store = store_of(a, b);
    assert_refused(&mut store, &[a, b]);
}

#[test]
fn a_gap_on_one_side_of_midnight_still_merges() {
    let a = (day_at(16, 0, 1), day_at(16, 0, 3));
    let b = (day_at(16, 0, 5), day_at(16, 0, 8));
    let mut store = store_of(a, b);
    assert!(store.merge_time_entries("a", "b", NOW).is_ok());
}

#[test]
fn undoing_a_gap_merge_puts_the_gap_back() {
    let mut store = store(first().1 + 4 * MIN);
    store.merge_time_entries("a", "b", NOW).unwrap();
    assert_eq!(store.undo(NOW).unwrap(), Some("Merge"));
    let list = entries(&store);
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!((list[0].1, list[0].2), first());
    assert_eq!(list[1].1, at(10, 24), "the gap is untracked again");
}

#[test]
fn undoing_an_overlap_merge_gives_each_entry_its_own_segments_back() {
    let b = (first().1 - 2 * MIN, first().1 + 6 * MIN);
    let mut store = store_of(first(), b);
    // Interleaved in time: a's last segment starts after b's first.
    store
        .conn()
        .execute_batch(&format!(
            "INSERT INTO segments (id,app,title,kind,started_at,ended_at,entry_id) VALUES
               (1,'App','one','activity',{},{},'a'),
               (2,'App','two','activity',{},{},'b'),
               (3,'App','three','activity',{},{},'a'),
               (4,'App','four','activity',{},{},'b');",
            at(10, 0),
            at(10, 10),
            at(10, 18),
            at(10, 19),
            at(10, 19),
            at(10, 20),
            at(10, 20),
            at(10, 26)
        ))
        .unwrap();
    let owners = |store: &ActivityStore| -> Vec<String> {
        let mut statement = store
            .conn()
            .prepare("SELECT entry_id FROM segments ORDER BY id;")
            .unwrap();
        let rows = statement.query_map([], |row| row.get(0)).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };

    store.merge_time_entries("a", "b", NOW).unwrap();
    assert_eq!(owners(&store), ["a", "a", "a", "a"]);
    assert_eq!(store.undo(NOW).unwrap(), Some("Merge"));
    assert_eq!(owners(&store), ["a", "b", "a", "b"]);
    let list = entries(&store);
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!((list[0].1, list[0].2), first());
    assert_eq!((list[1].1, list[1].2), b);
    assert_eq!(store.redo(NOW).unwrap(), Some("Merge"));
    assert_eq!(owners(&store), ["a", "a", "a", "a"]);
}
