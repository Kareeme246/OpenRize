//! The arbiter: turns tier outputs into one value and one confidence per
//! field, plus ranked alternatives and the signals behind them.
//!
//! - A T0 rule hit is p = 1.0, and we're done.
//! - Otherwise each available tier contributes a distribution over labels:
//!   kNN vote share (similarity-weighted, shrunk toward "unsure" when few
//!   neighbors exist), the personal classifier's probabilities, the LLM's
//!   vote share across samples, and (projects only) how much of the block's
//!   titles and URLs mention each project.
//! - `raw = Σ w·p / Σ w` over the tiers that fired. The LLM/kNN/personal
//!   weights move from the LLM toward the personal tiers as labeled examples
//!   accumulate: 0.7/0.2/0.1 at cold start, 0.3/0.35/0.35 at 500+ labels.
//! - Until `COLD_START_OUTCOMES` suggestions have a user outcome, displayed
//!   confidence is capped at `COLD_START_CAP`. Isotonic calibration on the
//!   outcome history (P4) replaces the cap once it exists.

use std::collections::HashMap;

use super::rules::RuleHit;
use super::{Field, COLD_START_CAP, COLD_START_OUTCOMES};
use crate::models::{Dominant, Signal};

/// A field value. `None` is "No project" on the project field; the
/// category field never uses it.
pub type Label = Option<String>;

/// Pseudo-count of "unsure" mass added to the kNN vote, in similarity
/// units: two perfect neighbors give 2/3, not 100%.
const KNN_PRIOR: f64 = 1.0;
const MENTION_WEIGHT: f64 = 0.25;
const MAX_ALTERNATIVES: usize = 8;

#[derive(Debug, Clone, Default)]
pub struct Votes {
    pub counts: HashMap<Label, u32>,
    pub samples: u32,
}

impl Votes {
    pub fn top(&self) -> Option<&Label> {
        top_label(self.counts.iter().map(|(label, count)| (label, *count as f64)))
    }
}

pub struct Evidence<'a> {
    pub field: Field,
    pub rule: Option<&'a RuleHit>,
    /// The field's label on each nearest neighbor, with its similarity.
    pub neighbors: Vec<(Label, f32)>,
    pub personal: Option<HashMap<Label, f64>>,
    pub llm: Option<Votes>,
    /// Project field only: share of active time mentioning each project.
    pub mentions: HashMap<Label, f64>,
    /// Approved, labeled entries so far (drives the weight schedule).
    pub labeled: u32,
    /// Suggestions with a user outcome so far (drives the cold-start cap).
    pub outcomes: u32,
    pub dominant: Option<&'a Dominant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub value: Label,
    pub confidence: f64,
    pub alternatives: Vec<(Label, f64)>,
    pub signals: Vec<Signal>,
    /// rules | full | fallback
    pub engine: &'static str,
}

/// LLM / kNN / personal weights for the given number of labeled examples.
pub fn weights(labeled: u32) -> (f64, f64, f64) {
    let t = (labeled as f64 / 500.0).min(1.0);
    (0.7 - 0.4 * t, 0.2 + 0.15 * t, 0.1 + 0.25 * t)
}

/// The fast tiers' pick, used to decide whether the LLM needs extra samples.
pub fn t1_top(
    neighbors: &[(Label, f32)],
    personal: Option<&HashMap<Label, f64>>,
    labeled: u32,
) -> Option<Label> {
    let (_, w_knn, w_personal) = weights(labeled);
    let mut tiers = Vec::new();
    if !neighbors.is_empty() {
        tiers.push((w_knn, knn_distribution(neighbors)));
    }
    if let Some(personal) = personal.filter(|p| !p.is_empty()) {
        tiers.push((w_personal, personal.clone()));
    }
    let blended = blend(&tiers);
    top_label(blended.iter().map(|(label, p)| (label, *p))).cloned()
}

pub fn decide(evidence: &Evidence<'_>, name: &dyn Fn(&Label) -> String) -> Option<Decision> {
    let (w_llm, w_knn, w_personal) = weights(evidence.labeled);
    let mut tiers: Vec<(f64, HashMap<Label, f64>)> = Vec::new();
    if !evidence.neighbors.is_empty() {
        tiers.push((w_knn, knn_distribution(&evidence.neighbors)));
    }
    if let Some(personal) = evidence.personal.as_ref().filter(|p| !p.is_empty()) {
        tiers.push((w_personal, personal.clone()));
    }
    if let Some(llm) = evidence.llm.as_ref().filter(|v| v.samples > 0) {
        let share = llm
            .counts
            .iter()
            .map(|(label, count)| (label.clone(), *count as f64 / llm.samples as f64))
            .collect();
        tiers.push((w_llm, share));
    }
    if !evidence.mentions.is_empty() {
        tiers.push((MENTION_WEIGHT, evidence.mentions.clone()));
    }
    let mut blended = blend(&tiers);
    if evidence.field == Field::Category {
        blended.remove(&None);
    }

    let cap = |p: f64| {
        if evidence.outcomes < COLD_START_OUTCOMES {
            p.min(COLD_START_CAP)
        } else {
            p
        }
    };

    let (value, confidence, engine) = if let Some(hit) = evidence.rule {
        (hit.value.clone(), 1.0, "rules")
    } else {
        let top = top_label(blended.iter().map(|(label, p)| (label, *p)))?.clone();
        let p = blended.get(&top).copied().unwrap_or(0.0);
        let engine = if evidence.llm.is_some() {
            "full"
        } else {
            "fallback"
        };
        (top, cap(p), engine)
    };

    let mut alternatives: Vec<(Label, f64)> = blended
        .iter()
        .filter(|(label, p)| **label != value && **p >= 0.005)
        .map(|(label, p)| (label.clone(), cap(*p)))
        .collect();
    alternatives.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    alternatives.truncate(MAX_ALTERNATIVES);

    let signals = signals(evidence, &value, name);
    Some(Decision {
        value,
        confidence,
        alternatives,
        signals,
        engine,
    })
}

/// The "Why" line: only the signals that fired, in a fixed order.
fn signals(evidence: &Evidence<'_>, value: &Label, name: &dyn Fn(&Label) -> String) -> Vec<Signal> {
    let mut out = Vec::new();
    let mut push = |kind: &str, text: String| {
        out.push(Signal {
            kind: kind.to_string(),
            text,
        })
    };

    if evidence.field == Field::Category {
        if let Some(dominant) = evidence.dominant {
            push(
                "dominant",
                format!(
                    "{}% of this block was in {}",
                    (dominant.share * 100.0).round(),
                    dominant.label
                ),
            );
        }
    }

    if let Some(hit) = evidence.rule {
        push(
            "rule",
            format!(
                "{} covers {}% of the block",
                hit.rule.describe(),
                (hit.coverage * 100.0).round()
            ),
        );
        return out;
    }

    if let Some(share) = evidence.mentions.get(value).filter(|share| **share > 0.0) {
        push(
            "mention",
            format!(
                "titles mention {} ({}% of the time)",
                name(value),
                (share * 100.0).round()
            ),
        );
    }

    if !evidence.neighbors.is_empty() {
        let agreeing = evidence
            .neighbors
            .iter()
            .filter(|(label, _)| label == value)
            .count();
        let total = evidence.neighbors.len();
        if agreeing > 0 {
            let what = match (evidence.field, value) {
                (Field::Project, None) => "had no project".to_string(),
                _ => format!("were {}", name(value)),
            };
            push(
                "knn",
                format!("{agreeing} of your {total} most similar entries {what}"),
            );
        }
    }

    if let Some(personal) = evidence.personal.as_ref().filter(|p| !p.is_empty()) {
        let personal_top = top_label(personal.iter().map(|(label, p)| (label, *p)));
        if personal_top == Some(value) {
            let p = personal.get(value).copied().unwrap_or(0.0);
            push(
                "personal",
                format!(
                    "your personal model agrees ({}%)",
                    (p * 100.0).round()
                ),
            );
        }
    }

    if let Some(llm) = evidence.llm.as_ref().filter(|v| v.samples > 0) {
        let count = llm.counts.get(value).copied().unwrap_or(0);
        let text = if count == llm.samples && llm.samples == 1 {
            "the on-device model agrees".to_string()
        } else if count == llm.samples {
            format!("the on-device model agrees ({count} of {count} tries)")
        } else if count > 0 {
            format!(
                "the on-device model picked it in {count} of {} tries",
                llm.samples
            )
        } else {
            match llm.top() {
                Some(other) => format!("the on-device model suggested {}", name(other)),
                None => String::new(),
            }
        };
        if !text.is_empty() {
            push("llm", text);
        }
    }

    out
}

fn knn_distribution(neighbors: &[(Label, f32)]) -> HashMap<Label, f64> {
    let total: f64 = neighbors.iter().map(|(_, s)| f64::from(s.max(0.0))).sum();
    let mut out: HashMap<Label, f64> = HashMap::new();
    for (label, similarity) in neighbors {
        *out.entry(label.clone()).or_default() += f64::from(similarity.max(0.0));
    }
    for value in out.values_mut() {
        *value /= total + KNN_PRIOR;
    }
    out
}

fn blend(tiers: &[(f64, HashMap<Label, f64>)]) -> HashMap<Label, f64> {
    let total_weight: f64 = tiers.iter().map(|(w, _)| w).sum();
    let mut out: HashMap<Label, f64> = HashMap::new();
    if total_weight <= 0.0 {
        return out;
    }
    for (weight, distribution) in tiers {
        for (label, p) in distribution {
            *out.entry(label.clone()).or_default() += weight * p / total_weight;
        }
    }
    out
}

/// Highest score wins; ties break on the label so results are deterministic.
fn top_label<'a>(scores: impl Iterator<Item = (&'a Label, f64)>) -> Option<&'a Label> {
    scores
        .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(label, _)| label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::rules::Rule;

    fn l(value: &str) -> Label {
        Some(value.to_string())
    }

    fn name(label: &Label) -> String {
        label.clone().unwrap_or_else(|| "No project".to_string())
    }

    fn evidence<'a>() -> Evidence<'a> {
        Evidence {
            field: Field::Category,
            rule: None,
            neighbors: Vec::new(),
            personal: None,
            llm: None,
            mentions: HashMap::new(),
            labeled: 0,
            outcomes: 0,
            dominant: None,
        }
    }

    #[test]
    fn a_rule_hit_is_certain_even_at_cold_start() {
        let hit = RuleHit {
            value: l("design"),
            rule: Rule {
                id: "r".into(),
                match_kind: "domain".into(),
                pattern: "figma.com".into(),
                category_id: l("design"),
                project_id: None,
                priority: 0,
                origin: "manual".into(),
            },
            coverage: 0.8,
        };
        let e = Evidence {
            rule: Some(&hit),
            neighbors: vec![(l("coding"), 0.9)],
            ..evidence()
        };
        let decision = decide(&e, &name).unwrap();
        assert_eq!(decision.value, l("design"));
        assert_eq!(decision.confidence, 1.0);
        assert_eq!(decision.engine, "rules");
        assert_eq!(decision.alternatives[0].0, l("coding"));
        assert!(decision.signals[0].text.contains("figma.com"));
    }

    #[test]
    fn cold_start_caps_confidence_until_enough_outcomes() {
        let llm = Votes {
            counts: HashMap::from([(l("coding"), 3)]),
            samples: 3,
        };
        let e = Evidence {
            llm: Some(llm.clone()),
            ..evidence()
        };
        assert_eq!(decide(&e, &name).unwrap().confidence, COLD_START_CAP);

        let calibrated = Evidence {
            llm: Some(llm),
            outcomes: COLD_START_OUTCOMES,
            ..evidence()
        };
        let decision = decide(&calibrated, &name).unwrap();
        assert_eq!(decision.confidence, 1.0);
        assert_eq!(decision.engine, "full");
        assert_eq!(decision.signals[0].text, "the on-device model agrees (3 of 3 tries)");
    }

    #[test]
    fn knn_votes_are_shrunk_toward_unsure() {
        let e = Evidence {
            neighbors: vec![(l("coding"), 1.0), (l("coding"), 1.0)],
            outcomes: COLD_START_OUTCOMES,
            ..evidence()
        };
        let decision = decide(&e, &name).unwrap();
        assert!((decision.confidence - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(decision.engine, "fallback");
        assert_eq!(
            decision.signals[0].text,
            "2 of your 2 most similar entries were coding"
        );
    }

    #[test]
    fn tiers_blend_by_the_weight_schedule() {
        let e = Evidence {
            neighbors: vec![(l("research"), 1.0)],
            llm: Some(Votes {
                counts: HashMap::from([(l("coding"), 1)]),
                samples: 1,
            }),
            outcomes: COLD_START_OUTCOMES,
            ..evidence()
        };
        let decision = decide(&e, &name).unwrap();
        // Cold-start weights: LLM 0.7, kNN 0.2 (renormalized over 0.9).
        assert_eq!(decision.value, l("coding"));
        assert!((decision.confidence - 0.7 / 0.9).abs() < 1e-9);
        let (alt, p) = &decision.alternatives[0];
        assert_eq!(*alt, l("research"));
        assert!((p - 0.2 * 0.5 / 0.9).abs() < 1e-9);
    }

    #[test]
    fn no_project_is_a_real_answer() {
        let e = Evidence {
            field: Field::Project,
            neighbors: vec![(None, 0.9), (None, 0.8), (l("openrize"), 0.4)],
            ..evidence()
        };
        let decision = decide(&e, &name).unwrap();
        assert_eq!(decision.value, None);
        assert_eq!(
            decision.signals[0].text,
            "2 of your 3 most similar entries had no project"
        );
    }

    #[test]
    fn nothing_to_go_on_means_no_suggestion() {
        assert!(decide(&evidence(), &name).is_none());
    }

    #[test]
    fn weights_shift_toward_personal_tiers() {
        assert_eq!(weights(0), (0.7, 0.2, 0.1));
        let (llm, knn, personal) = weights(500);
        assert!((llm - 0.3).abs() < 1e-9 && (knn - 0.35).abs() < 1e-9);
        assert!((personal - 0.35).abs() < 1e-9);
    }
}
