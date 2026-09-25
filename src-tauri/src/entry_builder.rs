use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Inactivity at least this long ends a session; anything shorter is a short
/// break inside it.
pub const SHORT_BREAK_THRESHOLD_MS: u64 = 5 * 60 * 1000; // 5 min
/// A finished session shorter than this is noise (a nudged mouse), not work.
pub const MIN_SESSION_MS: u64 = 60 * 1000; // 1 min

#[derive(Debug, Clone)]
pub struct EntrySettings {
    pub min_duration_ms: u64,
    pub short_break_threshold_ms: u64,
}

impl Default for EntrySettings {
    fn default() -> Self {
        Self {
            min_duration_ms: MIN_SESSION_MS,
            short_break_threshold_ms: SHORT_BREAK_THRESHOLD_MS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SegmentInput {
    pub id: i64,
    pub app: String,
    pub title: String,
    pub kind: String,
    pub label: Option<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub entry_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuiltTimeEntry {
    pub id: String,
    pub started_at: u64,
    pub ended_at: u64,
    pub description: String,
    pub category_id: Option<String>,
    pub project_id: Option<String>,
    pub status: String,
    pub approved_by: Option<String>,
    pub source: String,
    pub billable: bool,
    pub invoice_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted_at: Option<u64>,
    pub segment_ids: Vec<i64>,
}

/// Pure entry builder: folds raw activity segments into sessions, one
/// reviewable time entry per session.
///
/// Rules:
/// 1. A session is one continuous stretch of activity, whatever apps it
///    moves through. Only inactivity splits it: a gap of at least
///    `short_break_threshold_ms` (5 min) between one activity segment and
///    the next, whether the gap is an idle or manual break or time nothing
///    was captured, ends the session. Break segments carry no work, so the
///    gap is measured between activity segments.
/// 2. The last session stays `building` while it is live (a segment is still
///    open, or the last one ended less than the threshold ago). Everything
///    before it has closed and is `pending`, which is when it gets
///    categorized: once per session.
/// 3. A closed session shorter than `min_duration_ms` (1 min) is dropped
///    rather than becoming an entry of its own. Zero-length segments (an open
///    segment the app could not close before it quit) carry no time and are
///    ignored.
/// 4. Frozen entries are preserved and never altered or re-segmented, and
///    their segments stay out of the build. Callers freeze every entry that
///    has closed (so its suggestion and any edits stay attached to a stable
///    id), plus deleted ones, whose time must stay unassigned rather than be
///    rebuilt into a new entry.
/// 5. Ids are stable across rebuilds: an entry keeps the id its segments were
///    linked to, so re-running the builder updates the open entry in place
///    instead of minting a duplicate.
pub fn build_entries(
    segments: &[SegmentInput],
    frozen_entries: &[BuiltTimeEntry],
    settings: &EntrySettings,
    now: u64,
) -> Vec<BuiltTimeEntry> {
    let frozen_ids: HashSet<&str> = frozen_entries.iter().map(|e| e.id.as_str()).collect();
    let mut work: Vec<&SegmentInput> = segments
        .iter()
        .filter(|seg| seg.kind != "break")
        .filter(|seg| seg.ended_at.is_none_or(|end| end > seg.started_at))
        .filter(|seg| !belongs_to_frozen(seg, &frozen_ids, frozen_entries, now))
        .collect();
    work.sort_by_key(|seg| seg.started_at);

    let mut results: Vec<BuiltTimeEntry> = frozen_entries.to_vec();
    let mut claimed: HashSet<String> = frozen_ids.iter().map(|id| id.to_string()).collect();
    let gap = settings.short_break_threshold_ms;

    let mut session: Vec<&SegmentInput> = Vec::new();
    let mut session_end = 0;
    for seg in work {
        let seg_end = seg.ended_at.unwrap_or(now);
        if !session.is_empty() && seg.started_at.saturating_sub(session_end) >= gap {
            push_closed(
                &mut results,
                &session,
                session_end,
                settings,
                now,
                &mut claimed,
            );
            session.clear();
        }
        session_end = if session.is_empty() {
            seg_end
        } else {
            session_end.max(seg_end)
        };
        session.push(seg);
    }

    if !session.is_empty() {
        let live = session.iter().any(|seg| seg.ended_at.is_none())
            || now.saturating_sub(session_end) < gap;
        if live {
            let mut entry = create_entry_from_segments(&session, session_end, now, &mut claimed);
            entry.status = "building".to_string();
            results.push(entry);
        } else {
            push_closed(
                &mut results,
                &session,
                session_end,
                settings,
                now,
                &mut claimed,
            );
        }
    }

    results.sort_by_key(|e| e.started_at);
    results
}

/// Adds a finished session as a pending entry, unless it is too short to be work.
fn push_closed(
    results: &mut Vec<BuiltTimeEntry>,
    session: &[&SegmentInput],
    session_end: u64,
    settings: &EntrySettings,
    now: u64,
    claimed: &mut HashSet<String>,
) {
    if session_end.saturating_sub(session[0].started_at) >= settings.min_duration_ms {
        results.push(create_entry_from_segments(
            session,
            session_end,
            now,
            claimed,
        ));
    }
}

/// Whether a segment is already accounted for by a frozen entry: linked to
/// it, or inside its span (a deleted entry's segments are unlinked, but its
/// time must stay unassigned).
fn belongs_to_frozen(
    seg: &SegmentInput,
    frozen_ids: &HashSet<&str>,
    frozen_entries: &[BuiltTimeEntry],
    now: u64,
) -> bool {
    if seg
        .entry_id
        .as_deref()
        .is_some_and(|id| frozen_ids.contains(id))
    {
        return true;
    }
    let seg_end = seg.ended_at.unwrap_or(now);
    frozen_entries
        .iter()
        .any(|fe| seg.started_at < fe.ended_at && seg_end > fe.started_at)
}

fn create_entry_from_segments(
    segments: &[&SegmentInput],
    ended_at: u64,
    now: u64,
    claimed: &mut HashSet<String>,
) -> BuiltTimeEntry {
    // Reuse the id an earlier build gave these segments, unless an earlier
    // entry in this build already took it.
    let id = segments
        .iter()
        .filter_map(|s| s.entry_id.as_ref())
        .find(|id| !id.is_empty() && !claimed.contains(*id))
        .cloned()
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    claimed.insert(id.clone());

    let started_at = segments[0].started_at;
    let segment_ids: Vec<i64> = segments.iter().map(|s| s.id).collect();

    // Summarize dominant app & titles for description
    let description = generate_description(segments);

    BuiltTimeEntry {
        id,
        started_at,
        ended_at,
        description,
        category_id: None,
        project_id: None,
        status: "pending".to_string(),
        approved_by: None,
        source: "auto".to_string(),
        billable: false,
        invoice_id: None,
        created_at: now,
        updated_at: now,
        deleted_at: None,
        segment_ids,
    }
}

fn generate_description(segments: &[&SegmentInput]) -> String {
    // Collect non-empty titles and apps
    let mut app_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let mut titles_by_app: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for seg in segments {
        let dur = seg
            .ended_at
            .unwrap_or(seg.started_at + 1000)
            .saturating_sub(seg.started_at);
        *app_counts.entry(seg.app.clone()).or_insert(0) += dur;

        let clean_title = seg.title.trim();
        if !clean_title.is_empty() && clean_title != seg.app {
            let list = titles_by_app.entry(seg.app.clone()).or_default();
            if !list.contains(&clean_title.to_string()) && list.len() < 3 {
                list.push(clean_title.to_string());
            }
        }
    }

    // Find dominant app
    let dominant_app = app_counts
        .into_iter()
        .max_by_key(|(_, dur)| *dur)
        .map(|(app, _)| app)
        .unwrap_or_else(|| "Work".to_string());

    if let Some(titles) = titles_by_app.get(&dominant_app) {
        if !titles.is_empty() {
            return format!("{}: {}", dominant_app, titles.join(", "));
        }
    }

    dominant_app
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    fn make_seg(id: i64, app: &str, title: &str, kind: &str, start: u64, end: u64) -> SegmentInput {
        SegmentInput {
            id,
            app: app.to_string(),
            title: title.to_string(),
            kind: kind.to_string(),
            label: None,
            started_at: start,
            ended_at: Some(end),
            entry_id: None,
        }
    }

    fn frozen_entry(id: &str, started_at: u64, ended_at: u64, status: &str) -> BuiltTimeEntry {
        BuiltTimeEntry {
            id: id.to_string(),
            started_at,
            ended_at,
            description: "Closed".to_string(),
            category_id: None,
            project_id: None,
            status: status.to_string(),
            approved_by: None,
            source: "auto".to_string(),
            billable: false,
            invoice_id: None,
            created_at: 0,
            updated_at: 0,
            deleted_at: None,
            segment_ids: vec![],
        }
    }

    /// An hour of work hopping between apps every 20 seconds.
    fn busy_hour() -> Vec<SegmentInput> {
        let apps = ["Code", "Terminal", "Safari", "Slack"];
        (0..180)
            .map(|i| {
                let app = apps[i as usize % apps.len()];
                make_seg(
                    i,
                    app,
                    app,
                    "activity",
                    i as u64 * 20_000,
                    (i as u64 + 1) * 20_000,
                )
            })
            .collect()
    }

    #[test]
    fn an_hour_across_many_apps_is_one_session() {
        let segs = busy_hour();
        let settings = EntrySettings::default();

        let live = build_entries(&segs, &[], &settings, 60 * MIN + 30_000);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].status, "building");

        let closed = build_entries(&segs, &[], &settings, 65 * MIN);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].status, "pending");
        assert_eq!(closed[0].started_at, 0);
        assert_eq!(closed[0].ended_at, 60 * MIN);
        assert_eq!(closed[0].segment_ids.len(), 180);
    }

    #[test]
    fn five_minutes_of_inactivity_starts_a_new_session() {
        let segs = vec![
            make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN),
            make_seg(2, "Idle", "No activity", "break", 10 * MIN, 15 * MIN),
            make_seg(3, "Slack", "chat", "activity", 15 * MIN, 25 * MIN),
            make_seg(4, "Code", "main.rs", "activity", 25 * MIN, 30 * MIN),
        ];
        let entries = build_entries(&segs, &[], &EntrySettings::default(), 30 * MIN);

        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].started_at, entries[0].ended_at), (0, 10 * MIN));
        assert_eq!(entries[0].status, "pending");
        assert_eq!(entries[1].started_at, 15 * MIN);
        assert_eq!(entries[1].segment_ids, vec![3, 4]);
        assert_eq!(entries[1].status, "building");
    }

    #[test]
    fn a_break_under_five_minutes_stays_in_the_session() {
        let segs = vec![
            make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN),
            make_seg(2, "Idle", "No activity", "break", 10 * MIN, 14 * MIN),
            make_seg(3, "Code", "main.rs", "activity", 14 * MIN, 20 * MIN),
        ];
        let entries = build_entries(&segs, &[], &EntrySettings::default(), 20 * MIN);

        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].started_at, entries[0].ended_at), (0, 20 * MIN));
        assert_eq!(entries[0].segment_ids, vec![1, 3]);
    }

    #[test]
    fn an_uncaptured_gap_of_five_minutes_ends_the_session() {
        let segs = vec![
            make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN),
            // Nothing captured (asleep, capture paused) for 6 minutes.
            make_seg(2, "Safari", "docs.rs", "activity", 16 * MIN, 20 * MIN),
        ];
        let entries = build_entries(&segs, &[], &EntrySettings::default(), 20 * MIN);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].ended_at, 10 * MIN);
        assert_eq!(entries[1].started_at, 16 * MIN);
    }

    #[test]
    fn the_last_session_closes_once_five_idle_minutes_pass() {
        let segs = vec![make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN)];
        let settings = EntrySettings::default();

        assert_eq!(
            build_entries(&segs, &[], &settings, 14 * MIN)[0].status,
            "building"
        );
        assert_eq!(
            build_entries(&segs, &[], &settings, 15 * MIN)[0].status,
            "pending"
        );
    }

    #[test]
    fn an_open_segment_keeps_the_session_building() {
        let mut segs = vec![make_seg(1, "Code", "main.rs", "activity", 0, 0)];
        segs[0].ended_at = None;
        let entries = build_entries(&segs, &[], &EntrySettings::default(), 90 * MIN);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "building");
        assert_eq!(entries[0].ended_at, 90 * MIN);
    }

    #[test]
    fn a_session_under_a_minute_is_not_an_entry() {
        let segs = vec![
            make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN),
            // A nudged mouse between two idle stretches.
            make_seg(
                2,
                "Finder",
                "Desktop",
                "activity",
                20 * MIN,
                20 * MIN + 20_000,
            ),
        ];
        let entries = build_entries(&segs, &[], &EntrySettings::default(), 40 * MIN);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].segment_ids, vec![1]);
    }

    #[test]
    fn zero_length_segments_never_become_entries() {
        // The segment left open when the app quit is closed at its own
        // start. It carries no time, so it must not mint an entry on every
        // rebuild after the session before it froze.
        let frozen = frozen_entry("closed", 0, 10 * MIN, "pending");
        let mut segs = vec![
            make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN),
            make_seg(2, "Code", "main.rs", "activity", 10 * MIN, 10 * MIN),
            make_seg(3, "Code", "main.rs", "activity", 30 * MIN, 30 * MIN),
        ];
        segs[0].entry_id = Some("closed".to_string());
        segs[1].entry_id = Some("closed".to_string());

        for now in [31 * MIN, 32 * MIN, 60 * MIN] {
            let entries = build_entries(
                &segs,
                std::slice::from_ref(&frozen),
                &EntrySettings::default(),
                now,
            );
            assert_eq!(entries, vec![frozen.clone()]);
        }
    }

    #[test]
    fn rebuilding_keeps_the_ids_segments_were_linked_to() {
        let mut segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 10 * MIN),
            make_seg(2, "Break", "Idle", "break", 10 * MIN, 20 * MIN),
            make_seg(3, "Slack", "chat", "activity", 20 * MIN, 22 * MIN),
        ];
        let settings = EntrySettings::default();
        let first = build_entries(&segs, &[], &settings, 22 * MIN);
        segs[0].entry_id = Some(first[0].id.clone());
        segs[2].entry_id = Some(first[1].id.clone());

        let second = build_entries(&segs, &[], &settings, 23 * MIN);
        let ids =
            |entries: &[BuiltTimeEntry]| entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&first), ids(&second));
    }

    #[test]
    fn a_closed_session_is_never_reopened_by_later_activity() {
        let frozen = frozen_entry("closed", 0, 10 * MIN, "processing");
        let mut segs = vec![
            make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN),
            make_seg(2, "Slack", "chat", "activity", 16 * MIN, 20 * MIN),
        ];
        segs[0].entry_id = Some("closed".to_string());
        let entries = build_entries(
            &segs,
            std::slice::from_ref(&frozen),
            &EntrySettings::default(),
            20 * MIN,
        );

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], frozen);
        assert_eq!(entries[1].segment_ids, vec![2]);
        assert_eq!(entries[1].status, "building");
    }

    #[test]
    fn a_deleted_entrys_time_stays_unassigned() {
        let mut deleted = frozen_entry("gone", 0, 10 * MIN, "pending");
        deleted.deleted_at = Some(11 * MIN);
        let segs = vec![make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN)];
        let entries = build_entries(
            &segs,
            std::slice::from_ref(&deleted),
            &EntrySettings::default(),
            30 * MIN,
        );

        assert_eq!(entries, vec![deleted]);
    }
}
