//! Golden-set benchmark: an `#[ignore]`d test that scores each candidate model through the real
//! `pipeline::label` (`cargo test --lib ai::bench -- --ignored --nocapture`). Configured by the
//! `OPENRIZE_{GOLDEN_SET,BENCH_PROVIDERS,ML_SIDECAR,LLAMA_SERVER,BENCH_EMBEDDER,BENCH_OUT}` env vars.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use chrono::{DateTime, Local};
use openrize_core::ai::arbiter::{Decision, Votes};
use openrize_core::ai::calibration;
use openrize_core::ai::knn;
use openrize_core::ai::prompt::{Choice, MAX_DESCRIPTION_CHARS, MAX_DESCRIPTION_WORDS, NO_PROJECT};
use openrize_core::settings::AiSuggest;
use serde::{Deserialize, Serialize};

use super::apple::AppleProvider;
use super::features::{self, EntryFeatures};
use super::openai::{Endpoint, LocalModel, OpenAiProvider};
use super::pipeline::{self, Inputs, Labeled};
use super::provider::Provider;
use super::runtime::{LaunchSpec, LlamaServer};
use crate::activity::{ActivitySegment, KIND_ACTIVITY};

const BUNDLED_GOLDEN_SET: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/src/ai/bench/synthetic.json");

#[derive(Debug, Deserialize)]
struct GoldenSet {
    version: u32,
    #[serde(default)]
    note: String,
    workspaces: Vec<Workspace>,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    name: String,
    suggest: AiSuggest,
    #[serde(default)]
    custom: String,
    categories: Vec<Choice>,
    #[serde(default)]
    projects: Vec<Choice>,
    #[serde(default)]
    history: Vec<HistoryItem>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct HistoryItem {
    block: Block,
    category: String,
    project: Option<String>,
    #[serde(default)]
    corrected: bool,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    block: Block,
    category: String,
    project: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Block {
    start: String,
    segments: Vec<SegmentSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SegmentSpec {
    app: String,
    #[serde(default)]
    bundle_id: Option<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    minutes: f64,
    #[serde(default)]
    private: bool,
}

struct Built {
    segments: Vec<ActivitySegment>,
    features: EntryFeatures,
}

fn build(block: &Block, label: &str) -> Result<Built, String> {
    let start = DateTime::parse_from_rfc3339(&block.start)
        .map_err(|error| format!("{label}: bad start {:?}: {error}", block.start))?
        .timestamp_millis();
    let start = u64::try_from(start).map_err(|_| format!("{label}: start is before 1970"))?;
    let mut at = start;
    let mut segments = Vec::new();
    for (index, spec) in block.segments.iter().enumerate() {
        let ms = (spec.minutes * 60_000.0).round() as u64;
        segments.push(ActivitySegment {
            id: index as i64 + 1,
            app: spec.app.clone(),
            title: spec.title.clone(),
            kind: KIND_ACTIVITY.to_string(),
            label: None,
            started_at: at,
            ended_at: Some(at + ms),
            reviewed: true,
            app_id: None,
            bundle_id: spec.bundle_id.clone(),
            url: spec.url.clone(),
            domain: spec.domain.clone(),
            entry_id: None,
            private: spec.private,
        });
        at += ms;
    }
    if segments.is_empty() {
        return Err(format!("{label}: a block needs at least one segment"));
    }
    let features = features::extract(start, at, &segments, None);
    Ok(Built { segments, features })
}

fn load_golden_set(path: &Path) -> Result<GoldenSet, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read golden set {}: {error}", path.display()))?;
    let set: GoldenSet = serde_json::from_str(&text)
        .map_err(|error| format!("cannot parse golden set {}: {error}", path.display()))?;
    if set.version != 1 {
        return Err(format!(
            "golden set version {} is not supported",
            set.version
        ));
    }
    for workspace in &set.workspaces {
        let ids = |choices: &[Choice]| choices.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
        let (categories, projects) = (ids(&workspace.categories), ids(&workspace.projects));
        let labels = workspace
            .history
            .iter()
            .map(|h| (&h.category, &h.project))
            .chain(workspace.cases.iter().map(|c| (&c.category, &c.project)));
        for (category, project) in labels {
            if !categories.contains(category) {
                return Err(format!(
                    "{}: unknown category id {category:?}",
                    workspace.name
                ));
            }
            if let Some(project) = project {
                if !projects.contains(project) {
                    return Err(format!(
                        "{}: unknown project id {project:?}",
                        workspace.name
                    ));
                }
            }
        }
    }
    Ok(set)
}

enum Spec {
    Apple,
    Local { path: PathBuf, thinking: bool },
}

/// `local=<path.gguf>[#thinking]`; thinking is off for `*qwen*` files unless flagged.
fn parse_providers(text: &str) -> Result<Vec<Spec>, String> {
    let mut specs = Vec::new();
    for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if item == "apple" {
            specs.push(Spec::Apple);
        } else if let Some(rest) = item.strip_prefix("local=") {
            let (path, flag) = match rest.split_once('#') {
                Some((path, flag)) => (path, Some(flag)),
                None => (rest, None),
            };
            let path = PathBuf::from(path);
            let thinking = match flag {
                None => !file_name(&path).to_lowercase().contains("qwen"),
                Some("thinking") => true,
                Some(other) => return Err(format!("unknown provider flag #{other}")),
            };
            specs.push(Spec::Local { path, thinking });
        } else {
            return Err(format!(
                "unknown provider {item:?}: use `apple` or `local=<path to .gguf>`"
            ));
        }
    }
    if specs.is_empty() {
        return Err("OPENRIZE_BENCH_PROVIDERS names no providers".to_string());
    }
    // Apple is the baseline, so it prints first.
    specs.sort_by_key(|spec| !matches!(spec, Spec::Apple));
    Ok(specs)
}

/// `model-00001-of-00002` is one model: drop the part counter.
fn strip_split_suffix(stem: &str) -> String {
    let bytes = stem.as_bytes();
    let n = stem.len();
    let digits = |s: &[u8]| s.len() == 5 && s.iter().all(u8::is_ascii_digit);
    if n > 15
        && digits(&bytes[n - 5..])
        && &stem[n - 9..n - 5] == "-of-"
        && digits(&bytes[n - 14..n - 9])
        && bytes[n - 15] == b'-'
    {
        stem[..n - 15].to_string()
    } else {
        stem.to_string()
    }
}

fn file_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn sidecar_path() -> Option<PathBuf> {
    if let Some(path) = env("OPENRIZE_ML_SIDECAR") {
        return Some(PathBuf::from(path));
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            file_name(path).starts_with("openrize-ml-")
                && path.extension().is_none_or(|ext| ext == "exe")
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseResult {
    provider: String,
    workspace: String,
    id: String,
    reference_description: Option<String>,
    gold_category: String,
    gold_project: Option<String>,
    want_project: bool,
    category: Option<String>,
    category_confidence: Option<f64>,
    category_tier: Option<String>,
    category_correct: bool,
    llm_category: Option<String>,
    llm_category_correct: bool,
    llm_category_votes: BTreeMap<String, u32>,
    project: Option<String>,
    project_confidence: Option<f64>,
    project_correct: Option<bool>,
    llm_project: Option<String>,
    llm_project_correct: Option<bool>,
    llm_project_votes: BTreeMap<String, u32>,
    description: Option<String>,
    /// `None` when the block may not carry one (no titles, or a private window).
    description_ok: Option<bool>,
    samples: u32,
    latency_ms: f64,
    llm_latency_ms: u64,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Aggregate {
    cases: usize,
    category_accuracy: f64,
    llm_category_accuracy: f64,
    project_cases: usize,
    project_accuracy: Option<f64>,
    llm_project_accuracy: Option<f64>,
    calibration_error: Option<f64>,
    description_cases: usize,
    description_acceptability: Option<f64>,
    latency_mean_ms: f64,
    latency_p50_ms: f64,
    latency_p95_ms: f64,
    llm_errors: usize,
    mean_samples: f64,
    peak_memory_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderReport {
    label: String,
    tag: String,
    llama_server: Option<String>,
    aggregate: Aggregate,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    date: String,
    os: String,
    arch: String,
    cpus: usize,
    golden_set: String,
    golden_set_note: String,
    cases: usize,
    embedder: String,
    calibration: &'static str,
    providers: Vec<ProviderReport>,
}

fn label_text(label: &Option<String>) -> String {
    label.clone().unwrap_or_else(|| NO_PROJECT.to_string())
}

fn vote_map(votes: Option<&Votes>) -> BTreeMap<String, u32> {
    votes
        .map(|votes| {
            votes
                .counts
                .iter()
                .map(|(label, count)| (label_text(label), *count))
                .collect()
        })
        .unwrap_or_default()
}

fn top(votes: Option<&Votes>) -> Option<Option<String>> {
    votes.and_then(|v| v.top()).cloned()
}

fn description_ok(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && text.split_whitespace().count() <= MAX_DESCRIPTION_WORDS
        && text.chars().count() <= MAX_DESCRIPTION_CHARS
}

fn score(
    provider: &str,
    workspace: &Workspace,
    case: &Case,
    built: &Built,
    want_project: bool,
    labeled: &Labeled,
    wall_ms: f64,
) -> CaseResult {
    let category: Option<&Decision> = labeled.category.as_ref();
    let category_value = category.and_then(|d| d.value.clone());
    let trace = labeled.llm.as_ref();
    let llm_category = top(trace.and_then(|t| t.category.as_ref()));
    let llm_project = top(trace.and_then(|t| t.project.as_ref()));
    let project_value = labeled.project.as_ref().map(|d| d.value.clone());
    let may_describe = features::has_titles(&built.segments)
        && !built.segments.iter().any(|segment| segment.private);
    CaseResult {
        provider: provider.to_string(),
        workspace: workspace.name.clone(),
        id: case.id.clone(),
        reference_description: case.description.clone(),
        gold_category: case.category.clone(),
        gold_project: case.project.clone(),
        want_project,
        category_correct: category_value.as_deref() == Some(case.category.as_str()),
        category: category_value,
        category_confidence: category.map(|d| d.confidence),
        category_tier: category.map(|d| d.tier.to_string()),
        llm_category_correct: llm_category
            .as_ref()
            .is_some_and(|c| c.as_deref() == Some(case.category.as_str())),
        llm_category: llm_category.map(|c| label_text(&c)),
        llm_category_votes: vote_map(trace.and_then(|t| t.category.as_ref())),
        project_correct: want_project
            .then(|| project_value.as_ref() == Some(&case.project))
            .filter(|_| project_value.is_some()),
        project: project_value.as_ref().map(label_text),
        project_confidence: labeled.project.as_ref().map(|d| d.confidence),
        llm_project_correct: want_project.then(|| llm_project.as_ref() == Some(&case.project)),
        llm_project: llm_project.map(|p| label_text(&p)),
        llm_project_votes: vote_map(trace.and_then(|t| t.project.as_ref())),
        description_ok: may_describe
            .then(|| labeled.description.as_deref().is_some_and(description_ok)),
        description: labeled.description.clone(),
        samples: trace.map_or(0, |t| t.samples),
        latency_ms: wall_ms,
        llm_latency_ms: trace.map_or(0, |t| t.latency_ms),
        error: trace.and_then(|t| t.error.clone()),
    }
}

fn ratio(hits: usize, total: usize) -> Option<f64> {
    (total > 0).then(|| hits as f64 / total as f64)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn aggregate(results: &[CaseResult], peak_memory_bytes: Option<u64>) -> Aggregate {
    let n = results.len();
    let count = |f: &dyn Fn(&CaseResult) -> bool| results.iter().filter(|r| f(r)).count();
    let project_cases = count(&|r| r.want_project);
    let description_cases = count(&|r| r.description_ok.is_some());
    let samples: Vec<(f64, bool)> = results
        .iter()
        .filter_map(|r| Some((r.category_confidence?, r.category_correct)))
        .collect();
    let mut latencies: Vec<f64> = results.iter().map(|r| r.latency_ms).collect();
    latencies.sort_by(f64::total_cmp);
    Aggregate {
        cases: n,
        category_accuracy: ratio(count(&|r| r.category_correct), n).unwrap_or(0.0),
        llm_category_accuracy: ratio(count(&|r| r.llm_category_correct), n).unwrap_or(0.0),
        project_cases,
        project_accuracy: ratio(count(&|r| r.project_correct == Some(true)), project_cases),
        llm_project_accuracy: ratio(
            count(&|r| r.llm_project_correct == Some(true)),
            project_cases,
        ),
        calibration_error: calibration::expected_error(&calibration::reliability(&samples, 10)),
        description_cases,
        description_acceptability: ratio(
            count(&|r| r.description_ok == Some(true)),
            description_cases,
        ),
        latency_mean_ms: latencies.iter().sum::<f64>() / n.max(1) as f64,
        latency_p50_ms: percentile(&latencies, 0.5),
        latency_p95_ms: percentile(&latencies, 0.95),
        llm_errors: count(&|r| r.error.is_some()),
        mean_samples: results.iter().map(|r| f64::from(r.samples)).sum::<f64>() / n.max(1) as f64,
        peak_memory_bytes,
    }
}

struct Prepared<'a> {
    workspace: &'a Workspace,
    want_project: bool,
    pool: Vec<knn::Labeled>,
    cases: Vec<(Built, Option<Vec<f32>>)>,
}

fn embed_all(
    embedder: Option<&mut AppleProvider>,
    texts: &[String],
) -> Result<Vec<Option<Vec<f32>>>, String> {
    match embedder {
        Some(embedder) if !texts.is_empty() => embedder.embed(texts),
        _ => Ok(vec![None; texts.len()]),
    }
}

fn prepare<'a>(
    set: &'a GoldenSet,
    mut embedder: Option<&mut AppleProvider>,
) -> Result<Vec<Prepared<'a>>, String> {
    let mut prepared = Vec::new();
    for workspace in &set.workspaces {
        let history = workspace
            .history
            .iter()
            .enumerate()
            .map(|(i, h)| build(&h.block, &format!("{} history {i}", workspace.name)))
            .collect::<Result<Vec<_>, _>>()?;
        let contents: Vec<String> = history.iter().map(|b| b.features.content.clone()).collect();
        let vectors = embed_all(embedder.as_deref_mut(), &contents)?;
        let pool = workspace
            .history
            .iter()
            .zip(history)
            .zip(vectors)
            .filter_map(|((item, built), vector)| {
                Some(knn::Labeled {
                    vector: vector?,
                    category_id: Some(item.category.clone()),
                    project_id: item.project.clone(),
                    features: built.features.text,
                    corrected: item.corrected,
                })
            })
            .collect();

        let built = workspace
            .cases
            .iter()
            .map(|c| build(&c.block, &format!("{} {}", workspace.name, c.id)))
            .collect::<Result<Vec<_>, _>>()?;
        let contents: Vec<String> = built.iter().map(|b| b.features.content.clone()).collect();
        let vectors = embed_all(embedder.as_deref_mut(), &contents)?;
        prepared.push(Prepared {
            workspace,
            want_project: workspace.suggest == AiSuggest::CategoryProject
                && !workspace.projects.is_empty(),
            pool,
            cases: built.into_iter().zip(vectors).collect(),
        });
    }
    Ok(prepared)
}

fn label_one(provider: &mut dyn Provider, prepared: &Prepared<'_>, index: usize) -> (Labeled, f64) {
    let workspace = prepared.workspace;
    let (built, vector) = &prepared.cases[index];
    let started = Instant::now();
    let labeled = pipeline::label(
        Inputs {
            features: &built.features,
            segments: &built.segments,
            categories: &workspace.categories,
            projects: &workspace.projects,
            want_project: prepared.want_project,
            rule_category: None,
            rule_project: None,
            vector: vector.as_deref(),
            pool: &prepared.pool,
            personal_category: None,
            personal_project: None,
            labeled: workspace.history.len() as u32,
            calibration: None,
            custom: &workspace.custom,
        },
        Some(provider),
    );
    (labeled, started.elapsed().as_secs_f64() * 1000.0)
}

struct Started {
    provider: Box<dyn Provider>,
    label: String,
    llama_server: Option<String>,
}

fn start(spec: &Spec, sidecar: Option<&Path>) -> Result<Started, String> {
    match spec {
        Spec::Apple => {
            let path =
                sidecar.ok_or("the openrize-ml sidecar was not found (set OPENRIZE_ML_SIDECAR)")?;
            Ok(Started {
                provider: Box::new(AppleProvider::at(path.to_path_buf())),
                label: "apple".to_string(),
                llama_server: None,
            })
        }
        Spec::Local { path, thinking } => {
            let binary = env("OPENRIZE_LLAMA_SERVER").unwrap_or_else(|| "llama-server".to_string());
            let stem = strip_split_suffix(&file_name(path));
            let server = LlamaServer::start(&LaunchSpec::new(&binary, path))?;
            let model = LocalModel {
                identity: stem.clone(),
                model: stem.clone(),
                disable_thinking: !thinking,
            };
            Ok(Started {
                provider: Box::new(OpenAiProvider::new(Endpoint::Local(server), model)?),
                label: format!("local:{stem}"),
                llama_server: Some(llama_version(&binary)),
            })
        }
    }
}

fn llama_version(binary: &str) -> String {
    let Ok(output) = Command::new(binary).arg("--version").output() else {
        return "unknown".to_string();
    };
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let all: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    all.iter()
        .find(|l| l.starts_with("version"))
        .or(all.first())
        .map_or("unknown", |l| l)
        .to_string()
}

fn run_provider(
    spec: &Spec,
    sidecar: Option<&Path>,
    prepared: &[Prepared<'_>],
) -> Result<(ProviderReport, Vec<CaseResult>), String> {
    let Started {
        mut provider,
        label,
        llama_server,
    } = start(spec, sidecar)?;
    let capabilities = provider.capabilities()?;
    if !capabilities.llm.is_ready() {
        return Err(format!(
            "{label}: language model not ready: {:?}",
            capabilities.llm
        ));
    }
    let tag = provider.id().tag();
    let total: usize = prepared.iter().map(|p| p.cases.len()).sum();
    eprintln!("[{label}] {tag}: {total} cases");

    // One unscored pass so loading the model is not billed to the first case.
    if let Some(first) = prepared.iter().find(|p| !p.cases.is_empty()) {
        let _ = label_one(provider.as_mut(), first, 0);
    }

    // Peak footprint omits mmap'd weights on some platforms, so also sample
    // the resident size after every case and report the larger.
    let pid = provider.pid();
    let mut peak_resident = 0u64;
    let mut results = Vec::with_capacity(total);
    for p in prepared {
        for (index, case) in p.workspace.cases.iter().enumerate() {
            let (labeled, wall_ms) = label_one(provider.as_mut(), p, index);
            let result = score(
                &label,
                p.workspace,
                case,
                &p.cases[index].0,
                p.want_project,
                &labeled,
                wall_ms,
            );
            eprintln!(
                "[{label}] {:<7} {:>6.0} ms  cat {} {:<10}{}",
                case.id,
                wall_ms,
                if result.category_correct {
                    "ok "
                } else {
                    "MISS"
                },
                result.category.as_deref().unwrap_or("-"),
                result
                    .error
                    .as_deref()
                    .map(|e| format!("  ERROR {e}"))
                    .unwrap_or_default(),
            );
            results.push(result);
            if let Some(bytes) = pid.and_then(|pid| crate::energy::resident_bytes(pid as i32)) {
                peak_resident = peak_resident.max(bytes);
            }
        }
    }
    let lifetime_peak = pid.and_then(|pid| crate::energy::peak_memory_bytes(pid as i32));
    let peak = [Some(peak_resident).filter(|b| *b > 0), lifetime_peak]
        .into_iter()
        .flatten()
        .max();
    let report = ProviderReport {
        label,
        tag,
        llama_server,
        aggregate: aggregate(&results, peak),
    };
    Ok((report, results))
}

fn pct(value: Option<f64>) -> String {
    value.map_or("   -".to_string(), |v| format!("{:>4.0}%", v * 100.0))
}

fn delta(value: Option<f64>, baseline: Option<f64>) -> String {
    match (value, baseline) {
        (Some(v), Some(b)) => format!("{:>+5.0}", (v - b) * 100.0),
        _ => "    -".to_string(),
    }
}

fn gib(bytes: Option<u64>) -> String {
    bytes.map_or("     -".to_string(), |b| {
        format!("{:>5.2}G", b as f64 / (1u64 << 30) as f64)
    })
}

fn print_report(report: &Report) {
    println!();
    println!("OpenRize golden-set benchmark - {}", report.date);
    println!(
        "{} {}, {} CPUs | golden set {} ({} cases) | embedder {}",
        report.os, report.arch, report.cpus, report.golden_set, report.cases, report.embedder
    );
    println!("note: {}", report.golden_set_note);
    println!("calibration: {}", report.calibration);
    for p in &report.providers {
        println!(
            "provider {}: {}{}",
            p.label,
            p.tag,
            p.llama_server
                .as_ref()
                .map(|v| format!(" | llama-server {v}"))
                .unwrap_or_default()
        );
    }
    println!();
    println!(
        "{:<44} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>6} {:>6} {:>6} {:>3} {:>4} {:>6} {:>5} {:>5}",
        "provider", "cat", "catLM", "proj", "prjLM", "ECE", "desc", "mean", "p50", "p95", "err", "smp", "RAM", "dCat", "dPrj"
    );
    let baseline = report
        .providers
        .iter()
        .find(|p| p.label == "apple")
        .map(|p| &p.aggregate);
    for p in &report.providers {
        let a = &p.aggregate;
        let (d_cat, d_prj) = match baseline {
            Some(b) if p.label != "apple" => (
                delta(Some(a.category_accuracy), Some(b.category_accuracy)),
                delta(a.project_accuracy, b.project_accuracy),
            ),
            _ => ("    -".to_string(), "    -".to_string()),
        };
        println!(
            "{:<44} {} {} {} {} {} {} {:>6.0} {:>6.0} {:>6.0} {:>3} {:>4.1} {} {} {}",
            p.label.chars().take(44).collect::<String>(),
            pct(Some(a.category_accuracy)),
            pct(Some(a.llm_category_accuracy)),
            pct(a.project_accuracy),
            pct(a.llm_project_accuracy),
            pct(a.calibration_error),
            pct(a.description_acceptability),
            a.latency_mean_ms,
            a.latency_p50_ms,
            a.latency_p95_ms,
            a.llm_errors,
            a.mean_samples,
            gib(a.peak_memory_bytes),
            d_cat,
            d_prj,
        );
    }
    println!();
    println!("cat/proj: arbiter decision equals gold. catLM/prjLM: the model's own top vote (kNN excluded).");
    println!("ECE: expected calibration error, lower is better. desc: share of eligible blocks with a valid description.");
    println!("latency in ms per entry (pipeline::label wall time, re-asks included). err: LLM errors. smp: mean samples per entry.");
    println!("dCat/dPrj: accuracy points versus apple. RAM: provider process peak resident size; Apple's Foundation Model runs in a system process, so its figure understates it.");
}

fn write_out(dir: &Path, report: &Report, cases: &[CaseResult]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("report.json"), json).map_err(|e| e.to_string())?;
    let mut lines = String::new();
    for case in cases {
        lines.push_str(&serde_json::to_string(case).map_err(|e| e.to_string())?);
        lines.push('\n');
    }
    std::fs::write(dir.join("cases.jsonl"), lines).map_err(|e| e.to_string())
}

fn run() -> Result<(), String> {
    let golden_path = env("OPENRIZE_GOLDEN_SET").unwrap_or_else(|| BUNDLED_GOLDEN_SET.to_string());
    let set = load_golden_set(Path::new(&golden_path))?;
    let sidecar = sidecar_path().filter(|path| path.exists());
    let specs =
        parse_providers(&env("OPENRIZE_BENCH_PROVIDERS").unwrap_or_else(|| "apple".to_string()))?;

    let embedder_name = env("OPENRIZE_BENCH_EMBEDDER")
        .unwrap_or_else(|| if sidecar.is_some() { "apple" } else { "none" }.to_string());
    let mut embedder = match embedder_name.as_str() {
        "apple" => {
            let path = sidecar.clone().ok_or(
                "the apple embedder needs the openrize-ml sidecar (set OPENRIZE_ML_SIDECAR)",
            )?;
            let mut embedder = AppleProvider::at(path);
            if embedder.capabilities()?.embed.is_none() {
                return Err("the sidecar reports no embedding model".to_string());
            }
            Some(embedder)
        }
        "none" => None,
        other => {
            return Err(format!(
                "OPENRIZE_BENCH_EMBEDDER must be apple or none, not {other:?}"
            ))
        }
    };
    let prepared = prepare(&set, embedder.as_mut())?;
    drop(embedder);

    let mut providers = Vec::new();
    let mut all_cases = Vec::new();
    for spec in &specs {
        let (report, cases) = run_provider(spec, sidecar.as_deref(), &prepared)?;
        providers.push(report);
        all_cases.extend(cases);
    }
    let report = Report {
        date: Local::now().format("%Y-%m-%d %H:%M %Z").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpus: std::thread::available_parallelism().map_or(1, |n| n.get()),
        golden_set: golden_path,
        golden_set_note: set.note.clone(),
        cases: set.workspaces.iter().map(|w| w.cases.len()).sum(),
        embedder: embedder_name,
        calibration: "no calibration curve: displayed confidence is the cold-start capped one",
        providers,
    };
    print_report(&report);
    if let Some(dir) = env("OPENRIZE_BENCH_OUT") {
        write_out(Path::new(&dir), &report, &all_cases)?;
        println!("wrote report.json and cases.jsonl to {dir}");
    }
    Ok(())
}

#[test]
#[ignore = "runs real models; see the module docs"]
fn golden_set() {
    if let Err(error) = run() {
        panic!("golden-set benchmark failed: {error}");
    }
}
