//! SQLite access for the AI pipeline: the job queue, suggestions and their
//! outcomes, the kNN store, rules, and trained model artifacts.
//!
//! Every function takes a plain `&Connection`, so the worker can read on its
//! own connection while writes go through the `ActivityStore` writer.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use super::arbiter::{Decision, Label};
use super::calibration::Calibrator;
use super::knn::{self, Labeled};
use super::rules::Rule;
use super::{Field, FIELD_CATEGORY, FIELD_PROJECT};
use crate::models::{
    Alternative, ClassifyJob, Dominant, EntryAi, FieldSuggestion, RuleSuggestion, Signal,
};

pub const JOB_CLASSIFY: &str = "classify";
pub const JOB_EMBED: &str = "embed";
pub const MAX_ATTEMPTS: u32 = 3;
const RETRY_BASE_MS: u64 = 30_000;
/// Consistent corrections for one app or domain before a rule is offered.
pub const RULE_SUGGESTION_MIN: u32 = 3;
/// `model_artifacts.kind` of the isotonic calibration curve.
pub const KIND_CALIBRATION: &str = "calibration";
/// Calibration fits on at most this many recent verdicts, so it follows
/// drift in the models and in the user's habits.
const CALIBRATION_WINDOW: u32 = 2_000;
/// An auto-approved suggestion nobody corrected within this long counts as
/// accepted. Without it, calibration would only ever hear about the
/// auto-approvals that were wrong.
pub const AUTO_TRUSTED_AFTER_MS: u64 = 24 * 60 * 60 * 1000;
/// Retired versions kept per kind, for the model history.
const ARTIFACT_HISTORY: u32 = 20;
/// Key in the activity `settings` table: when "Reset learned data" last ran.
const LEARNING_RESET_KEY: &str = "ai_learning_reset_at";

type Result<T> = std::result::Result<T, String>;

fn err(error: rusqlite::Error) -> String {
    error.to_string()
}

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

// --- Job queue -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: String,
    pub entry_id: String,
    pub kind: String,
    pub attempts: u32,
}

/// Queues (or re-queues) a job. A classify job also moves a pending entry to
/// `processing`, which the Calendar renders as "Categorizing…".
pub fn enqueue(conn: &Connection, entry_id: &str, kind: &str, now: u64) -> Result<()> {
    conn.execute(
        "INSERT INTO classify_jobs (id, entry_id, kind, state, attempts, next_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'queued', 0, ?4, ?4, ?4)
         ON CONFLICT(entry_id, kind) DO UPDATE SET
           state = 'queued', attempts = 0, next_at = excluded.next_at, last_error = NULL,
           created_at = excluded.created_at, updated_at = excluded.updated_at;",
        params![new_id(), entry_id, kind, now as i64],
    )
    .map_err(err)?;
    if kind == JOB_CLASSIFY {
        conn.execute(
            "UPDATE time_entries SET status = 'processing', updated_at = ?2
             WHERE id = ?1 AND status = 'pending';",
            params![entry_id, now as i64],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// Queues every pending entry in the range that has never been classified.
/// Returns how many were queued.
pub fn enqueue_unclassified(conn: &Connection, start: u64, end: u64, now: u64) -> Result<usize> {
    let ids: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT e.id FROM time_entries e
                 WHERE e.deleted_at IS NULL AND e.status = 'pending' AND e.source = 'auto'
                   AND e.ended_at >= ?1 AND e.started_at <= ?2
                   AND NOT EXISTS (SELECT 1 FROM suggestions s WHERE s.entry_id = e.id)
                   AND NOT EXISTS (
                     SELECT 1 FROM classify_jobs j WHERE j.entry_id = e.id AND j.kind = 'classify'
                   );",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![start as i64, end as i64], |row| row.get(0))
            .map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)?
    };
    for id in &ids {
        enqueue(conn, id, JOB_CLASSIFY, now)?;
    }
    Ok(ids.len())
}

/// The next job to run. Newest first, so a block that just closed is
/// categorized before any backlog.
pub fn next_due(conn: &Connection, now: u64) -> Result<Option<Job>> {
    conn.query_row(
        "SELECT id, entry_id, kind, attempts FROM classify_jobs
         WHERE state = 'queued' AND next_at <= ?1
         ORDER BY created_at DESC LIMIT 1;",
        params![now as i64],
        |row| {
            Ok(Job {
                id: row.get(0)?,
                entry_id: row.get(1)?,
                kind: row.get(2)?,
                attempts: row.get::<_, i64>(3)? as u32,
            })
        },
    )
    .optional()
    .map_err(err)
}

pub fn set_job_state(conn: &Connection, job_id: &str, state: &str, now: u64) -> Result<()> {
    conn.execute(
        "UPDATE classify_jobs SET state = ?2, updated_at = ?3 WHERE id = ?1;",
        params![job_id, state, now as i64],
    )
    .map(|_| ())
    .map_err(err)
}

/// Records a failed attempt: retries with exponential backoff, or gives up
/// after `MAX_ATTEMPTS`, leaving the entry pending with no suggestion.
/// Returns whether it gave up.
pub fn fail_job(conn: &Connection, job: &Job, error: &str, now: u64) -> Result<bool> {
    let attempts = job.attempts + 1;
    let gave_up = attempts >= MAX_ATTEMPTS;
    let next_at = now + RETRY_BASE_MS * (1 << attempts.min(6));
    conn.execute(
        "UPDATE classify_jobs SET state = ?2, attempts = ?3, next_at = ?4, last_error = ?5, updated_at = ?6
         WHERE id = ?1;",
        params![
            job.id,
            if gave_up { "failed" } else { "queued" },
            attempts,
            next_at as i64,
            error,
            now as i64
        ],
    )
    .map_err(err)?;
    if gave_up && job.kind == JOB_CLASSIFY {
        conn.execute(
            "UPDATE time_entries SET status = 'pending', updated_at = ?2
             WHERE id = ?1 AND status = 'processing';",
            params![job.entry_id, now as i64],
        )
        .map_err(err)?;
    }
    Ok(gave_up)
}

/// A crash mid-job leaves it `running`; put it back in the queue.
pub fn requeue_interrupted(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE classify_jobs SET state = 'queued' WHERE state = 'running';",
        [],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn queued_count(conn: &Connection) -> Result<u32> {
    conn.query_row(
        "SELECT COUNT(*) FROM classify_jobs WHERE state IN ('queued', 'running');",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n as u32)
    .map_err(err)
}

pub fn job_for(conn: &Connection, entry_id: &str) -> Result<Option<ClassifyJob>> {
    conn.query_row(
        "SELECT state, attempts, last_error FROM classify_jobs WHERE entry_id = ?1 AND kind = 'classify';",
        params![entry_id],
        |row| {
            Ok(ClassifyJob {
                state: row.get(0)?,
                attempts: row.get::<_, i64>(1)? as u32,
                last_error: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

/// Re-queues pending entries whose suggestion came from the fallback engine,
/// once the Foundation Model becomes available ("backfilled later").
pub fn requeue_fallback(conn: &Connection, since: u64, now: u64) -> Result<usize> {
    let ids: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT e.id FROM time_entries e
                 JOIN suggestions s ON s.entry_id = e.id
                 WHERE e.deleted_at IS NULL AND e.status = 'pending' AND e.started_at >= ?1
                   AND s.engine = 'fallback' AND s.outcome IS NULL
                   AND s.created_at = (
                     SELECT MAX(created_at) FROM suggestions WHERE entry_id = e.id AND field = s.field
                   );",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![since as i64], |row| row.get(0))
            .map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)?
    };
    for id in &ids {
        enqueue(conn, id, JOB_CLASSIFY, now)?;
    }
    Ok(ids.len())
}

// --- Suggestions ------------------------------------------------------------

/// "78% of this block was in Xcode · 11 of your 12 most similar entries
/// were Coding · the on-device model agrees".
pub fn why_text(signals: &[Signal]) -> String {
    let joined = signals
        .iter()
        .map(|signal| signal.text.as_str())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let mut chars = joined.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => joined,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn insert_suggestion(
    conn: &Connection,
    entry_id: &str,
    field: Field,
    decision: &Decision,
    dominant: Option<&Dominant>,
    model_version: &str,
    outcome: Option<&str>,
    now: u64,
) -> Result<String> {
    let id = new_id();
    let signals = json!({ "dominant": dominant, "items": decision.signals });
    let alternatives: Vec<Alternative> = decision
        .alternatives
        .iter()
        .map(|(value_id, confidence)| Alternative {
            value_id: value_id.clone(),
            confidence: *confidence,
        })
        .collect();
    conn.execute(
        "INSERT INTO suggestions (id, entry_id, field, value_id, confidence, raw_confidence, tier, signals, rationale, alternatives, engine, model_version, outcome, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14);",
        params![
            id,
            entry_id,
            field.as_str(),
            decision.value,
            decision.confidence,
            decision.raw_confidence,
            decision.tier,
            signals.to_string(),
            why_text(&decision.signals),
            serde_json::to_string(&alternatives).map_err(|e| e.to_string())?,
            decision.engine,
            model_version,
            outcome,
            now as i64,
        ],
    )
    .map_err(err)?;
    Ok(id)
}

#[derive(serde::Deserialize, Default)]
struct StoredSignals {
    dominant: Option<Dominant>,
    #[serde(default)]
    items: Vec<Signal>,
}

/// The latest suggestion per field, category first.
pub fn latest_suggestions(conn: &Connection, entry_id: &str) -> Result<Vec<FieldSuggestion>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, entry_id, field, value_id, confidence, signals, rationale, alternatives, engine, model_version, outcome, created_at
             FROM suggestions s
             WHERE entry_id = ?1 AND deleted_at IS NULL
               AND created_at = (
                 SELECT MAX(created_at) FROM suggestions
                 WHERE entry_id = s.entry_id AND field = s.field AND deleted_at IS NULL
               )
             ORDER BY CASE field WHEN 'category' THEN 0 ELSE 1 END;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![entry_id], |row| {
            let signals: StoredSignals = row
                .get::<_, Option<String>>(5)?
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();
            let alternatives: Vec<Alternative> = row
                .get::<_, Option<String>>(7)?
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();
            Ok(FieldSuggestion {
                id: row.get(0)?,
                entry_id: row.get(1)?,
                field: row.get(2)?,
                value_id: row.get(3)?,
                confidence: row.get(4)?,
                signals: signals.items,
                dominant: signals.dominant,
                rationale: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                alternatives,
                engine: row.get(8)?,
                model_version: row.get(9)?,
                outcome: row.get(10)?,
                created_at: row.get::<_, i64>(11)? as u64,
            })
        })
        .map_err(err)?;
    let mut list: Vec<FieldSuggestion> = rows.collect::<rusqlite::Result<_>>().map_err(err)?;
    // Two rows can share a millisecond only in tests; keep one per field.
    list.dedup_by(|a, b| a.field == b.field);
    Ok(list)
}

/// Job state and headline confidences for every entry in a range.
pub fn summaries(conn: &Connection, start: u64, end: u64) -> Result<HashMap<String, EntryAi>> {
    let mut out: HashMap<String, EntryAi> = HashMap::new();
    let mut stmt = conn
        .prepare(
            "SELECT e.id, j.state,
               (SELECT CASE WHEN outcome = 'rejected' THEN NULL ELSE confidence END FROM suggestions
                 WHERE entry_id = e.id AND field = 'category' AND deleted_at IS NULL
                 ORDER BY created_at DESC LIMIT 1),
               (SELECT CASE WHEN outcome = 'rejected' THEN NULL ELSE confidence END FROM suggestions
                 WHERE entry_id = e.id AND field = 'project' AND deleted_at IS NULL
                 ORDER BY created_at DESC LIMIT 1)
             FROM time_entries e
             LEFT JOIN classify_jobs j ON j.entry_id = e.id AND j.kind = 'classify'
             WHERE e.deleted_at IS NULL AND e.ended_at >= ?1 AND e.started_at <= ?2;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![start as i64, end as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                EntryAi {
                    state: row.get(1)?,
                    category_confidence: row.get(2)?,
                    project_confidence: row.get(3)?,
                },
            ))
        })
        .map_err(err)?;
    for row in rows {
        let (id, ai) = row.map_err(err)?;
        if ai.state.is_some() || ai.category_confidence.is_some() || ai.project_confidence.is_some()
        {
            out.insert(id, ai);
        }
    }
    Ok(out)
}

/// On approval, each undecided suggestion is `accepted` when the entry kept
/// its value and `changed` when the user picked something else. These
/// outcomes feed the cold-start cap, rule suggestions, and calibration.
pub fn record_approval(
    conn: &Connection,
    entry_id: &str,
    category_id: Option<&str>,
    project_id: Option<&str>,
    now: u64,
) -> Result<()> {
    for suggestion in latest_suggestions(conn, entry_id)? {
        if suggestion.outcome.is_some() {
            continue;
        }
        let current = if suggestion.field == FIELD_CATEGORY {
            category_id
        } else {
            project_id
        };
        let outcome = if suggestion.value_id.as_deref() == current {
            "accepted"
        } else {
            "changed"
        };
        conn.execute(
            "UPDATE suggestions SET outcome = ?2, updated_at = ?3 WHERE id = ?1;",
            params![suggestion.id, outcome, now as i64],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// Marks the undecided suggestions rejected and returns them, so the caller
/// can clear the values they pre-filled.
pub fn record_rejection(
    conn: &Connection,
    entry_id: &str,
    now: u64,
) -> Result<Vec<FieldSuggestion>> {
    let mut rejected = Vec::new();
    for suggestion in latest_suggestions(conn, entry_id)? {
        if suggestion.outcome.is_some() {
            continue;
        }
        conn.execute(
            "UPDATE suggestions SET outcome = 'rejected', updated_at = ?2 WHERE id = ?1;",
            params![suggestion.id, now as i64],
        )
        .map_err(err)?;
        rejected.push(suggestion);
    }
    Ok(rejected)
}

/// A later edit to an approved entry is a correction too: a suggestion that
/// was accepted or auto-approved becomes `changed` when the user moves the
/// entry to another value, so calibration, the metrics, and rule
/// suggestions all hear about it.
pub fn record_edit(
    conn: &Connection,
    entry_id: &str,
    category_id: Option<&str>,
    project_id: Option<&str>,
    now: u64,
) -> Result<()> {
    for suggestion in latest_suggestions(conn, entry_id)? {
        if !matches!(suggestion.outcome.as_deref(), Some("accepted" | "auto")) {
            continue;
        }
        let current = if suggestion.field == FIELD_CATEGORY {
            category_id
        } else {
            project_id
        };
        if suggestion.value_id.as_deref() != current {
            conn.execute(
                "UPDATE suggestions SET outcome = 'changed', updated_at = ?2 WHERE id = ?1;",
                params![suggestion.id, now as i64],
            )
            .map_err(err)?;
        }
    }
    Ok(())
}

// --- Calibration -------------------------------------------------------------

/// When learning last started from scratch (0 = never reset).
pub fn learning_since(conn: &Connection) -> Result<u64> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1;",
        params![LEARNING_RESET_KEY],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|value| value.and_then(|v| v.parse().ok()).unwrap_or(0))
    .map_err(err)
}

/// The suggestions calibration learns from: model (non-rule) suggestions
/// since the last reset with a user verdict, plus auto-approvals that stood
/// for `AUTO_TRUSTED_AFTER_MS`. `?1` is the reset time, `?2` the trust cutoff.
const VERDICTS: &str = "FROM suggestions s JOIN time_entries e ON e.id = s.entry_id
     WHERE s.deleted_at IS NULL AND e.deleted_at IS NULL
       AND s.engine != 'rules' AND s.raw_confidence IS NOT NULL AND s.created_at >= ?1
       AND (s.outcome IN ('accepted', 'changed', 'rejected')
            OR (s.outcome = 'auto' AND s.updated_at <= ?2))";

/// Verdicts calibration can learn from so far (drives the cold-start cap
/// and the "12/50 reviewed" readout).
pub fn outcome_count(conn: &Connection, now: u64) -> Result<u32> {
    let since = learning_since(conn)?;
    conn.query_row(
        &format!("SELECT COUNT(*) {VERDICTS};"),
        params![
            since as i64,
            now.saturating_sub(AUTO_TRUSTED_AFTER_MS) as i64
        ],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n as u32)
    .map_err(err)
}

/// `(raw score, accepted)` for the most recent verdicts.
pub fn calibration_samples(conn: &Connection, now: u64) -> Result<Vec<(f64, bool)>> {
    let since = learning_since(conn)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT s.raw_confidence, s.outcome IN ('accepted', 'auto') {VERDICTS}
             ORDER BY s.created_at DESC LIMIT {CALIBRATION_WINDOW};"
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map(
            params![
                since as i64,
                now.saturating_sub(AUTO_TRUSTED_AFTER_MS) as i64
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

pub fn active_calibration(conn: &Connection) -> Result<Option<Calibrator>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT calibration FROM model_artifacts
             WHERE kind = ?1 AND active = 1 ORDER BY trained_at DESC LIMIT 1;",
            params![KIND_CALIBRATION],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?
        .flatten();
    Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
}

/// Versions a freshly fitted curve and makes it the active one.
pub fn insert_calibration(conn: &Connection, calibrator: &Calibrator, now: u64) -> Result<()> {
    let json = serde_json::to_string(calibrator).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE model_artifacts SET active = 0 WHERE kind = ?1;",
        params![KIND_CALIBRATION],
    )
    .map_err(err)?;
    conn.execute(
        "INSERT INTO model_artifacts (id, kind, path, trained_at, n_examples, holdout_acc, calibration, active)
         VALUES (?1, ?2, '', ?3, ?4, NULL, ?5, 1);",
        params![new_id(), KIND_CALIBRATION, now as i64, calibrator.samples, json],
    )
    .map_err(err)?;
    prune_artifacts(conn, KIND_CALIBRATION)?;
    Ok(())
}

pub fn labeled_count(conn: &Connection) -> Result<u32> {
    conn.query_row(
        "SELECT COUNT(*) FROM time_entries e JOIN entry_embeddings v ON v.entry_id = e.id
         WHERE e.deleted_at IS NULL AND e.status = 'approved' AND e.category_id IS NOT NULL;",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n as u32)
    .map_err(err)
}

/// "Reset learned data": forgets the personal models, the calibration
/// curve, the kNN store (and with it the few-shot pool), and the verdict
/// history calibration counts. Entries, categories, projects, and rules are
/// the user's data and stay. Returns the model files to delete.
pub fn reset_learned(conn: &Connection, now: u64) -> Result<Vec<String>> {
    let paths: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT path FROM model_artifacts WHERE path != '';")
            .map_err(err)?;
        let rows = stmt.query_map([], |row| row.get(0)).map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)?
    };
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute_batch(
        "DELETE FROM model_artifacts;
         DELETE FROM entry_embeddings;
         DELETE FROM classify_jobs WHERE kind = 'embed' AND state IN ('queued', 'failed');",
    )
    .map_err(err)?;
    tx.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
        params![LEARNING_RESET_KEY, now.to_string()],
    )
    .map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(paths)
}

// --- kNN store ------------------------------------------------------------

pub fn upsert_embedding(
    conn: &Connection,
    entry_id: &str,
    vector: &[f32],
    text_hash: &str,
    features: &str,
    model: &str,
    now: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO entry_embeddings (entry_id, vec, text_hash, features, model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(entry_id) DO UPDATE SET vec = excluded.vec, text_hash = excluded.text_hash,
           features = excluded.features, model = excluded.model, created_at = excluded.created_at;",
        params![
            entry_id,
            knn::encode(vector),
            text_hash,
            features,
            model,
            now as i64
        ],
    )
    .map(|_| ())
    .map_err(err)
}

/// A stored vector, when it was computed from exactly this text.
pub fn embedding_for(
    conn: &Connection,
    entry_id: &str,
    text_hash: &str,
) -> Result<Option<Vec<f32>>> {
    conn.query_row(
        "SELECT vec FROM entry_embeddings WHERE entry_id = ?1 AND text_hash = ?2;",
        params![entry_id, text_hash],
        |row| row.get::<_, Vec<u8>>(0),
    )
    .optional()
    .map(|blob| blob.map(|blob| knn::decode(&blob)))
    .map_err(err)
}

/// Approved entries with vectors since `since`: the kNN pool.
pub fn labeled_pool(conn: &Connection, since: u64, exclude: &str) -> Result<Vec<Labeled>> {
    let mut stmt = conn
        .prepare(
            "SELECT v.vec, e.category_id, e.project_id, v.features,
               EXISTS (SELECT 1 FROM suggestions s WHERE s.entry_id = e.id
                         AND s.outcome IN ('changed', 'rejected'))
             FROM entry_embeddings v JOIN time_entries e ON e.id = v.entry_id
             WHERE e.deleted_at IS NULL AND e.status = 'approved' AND e.started_at >= ?1 AND e.id != ?2;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![since as i64, exclude], |row| {
            Ok(Labeled {
                vector: knn::decode(&row.get::<_, Vec<u8>>(0)?),
                category_id: row.get(1)?,
                project_id: row.get(2)?,
                features: row.get(3)?,
                corrected: row.get(4)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

// --- Rules ----------------------------------------------------------------

/// Enabled rules plus the Apps page defaults, which act as app/domain rules.
pub fn load_rules(conn: &Connection) -> Result<Vec<Rule>> {
    let mut rules: Vec<Rule> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, match_kind, pattern, category_id, project_id, priority, origin FROM rules
                 WHERE deleted_at IS NULL AND enabled = 1;",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Rule {
                    id: row.get(0)?,
                    match_kind: row.get(1)?,
                    pattern: row.get(2)?,
                    category_id: row.get(3)?,
                    project_id: row.get(4)?,
                    priority: row.get(5)?,
                    origin: row.get(6)?,
                })
            })
            .map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)?
    };

    let mut stmt = conn
        .prepare(
            "SELECT id, kind, identifier, default_category_id, default_project_id FROM apps
             WHERE deleted_at IS NULL AND excluded = 0
               AND (default_category_id IS NOT NULL OR default_project_id IS NOT NULL);",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| {
            let kind: String = row.get(1)?;
            Ok(Rule {
                id: format!("app:{}", row.get::<_, String>(0)?),
                match_kind: if kind == "site" { "domain" } else { "app" }.to_string(),
                pattern: row.get(2)?,
                category_id: row.get(3)?,
                project_id: row.get(4)?,
                priority: -1,
                origin: "app".to_string(),
            })
        })
        .map_err(err)?;
    for rule in rows {
        rules.push(rule.map_err(err)?);
    }
    Ok(rules)
}

#[allow(clippy::too_many_arguments)]
pub fn create_rule(
    conn: &Connection,
    match_kind: &str,
    pattern: &str,
    category_id: Option<&str>,
    project_id: Option<&str>,
    origin: &str,
    enabled: bool,
    now: u64,
) -> Result<String> {
    let id = new_id();
    conn.execute(
        "INSERT INTO rules (id, match_kind, pattern, category_id, project_id, priority, enabled, origin, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?8);",
        params![
            id,
            match_kind,
            pattern,
            category_id,
            project_id,
            enabled as i64,
            origin,
            now as i64
        ],
    )
    .map_err(err)?;
    Ok(id)
}

/// Offers "Always categorize figma.com as Design?" once the user has made
/// `RULE_SUGGESTION_MIN` consistent corrections for the entry's dominant app
/// or domain (counting a correction pending on this entry), and no rule or
/// dismissal exists for it yet.
pub fn rule_suggestion(
    conn: &Connection,
    entry_id: &str,
    category_id: Option<&str>,
    project_id: Option<&str>,
    suggestions: &[FieldSuggestion],
) -> Result<Option<RuleSuggestion>> {
    for suggestion in suggestions {
        let (field_column, current) = if suggestion.field == FIELD_CATEGORY {
            ("category_id", category_id)
        } else if suggestion.field == FIELD_PROJECT {
            ("project_id", project_id)
        } else {
            continue;
        };
        let (Some(current), Some(dominant)) = (current, suggestion.dominant.as_ref()) else {
            continue;
        };
        let corrected_here = suggestion.value_id.as_deref() != Some(current)
            && matches!(
                suggestion.outcome.as_deref(),
                None | Some("changed" | "rejected")
            );
        if !corrected_here {
            continue;
        }

        let existing: bool = conn
            .query_row(
                &format!(
                    "SELECT EXISTS (SELECT 1 FROM rules WHERE deleted_at IS NULL AND match_kind = ?1
                       AND lower(pattern) = lower(?2) AND {field_column} IS NOT NULL);"
                ),
                params![dominant.kind, dominant.key],
                |row| row.get(0),
            )
            .map_err(err)?;
        if existing {
            continue;
        }

        let earlier: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM time_entries e
                     WHERE e.deleted_at IS NULL AND e.status = 'approved' AND e.id != ?1
                       AND e.{field_column} = ?2
                       AND EXISTS (
                         SELECT 1 FROM suggestions s
                         WHERE s.entry_id = e.id AND s.field = ?3
                           AND s.outcome IN ('changed', 'rejected')
                           AND s.value_id IS NOT e.{field_column}
                           AND json_extract(s.signals, '$.dominant.kind') = ?4
                           AND json_extract(s.signals, '$.dominant.key') = ?5
                       );"
                ),
                params![
                    entry_id,
                    current,
                    suggestion.field,
                    dominant.kind,
                    dominant.key
                ],
                |row| row.get(0),
            )
            .map_err(err)?;
        let corrections = earlier as u32 + 1;
        if corrections >= RULE_SUGGESTION_MIN {
            return Ok(Some(RuleSuggestion {
                field: suggestion.field.clone(),
                match_kind: dominant.kind.clone(),
                pattern: dominant.key.clone(),
                label: dominant.label.clone(),
                value_id: Some(current.to_string()),
                corrections,
            }));
        }
    }
    Ok(None)
}

// --- Personal model artifacts ------------------------------------------------

#[derive(Debug, Clone)]
pub struct Artifact {
    pub id: String,
    pub path: String,
}

pub fn active_artifact(conn: &Connection, kind: &str) -> Result<Option<Artifact>> {
    conn.query_row(
        "SELECT id, path FROM model_artifacts
         WHERE kind = ?1 AND active = 1 ORDER BY trained_at DESC LIMIT 1;",
        params![kind],
        |row| {
            Ok(Artifact {
                id: row.get(0)?,
                path: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

/// Every trained version is recorded, including ones that lost the holdout
/// comparison (`active = 0`), so Settings can show why a retrain didn't swap.
pub fn insert_artifact(
    conn: &Connection,
    kind: &str,
    path: &str,
    n_examples: u32,
    holdout_acc: Option<f64>,
    active: bool,
    now: u64,
) -> Result<()> {
    if active {
        conn.execute(
            "UPDATE model_artifacts SET active = 0 WHERE kind = ?1;",
            params![kind],
        )
        .map_err(err)?;
    }
    conn.execute(
        "INSERT INTO model_artifacts (id, kind, path, trained_at, n_examples, holdout_acc, active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
        params![
            new_id(),
            kind,
            path,
            now as i64,
            n_examples,
            holdout_acc,
            active as i64
        ],
    )
    .map_err(err)?;
    prune_artifacts(conn, kind)
}

/// Drops all but the newest `ARTIFACT_HISTORY` inactive versions of a kind.
/// Their model files are already gone: the worker deletes a model's
/// directory as soon as it stops being active.
fn prune_artifacts(conn: &Connection, kind: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM model_artifacts WHERE kind = ?1 AND active = 0 AND id NOT IN (
           SELECT id FROM model_artifacts WHERE kind = ?1 AND active = 0
           ORDER BY trained_at DESC LIMIT ?2
         );",
        params![kind, ARTIFACT_HISTORY],
    )
    .map(|_| ())
    .map_err(err)
}

/// A trained version as Settings shows it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVersion {
    pub id: String,
    pub trained_at: u64,
    pub examples: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holdout_accuracy: Option<f64>,
    pub active: bool,
}

/// Newest first.
pub fn artifact_history(conn: &Connection, kind: &str, limit: u32) -> Result<Vec<ArtifactVersion>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, trained_at, n_examples, holdout_acc, active FROM model_artifacts
             WHERE kind = ?1 ORDER BY trained_at DESC, active DESC LIMIT ?2;",
        )
        .map_err(err)?;
    let rows = stmt
        .query_map(params![kind, limit], |row| {
            Ok(ArtifactVersion {
                id: row.get(0)?,
                trained_at: row.get::<_, i64>(1)? as u64,
                examples: row.get::<_, i64>(2)? as u32,
                holdout_accuracy: row.get(3)?,
                active: row.get::<_, i64>(4)? != 0,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

/// (entry id, content text, label) for every approved entry with a vector.
/// Project rows use `none` for "No project".
pub fn training_rows(conn: &Connection, field: Field) -> Result<Vec<(String, String, String)>> {
    let column = match field {
        Field::Category => "e.category_id",
        Field::Project => "COALESCE(e.project_id, 'none')",
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT e.id, v.features, {column} FROM time_entries e
             JOIN entry_embeddings v ON v.entry_id = e.id
             WHERE e.deleted_at IS NULL AND e.status = 'approved' AND e.category_id IS NOT NULL;"
        ))
        .map_err(err)?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

/// When a model of this kind was last trained or fitted, kept or not.
pub fn last_trained_at(conn: &Connection, kind: &str) -> Result<Option<u64>> {
    conn.query_row(
        "SELECT MAX(trained_at) FROM model_artifacts WHERE kind = ?1;",
        params![kind],
        |row| row.get::<_, Option<i64>>(0),
    )
    .map(|at| at.map(|at| at as u64))
    .map_err(err)
}

/// Verdicts recorded after `after` (calibration refit trigger).
pub fn verdicts_since(conn: &Connection, after: u64, now: u64) -> Result<u32> {
    let since = learning_since(conn)?;
    conn.query_row(
        &format!("SELECT COUNT(*) {VERDICTS} AND s.updated_at > ?3;"),
        params![
            since as i64,
            now.saturating_sub(AUTO_TRUSTED_AFTER_MS) as i64,
            after as i64
        ],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n as u32)
    .map_err(err)
}

/// Labeled entries approved since `since` (retraining trigger).
pub fn labeled_since(conn: &Connection, since: u64) -> Result<u32> {
    conn.query_row(
        "SELECT COUNT(*) FROM time_entries e JOIN entry_embeddings v ON v.entry_id = e.id
         WHERE e.deleted_at IS NULL AND e.status = 'approved' AND e.category_id IS NOT NULL
           AND e.updated_at >= ?1;",
        params![since as i64],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n as u32)
    .map_err(err)
}

/// Maps personal-model labels back to field values (`none` = no project).
pub fn label_from_model(label: &str) -> Label {
    (label != "none").then(|| label.to_string())
}
