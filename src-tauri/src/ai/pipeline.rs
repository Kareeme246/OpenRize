//! Everything after the database reads in one classification. The worker and the golden-set
//! benchmark both call `label`, so the benchmark measures exactly what ships.

use std::collections::{HashMap, HashSet};

use super::arbiter::{self, Decision, Evidence, Label, Votes};
use super::calibration::Calibrator;
use super::features::{self, EntryFeatures};
use super::knn;
use super::prompt::{Choice, Example, LlmRequest};
use super::provider::Provider;
use super::rules::RuleHit;
use super::Field;
use crate::activity::{self, ActivitySegment};

/// With more choices than this, only the fast tiers' top picks are sent to
/// the model, to stay inside its 4,096-token context.
const MAX_LLM_CATEGORIES: usize = 16;
const MAX_LLM_PROJECTS: usize = 15;
const FEW_SHOT_EXAMPLES: usize = 4;

pub struct Inputs<'a> {
    pub features: &'a EntryFeatures,
    pub segments: &'a [ActivitySegment],
    pub categories: &'a [Choice],
    pub projects: &'a [Choice],
    pub want_project: bool,
    pub rule_category: Option<&'a RuleHit>,
    pub rule_project: Option<&'a RuleHit>,
    pub vector: Option<&'a [f32]>,
    pub pool: &'a [knn::Labeled],
    /// Raw personal-model predictions; dead ids are dropped here.
    pub personal_category: Option<HashMap<Label, f64>>,
    pub personal_project: Option<HashMap<Label, f64>>,
    pub labeled: u32,
    pub calibration: Option<&'a Calibrator>,
    pub custom: &'a str,
}

pub struct Labeled {
    pub category: Option<Decision>,
    pub project: Option<Decision>,
    /// Stored only when the block has titles and no private window.
    pub description: Option<String>,
    pub llm: Option<LlmTrace>,
}

#[cfg_attr(not(test), allow(dead_code))] // only the golden-set benchmark reads most fields
pub struct LlmTrace {
    pub category: Option<Votes>,
    pub project: Option<Votes>,
    pub samples: u32,
    pub latency_ms: u64,
    pub error: Option<String>,
}

pub fn label(inputs: Inputs<'_>, llm: Option<&mut dyn Provider>) -> Labeled {
    let Inputs {
        features,
        segments,
        categories,
        projects,
        want_project,
        rule_category,
        rule_project,
        vector,
        pool,
        personal_category,
        personal_project,
        labeled,
        calibration,
        custom,
    } = inputs;

    let neighbors = vector
        .map(|v| knn::nearest(v, pool, knn::K))
        .unwrap_or_default();
    let live_category_ids: HashSet<&str> = categories.iter().map(|c| c.id.as_str()).collect();
    let category_neighbors: Vec<(Label, f32)> = neighbors
        .iter()
        .filter(|n| {
            n.item
                .category_id
                .as_deref()
                .is_some_and(|id| live_category_ids.contains(id))
        })
        .map(|n| (n.item.category_id.clone(), n.similarity))
        .collect();

    let live_project_ids: HashSet<&str> = projects.iter().map(|c| c.id.as_str()).collect();
    let project_neighbors: Vec<(Label, f32)> = neighbors
        .iter()
        .filter(|n| {
            n.item
                .project_id
                .as_deref()
                .is_some_and(|id| live_project_ids.contains(id))
        })
        .map(|n| (n.item.project_id.clone(), n.similarity))
        .collect();

    let live_only = |dist: HashMap<Label, f64>, live: &HashSet<&str>| {
        Some(
            dist.into_iter()
                .filter(|(label, _)| label.as_deref().is_some_and(|id| live.contains(id)))
                .collect::<HashMap<Label, f64>>(),
        )
        .filter(|dist| !dist.is_empty())
    };
    let personal_category = personal_category.and_then(|d| live_only(d, &live_category_ids));
    let personal_project = personal_project.and_then(|d| live_only(d, &live_project_ids));

    let t1_category = arbiter::t1_top(&category_neighbors, personal_category.as_ref(), labeled);
    let t1_project = arbiter::t1_top(&project_neighbors, personal_project.as_ref(), labeled);
    let mut llm_category: Option<Votes> = None;
    let mut llm_project: Option<Votes> = None;
    let mut description: Option<String> = None;
    let mut trace: Option<LlmTrace> = None;
    if let Some(llm) = llm {
        let mut cat_counts: HashMap<&str, usize> = HashMap::new();
        let mut proj_counts: HashMap<&str, usize> = HashMap::new();
        for item in pool {
            if let Some(id) = item.category_id.as_deref() {
                *cat_counts.entry(id).or_default() += 1;
            }
            if let Some(id) = item.project_id.as_deref() {
                *proj_counts.entry(id).or_default() += 1;
            }
        }
        let cold_categories: HashSet<&str> = categories
            .iter()
            .filter(|c| cat_counts.get(c.id.as_str()).copied().unwrap_or(0) < 5)
            .map(|c| c.id.as_str())
            .collect();
        let cold_projects: HashSet<&str> = projects
            .iter()
            .filter(|p| proj_counts.get(p.id.as_str()).copied().unwrap_or(0) < 5)
            .map(|p| p.id.as_str())
            .collect();

        let request = LlmRequest {
            features: features.text.clone(),
            categories: shortlist(
                categories,
                MAX_LLM_CATEGORIES,
                &category_neighbors,
                &cold_categories,
            )
            .into_iter()
            .cloned()
            .collect(),
            projects: want_project.then(|| {
                shortlist(
                    projects,
                    MAX_LLM_PROJECTS,
                    &project_neighbors,
                    &cold_projects,
                )
                .into_iter()
                .cloned()
                .collect()
            }),
            examples: few_shot(&neighbors, categories, projects, want_project),
            custom: custom.to_string(),
        };
        let mut samples = 0;
        let mut latency_ms = 0;
        match llm.classify(&request, 1, None) {
            Ok(first) => {
                let mut category_votes = first.category;
                let mut project_votes = first.project.unwrap_or_default();
                description = first.description;
                samples += first.samples;
                latency_ms += first.latency_ms;
                // Adaptive sampling: one call normally, two more when the
                // model disagrees with (or has nothing to check against)
                // the fast tiers, so vote share can measure its certainty.
                let disagrees = |votes: &Votes, t1: &Option<Label>| match t1 {
                    Some(t1) => votes.top() != Some(t1),
                    None => true,
                };
                let unsure = (rule_category.is_none() && disagrees(&category_votes, &t1_category))
                    || (want_project
                        && rule_project.is_none()
                        && disagrees(&project_votes, &t1_project));
                if unsure {
                    match llm.classify(&request, 2, Some(0.9)) {
                        Ok(more) => {
                            merge_votes(&mut category_votes, more.category);
                            merge_votes(&mut project_votes, more.project.unwrap_or_default());
                            samples += more.samples;
                            latency_ms += more.latency_ms;
                        }
                        Err(error) => eprintln!("ai worker: extra samples failed: {error}"),
                    }
                }
                llm_category = Some(category_votes.clone());
                llm_project = want_project.then_some(project_votes);
                trace = Some(LlmTrace {
                    category: Some(category_votes),
                    project: llm_project.clone(),
                    samples,
                    latency_ms,
                    error: None,
                });
            }
            Err(error) => {
                eprintln!("ai worker: language model failed: {error}");
                trace = Some(LlmTrace {
                    category: None,
                    project: None,
                    samples,
                    latency_ms,
                    error: Some(error),
                });
            }
        }
    }

    let names: HashMap<&str, &str> = categories
        .iter()
        .chain(projects.iter())
        .map(|c| (c.id.as_str(), c.name.as_str()))
        .collect();
    let name = |label: &Label| match label {
        Some(id) => names
            .get(id.as_str())
            .copied()
            .unwrap_or("Unknown")
            .to_string(),
        None => "No project".to_string(),
    };
    let mut category = arbiter::decide(
        &Evidence {
            field: Field::Category,
            rule: rule_category,
            neighbors: category_neighbors,
            personal: personal_category,
            llm: llm_category,
            mentions: HashMap::new(),
            labeled,
            calibration,
            dominant: features.dominant.as_ref(),
        },
        &name,
    );
    let mut project = if want_project {
        arbiter::decide(
            &Evidence {
                field: Field::Project,
                rule: rule_project,
                neighbors: project_neighbors,
                personal: personal_project,
                llm: llm_project,
                mentions: mentions(projects, segments),
                labeled,
                calibration,
                dominant: features.dominant.as_ref(),
            },
            &name,
        )
    } else {
        None
    };

    // A private window's detail is read for this classification and then
    // forgotten, so nothing stored may repeat it: not a rule's pattern in
    // the "Why" line, not a sentence written from its titles.
    let any_private = segments.iter().any(|segment| segment.private);
    if any_private {
        for decision in [&mut category, &mut project].into_iter().flatten() {
            decision.hide_rule_details();
        }
    }
    let has_titles = features::has_titles(segments) && !any_private;
    Labeled {
        category,
        project,
        description: if has_titles { description } else { None },
        llm: trace,
    }
}

/// With too many choices for the model's context, keep cold choices (so new
/// categories get a chance) and the fast tiers' top picks, truncated to `max`.
fn shortlist<'a>(
    choices: &'a [Choice],
    max: usize,
    neighbors: &[(Label, f32)],
    cold: &HashSet<&str>,
) -> Vec<&'a Choice> {
    if choices.len() <= max {
        return choices.iter().collect();
    }
    let mut score: HashMap<&str, f32> = HashMap::new();
    for (label, similarity) in neighbors {
        if let Some(id) = label {
            *score.entry(id.as_str()).or_default() += similarity;
        }
    }
    let mut ranked: Vec<&Choice> = choices.iter().collect();
    ranked.sort_by(|a, b| {
        let a_cold = cold.contains(a.id.as_str());
        let b_cold = cold.contains(b.id.as_str());
        if a_cold != b_cold {
            return b_cold.cmp(&a_cold);
        }
        let a_score = score.get(a.id.as_str()).copied().unwrap_or(0.0);
        let b_score = score.get(b.id.as_str()).copied().unwrap_or(0.0);
        b_score.total_cmp(&a_score)
    });
    ranked.truncate(max);
    ranked
}

/// The closest approved entries, as few-shot examples for the model.
/// Corrections come first: an entry the user had to fix is exactly where the
/// model needs the hint.
fn few_shot(
    neighbors: &[knn::Neighbor<'_>],
    categories: &[Choice],
    projects: &[Choice],
    want_project: bool,
) -> Vec<Example> {
    let name = |choices: &[Choice], id: &Option<String>| {
        id.as_ref()
            .and_then(|id| choices.iter().find(|c| &c.id == id))
            .map(|c| c.name.clone())
    };
    let (corrected, rest): (Vec<_>, Vec<_>) = neighbors.iter().partition(|n| n.item.corrected);
    corrected
        .into_iter()
        .chain(rest)
        .filter_map(|n| {
            let category = name(categories, &n.item.category_id)?;
            let project = want_project
                .then(|| name(projects, &n.item.project_id))
                .flatten();
            Some(Example {
                features: n.item.features.clone(),
                category,
                project,
            })
        })
        .take(FEW_SHOT_EXAMPLES)
        .collect()
}

/// Share of active time whose title, URL, or domain names each project (its
/// name, or one of its comma- or line-separated AI hints).
fn mentions(projects: &[Choice], segments: &[ActivitySegment]) -> HashMap<Label, f64> {
    let mut active_ms = 0u64;
    let mut covered: HashMap<Label, u64> = HashMap::new();
    let needles: Vec<(&Choice, Vec<String>)> = projects
        .iter()
        .map(|project| {
            let mut words = vec![project.name.to_lowercase()];
            if let Some(hints) = &project.hint {
                words.extend(
                    hints
                        .split([',', '\n'])
                        .map(|w| w.trim().to_lowercase())
                        .filter(|w| !w.is_empty()),
                );
            }
            words.retain(|w| w.chars().count() >= 3);
            (project, words)
        })
        .collect();
    for segment in segments.iter().filter(|s| s.kind != activity::KIND_BREAK) {
        let ms = segment
            .ended_at
            .unwrap_or(segment.started_at)
            .saturating_sub(segment.started_at);
        active_ms += ms;
        let haystack = format!(
            "{} {} {}",
            segment.title,
            segment.url.as_deref().unwrap_or(""),
            segment.domain.as_deref().unwrap_or("")
        )
        .to_lowercase();
        for (project, words) in &needles {
            if words.iter().any(|w| haystack.contains(w.as_str())) {
                *covered.entry(Some(project.id.clone())).or_default() += ms;
            }
        }
    }
    if active_ms == 0 {
        return HashMap::new();
    }
    covered
        .into_iter()
        .map(|(label, ms)| (label, ms as f64 / active_ms as f64))
        .collect()
}

fn merge_votes(into: &mut Votes, more: Votes) {
    for (label, count) in more.counts {
        *into.counts.entry(label).or_default() += count;
    }
    into.samples += more.samples;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_measure_time_naming_a_project() {
        let projects = vec![Choice {
            id: "p1".into(),
            name: "OpenRize".into(),
            description: None,
            hint: Some("rize, tauri".into()),
        }];
        let seg = |title: &str, ms: u64| ActivitySegment {
            id: 0,
            app: "Xcode".into(),
            title: title.into(),
            kind: "activity".into(),
            label: None,
            started_at: 0,
            ended_at: Some(ms),
            reviewed: false,
            app_id: None,
            bundle_id: None,
            url: None,
            domain: None,
            entry_id: None,
            private: false,
        };
        let found = mentions(
            &projects,
            &[seg("activity.rs - OpenRize", 750), seg("Inbox", 250)],
        );
        assert_eq!(found.get(&Some("p1".to_string())), Some(&0.75));
    }
}
