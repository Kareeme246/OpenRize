//! Feature extraction: the one deterministic rendering of an entry's segments.
//! `text` goes to the Foundation Model; `content` drops the time and context lines
//! so same-work blocks embed close together, and feeds NLEmbedding, kNN and the classifier.

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{Local, TimeZone};

use crate::activity::{ActivitySegment, KIND_BREAK};
use crate::models::Dominant;
use openrize_core::capture::PRIVATE_TITLE;

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

fn local_hostname() -> Option<String> {
    #[cfg(target_os = "macos")]
    unsafe {
        let mut buf = [0u8; 256];
        if libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) == 0 {
            let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            std::str::from_utf8(&buf[..nul]).ok().map(str::to_string)
        } else {
            None
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .ok()
    }
}

pub fn strip_host_prefix(title: &str) -> &str {
    static HOSTNAME: OnceLock<Option<String>> = OnceLock::new();
    let host = HOSTNAME.get_or_init(local_hostname);
    strip_host_prefix_with(title, host.as_deref())
}

pub fn strip_host_prefix_with<'a>(title: &'a str, host: Option<&str>) -> &'a str {
    match host {
        Some(h) if !h.is_empty() => {
            let mut prefix = String::with_capacity(h.len() + 2);
            prefix.push_str(h);
            prefix.push_str(": ");
            title.strip_prefix(&prefix).unwrap_or(title)
        }
        _ => title,
    }
}

pub fn is_tracker_bundle_id(bundle_id: &str) -> bool {
    bundle_id == openrize_core::APP_IDENTIFIER
        || bundle_id.starts_with("com.offlinestudios.openrize")
}

pub fn is_tracker_segment(segment: &ActivitySegment) -> bool {
    segment
        .bundle_id
        .as_deref()
        .is_some_and(is_tracker_bundle_id)
        || segment.app.eq_ignore_ascii_case("openrize")
}

/// A block has meaningful titles when a non-tracker segment has a domain or a window
/// title other than its app name (after stripping a terminal host prefix). A private
/// window never counts: its title must not reach an entry's description.
pub fn has_titles(segments: &[ActivitySegment]) -> bool {
    segments.iter().any(|segment| {
        if segment.kind == KIND_BREAK || segment.private || is_tracker_segment(segment) {
            return false;
        }
        if segment
            .domain
            .as_deref()
            .is_some_and(|d| !d.trim().is_empty())
        {
            return true;
        }
        let title = strip_host_prefix(segment.title.trim());
        !title.is_empty() && title != segment.app
    })
}

struct Usage {
    apps: Vec<AppUsage>,
    dominant: Option<Dominant>,
    active_ms: u64,
}

/// With `scrub_private`, a private window counts as its app alone: no title, no domain,
/// never the dominant key. Anything stored or learned from must be built this way; only
/// the one-shot model prompt may see it.
fn usage(
    started_at: u64,
    ended_at: u64,
    segments: &[ActivitySegment],
    scrub_private: bool,
) -> Usage {
    let mut apps: Vec<AppUsage> = Vec::new();
    let mut keys: HashMap<(String, String), (String, u64)> = HashMap::new();
    let mut active_ms = 0u64;

    for segment in segments
        .iter()
        .filter(|s| s.kind != KIND_BREAK && !is_tracker_segment(s))
    {
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
        // An already-erased private window reads as the marker; never a title.
        let detail = !(segment.private && (scrub_private || segment.title == PRIVATE_TITLE));
        let domain = segment
            .domain
            .as_deref()
            .filter(|d| !d.is_empty() && detail);
        let title = strip_host_prefix(segment.title.trim());
        if detail && !title.is_empty() && title != segment.app {
            add_ms(&mut app.titles, title, ms);
        }
        if let Some(domain) = domain {
            add_ms(&mut app.domains, domain, ms);
        }

        // Rule suggestions key on a domain when there is one (figma.com is
        // more specific than "Safari"), else on the app's bundle id.
        let (kind, key, label) = match domain {
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
    Usage {
        apps,
        dominant,
        active_ms,
    }
}

fn render(usage: &Usage) -> String {
    let mut content = String::new();
    for app in usage.apps.iter().take(MAX_APPS) {
        let share = percent(app.ms, usage.active_ms);
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
    truncate(content.trim_end(), MAX_TEXT_CHARS)
}

/// `text` goes to the Foundation Model once and is never stored, so it may carry a private
/// window's detail. `content` and `dominant` are stored, embedded, trained on and shown as
/// the "Why" line, so a private window is only its app there.
pub fn extract(
    started_at: u64,
    ended_at: u64,
    segments: &[ActivitySegment],
    previous: Option<PreviousEntry<'_>>,
) -> EntryFeatures {
    let kept = usage(started_at, ended_at, segments, true);
    let content = render(&kept);
    let prompt = if segments.iter().any(|s| s.private) {
        render(&usage(started_at, ended_at, segments, false))
    } else {
        content.clone()
    };

    let mut text = format!(
        "{}–{} ({} min)\n{}",
        local_time(started_at, "%a %H:%M"),
        local_time(ended_at, "%H:%M"),
        ended_at.saturating_sub(started_at) / 60_000,
        prompt
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
        dominant: kept.dominant,
        active_ms: kept.active_ms,
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
            private: false,
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

    #[test]
    fn strips_terminal_host_prefix() {
        assert_eq!(
            strip_host_prefix_with("Mac.lan: flow-state", Some("Mac.lan")),
            "flow-state"
        );
        assert_eq!(
            strip_host_prefix_with("Mac.lan: WezTerm", Some("Mac.lan")),
            "WezTerm"
        );
        assert_eq!(
            strip_host_prefix_with("Note: important task", Some("Mac.lan")),
            "Note: important task"
        );
        assert_eq!(
            strip_host_prefix_with("flow-state", Some("Mac.lan")),
            "flow-state"
        );
    }

    #[test]
    fn drops_tracker_own_window() {
        let mut tracker_seg = seg("openrize", "OpenRize", None, 900_000, 1_000_000);
        tracker_seg.bundle_id = Some(openrize_core::APP_IDENTIFIER.to_string());
        let segments = vec![seg("WezTerm", "flow-state", None, 0, 900_000), tracker_seg];
        let features = extract(0, 1_000_000, &segments, None);
        assert_eq!(features.active_ms, 900_000);
        assert!(!features.content.contains("openrize"));
        assert!(!features.content.contains("OpenRize"));
        assert!(features.content.starts_with("WezTerm 100%"));
    }

    #[test]
    fn has_titles_evaluation() {
        let mut tracker = seg("openrize", "OpenRize", None, 0, 100_000);
        tracker.bundle_id = Some(openrize_core::APP_IDENTIFIER.to_string());
        assert!(!has_titles(&[tracker]));

        let wezterm_empty = seg("WezTerm", "", None, 0, 100_000);
        assert!(!has_titles(&[wezterm_empty]));
        let wezterm_same = seg("WezTerm", "WezTerm", None, 0, 100_000);
        assert!(!has_titles(&[wezterm_same]));

        let safari_domain = seg("Safari", "Safari", Some("docs.rs"), 0, 100_000);
        assert!(has_titles(&[safari_domain]));

        let wezterm_title = seg("WezTerm", "cargo build", None, 0, 100_000);
        assert!(has_titles(&[wezterm_title]));
    }

    fn private_seg(title: &str, domain: Option<&str>, start: u64, end: u64) -> ActivitySegment {
        let mut segment = seg("Zen", title, domain, start, end);
        segment.private = true;
        segment
    }

    #[test]
    fn a_private_window_is_only_its_app_in_what_is_stored() {
        let segments = vec![
            seg("Xcode", "activity.rs - OpenRize", None, 0, 400_000),
            private_seg(
                "quarterly layoffs plan",
                Some("hr.example"),
                400_000,
                1_000_000,
            ),
        ];
        let features = extract(0, 1_000_000, &segments, None);

        assert_eq!(
            features.content,
            "Zen 60%\nXcode 40%: \"activity.rs - OpenRize\""
        );
        let dominant = features.dominant.expect("a dominant app");
        assert_eq!(
            (dominant.kind.as_str(), dominant.label.as_str()),
            ("app", "Zen")
        );
        assert!(features.text.contains("quarterly layoffs plan"));
        assert!(features.text.contains("hr.example"));
        assert_eq!(features.active_ms, 1_000_000);
    }

    #[test]
    fn a_private_window_already_erased_adds_no_marker_text() {
        let segments = vec![private_seg(PRIVATE_TITLE, None, 0, 600_000)];
        let features = extract(0, 600_000, &segments, None);
        assert_eq!(features.content, "Zen 100%");
        assert!(features.text.ends_with("\nZen 100%"));
        assert!(!has_titles(&segments));
    }

    #[test]
    fn a_private_window_never_counts_as_titled() {
        assert!(!has_titles(&[private_seg(
            "secret",
            Some("a.example"),
            0,
            1
        )]));
    }
}
