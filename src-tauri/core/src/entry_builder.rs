use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Inactivity longer than this ends a session; anything up to it is a short
/// break inside it.
pub const SHORT_BREAK_THRESHOLD_MS: u64 = 5 * 60 * 1000; // 5 min
/// A finished session shorter than this is not worth tracking on its own (a
/// nudged mouse, a glance at a notification).
pub const MIN_SESSION_MS: u64 = 5 * 60 * 1000; // 5 min

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

/// Entry ids minted for time spent in an agent's pane start with this, so the
/// classifier knows the project is already settled.
pub const CARVE_ID_PREFIX: &str = "ag-";
/// Time in an agent's pane shorter than this stays part of the session around
/// it: it is a glance, not a stretch of work on the agent's project.
pub const MIN_CARVE_MS: u64 = 2 * 60 * 1000;
/// Stretches in the same pane closer together than this are one stretch.
pub const CARVE_MERGE_GAP_MS: u64 = 60 * 1000;

/// One row of the time the person had an agent's pane in front of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusInput {
    pub id: i64,
    pub project_id: String,
    pub agent: String,
    pub started_at: u64,
    pub ended_at: u64,
}

/// A stretch of a session that belongs to an agent's project: the person was
/// in that agent's pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carve {
    /// The id its entry will carry, stable while the stretch grows.
    pub id: String,
    pub project_id: String,
    pub label: String,
    pub start: u64,
    pub end: u64,
}

/// Joins focus rows into stretches: same project, gaps up to a minute closed,
/// anything under two minutes dropped.
pub fn carves_from_focus(rows: &[FocusInput]) -> Vec<Carve> {
    let mut sorted: Vec<&FocusInput> = rows
        .iter()
        .filter(|row| row.ended_at > row.started_at)
        .collect();
    sorted.sort_by_key(|row| (row.started_at, row.id));
    let mut carves: Vec<Carve> = Vec::new();
    let mut anchors: Vec<i64> = Vec::new();
    for row in sorted {
        match carves.last_mut() {
            Some(last)
                if last.project_id == row.project_id
                    && row.started_at <= last.end + CARVE_MERGE_GAP_MS =>
            {
                last.end = last.end.max(row.ended_at);
            }
            _ => {
                carves.push(Carve {
                    id: String::new(),
                    project_id: row.project_id.clone(),
                    label: format!("{} agent", row.agent),
                    start: row.started_at,
                    end: row.ended_at,
                });
                anchors.push(row.id);
            }
        }
    }
    carves
        .into_iter()
        .zip(anchors)
        .filter(|(carve, _)| carve.end - carve.start >= MIN_CARVE_MS)
        .map(|(mut carve, anchor)| {
            carve.id = format!("{CARVE_ID_PREFIX}{anchor}");
            carve
        })
        .collect()
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
///    moves through. Only inactivity splits it: a gap longer than
///    `short_break_threshold_ms` (5 min) between one activity segment and
///    the next, whether the gap is an idle or manual break or time nothing
///    was captured, ends the session. Break segments carry no work, so the
///    gap is measured between activity segments, and the session spans the
///    whole wall-clock range, short gaps included.
/// 2. The last session stays `building` while it is live (a segment is still
///    open, or the last one ended no more than the threshold ago). Everything
///    before it has closed and is `pending`, which is when it gets
///    categorized: once per session.
/// 3. A closed session shorter than `min_duration_ms` (5 min) is dropped
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
/// 6. Time spent in an agent's pane is carved out of its session as an entry
///    of its own on that agent's project (`carves`). It is the only slicing
///    the builder does: the pieces partition the session, a stretch under
///    two minutes stays in the surrounding entry, and a leftover under two
///    minutes next to a carve joins it.
#[cfg(test)]
pub fn build_entries(
    segments: &[SegmentInput],
    frozen_entries: &[BuiltTimeEntry],
    settings: &EntrySettings,
    now: u64,
) -> Vec<BuiltTimeEntry> {
    build_entries_with(segments, frozen_entries, settings, now, &[])
}

/// `build_entries`, with the time the person spent in agents' panes carved out
/// of the sessions around it (rule 6): a session that contains such a stretch
/// becomes several entries that together cover exactly the same time, so
/// work totals are unchanged.
pub fn build_entries_with(
    segments: &[SegmentInput],
    frozen_entries: &[BuiltTimeEntry],
    settings: &EntrySettings,
    now: u64,
    carves: &[Carve],
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
        if !session.is_empty() && seg.started_at.saturating_sub(session_end) > gap {
            push_closed(
                &mut results,
                &session,
                session_end,
                settings,
                now,
                &mut claimed,
                carves,
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
            || now.saturating_sub(session_end) <= gap;
        if live {
            let pieces = split_session(&session, session_end, now, &mut claimed, carves, true);
            for mut entry in pieces {
                if entry.status == "building" {
                    // An ongoing session isn't named after its apps until it closes.
                    entry.description = UNTITLED_SESSION.to_string();
                }
                results.push(entry);
            }
        } else {
            push_closed(
                &mut results,
                &session,
                session_end,
                settings,
                now,
                &mut claimed,
                carves,
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
    carves: &[Carve],
) {
    if session_end.saturating_sub(session[0].started_at) >= settings.min_duration_ms {
        results.extend(split_session(
            session,
            session_end,
            now,
            claimed,
            carves,
            false,
        ));
    }
}

/// One piece of a session: a stretch with the project of the agent pane it
/// was spent in, or the rest of the session.
struct Piece<'a> {
    start: u64,
    end: u64,
    carve: Option<&'a Carve>,
}

/// Cuts a session at the carved stretches. With none it is one piece.
fn pieces_of<'a>(start: u64, end: u64, carves: &'a [Carve]) -> Vec<Piece<'a>> {
    struct Cut<'a> {
        start: u64,
        end: u64,
        carve: &'a Carve,
    }
    let mut cuts: Vec<Cut<'a>> = Vec::new();
    let mut floor = start;
    let mut sorted: Vec<&Carve> = carves.iter().collect();
    sorted.sort_by_key(|carve| carve.start);
    for carve in sorted {
        let (from, to) = (carve.start.max(floor), carve.end.min(end));
        if to > from && to - from >= MIN_CARVE_MS {
            cuts.push(Cut {
                start: from,
                end: to,
                carve,
            });
            floor = to;
        }
    }
    // A leftover too short to stand alone joins the carve beside it.
    for index in 0..cuts.len() {
        let before = if index == 0 {
            start
        } else {
            cuts[index - 1].end
        };
        if cuts[index].start - before < MIN_CARVE_MS {
            if index == 0 {
                cuts[index].start = start;
            } else {
                cuts[index - 1].end = cuts[index].start;
            }
        }
    }
    if let Some(last) = cuts.last_mut() {
        if end - last.end < MIN_CARVE_MS {
            last.end = end;
        }
    }
    let mut pieces = Vec::new();
    let mut cursor = start;
    for cut in &cuts {
        if cut.start > cursor {
            pieces.push(Piece {
                start: cursor,
                end: cut.start,
                carve: None,
            });
        }
        pieces.push(Piece {
            start: cut.start,
            end: cut.end,
            carve: Some(cut.carve),
        });
        cursor = cut.end;
    }
    if cursor < end {
        pieces.push(Piece {
            start: cursor,
            end,
            carve: None,
        });
    }
    if pieces.is_empty() {
        pieces.push(Piece {
            start,
            end,
            carve: None,
        });
    }
    pieces
}

/// A session as one entry, or as one per piece when agent panes carved it up.
/// A live session is `building` only where it can still change: its last
/// piece, and any carve that could still be joined by the next stretch.
fn split_session(
    session: &[&SegmentInput],
    session_end: u64,
    now: u64,
    claimed: &mut HashSet<String>,
    carves: &[Carve],
    live: bool,
) -> Vec<BuiltTimeEntry> {
    let start = session[0].started_at;
    let pieces = pieces_of(start, session_end, carves);
    if pieces.len() == 1 && pieces[0].carve.is_none() {
        let mut entry = create_entry_from_segments(session, session_end, now, claimed);
        if live {
            entry.status = "building".to_string();
        }
        return vec![entry];
    }
    let last = pieces.len() - 1;
    pieces
        .iter()
        .enumerate()
        .map(|(index, piece)| {
            let inside: Vec<&SegmentInput> = session
                .iter()
                .copied()
                .filter(|seg| {
                    seg.started_at >= piece.start
                        && (seg.started_at < piece.end
                            || (index == last && seg.started_at <= piece.end))
                })
                .collect();
            let id = match piece.carve {
                Some(carve) => carve.id.clone(),
                None => inside
                    .iter()
                    .filter_map(|s| s.entry_id.as_ref())
                    .find(|id| !id.is_empty() && !claimed.contains(*id))
                    .cloned()
                    .unwrap_or_else(|| format!("rm-{}", piece.start)),
            };
            claimed.insert(id.clone());
            let description = match (&piece.carve, inside.is_empty()) {
                (_, false) => generate_description(&inside),
                (Some(carve), true) => carve.label.clone(),
                (None, true) => "Work".to_string(),
            };
            let still_open =
                live && (index == last || now.saturating_sub(piece.end) <= CARVE_MERGE_GAP_MS);
            BuiltTimeEntry {
                id,
                started_at: piece.start,
                ended_at: piece.end,
                description,
                category_id: None,
                project_id: piece.carve.map(|carve| carve.project_id.clone()),
                status: if still_open { "building" } else { "pending" }.to_string(),
                approved_by: None,
                source: "auto".to_string(),
                billable: false,
                invoice_id: None,
                created_at: now,
                updated_at: now,
                deleted_at: None,
                segment_ids: inside.iter().map(|s| s.id).collect(),
            }
        })
        .collect()
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

/// The name an ongoing session carries until it closes.
const UNTITLED_SESSION: &str = "Untitled session";

fn generate_description(segments: &[&SegmentInput]) -> String {
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
        // A title equal to the app name is uninformative in a description
        // like "Slack: Slack", so it's dropped; titles are capped at 3 per
        // app to keep the generated description short.
        if !clean_title.is_empty() && clean_title != seg.app {
            let list = titles_by_app.entry(seg.app.clone()).or_default();
            if !list.contains(&clean_title.to_string()) && list.len() < 3 {
                list.push(clean_title.to_string());
            }
        }
    }

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
        assert_eq!(live[0].description, UNTITLED_SESSION);

        let closed = build_entries(&segs, &[], &settings, 66 * MIN);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].status, "pending");
        assert_eq!(closed[0].started_at, 0);
        assert_eq!(closed[0].ended_at, 60 * MIN);
        assert_eq!(closed[0].segment_ids.len(), 180);
    }

    #[test]
    fn more_than_five_minutes_of_inactivity_starts_a_new_session() {
        let segs = vec![
            make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN),
            make_seg(2, "Idle", "No activity", "break", 10 * MIN, 16 * MIN),
            make_seg(3, "Slack", "chat", "activity", 16 * MIN, 25 * MIN),
            make_seg(4, "Code", "main.rs", "activity", 25 * MIN, 30 * MIN),
        ];
        let entries = build_entries(&segs, &[], &EntrySettings::default(), 30 * MIN);

        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].started_at, entries[0].ended_at), (0, 10 * MIN));
        assert_eq!(entries[0].status, "pending");
        assert_eq!(entries[1].started_at, 16 * MIN);
        assert_eq!(entries[1].segment_ids, vec![3, 4]);
        assert_eq!(entries[1].status, "building");
    }

    #[test]
    fn a_break_of_up_to_five_minutes_stays_in_the_session() {
        let segs = vec![
            make_seg(1, "Code", "main.rs", "activity", 0, 10 * MIN),
            make_seg(2, "Idle", "No activity", "break", 10 * MIN, 15 * MIN),
            make_seg(3, "Code", "main.rs", "activity", 15 * MIN, 20 * MIN),
        ];
        let entries = build_entries(&segs, &[], &EntrySettings::default(), 20 * MIN);

        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].started_at, entries[0].ended_at), (0, 20 * MIN));
        assert_eq!(entries[0].segment_ids, vec![1, 3]);
    }

    #[test]
    fn a_watched_show_broken_by_short_gaps_is_one_session() {
        // Zen in front the whole time, with a glance at another app and
        // short idle stretches: one session over the real wall-clock range.
        let segs = vec![
            make_seg(1, "Zen", "", "activity", 0, 40 * MIN),
            make_seg(2, "Finder", "Desktop", "activity", 40 * MIN, 41 * MIN),
            make_seg(3, "Zen", "", "activity", 41 * MIN, 80 * MIN),
            make_seg(4, "Idle", "No activity", "break", 80 * MIN, 84 * MIN),
            make_seg(5, "Zen", "", "activity", 84 * MIN, 120 * MIN),
            make_seg(6, "Idle", "No activity", "break", 120 * MIN, 125 * MIN),
            make_seg(7, "Zen", "", "activity", 125 * MIN, 180 * MIN),
        ];
        let entries = build_entries(&segs, &[], &EntrySettings::default(), 200 * MIN);

        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].started_at, entries[0].ended_at), (0, 180 * MIN));
        assert_eq!(entries[0].segment_ids, vec![1, 2, 3, 5, 7]);
        assert_eq!(entries[0].description, "Zen");
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
            "building"
        );
        assert_eq!(
            build_entries(&segs, &[], &settings, 15 * MIN + 1)[0].status,
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
    fn a_session_under_five_minutes_is_not_an_entry() {
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
            // A few minutes of real use, still under the floor.
            make_seg(3, "Slack", "chat", "activity", 30 * MIN, 34 * MIN),
            // Exactly the floor is kept.
            make_seg(4, "Code", "main.rs", "activity", 45 * MIN, 50 * MIN),
        ];
        let entries = build_entries(&segs, &[], &EntrySettings::default(), 60 * MIN);

        let kept: Vec<_> = entries.iter().map(|e| e.segment_ids.clone()).collect();
        assert_eq!(kept, vec![vec![1], vec![4]]);
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

    fn focus(id: i64, project: &str, start: u64, end: u64) -> FocusInput {
        FocusInput {
            id,
            project_id: project.to_string(),
            agent: "claude".to_string(),
            started_at: start,
            ended_at: end,
        }
    }

    /// A 60 minute session, one segment per minute.
    fn hour_of_segments() -> Vec<SegmentInput> {
        (0..60)
            .map(|i| {
                make_seg(
                    i,
                    "Code",
                    "main.rs",
                    "activity",
                    i as u64 * MIN,
                    (i as u64 + 1) * MIN,
                )
            })
            .collect()
    }

    fn covered(entries: &[BuiltTimeEntry]) -> u64 {
        entries.iter().map(|e| e.ended_at - e.started_at).sum()
    }

    #[test]
    fn focus_rows_join_across_short_gaps_and_short_stretches_vanish() {
        let carves = carves_from_focus(&[
            focus(7, "a", 10 * MIN, 12 * MIN),
            // 30 s later: the same stretch.
            focus(8, "a", 12 * MIN + 30_000, 15 * MIN),
            // A glance.
            focus(9, "b", 30 * MIN, 30 * MIN + 90_000),
        ]);

        assert_eq!(carves.len(), 1);
        assert_eq!(carves[0].id, "ag-7");
        assert_eq!((carves[0].start, carves[0].end), (10 * MIN, 15 * MIN));
        assert_eq!(carves[0].project_id, "a");
    }

    #[test]
    fn a_carve_splits_a_session_without_changing_its_total() {
        let segs = hour_of_segments();
        let carves = carves_from_focus(&[focus(1, "proj", 20 * MIN, 35 * MIN)]);

        let entries = build_entries_with(&segs, &[], &EntrySettings::default(), 90 * MIN, &carves);

        assert_eq!(entries.len(), 3);
        assert_eq!(covered(&entries), 60 * MIN);
        assert_eq!(entries[1].id, "ag-1");
        assert_eq!(entries[1].project_id.as_deref(), Some("proj"));
        assert_eq!(
            (entries[1].started_at, entries[1].ended_at),
            (20 * MIN, 35 * MIN)
        );
        assert_eq!(entries[0].project_id, None);
        assert_eq!(entries[2].ended_at, 60 * MIN);
        assert!(entries.iter().all(|e| e.status == "pending"));
        // Every segment lands in exactly one piece.
        let mut ids: Vec<i64> = entries.iter().flat_map(|e| e.segment_ids.clone()).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..60).collect::<Vec<_>>());
    }

    #[test]
    fn a_leftover_under_two_minutes_joins_the_carve() {
        let segs = hour_of_segments();
        let carves = carves_from_focus(&[focus(1, "proj", 90_000, 59 * MIN + 30_000)]);

        let entries = build_entries_with(&segs, &[], &EntrySettings::default(), 90 * MIN, &carves);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "ag-1");
        assert_eq!((entries[0].started_at, entries[0].ended_at), (0, 60 * MIN));
    }

    #[test]
    fn carve_ids_are_stable_while_the_stretch_grows() {
        let segs = hour_of_segments();
        let early = carves_from_focus(&[focus(4, "proj", 20 * MIN, 30 * MIN)]);
        let later = carves_from_focus(&[
            focus(4, "proj", 20 * MIN, 30 * MIN),
            focus(5, "proj", 30 * MIN + 10_000, 40 * MIN),
        ]);
        let settings = EntrySettings::default();

        let a = build_entries_with(&segs, &[], &settings, 90 * MIN, &early);
        let b = build_entries_with(&segs, &[], &settings, 90 * MIN, &later);

        assert!(a.iter().any(|e| e.id == "ag-4"));
        let grown = b.iter().find(|e| e.id == "ag-4").unwrap();
        assert_eq!(grown.ended_at, 40 * MIN);
    }

    #[test]
    fn a_live_session_keeps_only_its_open_pieces_building() {
        let segs = hour_of_segments();
        let carves = carves_from_focus(&[focus(1, "proj", 10 * MIN, 20 * MIN)]);

        let entries = build_entries_with(&segs, &[], &EntrySettings::default(), 61 * MIN, &carves);

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].status, "pending");
        assert_eq!(entries[1].status, "pending");
        assert_eq!(entries[2].status, "building");
    }
}
