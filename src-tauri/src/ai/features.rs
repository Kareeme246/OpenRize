//! Feature extraction: the one deterministic rendering of an entry's
//! segments that every tier reads.
//!
//! ```text
//! Tue 10:15–10:48 (33 min)
//! Xcode 78%: "activity.rs - OpenRize", "commands.rs - OpenRize"
//! Safari 22% docs.rs: "AppHandle in tauri - Rust"
//! Context: previous entry = Coding (09:38–10:15)
//! ```
//!
//! Two renderings come out of it. `text` (above) goes to the Foundation
//! Model. `content` drops the time and context lines, so two blocks of the
//! same work embed close together whatever the hour; it feeds NLEmbedding,
//! kNN, and the personal classifier.

use std::collections::HashMap;

use chrono::{Local, TimeZone};

use crate::activity::{ActivitySegment, KIND_BREAK};
use crate::models::Dominant;

/// About 700 tokens, the entry's share of the model's 4,096-token context.
const MAX_TEXT_CHARS: usize = 2_800;
const MAX_APPS: usize = 6;
const MAX_TITLES_PER_APP: usize = 4;
const MAX_TITLE_CHARS: usize = 90;
const MAX_DOMAINS_PER_APP: usize = 3;

pub struct EntryFeatures {
    pub text: String,
    pub content: String,
    pub dominant: Option<Dominant>,
    pub active_ms: u64,
}

/// The previous entry, when it ended shortly before this one.
pub struct PreviousEntry<'a> {
    pub category: &'a str,
    pub started_at: u64,
    pub ended_at: u64,
}

struct AppUsage {
    name: String,
    ms: u64,
    titles: Vec<(String, u64)>,
    domains: Vec<(String, u64)>,
}

pub fn extract(
    started_at: u64,
    ended_at: u64,
    segments: &[ActivitySegment],
    previous: Option<PreviousEntry<'_>>,
) -> EntryFeatures {
    let mut apps: Vec<AppUsage> = Vec::new();
    let mut keys: HashMap<(String, String), (String, u64)> = HashMap::new();
    let mut active_ms = 0u64;

    for segment in segments.iter().filter(|s| s.kind != KIND_BREAK) {
        let end = segment.ended_at.unwrap_or(ended_at).min(ended_at);
        let ms = end.saturating_sub(segment.started_at.max(started_at));
        if ms == 0 {
            continue;
        }
        active_ms += ms;

        let index = match apps.iter().position(|app| app.name == segment.app) {
            Some(index) => index,
            None => {
                apps.push(AppUsage {
                    name: segment.app.clone(),
                    ms: 0,
                    titles: Vec::new(),
                    domains: Vec::new(),
                });
                apps.len() - 1
            }
        };
        let app = &mut apps[index];
        app.ms += ms;
        let title = segment.title.trim();
        if !title.is_empty() && title != segment.app {
            add_ms(&mut app.titles, title, ms);
        }
        if let Some(domain) = segment.domain.as_deref().filter(|d| !d.is_empty()) {
            add_ms(&mut app.domains, domain, ms);
        }

        // Rule suggestions key on a domain when there is one (figma.com is
        // more specific than "Safari"), else on the app's bundle id.
        let (kind, key, label) = match segment.domain.as_deref().filter(|d| !d.is_empty()) {
            Some(domain) => ("domain", domain.to_string(), domain.to_string()),
            None => (
                "app",
                segment
                    .bundle_id
                    .clone()
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| segment.app.clone()),
                segment.app.clone(),
            ),
        };
        keys.entry((kind.to_string(), key)).or_insert((label, 0)).1 += ms;
    }

    apps.sort_by_key(|app| std::cmp::Reverse(app.ms));
    for app in &mut apps {
        app.titles.sort_by_key(|(_, ms)| std::cmp::Reverse(*ms));
        app.domains.sort_by_key(|(_, ms)| std::cmp::Reverse(*ms));
    }

    let dominant = keys
        .into_iter()
        .max_by(|a, b| a.1 .1.cmp(&b.1 .1).then_with(|| b.0.cmp(&a.0)))
        .map(|((kind, key), (label, ms))| Dominant {
            kind,
            key,
            label,
            share: if active_ms > 0 {
                ms as f64 / active_ms as f64
            } else {
                0.0
            },
        });

    let mut content = String::new();
    for app in apps.iter().take(MAX_APPS) {
        let share = percent(app.ms, active_ms);
        let mut line = format!("{} {share}%", app.name);
        let domains: Vec<&str> = app
            .domains
            .iter()
            .take(MAX_DOMAINS_PER_APP)
            .map(|(domain, _)| domain.as_str())
            .collect();
        if !domains.is_empty() {
            line.push(' ');
            line.push_str(&domains.join(", "));
        }
        let titles: Vec<String> = app
            .titles
            .iter()
            .take(MAX_TITLES_PER_APP)
            .map(|(title, _)| format!("\"{}\"", truncate(title, MAX_TITLE_CHARS)))
            .collect();
        if !titles.is_empty() {
            line.push_str(": ");
            line.push_str(&titles.join(", "));
        }
        content.push_str(&line);
        content.push('\n');
    }
    if content.is_empty() {
        content.push_str("No app activity\n");
    }
    let content = truncate(content.trim_end(), MAX_TEXT_CHARS);

    let mut text = format!(
        "{}–{} ({} min)\n{}",
        local_time(started_at, "%a %H:%M"),
        local_time(ended_at, "%H:%M"),
        ended_at.saturating_sub(started_at) / 60_000,
        content
    );
    if let Some(previous) = previous {
        text.push_str(&format!(
            "\nContext: previous entry = {} ({}–{})",
            previous.category,
            local_time(previous.started_at, "%H:%M"),
            local_time(previous.ended_at, "%H:%M"),
        ));
    }

    EntryFeatures {
        text,
        content,
        dominant,
        active_ms,
    }
}

fn add_ms(list: &mut Vec<(String, u64)>, key: &str, ms: u64) {
    match list.iter_mut().find(|(existing, _)| existing == key) {
        Some((_, total)) => *total += ms,
        None => list.push((key.to_string(), ms)),
    }
}

fn percent(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        0
    } else {
        ((part as f64 / whole as f64) * 100.0).round() as u64
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn local_time(epoch_ms: u64, format: &str) -> String {
    Local
        .timestamp_millis_opt(epoch_ms as i64)
        .single()
        .map(|time| time.format(format).to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(app: &str, title: &str, domain: Option<&str>, start: u64, end: u64) -> ActivitySegment {
        ActivitySegment {
            id: 0,
            app: app.to_string(),
            title: title.to_string(),
            kind: "activity".to_string(),
            label: None,
            started_at: start,
            ended_at: Some(end),
            reviewed: false,
            app_id: None,
            bundle_id: Some(format!("com.example.{}", app.to_lowercase())),
            url: None,
            domain: domain.map(str::to_string),
            entry_id: None,
        }
    }

    #[test]
    fn renders_apps_by_share_with_titles_and_domains() {
        let segments = vec![
            seg("Xcode", "activity.rs - OpenRize", None, 0, 1_560_000),
            seg(
                "Safari",
                "AppHandle in tauri - Rust",
                Some("docs.rs"),
                1_560_000,
                2_000_000,
            ),
        ];
        let features = extract(0, 2_000_000, &segments, None);

        assert_eq!(
            features.content,
            "Xcode 78%: \"activity.rs - OpenRize\"\nSafari 22% docs.rs: \"AppHandle in tauri - Rust\""
        );
        assert!(features.text.contains("(33 min)"));
        let dominant = features.dominant.unwrap();
        assert_eq!(dominant.kind, "app");
        assert_eq!(dominant.key, "com.example.xcode");
        assert!((dominant.share - 0.78).abs() < 0.01);
    }

    #[test]
    fn breaks_do_not_count_as_activity() {
        let mut idle = seg("Idle", "No activity", None, 0, 600_000);
        idle.kind = KIND_BREAK.to_string();
        let segments = vec![idle, seg("Slack", "#general", None, 600_000, 900_000)];
        let features = extract(0, 900_000, &segments, None);
        assert_eq!(features.active_ms, 300_000);
        assert!(features.content.starts_with("Slack 100%"));
    }
}
