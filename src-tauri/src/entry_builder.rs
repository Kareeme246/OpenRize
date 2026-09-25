use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub const SHORT_BREAK_THRESHOLD_MS: u64 = 2 * 60 * 1000; // 2 min

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntrySettings {
    pub target_duration_ms: u64,
    pub min_duration_ms: u64,
    pub absorb_threshold_ms: u64,
    #[serde(default = "default_short_break_threshold")]
    pub short_break_threshold_ms: u64,
}

fn default_short_break_threshold() -> u64 {
    SHORT_BREAK_THRESHOLD_MS
}

impl Default for EntrySettings {
    fn default() -> Self {
        Self {
            target_duration_ms: 30 * 60 * 1000, // 30 min
            min_duration_ms: 8 * 60 * 1000,     // 8 min
            absorb_threshold_ms: 10 * 1000,     // 10 sec
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

/// Pure entry builder: folds raw activity segments into reviewable time entries.
///
/// Rules:
/// 1. Absorbs micro-segments under `absorb_threshold_ms` (10s) into neighbors.
/// 2. Forgives short breaks and gaps (< `short_break_threshold_ms`, 2m) as continuous work.
/// 3. Closes entry on real breaks (>= 2m), context shifts at target (~30m), or max target (45m).
/// 4. Minimum entry (8 min): leftover merges into previous entry if gap <= 2 min.
/// 5. Active session stays "building" while work is current (within 2m of now).
/// 6. Frozen entries are preserved and never altered or re-segmented. Callers
///    freeze every entry that has closed (so its suggestion and any edits
///    stay attached to a stable id), plus deleted ones, whose time must stay
///    unassigned rather than be rebuilt into a new entry.
/// 7. Ids are stable across rebuilds: an entry keeps the id its first
///    segment was linked to, so re-running the builder updates the open entry
///    in place instead of minting a duplicate.
pub fn build_entries(
    segments: &[SegmentInput],
    frozen_entries: &[BuiltTimeEntry],
    settings: &EntrySettings,
    now: u64,
) -> Vec<BuiltTimeEntry> {
    if segments.is_empty() {
        return frozen_entries.to_vec();
    }

    // 1. Filter out segments that fall inside frozen entries
    let mut unfrozen_segments: Vec<SegmentInput> = Vec::new();
    for seg in segments {
        let seg_end = seg.ended_at.unwrap_or(now);
        let overlaps_frozen = frozen_entries.iter().any(|fe| {
            // Check overlap
            seg.started_at < fe.ended_at && seg_end > fe.started_at
        });
        if !overlaps_frozen {
            unfrozen_segments.push(seg.clone());
        }
    }

    if unfrozen_segments.is_empty() {
        return frozen_entries.to_vec();
    }

    // Sort by started_at
    unfrozen_segments.sort_by_key(|s| s.started_at);

    // 2. Absorb micro-segments (< absorb_threshold_ms)
    let absorbed = absorb_micro_segments(&unfrozen_segments, settings.absorb_threshold_ms, now);

    // 3. Group work segments into entries separated by breaks
    let mut results: Vec<BuiltTimeEntry> = frozen_entries.to_vec();
    let frozen_ids: HashSet<String> = frozen_entries.iter().map(|e| e.id.clone()).collect();
    let mut claimed: HashSet<String> = frozen_ids.clone();
    let max_target = (settings.target_duration_ms as f64 * 1.5) as u64;

    let mut current_segments: Vec<SegmentInput> = Vec::new();

    for seg in absorbed {
        // If this is a break segment:
        if seg.kind == "break" {
            let break_dur = seg.ended_at.unwrap_or(now).saturating_sub(seg.started_at);
            // Short break / brief idleness (< short_break_threshold_ms): does not split continuous work
            if break_dur < settings.short_break_threshold_ms {
                continue;
            }

            // Real break: close current entry
            if !current_segments.is_empty() {
                if let Some(entry) =
                    create_entry_from_segments(&current_segments, now, &mut claimed)
                {
                    push_or_merge_entry(&mut results, entry, &frozen_ids, settings, now);
                }
                current_segments.clear();
            }
            continue;
        }

        let seg_end = seg.ended_at.unwrap_or(now);
        if current_segments.is_empty() {
            current_segments.push(seg);
        } else {
            let prev_seg = current_segments.last().unwrap();
            let prev_end = prev_seg.ended_at.unwrap_or(now);
            let gap = seg.started_at.saturating_sub(prev_end);

            // A gap between activity segments >= short_break_threshold_ms is a real break
            if gap >= settings.short_break_threshold_ms {
                if let Some(entry) =
                    create_entry_from_segments(&current_segments, now, &mut claimed)
                {
                    push_or_merge_entry(&mut results, entry, &frozen_ids, settings, now);
                }
                current_segments.clear();
                current_segments.push(seg);
                continue;
            }

            let entry_start = current_segments[0].started_at;
            let current_duration = seg_end.saturating_sub(entry_start);
            let prev_app = &prev_seg.app;
            let app_changed = &seg.app != prev_app;

            // Close entry if:
            // - duration >= target_duration_ms AND app changed (natural context shift)
            // - or duration >= 1.5 * target_duration_ms
            if (current_duration >= settings.target_duration_ms && app_changed)
                || current_duration >= max_target
            {
                if let Some(entry) =
                    create_entry_from_segments(&current_segments, now, &mut claimed)
                {
                    push_or_merge_entry(&mut results, entry, &frozen_ids, settings, now);
                }
                current_segments.clear();
            }
            current_segments.push(seg);
        }
    }

    // Process leftover open entry
    if !current_segments.is_empty() {
        if let Some(mut entry) = create_entry_from_segments(&current_segments, now, &mut claimed) {
            let last_seg_end = current_segments
                .last()
                .and_then(|s| s.ended_at)
                .unwrap_or(now);
            let is_active = current_segments.iter().any(|s| s.ended_at.is_none())
                || now.saturating_sub(last_seg_end) <= settings.short_break_threshold_ms;

            if is_active {
                entry.status = "building".to_string();
                results.push(entry);
            } else {
                push_or_merge_entry(&mut results, entry, &frozen_ids, settings, now);
            }
        }
    }

    results.sort_by_key(|e| e.started_at);
    results
}

fn push_or_merge_entry(
    results: &mut Vec<BuiltTimeEntry>,
    entry: BuiltTimeEntry,
    frozen_ids: &HashSet<String>,
    settings: &EntrySettings,
    now: u64,
) {
    let entry_dur = entry.ended_at.saturating_sub(entry.started_at);
    if entry_dur < settings.min_duration_ms {
        if let Some(prev) = results.last_mut() {
            let gap = entry.started_at.saturating_sub(prev.ended_at);
            if gap <= settings.short_break_threshold_ms && !frozen_ids.contains(&prev.id) {
                // Merge into previous unfrozen entry
                prev.ended_at = entry.ended_at;
                prev.segment_ids.extend(entry.segment_ids);
                prev.updated_at = now;
                return;
            }
        }
    }
    results.push(entry);
}

fn absorb_micro_segments(
    segments: &[SegmentInput],
    threshold_ms: u64,
    now: u64,
) -> Vec<SegmentInput> {
    if segments.len() <= 1 {
        return segments.to_vec();
    }

    let mut result: Vec<SegmentInput> = Vec::new();

    for seg in segments {
        let dur = seg.ended_at.unwrap_or(now).saturating_sub(seg.started_at);
        // Only absorb micro activity segments into previous activity segments
        if dur < threshold_ms && seg.kind != "break" && !result.is_empty() {
            let prev = result.last_mut().unwrap();
            if prev.kind != "break" {
                let seg_end = seg.ended_at.unwrap_or(now);
                if seg_end > prev.ended_at.unwrap_or(now) {
                    prev.ended_at = Some(seg_end);
                }
                continue;
            }
        }
        result.push(seg.clone());
    }

    result
}

fn create_entry_from_segments(
    segments: &[SegmentInput],
    now: u64,
    claimed: &mut HashSet<String>,
) -> Option<BuiltTimeEntry> {
    if segments.is_empty() {
        return None;
    }

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
    let ended_at = segments.last().unwrap().ended_at.unwrap_or(now);
    let segment_ids: Vec<i64> = segments.iter().map(|s| s.id).collect();

    // Summarize dominant app & titles for description
    let description = generate_description(segments);

    Some(BuiltTimeEntry {
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
    })
}

fn generate_description(segments: &[SegmentInput]) -> String {
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

    #[test]
    fn absorbs_micro_segments_under_10s() {
        let segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 60_000),
            make_seg(2, "Finder", "Open", "activity", 60_000, 65_000), // 5s alt-tab micro-segment
            make_seg(3, "Xcode", "main.rs", "activity", 65_000, 120_000),
        ];

        let settings = EntrySettings::default();
        let entries = build_entries(&segs, &[], &settings, 120_000);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].started_at, 0);
        assert_eq!(entries[0].ended_at, 120_000);
        assert_eq!(entries[0].segment_ids, vec![1, 3]);
    }

    #[test]
    fn closes_entry_on_idle_break() {
        let segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 600_000), // 10 min
            make_seg(2, "Break", "Idle", "break", 600_000, 900_000), // 5 min break
            make_seg(3, "Xcode", "main.rs", "activity", 900_000, 1_500_000), // 10 min
        ];

        let settings = EntrySettings::default();
        let entries = build_entries(&segs, &[], &settings, 1_500_000);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].started_at, 0);
        assert_eq!(entries[0].ended_at, 600_000);
        assert_eq!(entries[1].started_at, 900_000);
        assert_eq!(entries[1].ended_at, 1_500_000);
    }

    #[test]
    fn splits_on_target_duration_with_app_switch() {
        let segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 1_900_000), // ~31 min
            make_seg(2, "Safari", "docs.rs", "activity", 1_900_000, 2_600_000), // ~11 min
        ];

        let settings = EntrySettings::default();
        let entries = build_entries(&segs, &[], &settings, 2_600_000);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].started_at, 0);
        assert_eq!(entries[0].ended_at, 1_900_000);
        assert_eq!(entries[1].started_at, 1_900_000);
        assert_eq!(entries[1].ended_at, 2_600_000);
    }

    #[test]
    fn rebuilding_keeps_the_ids_segments_were_linked_to() {
        let mut segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 600_000),
            make_seg(2, "Break", "Idle", "break", 600_000, 900_000),
            make_seg(3, "Slack", "chat", "activity", 900_000, 1_200_000),
        ];
        let settings = EntrySettings::default();
        let first = build_entries(&segs, &[], &settings, 1_200_000);
        segs[0].entry_id = Some(first[0].id.clone());
        segs[2].entry_id = Some(first[1].id.clone());

        let second = build_entries(&segs, &[], &settings, 1_300_000);
        let ids =
            |entries: &[BuiltTimeEntry]| entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&first), ids(&second));
    }

    #[test]
    fn a_leftover_never_merges_into_a_frozen_entry() {
        let frozen = BuiltTimeEntry {
            id: "closed".to_string(),
            started_at: 0,
            ended_at: 600_000,
            description: "Closed".to_string(),
            category_id: None,
            project_id: None,
            status: "pending".to_string(),
            approved_by: None,
            source: "auto".to_string(),
            billable: false,
            invoice_id: None,
            created_at: 0,
            updated_at: 0,
            deleted_at: None,
            segment_ids: vec![],
        };
        let segs = vec![make_seg(2, "Slack", "chat", "activity", 660_000, 720_000)];
        let entries = build_entries(
            &segs,
            std::slice::from_ref(&frozen),
            &EntrySettings::default(),
            720_000,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].ended_at, 600_000);
    }

    #[test]
    fn preserves_frozen_approved_entries() {
        let frozen = BuiltTimeEntry {
            id: "frozen-1".to_string(),
            started_at: 0,
            ended_at: 1_800_000,
            description: "Frozen work".to_string(),
            category_id: Some("cat-1".to_string()),
            project_id: Some("proj-1".to_string()),
            status: "approved".to_string(),
            approved_by: Some("user".to_string()),
            source: "manual".to_string(),
            billable: true,
            invoice_id: None,
            created_at: 0,
            updated_at: 0,
            deleted_at: None,
            segment_ids: vec![1],
        };

        let segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 1_800_000),
            make_seg(2, "Slack", "chat", "activity", 1_800_000, 2_400_000),
        ];

        let settings = EntrySettings::default();
        let entries = build_entries(&segs, std::slice::from_ref(&frozen), &settings, 2_400_000);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "frozen-1");
        assert_eq!(entries[0].status, "approved");
        assert_eq!(entries[1].started_at, 1_800_000);
    }

    #[test]
    fn short_break_under_2_minutes_does_not_split_entry() {
        let segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 300_000), // 5 min
            make_seg(2, "Break", "Idle", "break", 300_000, 360_000), // 1 min break
            make_seg(3, "Xcode", "main.rs", "activity", 360_000, 600_000), // 4 min
        ];

        let settings = EntrySettings::default();
        let entries = build_entries(&segs, &[], &settings, 600_000);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].started_at, 0);
        assert_eq!(entries[0].ended_at, 600_000);
        assert_eq!(entries[0].segment_ids, vec![1, 3]);
    }

    #[test]
    fn short_gap_between_segments_does_not_split_entry() {
        let segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 300_000), // 5 min
            // 45s uncaptured gap between 300_000 and 345_000
            make_seg(2, "Safari", "docs.rs", "activity", 345_000, 600_000),
        ];

        let settings = EntrySettings::default();
        let entries = build_entries(&segs, &[], &settings, 600_000);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].started_at, 0);
        assert_eq!(entries[0].ended_at, 600_000);
    }

    #[test]
    fn rapid_app_switching_stays_in_single_entry() {
        let segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 30_000),
            make_seg(2, "Terminal", "cargo test", "activity", 30_000, 60_000),
            make_seg(3, "Safari", "docs.rs", "activity", 60_000, 90_000),
            make_seg(4, "Slack", "#dev", "activity", 90_000, 120_000),
            make_seg(5, "Xcode", "main.rs", "activity", 120_000, 180_000),
        ];

        let settings = EntrySettings::default();
        let entries = build_entries(&segs, &[], &settings, 180_000);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].started_at, 0);
        assert_eq!(entries[0].ended_at, 180_000);
        assert_eq!(entries[0].status, "building");
    }

    #[test]
    fn active_session_stays_building_when_recent() {
        let segs = vec![
            make_seg(1, "Xcode", "main.rs", "activity", 0, 120_000),
            make_seg(2, "Terminal", "cargo check", "activity", 120_000, 180_000),
        ];

        let settings = EntrySettings::default();
        // now is only 5s after the last segment ended
        let entries = build_entries(&segs, &[], &settings, 185_000);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "building");

        // now is 10 minutes later with no new activity -> marked pending
        let closed_entries = build_entries(&segs, &[], &settings, 780_000);
        assert_eq!(closed_entries.len(), 1);
        assert_eq!(closed_entries[0].status, "pending");
    }
}
