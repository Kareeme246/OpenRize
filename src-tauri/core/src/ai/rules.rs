//! Tier 0: deterministic rules. A hit sets the field at confidence 1.0.
//!
//! Rules come from the `rules` table (manual, or accepted rule suggestions)
//! and from the Apps page, where an app's or site's default category or
//! project behaves as an `app`/`domain` rule. Each segment takes the most
//! specific matching rule (path/url prefix beats title, title beats domain,
//! domain beats app; then priority). A rule decides the entry only when the
//! time it covers is at least `HIT_COVERAGE` of the entry's active time, so
//! five minutes of Figma inside a coding block doesn't make it Design.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::RegexBuilder;

use super::Field;
use crate::activity::{ActivitySegment, KIND_BREAK};

pub const HIT_COVERAGE: f64 = 0.6;

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: String,
    /// app | domain | title_contains | title_regex | url_prefix | path_prefix
    pub match_kind: String,
    pub pattern: String,
    pub category_id: Option<String>,
    pub project_id: Option<String>,
    pub priority: i64,
    /// manual | suggested | hint (from a project's AI hints) | app (an
    /// Apps-page default)
    pub origin: String,
}

impl Rule {
    fn value(&self, field: Field) -> Option<&String> {
        match field {
            Field::Category => self.category_id.as_ref(),
            Field::Project => self.project_id.as_ref(),
        }
    }

    fn specificity(&self) -> u8 {
        match self.match_kind.as_str() {
            "path_prefix" | "url_prefix" => 4,
            "title_regex" | "title_contains" => 3,
            "domain" => 2,
            _ => 1,
        }
    }

    /// Short description for the "Why" line, e.g. `domain figma.com`.
    pub fn describe(&self) -> String {
        let kind = match self.match_kind.as_str() {
            "title_contains" | "title_regex" => "title",
            "url_prefix" => "url",
            "path_prefix" => "path",
            other => other,
        };
        let origin = match self.origin.as_str() {
            "app" => "Apps default",
            "hint" => "project hint",
            _ => "rule",
        };
        format!("{origin}: {kind} {}", self.pattern)
    }

    pub fn matches(&self, segment: &ActivitySegment) -> bool {
        let pattern = self.pattern.trim();
        if pattern.is_empty() {
            return false;
        }
        match self.match_kind.as_str() {
            "app" => {
                segment
                    .bundle_id
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(pattern))
                    || segment.app.eq_ignore_ascii_case(pattern)
            }
            "domain" => segment.domain.as_deref().is_some_and(|domain| {
                let domain = domain.to_ascii_lowercase();
                let pattern = pattern.to_ascii_lowercase();
                domain == pattern || domain.ends_with(&format!(".{pattern}"))
            }),
            "title_contains" => segment
                .title
                .to_lowercase()
                .contains(&pattern.to_lowercase()),
            "title_regex" => RegexBuilder::new(pattern)
                .case_insensitive(true)
                .size_limit(1 << 20)
                .build()
                .is_ok_and(|regex| regex.is_match(&segment.title)),
            "url_prefix" => segment
                .url
                .as_deref()
                .is_some_and(|url| strip_scheme(url).starts_with(strip_scheme(pattern))),
            "path_prefix" => {
                path_prefix_matches(segment, pattern, home_dir().as_deref(), cfg!(windows))
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleHit {
    pub value: Option<String>,
    pub rule: Rule,
    pub coverage: f64,
}

/// The rule outcome for one field, or `None` when no value covers enough of
/// the entry.
pub fn evaluate(rules: &[Rule], segments: &[ActivitySegment], field: Field) -> Option<RuleHit> {
    let candidates: Vec<&Rule> = rules.iter().filter(|r| r.value(field).is_some()).collect();
    if candidates.is_empty() {
        return None;
    }

    let mut active_ms = 0u64;
    // value -> (covered ms, the rule that covered the most of it)
    let mut covered: HashMap<String, (u64, HashMap<String, u64>)> = HashMap::new();
    for segment in segments.iter().filter(|s| s.kind != KIND_BREAK) {
        let ms = segment
            .ended_at
            .unwrap_or(segment.started_at)
            .saturating_sub(segment.started_at);
        active_ms += ms;
        let best = candidates
            .iter()
            .filter(|rule| rule.matches(segment))
            .max_by_key(|rule| (rule.specificity(), rule.priority));
        if let Some(rule) = best {
            let value = rule.value(field).cloned().unwrap_or_default();
            let slot = covered.entry(value).or_default();
            slot.0 += ms;
            *slot.1.entry(rule.id.clone()).or_default() += ms;
        }
    }
    if active_ms == 0 {
        return None;
    }

    let (value, (ms, by_rule)) = covered.into_iter().max_by_key(|(_, (ms, _))| *ms)?;
    let coverage = ms as f64 / active_ms as f64;
    if coverage < HIT_COVERAGE {
        return None;
    }
    let rule_id = by_rule.into_iter().max_by_key(|(_, ms)| *ms)?.0;
    let rule = candidates.into_iter().find(|r| r.id == rule_id)?.clone();
    Some(RuleHit {
        value: Some(value),
        rule,
        coverage,
    })
}

fn strip_scheme(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.strip_prefix("www.").unwrap_or(rest)
}

/// The user's home folder, which `~` in a path rule means. Windows has no
/// `HOME` outside shells like Git Bash, so it asks for the profile folder
/// (`%USERPROFILE%`) instead.
fn home_dir() -> Option<String> {
    if cfg!(windows) {
        static PROFILE: LazyLock<Option<String>> =
            LazyLock::new(|| dirs::home_dir().map(|home| home.to_string_lossy().into_owned()));
        PROFILE.clone()
    } else {
        std::env::var("HOME").ok()
    }
}

fn expand_home(path: &str, home: Option<&str>) -> String {
    match (
        path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")),
        home,
    ) {
        (Some(rest), Some(home)) => format!("{home}/{rest}"),
        _ => path.to_string(),
    }
}

/// Whether a segment's file URL starts with the rule's folder, or its window
/// title (a terminal's working directory, an editor's path) names it. Windows
/// paths are case-insensitive and may be written with either separator, so
/// there both sides are compared lowercased with `/` throughout.
fn path_prefix_matches(
    segment: &ActivitySegment,
    pattern: &str,
    home: Option<&str>,
    windows: bool,
) -> bool {
    let fold = |text: &str| {
        if windows {
            text.replace('\\', "/").to_lowercase()
        } else {
            text.to_string()
        }
    };
    let path = fold(&expand_home(pattern, home));
    let pattern = fold(pattern);
    let title = fold(&segment.title);
    let url_path = segment.url.as_deref().map(|url| {
        let url = fold(url.strip_prefix("file://").unwrap_or(url));
        // `file:///C:/Code` leaves `/C:/Code`; the drive is the path's start.
        match url.as_bytes() {
            [b'/', drive, b':', ..] if windows && drive.is_ascii_alphabetic() => url[1..].into(),
            _ => url,
        }
    });
    url_path.is_some_and(|url| url.starts_with(&path))
        || title.contains(&path)
        // Terminals show the working directory as `~/Code/...`.
        || title.contains(&pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, kind: &str, pattern: &str, category: Option<&str>) -> Rule {
        Rule {
            id: id.to_string(),
            match_kind: kind.to_string(),
            pattern: pattern.to_string(),
            category_id: category.map(str::to_string),
            project_id: None,
            priority: 0,
            origin: "manual".to_string(),
        }
    }

    fn seg(app: &str, title: &str, domain: Option<&str>, ms: u64) -> ActivitySegment {
        ActivitySegment {
            id: 0,
            app: app.to_string(),
            title: title.to_string(),
            kind: "activity".to_string(),
            label: None,
            started_at: 0,
            ended_at: Some(ms),
            reviewed: false,
            app_id: None,
            bundle_id: None,
            url: domain.map(|d| format!("https://{d}/file")),
            domain: domain.map(str::to_string),
            entry_id: None,
        }
    }

    #[test]
    fn a_rule_must_cover_most_of_the_entry() {
        let rules = vec![rule("r1", "domain", "figma.com", Some("design"))];
        let mostly_code = vec![
            seg("Xcode", "main.rs", None, 800),
            seg("Safari", "Figma", Some("figma.com"), 200),
        ];
        assert!(evaluate(&rules, &mostly_code, Field::Category).is_none());

        let mostly_figma = vec![
            seg("Xcode", "main.rs", None, 200),
            seg("Safari", "Figma", Some("www.figma.com"), 800),
        ];
        let hit = evaluate(&rules, &mostly_figma, Field::Category).unwrap();
        assert_eq!(hit.value.as_deref(), Some("design"));
        assert!((hit.coverage - 0.8).abs() < 1e-9);
    }

    #[test]
    fn the_most_specific_rule_wins_per_segment() {
        let rules = vec![
            rule("app", "app", "Safari", Some("research")),
            rule("title", "title_contains", "OpenRize", Some("coding")),
        ];
        let segments = vec![seg("Safari", "Kareeme246/OpenRize: PR #12", None, 1000)];
        let hit = evaluate(&rules, &segments, Field::Category).unwrap();
        assert_eq!(hit.value.as_deref(), Some("coding"));
        assert_eq!(hit.rule.id, "title");
    }

    #[test]
    fn rules_only_answer_the_field_they_set() {
        let rules = vec![rule("r1", "app", "Xcode", Some("coding"))];
        let segments = vec![seg("Xcode", "main.rs", None, 1000)];
        assert!(evaluate(&rules, &segments, Field::Project).is_none());
    }

    const PROFILE: &str = r"C:\Users\Kareem";

    #[test]
    fn tilde_expands_to_the_profile_with_either_separator() {
        assert_eq!(
            expand_home("~/Code/app", Some(PROFILE)),
            r"C:\Users\Kareem/Code/app"
        );
        assert_eq!(
            expand_home(r"~\Code\app", Some(PROFILE)),
            r"C:\Users\Kareem/Code\app"
        );
        assert_eq!(expand_home("~/Code", None), "~/Code");
        assert_eq!(expand_home("/srv/app", Some(PROFILE)), "/srv/app");
    }

    #[test]
    fn windows_path_rules_match_titles_and_file_urls_in_any_spelling() {
        let terminal = seg(
            "WindowsTerminal.exe",
            r"PowerShell - C:\Users\Kareem\Code\proj\src",
            None,
            1,
        );
        let prompt = seg("WindowsTerminal.exe", r"~\Code\proj", None, 1);
        let lower = seg("cmd.exe", r"c:/users/kareem/code/proj", None, 1);
        let mut file = seg("Code.exe", "main.rs - proj - Visual Studio Code", None, 1);
        file.url = Some("file:///C:/Users/Kareem/Code/proj/main.rs".to_string());
        let other = seg(
            "WindowsTerminal.exe",
            r"PowerShell - C:\Users\Kareem\Code\other",
            None,
            1,
        );

        for pattern in [
            "~/Code/proj",
            r"~\Code\proj",
            r"C:\Users\Kareem\Code\proj",
            "C:/users/kareem/code/proj",
        ] {
            for matching in [&terminal, &prompt, &lower, &file] {
                // A prompt that abbreviates the profile as `~` only names the
                // folder to a rule written with `~`.
                if pattern.starts_with("C:") && std::ptr::eq(matching, &prompt) {
                    continue;
                }
                assert!(
                    path_prefix_matches(matching, pattern, Some(PROFILE), true),
                    "{pattern} should match {:?}",
                    matching.title
                );
            }
            assert!(!path_prefix_matches(&other, pattern, Some(PROFILE), true));
        }
    }

    #[test]
    fn other_platforms_keep_paths_exact() {
        let title = seg("Terminal", "~/Code/proj", None, 1);
        assert!(path_prefix_matches(
            &title,
            "~/Code/proj",
            Some("/Users/k"),
            false
        ));
        // Case and backslashes mean something there.
        assert!(!path_prefix_matches(
            &title,
            "~/code/proj",
            Some("/Users/k"),
            false
        ));
        let expanded = seg("Terminal", "k@mac: /Users/k/Code/proj", None, 1);
        assert!(path_prefix_matches(
            &expanded,
            "~/Code/proj",
            Some("/Users/k"),
            false
        ));
        assert!(!path_prefix_matches(
            &expanded,
            "~/code/proj",
            Some("/Users/k"),
            false
        ));
    }
}
