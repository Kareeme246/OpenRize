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
    /// manual | suggested | app (an Apps-page default)
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
        let origin = if self.origin == "app" {
            "Apps default"
        } else {
            "rule"
        };
        format!("{origin}: {kind} {}", self.pattern)
    }

    fn matches(&self, segment: &ActivitySegment) -> bool {
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
                let path = expand_home(pattern);
                segment
                    .url
                    .as_deref()
                    .map(|url| url.strip_prefix("file://").unwrap_or(url))
                    .is_some_and(|url| url.starts_with(&path))
                    || segment.title.contains(&path)
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

fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => path.to_string(),
    }
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
}
