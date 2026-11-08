//! One background thread that builds entries, drains `classify_jobs`, owns the sidecar and
//! retrains models. Reads use its own connection (never blocking the 1 Hz sampler); the
//! writer lock is never held while the sidecar thinks.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use super::apple::{self, AppleProvider};
use super::arbiter::{Decision, Label};
use super::calibration::Calibrator;
use super::features::{self, EntryFeatures, PreviousEntry};
use super::knn;
use super::pipeline;
use super::power::Conditions;
use super::prompt::Choice;
use super::provider::{EmbedModel, Provider, ProviderId};
use super::rules;
use super::sidecar::{self, Capabilities};
use super::store::{self, Job, JOB_CLASSIFY};
use super::{
    update_status, AiRuntime, Field, RuntimeState, EVENT_SUGGESTION_READY, PREFILL_THRESHOLD,
};
use crate::activity::{self, ActivitySegment};
use crate::models::TimeEntry;
use crate::settings::{AiSuggest, Settings};
use crate::timers::now_epoch_ms;
use crate::AppState;

const POLL: Duration = Duration::from_secs(30);
/// A burst of window switches rebuilds at most this often.
const REBUILD_DEBOUNCE: Duration = Duration::from_secs(3);
/// While the Foundation Model is missing, check again this often (it may
/// finish downloading, or the user may turn Apple Intelligence on).
const REPROBE_EVERY: Duration = Duration::from_secs(30 * 60);
const RETRAIN_CHECK_EVERY: Duration = Duration::from_secs(10 * 60);
/// New labeled entries that trigger retraining the personal models, and the
/// fewest a model is trained on.
pub const RETRAIN_AFTER_LABELS: u32 = 20;
/// With fewer new labels, the models still retrain once this long after the
/// last attempt ("nightly").
const RETRAIN_AT_LEAST_EVERY_MS: u64 = 24 * 60 * 60 * 1000;
/// One entry in `HOLDOUT_ONE_IN` is held out to judge a retrained model.
const HOLDOUT_ONE_IN: u64 = 5;
const CALIBRATION_CHECK_EVERY: Duration = Duration::from_secs(60);
const CALIBRATION_REFIT_AFTER: u32 = 10;
const CONTEXT_GAP_MS: u64 = 15 * 60 * 1000;
const BACKFILL_WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1000;

pub fn spawn(app: AppHandle, db_path: PathBuf, ml_dir: PathBuf) {
    std::thread::Builder::new()
        .name("ai-worker".to_string())
        .spawn(move || match Connection::open(&db_path) {
            Ok(conn) => {
                let _ = conn.busy_timeout(Duration::from_secs(5));
                Worker::new(app, conn, ml_dir).run();
            }
            Err(error) => eprintln!("ai worker could not open the database: {error}"),
        })
        .expect("spawn ai worker");
}

struct Worker {
    app: AppHandle,
    conn: Connection,
    apple: AppleProvider,
    ml_dir: PathBuf,
    last_rebuild: Option<Instant>,
    last_probe: Option<Instant>,
    last_retrain_check: Option<Instant>,
    last_calibration_check: Option<Instant>,
    applied: Option<Capabilities>,
}

enum Retrained {
    Skipped(String),
    Swapped { accuracy: Option<f64> },
    Kept { new: f64, old: f64 },
}

impl Worker {
    fn new(app: AppHandle, conn: Connection, ml_dir: PathBuf) -> Self {
        Self {
            app,
            conn,
            apple: AppleProvider::new(),
            ml_dir,
            last_rebuild: None,
            last_probe: None,
            last_retrain_check: None,
            last_calibration_check: None,
            applied: None,
        }
    }

    fn run(mut self) {
        if let Err(error) = self.write(store::requeue_interrupted) {
            eprintln!("ai worker: {error}");
        }
        self.probe();
        loop {
            self.rebuild_if_due();
            let now = now_epoch_ms();
            match self.write(|conn| store::enqueue_live_due(conn, now)) {
                Ok(count) if count > 0 => self.emit_entries_changed(),
                Err(error) => eprintln!("ai worker: live classification: {error}"),
                _ => {}
            }
            // Drain what's due, but come back to rebuild between jobs so a
            // long backlog never delays a block that just closed.
            for _ in 0..10 {
                match store::next_due(&self.conn, now_epoch_ms()) {
                    Ok(Some(job)) => self.run_job(job),
                    Ok(None) => break,
                    Err(error) => {
                        eprintln!("ai worker: {error}");
                        break;
                    }
                }
            }
            if self.apple.stop_if_idle() {
                update_status(&self.app, |s| s.runtime = RuntimeState::Stopped);
            }
            self.reprobe_if_due();
            self.retrain_if_due();
            self.refit_calibration_if_due(false);
            self.refresh_counts();

            let due_now = store::next_due(&self.conn, now_epoch_ms())
                .ok()
                .flatten()
                .is_some();
            if !due_now {
                self.app.state::<AiRuntime>().wait(POLL);
            }
        }
    }

    fn write<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        let state = self.app.state::<AppState>();
        let store = state.activity.lock().map_err(|e| e.to_string())?;
        f(store.conn())
    }

    fn emit_entries_changed(&self) {
        let _ = self.app.emit(crate::EVENT_ENTRIES_CHANGED, ());
    }

    fn settings(&self) -> Settings {
        self.app.state::<AppState>().settings_snapshot()
    }

    fn rebuild_if_due(&mut self) {
        if self
            .last_rebuild
            .is_some_and(|at| at.elapsed() < REBUILD_DEBOUNCE)
        {
            return;
        }
        self.last_rebuild = Some(Instant::now());
        let now = now_epoch_ms();
        let changed = {
            let state = self.app.state::<AppState>();
            let Ok(mut store) = state.activity.lock() else {
                return;
            };
            store.rebuild_range(now.saturating_sub(activity::REBUILD_WINDOW_MS), now, now)
        };
        match changed {
            Ok(true) => self.emit_entries_changed(),
            Ok(false) => {}
            Err(error) => eprintln!("ai worker: rebuild failed: {error}"),
        }
    }

    fn probe(&mut self) {
        self.last_probe = Some(Instant::now());
        if !self.apple.is_installed() {
            self.note_embedder(None);
            update_status(&self.app, |s| {
                s.engine = "rules".to_string();
                s.set_capabilities(None);
                s.runtime = RuntimeState::Unavailable;
            });
            return;
        }
        match self.apple.sidecar_mut().probe() {
            Ok(_) => self.apply_capabilities(),
            Err(error) => update_status(&self.app, |s| {
                s.engine = "rules".to_string();
                s.runtime = RuntimeState::Unavailable;
                s.last_error = Some(error);
            }),
        }
    }

    fn reprobe_if_due(&mut self) {
        let llm_ready = self.applied.as_ref().is_some_and(|c| c.llm == "available");
        if llm_ready || !self.apple.is_installed() {
            return;
        }
        if self
            .last_probe
            .is_none_or(|at| at.elapsed() >= REPROBE_EVERY)
        {
            self.probe();
        }
    }

    /// Mirrors the sidecar's capabilities into `AiStatus`; when the Foundation
    /// Model just became available, re-queues entries the fallback engine handled.
    fn apply_capabilities(&mut self) {
        let Some(caps) = self.apple.sidecar_capabilities().cloned() else {
            return;
        };
        let was_ready = self.applied.as_ref().map(|c| c.llm == "available");
        let running = self.apple.is_running();
        let provider_caps = apple::capabilities_of(&caps);
        self.note_embedder(provider_caps.embed.as_ref().map(|e| e.model.as_str()));
        update_status(&self.app, |s| {
            s.engine = if caps.llm == "available" {
                "full"
            } else if caps.embed {
                "fallback"
            } else {
                "rules"
            }
            .to_string();
            s.set_capabilities(Some(&provider_caps));
            s.os = Some(caps.os.clone());
            s.runtime = if running {
                RuntimeState::Running
            } else {
                RuntimeState::Stopped
            };
        });
        if was_ready == Some(false) && caps.llm == "available" {
            let now = now_epoch_ms();
            let requeued =
                self.write(|conn| store::requeue_fallback(conn, now - BACKFILL_WINDOW_MS, now));
            if matches!(requeued, Ok(n) if n > 0) {
                self.emit_entries_changed();
            }
        }
        self.applied = Some(caps);
    }

    /// Tells the core which embedder exists so it stops queueing embed jobs when there is none.
    fn note_embedder(&self, model: Option<&str>) {
        if let Err(error) = self.write(|conn| store::set_embedder(conn, model)) {
            eprintln!("ai worker: recording the embedder: {error}");
        }
    }

    fn refresh_counts(&self) {
        let queued = store::queued_count(&self.conn).unwrap_or(0);
        let outcomes = store::outcome_count(&self.conn, now_epoch_ms()).unwrap_or(0);
        let calibrated = store::active_calibration(&self.conn)
            .ok()
            .flatten()
            .is_some();
        let personal = store::active_artifact(&self.conn, Field::Category.as_str())
            .ok()
            .flatten()
            .is_some();
        let running = self.apple.is_running();
        let installed = self.apple.is_installed();
        update_status(&self.app, |s| {
            s.queued = queued;
            s.outcomes = outcomes;
            s.calibrated = calibrated;
            s.personal = personal;
            s.runtime = if !installed {
                RuntimeState::Unavailable
            } else if running {
                RuntimeState::Running
            } else {
                RuntimeState::Stopped
            };
        });
    }

    /// `None` means only T0.
    fn capabilities(&mut self) -> Option<Capabilities> {
        if !self.apple.is_installed() {
            return None;
        }
        match self.apple.sidecar_mut().probe() {
            Ok(caps) => {
                if self.applied.as_ref() != Some(&caps) || !self.apple.is_running() {
                    self.apply_capabilities();
                } else {
                    update_status(&self.app, |s| s.runtime = RuntimeState::Running);
                }
                Some(caps)
            }
            Err(error) => {
                update_status(&self.app, |s| {
                    s.engine = "rules".to_string();
                    s.runtime = RuntimeState::Unavailable;
                    s.last_error = Some(error);
                });
                None
            }
        }
    }

    fn run_job(&mut self, job: Job) {
        {
            let runtime = self.app.state::<AiRuntime>();
            runtime.set_busy(true);
            runtime.set_sidecar_pid(self.apple.pid());
        }

        let now = now_epoch_ms();
        if let Err(error) = self.write(|conn| store::set_job_state(conn, &job.id, "running", now)) {
            self.app.state::<AiRuntime>().set_busy(false);
            eprintln!("ai worker: {error}");
            return;
        }
        let result = if job.kind == JOB_CLASSIFY {
            self.classify(&job)
        } else {
            self.embed_only(&job)
        };
        {
            let runtime = self.app.state::<AiRuntime>();
            runtime.set_busy(false);
            runtime.set_sidecar_pid(self.apple.pid());
        }
        let now = now_epoch_ms();
        match result {
            Ok(()) => {
                if let Err(error) =
                    self.write(|conn| store::set_job_state(conn, &job.id, "done", now))
                {
                    eprintln!("ai worker: {error}");
                }
                // The entry has been read; erase what any private window in it
                // showed (see `activity::redact`).
                match self.write(|conn| activity::redact_private_detail(conn, now)) {
                    Ok(0) => {}
                    Ok(_) => self.emit_entries_changed(),
                    Err(error) => eprintln!("ai worker: erasing private detail: {error}"),
                }
            }
            Err(error) => {
                eprintln!("ai worker: {} {} failed: {error}", job.kind, job.entry_id);
                let gave_up = self
                    .write(|conn| store::fail_job(conn, &job, &error, now))
                    .unwrap_or(false);
                update_status(&self.app, |s| s.last_error = Some(error));
                if gave_up {
                    self.emit_entries_changed();
                    let _ = self
                        .app
                        .emit(EVENT_SUGGESTION_READY, json!({ "entryId": job.entry_id }));
                }
            }
        }
    }

    fn load_entry(&self, id: &str) -> Result<Option<TimeEntry>, String> {
        self.conn
            .query_row(
                "SELECT id, started_at, ended_at, description, category_id, project_id, status, approved_by, source, billable, invoice_id, created_at, updated_at, deleted_at, description_origin
                 FROM time_entries WHERE id = ?1;",
                params![id],
                activity::time_entry_from_row,
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    fn load_segments(&self, entry: &TimeEntry) -> Result<Vec<ActivitySegment>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id, private
                 FROM segments
                 WHERE entry_id = ?1 OR (entry_id IS NULL AND started_at >= ?2 AND ended_at <= ?3 AND kind != 'break')
                 ORDER BY started_at ASC;",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![entry.id, entry.started_at as i64, entry.ended_at as i64],
                activity::segment_from_row,
            )
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())
    }

    fn load_choices(&self, sql: &str) -> Result<Vec<Choice>, String> {
        let mut stmt = self.conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Choice {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    hint: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())
    }

    fn previous_category(&self, entry: &TimeEntry) -> Option<(String, u64, u64)> {
        self.conn
            .query_row(
                "SELECT c.name, e.started_at, e.ended_at FROM time_entries e
                 JOIN categories c ON c.id = e.category_id
                 WHERE e.deleted_at IS NULL AND e.id != ?1
                   AND e.ended_at <= ?2 AND e.ended_at >= ?3
                 ORDER BY e.ended_at DESC LIMIT 1;",
                params![
                    entry.id,
                    entry.started_at as i64 + 60_000,
                    entry.started_at.saturating_sub(CONTEXT_GAP_MS) as i64
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)? as u64,
                        row.get::<_, i64>(2)? as u64,
                    ))
                },
            )
            .ok()
    }

    /// Reuses a stored vector for identical text.
    fn vector_for(
        &mut self,
        entry_id: &str,
        features: &EntryFeatures,
        embed: Option<&EmbedModel>,
    ) -> Result<Option<Vec<f32>>, String> {
        let hash = text_hash(&features.content);
        if let Some(vector) = store::embedding_for(&self.conn, entry_id, &hash)? {
            return Ok(Some(vector));
        }
        let Some(embed) = embed else {
            return Ok(None);
        };
        let vectors = self.apple.embed(std::slice::from_ref(&features.content))?;
        let Some(vector) = vectors.into_iter().next().flatten() else {
            return Ok(None);
        };
        let model = embed.model.clone();
        let now = now_epoch_ms();
        self.write(|conn| {
            store::upsert_embedding(
                conn,
                entry_id,
                &vector,
                &hash,
                &features.content,
                &model,
                now,
            )
        })?;
        Ok(Some(vector))
    }

    fn personal_predict(&mut self, field: Field, text: &str) -> Option<HashMap<Label, f64>> {
        let artifact = store::active_artifact(&self.conn, field.as_str()).ok()??;
        #[derive(Deserialize)]
        struct Predicted {
            hypotheses: Vec<HashMap<String, f64>>,
        }
        let reply: Result<Predicted, String> = self.apple.sidecar_mut().call(
            "personal_predict",
            json!({ "model": artifact.path, "texts": [text] }),
            sidecar::TIMEOUT_QUICK,
        );
        match reply {
            Ok(reply) => reply.hypotheses.into_iter().next().map(|h| {
                h.into_iter()
                    .map(|(label, p)| (store::label_from_model(&label), p))
                    .collect()
            }),
            Err(error) => {
                eprintln!("ai worker: personal model: {error}");
                None
            }
        }
    }

    fn embed_only(&mut self, job: &Job) -> Result<(), String> {
        let Some(entry) = self.load_entry(&job.entry_id)? else {
            return Ok(());
        };
        if entry.deleted_at.is_some() {
            return Ok(());
        }
        let segments = self.load_segments(&entry)?;
        let mut features = features::extract(entry.started_at, entry.ended_at, &segments, None);
        if features.active_ms == 0 {
            // A hand-made entry with no captured activity: its description
            // is all there is to embed.
            features.content = entry.description.clone();
        }
        if !self.apple.is_installed() {
            return Ok(());
        }
        // A failed probe is retried; a provider without an embedder has nothing to do.
        let caps = self
            .capabilities()
            .ok_or("the model provider is not running")?;
        let Some(model) = embed_model(Some(&caps)) else {
            return Ok(());
        };
        self.vector_for(&entry.id, &features, Some(&model))
            .map(|_| ())
    }

    fn classify(&mut self, job: &Job) -> Result<(), String> {
        let Some(entry) = self.load_entry(&job.entry_id)? else {
            return Ok(());
        };
        if entry.deleted_at.is_some() || entry.status == "approved" {
            return Ok(());
        }
        let settings = self.settings();
        let now = now_epoch_ms();

        let segments = self.load_segments(&entry)?;
        let categories = self.load_choices(
            "SELECT id, name, description, ai_prompt FROM categories
             WHERE deleted_at IS NULL AND archived = 0 ORDER BY sort, name;",
        )?;
        let projects = self.load_choices(
            "SELECT id, name, description, ai_hints FROM projects
             WHERE deleted_at IS NULL AND status = 'active' ORDER BY name;",
        )?;
        if categories.is_empty() {
            return Err("there are no categories to suggest".to_string());
        }
        let want_project =
            settings.ai_suggest == AiSuggest::CategoryProject && !projects.is_empty();

        let previous = self.previous_category(&entry);
        let features = features::extract(
            entry.started_at,
            entry.ended_at,
            &segments,
            previous
                .as_ref()
                .map(|(category, start, end)| PreviousEntry {
                    category,
                    started_at: *start,
                    ended_at: *end,
                }),
        );

        let rule_set = store::load_rules(&self.conn)?;
        let rule_category = rules::evaluate(&rule_set, &segments, Field::Category);
        let rule_project = want_project
            .then(|| rules::evaluate(&rule_set, &segments, Field::Project))
            .flatten();

        let caps = self.capabilities();
        let vector =
            match self.vector_for(&entry.id, &features, embed_model(caps.as_ref()).as_ref()) {
                Ok(vector) => vector,
                Err(error) => {
                    eprintln!("ai worker: embedding failed: {error}");
                    None
                }
            };
        let pool =
            store::labeled_pool(&self.conn, now.saturating_sub(knn::LOOKBACK_MS), &entry.id)?;
        let personal_category = caps
            .is_some()
            .then(|| self.personal_predict(Field::Category, &features.content))
            .flatten();
        let personal_project = (caps.is_some() && want_project)
            .then(|| self.personal_predict(Field::Project, &features.content))
            .flatten();
        let labeled_count = store::labeled_count(&self.conn)?;
        let calibrator = store::active_calibration(&self.conn)?;

        let llm = if caps.is_some() { self.llm() } else { None };
        let labeled = pipeline::label(
            pipeline::Inputs {
                features: &features,
                segments: &segments,
                categories: &categories,
                projects: &projects,
                want_project,
                rule_category: rule_category.as_ref(),
                rule_project: rule_project.as_ref(),
                vector: vector.as_deref(),
                pool: &pool,
                personal_category,
                personal_project,
                labeled: labeled_count,
                calibration: calibrator.as_ref(),
                custom: &settings.ai_custom_prompt,
            },
            llm,
        );
        if let Some(error) = labeled.llm.as_ref().and_then(|t| t.error.clone()) {
            update_status(&self.app, |s| s.last_error = Some(error));
        }
        let model_version = model_version(caps.as_ref(), &self.apple.id(), &self.conn);
        let outcome = Outcome {
            category: labeled.category,
            project: labeled.project,
            want_project,
            description: labeled.description,
            rule_category: rule_category.is_some(),
            rule_project: rule_project.is_some(),
        };
        let now = now_epoch_ms();
        self.write(|conn| {
            persist(
                conn,
                &entry.id,
                &outcome,
                features.dominant.as_ref(),
                &model_version,
                &settings,
                now,
            )
        })?;
        self.emit_entries_changed();
        let _ = self
            .app
            .emit(EVENT_SUGGESTION_READY, json!({ "entryId": entry.id }));
        Ok(())
    }

    /// The single active-LLM slot: Apple's model when ready. A picker for local models or
    /// none comes later (`docs/design/local-models.md`).
    fn llm(&mut self) -> Option<&mut dyn Provider> {
        let ready = self.apple.capabilities().is_ok_and(|c| c.llm.is_ready());
        ready.then_some(&mut self.apple as &mut dyn Provider)
    }

    /// Retrains on "Retrain now", or when due (20+ new labels, or any new one a day
    /// after the last attempt) and the Mac is idle, on AC and not in Low Power Mode.
    /// Calibration is refit right after, since the raw scores just changed.
    fn retrain_if_due(&mut self) {
        let requested = self.app.state::<AiRuntime>().take_retrain_request();
        if !requested
            && self
                .last_retrain_check
                .is_some_and(|at| at.elapsed() < RETRAIN_CHECK_EVERY)
        {
            return;
        }
        self.last_retrain_check = Some(Instant::now());
        if !self.apple.is_installed() {
            if requested {
                update_status(&self.app, |s| {
                    s.retrain = "idle".to_string();
                    s.retrain_note = Some("On-device ML isn't available in this build".to_string());
                });
            }
            return;
        }

        let now = now_epoch_ms();
        let due: Vec<Field> = [Field::Category, Field::Project]
            .into_iter()
            .filter(|field| requested || self.retrain_due(*field, now))
            .collect();
        if due.is_empty() {
            return;
        }
        if !requested {
            if let Some(why) = Conditions::read().blocker() {
                update_status(&self.app, |s| {
                    s.retrain_note = Some(format!("Retrain due, {why}"));
                });
                return;
            }
        }

        update_status(&self.app, |s| s.retrain = "running".to_string());
        {
            let runtime = self.app.state::<AiRuntime>();
            runtime.set_busy(true);
            runtime.set_sidecar_pid(self.apple.pid());
        }
        let mut notes = Vec::new();
        for field in due {
            let what = format!("{} model", field.as_str());
            match self.retrain(field) {
                Ok(Retrained::Skipped(why)) if requested || field == Field::Category => {
                    notes.push(format!("{}: {why}", capitalize(&what)))
                }
                Ok(Retrained::Skipped(_)) => {}
                Ok(Retrained::Swapped { accuracy }) => notes.push(match accuracy {
                    Some(accuracy) => format!(
                        "{} updated ({}% on the holdout)",
                        capitalize(&what),
                        (accuracy * 100.0).round()
                    ),
                    None => format!("{} updated", capitalize(&what)),
                }),
                Ok(Retrained::Kept { new, old }) => notes.push(format!(
                    "Kept the current {what}: the new one scored {}% vs {}% on the holdout",
                    (new * 100.0).round(),
                    (old * 100.0).round()
                )),
                Err(error) => {
                    eprintln!("ai worker: retraining the {what} failed: {error}");
                    notes.push(format!("Retraining the {what} failed: {error}"));
                }
            }
        }
        self.refit_calibration_if_due(true);
        let note = (!notes.is_empty()).then(|| notes.join(" · "));
        update_status(&self.app, |s| {
            s.retrain = "idle".to_string();
            s.retrain_note = note;
        });
        {
            let runtime = self.app.state::<AiRuntime>();
            runtime.set_busy(false);
            runtime.set_sidecar_pid(self.apple.pid());
        }
        self.refresh_counts();
    }

    fn retrain_due(&self, field: Field, now: u64) -> bool {
        let last = store::last_trained_at(&self.conn, field.as_str())
            .ok()
            .flatten()
            .unwrap_or(0);
        let new = store::labeled_since(&self.conn, last).unwrap_or(0);
        new >= RETRAIN_AFTER_LABELS
            || (new > 0 && now.saturating_sub(last) >= RETRAIN_AT_LEAST_EVERY_MS)
    }

    /// Trains on approved entries minus a 20% holdout and swaps the model in only
    /// if it scores at least as well as the current one on that holdout. Every
    /// attempt is versioned in `model_artifacts`.
    fn retrain(&mut self, field: Field) -> Result<Retrained, String> {
        let kind = field.as_str();
        let epoch = store::learning_since(&self.conn)?;
        let rows = store::training_rows(&self.conn, field)?;
        let labels: HashSet<&str> = rows.iter().map(|(_, _, label)| label.as_str()).collect();
        if rows.len() < RETRAIN_AFTER_LABELS as usize {
            return Ok(Retrained::Skipped(format!(
                "needs {RETRAIN_AFTER_LABELS} reviewed entries, has {}",
                rows.len()
            )));
        }
        if labels.len() < 2 {
            let what = match field {
                Field::Category => "at least two categories",
                Field::Project => "at least two projects (No project counts as one)",
            };
            return Ok(Retrained::Skipped(format!(
                "needs reviewed entries in {what}"
            )));
        }
        let active = store::active_artifact(&self.conn, kind)?;

        std::fs::create_dir_all(&self.ml_dir).map_err(|e| e.to_string())?;
        let dataset = self.ml_dir.join(format!("{kind}-train.jsonl"));
        let mut file = std::fs::File::create(&dataset).map_err(|e| e.to_string())?;
        let mut holdout: Vec<(&str, &str)> = Vec::new();
        for (entry_id, text, label) in &rows {
            let is_holdout = in_holdout(entry_id);
            if is_holdout {
                holdout.push((text, label));
            }
            let line = json!({ "text": text, "label": label, "holdout": is_holdout });
            writeln!(file, "{line}").map_err(|e| e.to_string())?;
        }
        drop(file);

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Trained {
            path: String,
            train_examples: u32,
            holdout_accuracy: Option<f64>,
        }
        let out = self
            .ml_dir
            .join(format!("{kind}-{}.mlmodelc", uuid::Uuid::now_v7()));
        let trained: Result<Trained, String> = self.apple.sidecar_mut().call(
            "train",
            json!({ "dataset": dataset, "out": out }),
            sidecar::TIMEOUT_TRAIN,
        );
        let _ = std::fs::remove_file(&dataset);
        let trained = trained?;

        let old_accuracy = match &active {
            Some(artifact) if !holdout.is_empty() => {
                self.holdout_accuracy(&artifact.path, &holdout)
            }
            _ => None,
        };
        let better = match (trained.holdout_accuracy, old_accuracy) {
            (Some(new), Some(old)) => new >= old,
            _ => true,
        };
        let now = now_epoch_ms();
        let recorded = self.write(|conn| {
            // "Reset learned data" ran while this model trained on the old
            // data: drop it instead of bringing that data back.
            if store::learning_since(conn)? != epoch {
                return Ok(false);
            }
            store::insert_artifact(
                conn,
                kind,
                &trained.path,
                trained.train_examples,
                trained.holdout_accuracy,
                better,
                now,
            )?;
            Ok(true)
        })?;
        if !recorded {
            let _ = std::fs::remove_dir_all(&trained.path);
            return Ok(Retrained::Skipped("learned data was reset".to_string()));
        }
        let retired = if better {
            active.map(|a| a.path)
        } else {
            Some(trained.path)
        };
        if let Some(path) = retired {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(match (better, trained.holdout_accuracy, old_accuracy) {
            (false, Some(new), Some(old)) => Retrained::Kept { new, old },
            (_, accuracy, _) => Retrained::Swapped { accuracy },
        })
    }

    /// Refits isotonic calibration once `CALIBRATION_REFIT_AFTER` new verdicts exist
    /// (or `MIN_SAMPLES` the first time), checked every minute; `force` refits now.
    /// Pure Rust over a few thousand rows, so it needs no power gate.
    fn refit_calibration_if_due(&mut self, force: bool) {
        if !force
            && self
                .last_calibration_check
                .is_some_and(|at| at.elapsed() < CALIBRATION_CHECK_EVERY)
        {
            return;
        }
        self.last_calibration_check = Some(Instant::now());
        if let Err(error) = self.refit_calibration(force) {
            eprintln!("ai worker: calibration refit failed: {error}");
        }
    }

    fn refit_calibration(&mut self, force: bool) -> Result<(), String> {
        let now = now_epoch_ms();
        let epoch = store::learning_since(&self.conn)?;
        let fitted_at = store::last_trained_at(&self.conn, store::KIND_CALIBRATION)?;
        let due = force
            || match fitted_at {
                None => true,
                Some(at) => store::verdicts_since(&self.conn, at, now)? >= CALIBRATION_REFIT_AFTER,
            };
        if !due {
            return Ok(());
        }
        let samples = store::calibration_samples(&self.conn, now)?;
        let Some(calibrator) = Calibrator::fit(&samples) else {
            return Ok(());
        };
        let saved = self.write(|conn| {
            if store::learning_since(conn)? != epoch {
                return Ok(false);
            }
            store::insert_calibration(conn, &calibrator, now)?;
            Ok(true)
        })?;
        if saved {
            update_status(&self.app, |s| s.calibrated = true);
        }
        Ok(())
    }

    fn holdout_accuracy(&mut self, model: &str, holdout: &[(&str, &str)]) -> Option<f64> {
        #[derive(Deserialize)]
        struct Predicted {
            hypotheses: Vec<HashMap<String, f64>>,
        }
        let texts: Vec<&str> = holdout.iter().map(|(text, _)| *text).collect();
        let reply: Predicted = self
            .apple
            .sidecar_mut()
            .call(
                "personal_predict",
                json!({ "model": model, "texts": texts }),
                sidecar::TIMEOUT_TRAIN,
            )
            .ok()?;
        let correct = reply
            .hypotheses
            .iter()
            .zip(holdout)
            .filter(|(hypotheses, (_, label))| {
                hypotheses
                    .iter()
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .is_some_and(|(top, _)| top == label)
            })
            .count();
        Some(correct as f64 / holdout.len() as f64)
    }
}

struct Outcome {
    category: Option<Decision>,
    project: Option<Decision>,
    want_project: bool,
    description: Option<String>,
    rule_category: bool,
    rule_project: bool,
}

/// Saves suggestions and applies them to the entry, all or nothing. Unset fields
/// prefill at `PREFILL_THRESHOLD`; only the template description is replaced; the
/// entry auto-approves if auto-accept is on, all fields clear it, and it is untouched.
fn persist(
    conn: &Connection,
    entry_id: &str,
    outcome: &Outcome,
    dominant: Option<&crate::models::Dominant>,
    model_version: &str,
    settings: &Settings,
    now: u64,
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let current: Option<(String, Option<String>, Option<String>, String)> = tx
        .query_row(
            "SELECT status, category_id, project_id, description_origin FROM time_entries
             WHERE id = ?1 AND deleted_at IS NULL;",
            params![entry_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((status, category_id, project_id, description_origin)) = current else {
        return Ok(());
    };
    if status == "approved" {
        return Ok(());
    }

    let threshold = f64::from(settings.auto_accept_percent) / 100.0;
    let clears =
        |decision: &Option<Decision>| decision.as_ref().is_some_and(|d| d.confidence >= threshold);
    let untouched = category_id.is_none() && project_id.is_none();
    let auto = status != "building"
        && settings.auto_accept
        && untouched
        && clears(&outcome.category)
        && (!outcome.want_project || clears(&outcome.project));
    let auto_outcome = auto.then_some("auto");

    let mut payload = serde_json::Map::new();
    let mut next_category = category_id.clone();
    let mut next_project = project_id.clone();
    if let Some(decision) = &outcome.category {
        store::insert_suggestion(
            &tx,
            entry_id,
            Field::Category,
            decision,
            dominant,
            model_version,
            auto_outcome,
            now,
        )?;
        payload.insert("category".into(), json!(decision.confidence));
        if next_category.is_none() && decision.confidence >= PREFILL_THRESHOLD {
            next_category = decision.value.clone();
        }
    }
    if let Some(decision) = &outcome.project {
        store::insert_suggestion(
            &tx,
            entry_id,
            Field::Project,
            decision,
            dominant,
            model_version,
            auto_outcome,
            now,
        )?;
        payload.insert("project".into(), json!(decision.confidence));
        if next_project.is_none() && decision.confidence >= PREFILL_THRESHOLD {
            next_project = decision.value.clone();
        }
    }

    let (next_status, approved_by) = if auto {
        let by_rules = outcome.rule_category && (!outcome.want_project || outcome.rule_project);
        ("approved", Some(if by_rules { "rule" } else { "auto" }))
    } else if status == "processing" {
        ("pending", None)
    } else {
        (status.as_str(), None)
    };
    let description = outcome
        .description
        .as_ref()
        .filter(|_| description_origin == "template");
    tx.execute(
        "UPDATE time_entries SET category_id = ?2, project_id = ?3, status = ?4,
           approved_by = COALESCE(?5, approved_by),
           description = COALESCE(?6, description),
           description_origin = CASE WHEN ?6 IS NULL THEN description_origin ELSE 'ai' END,
           updated_at = ?7
         WHERE id = ?1;",
        params![
            entry_id,
            next_category,
            next_project,
            next_status,
            approved_by,
            description,
            now as i64
        ],
    )
    .map_err(|e| e.to_string())?;

    let actor = if outcome.rule_category { "rule" } else { "ai" };
    let log = |kind: &str, payload: Option<String>| {
        tx.execute(
            "INSERT INTO entry_events (id, entry_id, kind, actor, payload, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            params![uuid::Uuid::now_v7().to_string(), entry_id, kind, actor, payload, now as i64],
        )
        .map_err(|e| e.to_string())
    };
    if outcome.category.is_some() || outcome.project.is_some() {
        log("suggested", Some(Value::Object(payload).to_string()))?;
    }
    if auto {
        log("auto_approved", None)?;
    }
    tx.commit().map_err(|e| e.to_string())
}

fn embed_model(caps: Option<&Capabilities>) -> Option<EmbedModel> {
    caps.and_then(|c| apple::capabilities_of(c).embed)
}

fn model_version(caps: Option<&Capabilities>, llm: &ProviderId, conn: &Connection) -> String {
    let artifact = |kind: &str| {
        store::active_artifact(conn, kind)
            .ok()
            .flatten()
            .map(|a| a.id)
            .unwrap_or_else(|| "none".to_string())
    };
    let calibration = artifact(store::KIND_CALIBRATION);
    match caps {
        Some(caps) => format!(
            "{} emb:r{} pm:{} cal:{calibration}",
            llm.tag(),
            caps.embed_revision,
            artifact(Field::Category.as_str())
        ),
        None => format!("rules-only cal:{calibration}"),
    }
}

/// Whether an entry belongs to the 20% holdout. Keyed on the entry id, so an
/// entry stays in (or out of) the holdout across retrains and the old and
/// new models are always compared on data neither trained on.
fn in_holdout(entry_id: &str) -> bool {
    fnv1a(entry_id).is_multiple_of(HOLDOUT_ONE_IN)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// FNV-1a, hex. Stable across Rust releases, unlike `DefaultHasher`.
fn text_hash(text: &str) -> String {
    format!("{:016x}", fnv1a(text))
}

fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::arbiter::Decision;

    fn db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        openrize_core::migrations::run_migrations(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO time_entries (id, started_at, ended_at, description, status, source, created_at, updated_at)
             VALUES ('e1', 0, 1000, 'Xcode: main.rs', 'processing', 'auto', 0, 0);",
            [],
        )
        .unwrap();
        conn
    }

    fn decision(value: &str, confidence: f64) -> Decision {
        Decision {
            value: Some(value.to_string()),
            confidence,
            raw_confidence: confidence,
            alternatives: vec![],
            signals: vec![],
            engine: "full",
            tier: "model",
        }
    }

    fn entry(conn: &Connection) -> (String, Option<String>, Option<String>, String) {
        conn.query_row(
            "SELECT status, category_id, approved_by, description FROM time_entries WHERE id = 'e1';",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
    }

    fn outcome(category: f64) -> Outcome {
        Outcome {
            category: Some(decision("coding", category)),
            project: None,
            want_project: false,
            description: Some("Refactored the activity store".to_string()),
            rule_category: false,
            rule_project: false,
        }
    }

    #[test]
    fn a_confident_suggestion_auto_approves() {
        let conn = db();
        persist(
            &conn,
            "e1",
            &outcome(0.97),
            None,
            "v",
            &Settings::default(),
            5,
        )
        .unwrap();
        let (status, category, by, description) = entry(&conn);
        assert_eq!(status, "approved");
        assert_eq!(category.as_deref(), Some("coding"));
        assert_eq!(by.as_deref(), Some("auto"));
        assert_eq!(description, "Refactored the activity store");
        let outcome: String = conn
            .query_row("SELECT outcome FROM suggestions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(outcome, "auto");
    }

    #[test]
    fn a_medium_suggestion_prefills_and_waits_for_review() {
        let conn = db();
        persist(
            &conn,
            "e1",
            &outcome(0.7),
            None,
            "v",
            &Settings::default(),
            5,
        )
        .unwrap();
        let (status, category, by, _) = entry(&conn);
        assert_eq!(status, "pending");
        assert_eq!(category.as_deref(), Some("coding"));
        assert_eq!(by, None);
    }

    #[test]
    fn a_low_suggestion_preselects_nothing() {
        let conn = db();
        persist(
            &conn,
            "e1",
            &outcome(0.4),
            None,
            "v",
            &Settings::default(),
            5,
        )
        .unwrap();
        let (status, category, _, _) = entry(&conn);
        assert_eq!(status, "pending");
        assert_eq!(category, None);
    }

    #[test]
    fn a_user_choice_is_never_overwritten() {
        let conn = db();
        conn.execute(
            "UPDATE time_entries SET category_id = 'design', description_origin = 'user' WHERE id = 'e1';",
            [],
        )
        .unwrap();
        persist(
            &conn,
            "e1",
            &outcome(0.99),
            None,
            "v",
            &Settings::default(),
            5,
        )
        .unwrap();
        let (status, category, _, description) = entry(&conn);
        assert_eq!(status, "pending");
        assert_eq!(category.as_deref(), Some("design"));
        assert_eq!(description, "Xcode: main.rs");
    }

    #[test]
    fn model_version_keeps_its_format() {
        let conn = db();
        let caps = Capabilities {
            llm: "available".to_string(),
            embed: true,
            embed_dimension: 512,
            embed_revision: 2,
            personal: true,
            os: "26.1".to_string(),
            version: "1".to_string(),
        };
        assert_eq!(
            model_version(Some(&caps), &apple::id_for(Some(&caps)), &conn),
            "fm:26.1:available emb:r2 pm:none cal:none"
        );
        assert_eq!(
            model_version(None, &apple::id_for(None), &conn),
            "rules-only cal:none"
        );
    }

    #[test]
    fn no_title_guard_keeps_template_description() {
        let conn = db();
        let mut no_title_outcome = outcome(0.95);
        no_title_outcome.description = None;
        persist(
            &conn,
            "e1",
            &no_title_outcome,
            None,
            "v",
            &Settings::default(),
            5,
        )
        .unwrap();
        let (_, _, _, description) = entry(&conn);
        assert_eq!(description, "Xcode: main.rs");
        let origin: String = conn
            .query_row(
                "SELECT description_origin FROM time_entries WHERE id = 'e1';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(origin, "template");
    }
}
