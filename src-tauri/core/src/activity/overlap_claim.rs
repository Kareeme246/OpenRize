//! One owner per minute: every write that gives an entry minutes takes them
//! from whoever held them, approved time is never taken, and one Undo gives
//! it all back. The `rize` CLI reaches the same store methods (see
//! `cli/tests/offline.rs` for it end to end).
use std::collections::BTreeMap;

use super::*;
use crate::models::{NewTimeEntry, UpdateTimeEntry};

const MIN: u64 = 60_000;
const DAY: u64 = 24 * 60 * MIN;
const NOW: u64 = 900 * MIN;
const TICK: u64 = 5_000;
/// Starting points for the runs of random writes below.
const SEEDS: [u64; 12] = [
    0x2545_F491_4F6C_DD1D,
    0x9E37_79B9_7F4A_7C15,
    1,
    2,
    3,
    0xDEAD_BEEF,
    0x00C0_FFEE,
    42,
    7_777,
    123_456_789,
    0xABCD_EF01,
    99,
];

fn store() -> ActivityStore {
    let mut store = ActivityStore::from_conn(Connection::open_in_memory().expect("in-memory db"))
        .expect("schema");
    store.tracking_hours.enabled = false;
    store
        .conn
        .execute_batch(
            "INSERT INTO categories (id,name,color,created_at,updated_at) VALUES ('c','Work','green',1,1);",
        )
        .unwrap();
    store
}

fn new_entry(from: u64, to: u64, review: bool) -> NewTimeEntry {
    NewTimeEntry {
        started_at: from * MIN,
        ended_at: to * MIN,
        description: "Work".to_string(),
        category_id: Some("c".to_string()),
        project_id: None,
        billable: Some(true),
        review,
    }
}

/// A hand-made entry from `from` to `to` minutes: pending, or approved.
fn add(store: &mut ActivityStore, from: u64, to: u64, approved: bool) -> String {
    store
        .create_manual_entry(new_entry(from, to, !approved), NOW)
        .unwrap()
        .id
}

fn edit_times(
    store: &mut ActivityStore,
    id: &str,
    from: Option<u64>,
    to: Option<u64>,
    now: u64,
) -> Result<TimeEntry, String> {
    store.update_time_entry(
        id,
        UpdateTimeEntry {
            started_at: from.map(|minutes| minutes * MIN),
            ended_at: to.map(|minutes| minutes * MIN),
            ..Default::default()
        },
        now,
    )
}

fn entries(store: &ActivityStore) -> Vec<TimeEntry> {
    store.list_time_entries(0, u64::MAX / 2).unwrap()
}

/// Live entries as (id, start, end) in minutes, in start order.
fn spans(store: &ActivityStore) -> Vec<(String, u64, u64)> {
    entries(store)
        .into_iter()
        .map(|e| (e.id, e.started_at / MIN, e.ended_at / MIN))
        .collect()
}

fn minutes(store: &ActivityStore) -> Vec<(u64, u64)> {
    spans(store).into_iter().map(|(_, s, e)| (s, e)).collect()
}

/// The integrity check behind "totals stay plain sums": on every day, the time
/// the entries cover (their union) is exactly the sum of their lengths, so no
/// minute is counted, or billed, twice.
fn assert_union_equals_sum(store: &ActivityStore) {
    if let Some(problem) = days_that_overlap(store) {
        panic!("{problem}");
    }
}

/// The first day whose entries cover less than their lengths add up to.
fn days_that_overlap(store: &ActivityStore) -> Option<String> {
    let mut days: BTreeMap<u64, Vec<(u64, u64)>> = BTreeMap::new();
    let mut statement = store
        .conn
        .prepare("SELECT started_at, ended_at FROM time_entries WHERE deleted_at IS NULL;")
        .unwrap();
    let live: Vec<(u64, u64)> = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))
        })
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    for (started, ended) in live {
        let mut from = started;
        while from < ended {
            let day = from / DAY;
            let to = ended.min((day + 1) * DAY);
            days.entry(day).or_default().push((from, to));
            from = to;
        }
    }
    for (day, mut spans) in days {
        let sum: u64 = spans.iter().map(|(from, to)| to - from).sum();
        spans.sort_unstable();
        let (mut union, mut reach) = (0, 0);
        for (from, to) in spans {
            union += to.saturating_sub(from.max(reach));
            reach = reach.max(to);
        }
        if union != sum {
            let mut list = entries(store);
            list.sort_by_key(|e| e.started_at);
            let pairs: Vec<_> = list
                .windows(2)
                .filter(|w| w[0].ended_at > w[1].started_at)
                .map(|w| {
                    [&w[0], &w[1]].map(|e| {
                        (
                            e.status.clone(),
                            e.started_at as f64 / 60_000.0,
                            e.ended_at as f64 / 60_000.0,
                        )
                    })
                })
                .collect();
            return Some(format!(
                "day {day}: entries cover {union}ms but add up to {sum}ms; overlapping (status, start min, end min): {pairs:?}"
            ));
        }
    }
    None
}

/// Who owns a stretch of activity: (entry, app and title, start, end).
type Ownership = (Option<String>, String, u64, u64);

/// Everything an Undo must give back: each entry but for `updated_at`, and
/// who owns which stretch of activity (cut pieces of one segment count as one).
fn state(store: &ActivityStore) -> (Vec<TimeEntry>, Vec<Ownership>) {
    let mut list = entries(store);
    for entry in &mut list {
        entry.updated_at = 0;
    }
    let mut statement = store
        .conn
        .prepare(
            "SELECT entry_id, app || title, started_at, COALESCE(ended_at, 9223372036854775807)
             FROM segments ORDER BY started_at, id;",
        )
        .unwrap();
    let mut owned: Vec<Ownership> = Vec::new();
    for row in statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, i64>(3)? as u64,
            ))
        })
        .unwrap()
    {
        let (owner, what, from, to) = row.unwrap();
        match owned.last_mut() {
            Some(last) if last.0 == owner && last.1 == what && last.3 == from => last.3 = to,
            _ => owned.push((owner, what, from, to)),
        }
    }
    (list, owned)
}

/// All the activity ever captured, in milliseconds.
fn recorded(store: &ActivityStore) -> i64 {
    store
        .conn
        .query_row(
            "SELECT SUM(COALESCE(ended_at, started_at) - started_at) FROM segments;",
            [],
            |row| row.get(0),
        )
        .unwrap()
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

/// Works from `from` to `to` (minutes), switching window every two minutes and
/// rebuilding every minute like the AI worker.
fn work(store: &mut ActivityStore, from: u64, to: u64) {
    let mut now = from * MIN;
    while now <= to * MIN {
        store
            .tick(sample(&format!("file {}", now / (2 * MIN))), 0, now)
            .unwrap();
        if now.is_multiple_of(MIN) {
            store.rebuild_range(0, now, now).unwrap();
        }
        now += TICK;
    }
}

#[test]
fn an_added_entry_trims_the_pending_entry_before_it() {
    let mut store = store();
    let earlier = add(&mut store, 0, 60, false);
    let added = add(&mut store, 40, 100, false);
    assert_eq!(
        spans(&store),
        vec![(earlier, 0, 40), (added, 40, 100)],
        "the new entry keeps the contested minutes"
    );
    assert_union_equals_sum(&store);
}

#[test]
fn an_added_entry_trims_the_start_of_the_pending_entry_after_it() {
    let mut store = store();
    let later = add(&mut store, 50, 100, false);
    let added = add(&mut store, 0, 70, false);
    assert_eq!(spans(&store), vec![(added, 0, 70), (later, 70, 100)]);
    assert_union_equals_sum(&store);
}

#[test]
fn an_added_entry_splits_the_pending_entry_around_it_and_both_halves_keep_its_details() {
    let mut store = store();
    let middle = add(&mut store, 0, 100, false);
    let added = add(&mut store, 30, 50, false);
    let list = entries(&store);
    assert_eq!(
        list.iter()
            .map(|e| (e.started_at / MIN, e.ended_at / MIN))
            .collect::<Vec<_>>(),
        vec![(0, 30), (30, 50), (50, 100)]
    );
    assert_eq!(list[0].id, middle);
    assert_eq!(list[1].id, added);
    for half in [&list[0], &list[2]] {
        assert_eq!(half.description, "Work");
        assert_eq!(half.category_id.as_deref(), Some("c"));
        assert!(half.billable);
        assert_eq!(half.status, "pending");
    }
    assert_ne!(list[2].id, middle);
    assert_union_equals_sum(&store);
}

#[test]
fn an_added_entry_removes_a_pending_entry_it_covers() {
    let mut store = store();
    add(&mut store, 20, 40, false);
    let added = add(&mut store, 10, 60, false);
    assert_eq!(spans(&store), vec![(added, 10, 60)]);
    assert_union_equals_sum(&store);
}

#[test]
fn one_added_entry_can_trim_split_and_remove_several_entries_at_once() {
    let mut store = store();
    let first = add(&mut store, 0, 30, false);
    add(&mut store, 30, 50, false);
    let last = add(&mut store, 50, 120, false);
    let added = add(&mut store, 20, 80, false);
    assert_eq!(
        spans(&store),
        vec![(first, 0, 20), (added, 20, 80), (last, 80, 120)]
    );
    assert_union_equals_sum(&store);
}

#[test]
fn an_added_entry_is_refused_over_an_approved_one_and_nothing_changes() {
    let mut store = store();
    let pending = add(&mut store, 0, 50, false);
    add(&mut store, 60, 90, true);
    let before = state(&store);
    let history = store.undo_stack.len();
    for (from, to) in [(40, 70), (70, 80), (50, 100), (60, 90)] {
        assert_eq!(
            store
                .create_manual_entry(new_entry(from, to, true), NOW)
                .unwrap_err(),
            APPROVED_OVERLAP
        );
        assert_eq!(state(&store), before);
        assert_eq!(store.undo_stack.len(), history);
    }
    // The pending entry in reach of the refused claim was not trimmed either.
    assert_eq!(store.time_entry(&pending).unwrap().ended_at, 50 * MIN);
}

#[test]
fn an_edit_that_would_overlap_an_approved_entry_is_refused() {
    let mut store = store();
    let pending = add(&mut store, 0, 50, false);
    add(&mut store, 60, 90, true);
    let before = state(&store);
    for (from, to) in [(None, Some(70)), (None, Some(100))] {
        assert_eq!(
            edit_times(&mut store, &pending, from, to, NOW).unwrap_err(),
            APPROVED_OVERLAP
        );
        assert_eq!(state(&store), before);
    }
    let later = add(&mut store, 100, 130, false);
    let before = state(&store);
    assert_eq!(
        edit_times(&mut store, &later, Some(80), None, NOW).unwrap_err(),
        APPROVED_OVERLAP
    );
    assert_eq!(state(&store), before);
    // Up to the approved entry's edge is fine.
    edit_times(&mut store, &pending, None, Some(60), NOW).unwrap();
    assert_union_equals_sum(&store);
}

#[test]
fn editing_an_end_takes_the_gained_minutes_from_the_pending_entry_next_to_it() {
    let mut store = store();
    let first = add(&mut store, 0, 50, false);
    let second = add(&mut store, 60, 120, false);
    let third = add(&mut store, 120, 150, false);

    edit_times(&mut store, &first, None, Some(80), NOW).unwrap();
    assert_eq!(
        spans(&store),
        vec![
            (first.clone(), 0, 80),
            (second.clone(), 80, 120),
            (third.clone(), 120, 150)
        ]
    );

    edit_times(&mut store, &third, Some(100), None, NOW).unwrap();
    assert_eq!(
        spans(&store),
        vec![(first, 0, 80), (second, 80, 100), (third, 100, 150)]
    );
    assert_union_equals_sum(&store);
}

#[test]
fn an_edit_that_swallows_a_whole_entry_removes_it() {
    let mut store = store();
    let first = add(&mut store, 0, 30, false);
    add(&mut store, 35, 45, false);
    edit_times(&mut store, &first, None, Some(60), NOW).unwrap();
    assert_eq!(minutes(&store), vec![(0, 60)]);
}

#[test]
fn an_overlap_that_was_already_there_is_left_alone() {
    let mut store = store();
    let first = add(&mut store, 0, 60, false);
    let second = add(&mut store, 100, 160, false);
    // How the record got here: before this change, edits could overlap.
    store
        .conn
        .execute(
            "UPDATE time_entries SET started_at = ?1 WHERE id = ?2;",
            params![(50 * MIN) as i64, second],
        )
        .unwrap();
    let before = minutes(&store);
    assert_eq!(before, vec![(0, 60), (50, 160)]);

    // Changing the description, or the end the overlap is not at, claims nothing.
    store
        .update_time_entry(
            &first,
            UpdateTimeEntry {
                description: Some("Renamed".into()),
                ..Default::default()
            },
            NOW,
        )
        .unwrap();
    assert_eq!(minutes(&store), before);
    edit_times(&mut store, &first, Some(10), None, NOW).unwrap();
    assert_eq!(minutes(&store), vec![(10, 60), (50, 160)]);
}

#[test]
fn an_edit_cannot_set_many_entries_to_the_same_times() {
    let mut store = store();
    let a = add(&mut store, 0, 30, false);
    let b = add(&mut store, 40, 70, false);
    let before = state(&store);
    let patch = UpdateTimeEntry {
        ended_at: Some(100 * MIN),
        ..Default::default()
    };
    assert!(store.update_time_entries(&[a, b], patch, NOW).is_err());
    assert_eq!(state(&store), before);
}

#[test]
fn one_undo_restores_the_state_before_a_claim_exactly_and_redo_repeats_it() {
    let mut store = store();
    work(&mut store, 10, 70);
    store.stop_session(70 * MIN).unwrap();
    store.rebuild_range(0, 80 * MIN, 80 * MIN).unwrap();
    let captured = add(&mut store, 100, 130, false);
    let tail = add(&mut store, 140, 200, false);
    let before = state(&store);
    assert!(before.1.iter().any(|(owner, ..)| owner.is_some()));

    // Takes the end of the captured session, all of one entry, and the start
    // of another; the middle of the captured session is split around.
    let added = add(&mut store, 30, 150, false);
    assert_ne!(state(&store), before);
    assert_union_equals_sum(&store);
    let after = state(&store);

    assert_eq!(store.undo(NOW).unwrap(), Some("Add"));
    assert_eq!(state(&store), before);
    assert!(store.time_entry(&added).is_err());
    assert_eq!(store.time_entry(&captured).unwrap().ended_at, 130 * MIN);
    assert_eq!(store.time_entry(&tail).unwrap().started_at, 140 * MIN);

    assert_eq!(store.redo(NOW).unwrap(), Some("Add"));
    assert_eq!(state(&store), after);
    assert_union_equals_sum(&store);
    assert_eq!(store.undo(NOW).unwrap(), Some("Add"));
    assert_eq!(state(&store), before);
    assert_eq!(store.undo(NOW).unwrap(), Some("Add"));
}

#[test]
fn a_claim_leaves_the_activity_it_covered_to_the_claiming_entry() {
    let mut store = store();
    work(&mut store, 10, 70);
    store.stop_session(70 * MIN).unwrap();
    store.rebuild_range(0, 80 * MIN, 80 * MIN).unwrap();
    let session = entries(&store)[0].id.clone();
    let recorded_before = recorded(&store);
    let added = add(&mut store, 30, 50, false);

    let detail = store.get_entry_detail(&added).unwrap();
    assert!(!detail.segments.is_empty());
    assert!(detail
        .segments
        .iter()
        .all(|s| s.started_at >= 30 * MIN && s.ended_at.unwrap() <= 50 * MIN));
    let head = store.get_entry_detail(&session).unwrap();
    assert!(head
        .segments
        .iter()
        .all(|s| s.ended_at.unwrap() <= 30 * MIN));
    let tail = entries(&store).pop().unwrap();
    let tail = store.get_entry_detail(&tail.id).unwrap();
    assert!(tail.segments.iter().all(|s| s.started_at >= 50 * MIN));
    // Nothing is deleted: the activity is all still there.
    assert_eq!(recorded(&store), recorded_before);
    assert_union_equals_sum(&store);
}

#[test]
fn dragging_over_the_middle_of_the_live_session_closes_it_and_capture_resumes_after() {
    let mut store = store();
    work(&mut store, 10, 70);
    let live = entries(&store);
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].status, "building");

    let added = add(&mut store, 30, 50, false);
    let list = entries(&store);
    assert_eq!(list.len(), 3, "{list:?}");
    assert_eq!(list[0].id, live[0].id);
    assert_eq!(
        (list[0].status.as_str(), list[0].ended_at / MIN),
        ("pending", 30)
    );
    assert_eq!(list[1].id, added);
    assert_eq!(list[2].status, "building");
    assert_eq!(list[2].started_at / MIN, 50);

    // Capture keeps going into the part after the new entry.
    work(&mut store, 70 + 1, 75);
    let list = entries(&store);
    assert_eq!(list.len(), 3, "{list:?}");
    assert_eq!(list[2].status, "building");
    assert_eq!(list[2].started_at / MIN, 50);
    assert!(list[2].ended_at >= 75 * MIN);
    assert_union_equals_sum(&store);

    // Undo rejoins everything recorded since, like undoing a live split.
    assert_eq!(store.undo(75 * MIN).unwrap(), Some("Add"));
    work(&mut store, 76, 77);
    let list = entries(&store);
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].id, live[0].id);
    assert_eq!(list[0].status, "building");
    assert_eq!(list[0].started_at, 10 * MIN);
    assert_union_equals_sum(&store);
}

#[test]
fn dragging_over_the_end_of_the_live_session_closes_it_before_the_new_entry() {
    let mut store = store();
    work(&mut store, 10, 70);
    let live = entries(&store)[0].id.clone();
    let added = add(&mut store, 60, 65, false);

    let list = entries(&store);
    assert_eq!(list.len(), 3, "{list:?}");
    assert_eq!(list[0].id, live);
    assert_eq!(
        (list[0].status.as_str(), list[0].ended_at / MIN),
        ("pending", 60)
    );
    assert_eq!(list[1].id, added);
    assert_eq!(list[2].status, "building");
    assert_union_equals_sum(&store);
    work(&mut store, 70 + 1, 80);
    assert_union_equals_sum(&store);
    assert_eq!(entries(&store).len(), 3);
}

#[test]
fn merging_claims_the_gap_it_spans() {
    let mut store = store();
    let a = add(&mut store, 0, 20, false);
    let in_gap = add(&mut store, 22, 23, false);
    let b = add(&mut store, 24, 40, false);
    let before = state(&store);

    let merged = store.merge_time_entries(&a, &b, NOW).unwrap();
    assert_eq!(merged.id, a);
    assert_eq!(minutes(&store), vec![(0, 40)]);
    assert!(store.time_entry(&in_gap).is_err());
    assert_union_equals_sum(&store);

    assert_eq!(store.undo(NOW).unwrap(), Some("Merge"));
    assert_eq!(state(&store), before);
    assert_eq!(store.redo(NOW).unwrap(), Some("Merge"));
    assert_eq!(minutes(&store), vec![(0, 40)]);
    assert_eq!(store.undo(NOW).unwrap(), Some("Merge"));
    assert_eq!(state(&store), before);
}

#[test]
fn merging_is_refused_when_an_approved_entry_sits_in_the_gap() {
    let mut store = store();
    let a = add(&mut store, 0, 20, false);
    add(&mut store, 22, 23, true);
    let b = add(&mut store, 24, 40, false);
    let before = state(&store);
    assert_eq!(
        store.merge_time_entries(&a, &b, NOW).unwrap_err(),
        APPROVED_OVERLAP
    );
    assert_eq!(state(&store), before);
}

/// An entry the way the record may already hold one: no claim, no checks.
fn insert_raw(store: &ActivityStore, id: &str, from: u64, to: u64) {
    store
        .conn
        .execute(
            "INSERT INTO time_entries (id,started_at,ended_at,description,status,source,created_at,updated_at)
             VALUES (?1,?2,?3,'Legacy','pending','manual',1,1);",
            params![id, (from * MIN) as i64, (to * MIN) as i64],
        )
        .unwrap();
}

#[test]
fn merging_two_overlapping_entries_leaves_an_overlap_with_a_third_alone() {
    let mut store = store();
    insert_raw(&store, "a", 0, 30);
    insert_raw(&store, "b", 20, 50);
    insert_raw(&store, "c", 10, 40);
    store.merge_time_entries("a", "b", NOW).unwrap();
    assert_eq!(minutes(&store), vec![(0, 50), (10, 40)]);
}

#[test]
fn a_claim_that_is_refused_midway_changes_nothing() {
    let mut store = store();
    add(&mut store, 0, 50, false);
    add(&mut store, 55, 80, true);
    let before = state(&store);
    // The first range would trim the pending entry; the second hits approved
    // time. Neither is applied.
    assert!(store
        .create_manual_entry(new_entry(40, 60, false), NOW)
        .is_err());
    assert_eq!(state(&store), before);
}

#[test]
fn gained_minutes_are_what_the_new_span_adds_to_the_old_ones() {
    use super::claim::gained;
    assert_eq!(gained(&[], 10, 20), vec![(10, 20)]);
    assert_eq!(gained(&[(10, 20)], 10, 20), vec![]);
    assert_eq!(gained(&[(10, 20)], 5, 25), vec![(5, 10), (20, 25)]);
    assert_eq!(gained(&[(10, 20)], 12, 30), vec![(20, 30)]);
    assert_eq!(gained(&[(10, 20)], 0, 15), vec![(0, 10)]);
    assert_eq!(gained(&[(0, 20), (30, 50)], 0, 50), vec![(20, 30)]);
    assert_eq!(gained(&[(0, 20), (10, 50)], 0, 60), vec![(50, 60)]);
    assert_eq!(gained(&[(40, 50)], 0, 10), vec![(0, 10)]);
}

/// A long run of random writes, undos and redos: after every step the day's
/// entries cover exactly as much time as their lengths add up to.
#[test]
fn the_per_day_union_equals_the_per_day_sum_after_any_run_of_writes() {
    let (mut applied, mut refused) = (0, 0);
    for start in (1..=300u64).map(|i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15)) {
        let mut seed: u64 = start;
        let mut random = move |below: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % below
        };
        let mut store = store();
        for _ in 0..250 {
            let list = entries(&store);
            let pick = |n: u64| -> Option<String> {
                (!list.is_empty()).then(|| list[(n % list.len() as u64) as usize].id.clone())
            };
            let result: Result<(), String> = match random(8) {
                0..=2 => {
                    let from = random(400);
                    let to = from + 1 + random(120);
                    store
                        .create_manual_entry(new_entry(from, to, random(3) != 0), NOW)
                        .map(|_| ())
                }
                3 | 4 => match pick(random(1000)) {
                    Some(id) => {
                        let (from, to) = if random(2) == 0 {
                            (Some(random(400)), None)
                        } else {
                            (None, Some(1 + random(520)))
                        };
                        edit_times(&mut store, &id, from, to, NOW).map(|_| ())
                    }
                    None => Ok(()),
                },
                5 => match (pick(random(1000)), pick(random(1000))) {
                    (Some(a), Some(b)) => store.merge_time_entries(&a, &b, NOW).map(|_| ()),
                    _ => Ok(()),
                },
                6 => match pick(random(1000)) {
                    Some(id) => {
                        if random(2) == 0 {
                            store.approve_time_entries(&[id], "user", NOW)
                        } else {
                            store.unapprove_time_entries(&[id], NOW)
                        }
                    }
                    None => Ok(()),
                },
                _ => {
                    if random(2) == 0 {
                        store.undo(NOW).map(|_| ())
                    } else {
                        store.redo(NOW).map(|_| ())
                    }
                }
            };
            match result {
                Ok(()) => applied += 1,
                Err(_) => refused += 1,
            }
            assert_union_equals_sum(&store);
        }
    }
    assert!(
        applied > 100 && refused > 20,
        "{applied} applied, {refused} refused"
    );
}

/// The same with capture running: the clock moves, the live session grows, and
/// hand-made entries are added and edited over past, live and future minutes.
/// (Undo, redo and merge are run against entries alone above: with a live
/// session regrowing underneath, undoing a change can hand back minutes the
/// session has since recorded into, and that interplay is not covered here.)
#[test]
fn the_union_equals_the_sum_while_capture_keeps_recording() {
    for start in SEEDS.into_iter().take(4) {
        let mut seed: u64 = start;
        let mut random = move |below: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % below
        };
        let mut store = store();
        let mut clock = 10;
        let mut log: Vec<String> = Vec::new();
        work(&mut store, 0, clock);
        for _ in 0..200 {
            let now = clock * MIN;
            let list = entries(&store);
            let pick = |n: u64| -> Option<String> {
                (!list.is_empty()).then(|| list[(n % list.len() as u64) as usize].id.clone())
            };
            // Refusals (over approved time) are part of the run.
            let _ = match random(6) {
                0..=2 => {
                    let to_clock = clock + 1 + random(8);
                    log.push(format!("work {clock}->{to_clock}"));
                    work(&mut store, clock, to_clock);
                    clock = to_clock;
                    Ok(())
                }
                3 => {
                    let from = clock.saturating_sub(random(90));
                    let to = from + 1 + random(60);
                    log.push(format!("create {from}-{to} at {clock}"));
                    store
                        .create_manual_entry(new_entry(from, to, random(4) != 0), now)
                        .map(|_| ())
                }
                _ => match pick(random(1000)) {
                    Some(id) => {
                        let (from, to) = if random(2) == 0 {
                            (Some(clock.saturating_sub(random(90))), None)
                        } else {
                            (
                                None,
                                Some(1 + clock.saturating_sub(random(60)) + random(30)),
                            )
                        };
                        log.push(format!("edit {from:?} {to:?} at {clock}"));
                        edit_times(&mut store, &id, from, to, now).map(|_| ())
                    }
                    None => Ok(()),
                },
            };
            let now = clock * MIN;
            store.rebuild_range(0, now + 120 * MIN, now).unwrap();
            if let Some(problem) = days_that_overlap(&store) {
                panic!(
                    "{problem}\n{}",
                    log[log.len().saturating_sub(12)..].join("\n")
                );
            }
        }
    }
}

#[test]
fn an_entry_dragged_out_past_now_fences_the_live_session_until_it_ends() {
    let mut store = store();
    work(&mut store, 0, 100);
    let live = entries(&store)[0].id.clone();
    let future = store
        .create_manual_entry(new_entry(100, 140, true), 100 * MIN)
        .unwrap()
        .id;
    // The live session ends where the entry begins, not inside it.
    work(&mut store, 101, 120);
    let list = entries(&store);
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!(
        (list[0].id.as_str(), list[0].ended_at / MIN),
        (live.as_str(), 100)
    );
    assert_eq!(list[1].id, future);
    assert_union_equals_sum(&store);

    // Capture resumes where it ends.
    work(&mut store, 121, 145);
    let list = entries(&store);
    assert_eq!(list.len(), 3, "{list:?}");
    assert_eq!(list[2].status, "building");
    assert_eq!(list[2].started_at / MIN, 140);
    assert_union_equals_sum(&store);
}

#[test]
fn undoing_a_merge_that_closed_the_live_entry_does_not_stretch_it_over_the_other_half() {
    let mut store = store();
    store
        .conn
        .execute_batch(
            "INSERT INTO time_entries (id,started_at,ended_at,description,status,source,created_at,updated_at)
             VALUES ('live',0,600000,'Live','building','manual',1,1);",
        )
        .unwrap();
    insert_raw(&store, "next", 10, 12);
    let before = minutes(&store);

    // The other half is hand-made, so the merge closes the live entry.
    store.merge_time_entries("live", "next", 12 * MIN).unwrap();
    assert_eq!(minutes(&store), vec![(0, 12)]);
    assert_eq!(store.undo(12 * MIN).unwrap(), Some("Merge"));
    assert_eq!(minutes(&store), before);
    assert_union_equals_sum(&store);
}
