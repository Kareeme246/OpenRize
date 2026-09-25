//! AI effectiveness (Settings → Categories & AI): how often suggestions are
//! accepted, auto-approved, or changed; which tier decided them; whether the
//! displayed confidence kept its promise; and what a given auto-accept
//! threshold would have done over the same period.
//!
//! Everything is computed from the latest suggestion per entry and field, so
//! a re-run replaces the suggestion it superseded instead of counting twice.

use std::collections::BTreeMap;

use rusqlite::{params, Connection};
use serde::Serialize;

use super::arbiter::{TIER_MODEL, TIER_PERSONAL, TIER_RULE};
use super::calibration::{self, Calibrator, ReliabilityBin};
use super::store::{self, ArtifactVersion, AUTO_TRUSTED_AFTER_MS, KIND_CALIBRATION};
use super::worker::RETRAIN_AFTER_LABELS;
use super::Field;
use crate::settings::{MAX_AUTO_ACCEPT_PERCENT, MIN_AUTO_ACCEPT_PERCENT};

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const RELIABILITY_BINS: usize = 10;
const HISTORY: u32 = 5;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierSplit {
    pub rule: u32,
    pub personal: u32,
    pub model: u32,
}

/// "At 95%, 38% of last month's entries would have auto-approved, and 1 of
/// them would have been wrong".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdPreview {
    pub percent: u8,
    /// Entries whose every suggestion clears the threshold today.
    pub auto_approved: u32,
    /// Of those, entries where the user changed or rejected a suggestion.
    pub wrong: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationReport {
    /// Displayed confidence vs actual accept rate over the period (model
    /// suggestions with a verdict; rule hits are certain by definition).
    pub bins: Vec<ReliabilityBin>,
    pub samples: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_error: Option<f64>,
    /// The active curve, as (raw, displayed) knots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve: Option<Vec<(f64, f64)>>,
    /// Newest first.
    pub history: Vec<ArtifactVersion>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReport {
    /// Versions of the category and project personal models, newest first.
    pub category: Vec<ArtifactVersion>,
    pub project: Vec<ArtifactVersion>,
    /// Labels approved since the category model last trained, toward the
    /// `retrain_after` that triggers the next retrain.
    pub new_labels: u32,
    pub retrain_after: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMetrics {
    pub days: u32,
    /// Entries that got a suggestion in the period.
    pub entries: u32,
    /// Of those, entries approved or rejected so far.
    pub decided: u32,
    /// Of the decided, entries approved without review.
    pub auto_approved: u32,
    /// Per-field suggestions a person ruled on, and how.
    pub reviewed: u32,
    pub accepted: u32,
    pub changed: u32,
    pub rejected: u32,
    pub tiers: TierSplit,
    pub calibration: CalibrationReport,
    /// One row per whole percent the threshold allows.
    pub thresholds: Vec<ThresholdPreview>,
    pub models: ModelReport,
    /// When "Reset learned data" last ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub learning_since: Option<u64>,
}

/// The latest suggestion for one entry and field.
#[derive(Debug, Clone)]
struct Row {
    entry_id: String,
    confidence: f64,
    raw_confidence: Option<f64>,
    engine: String,
    tier: Option<String>,
    outcome: Option<String>,
    updated_at: u64,
    status: String,
    approved_by: Option<String>,
}

impl Row {
    fn is_rule(&self) -> bool {
        self.engine == "rules"
    }

    /// Rows from before P4 have no tier; the engine is the best guess.
    fn tier(&self) -> &str {
        match self.tier.as_deref() {
            Some(tier) => tier,
            None if self.is_rule() => TIER_RULE,
            None if self.engine == "fallback" => TIER_PERSONAL,
            None => TIER_MODEL,
        }
    }

    fn wrong(&self) -> bool {
        matches!(self.outcome.as_deref(), Some("changed" | "rejected"))
    }

    /// Whether this row tells calibration anything: a person's verdict, or
    /// an auto-approval that stood for a day.
    fn verdict(&self, now: u64) -> Option<bool> {
        match self.outcome.as_deref() {
            Some("accepted") => Some(true),
            Some("changed" | "rejected") => Some(false),
            Some("auto") if self.updated_at <= now.saturating_sub(AUTO_TRUSTED_AFTER_MS) => {
                Some(true)
            }
            _ => None,
        }
    }

    /// What this suggestion would show today, under the current calibration.
    fn confidence_now(&self, calibrator: Option<&Calibrator>) -> f64 {
        if self.is_rule() {
            return self.confidence;
        }
        match self.raw_confidence {
            Some(raw) => calibration::display(raw, calibrator),
            None => self.confidence,
        }
    }
}

pub fn metrics(conn: &Connection, days: u32, now: u64) -> Result<AiMetrics, String> {
    let days = days.clamp(1, 365);
    let since = now.saturating_sub(u64::from(days) * DAY_MS);
    let rows = latest_rows(conn, since)?;
    let calibrator = store::active_calibration(conn)?;
    let mut out = summarize(&rows, calibrator.as_ref(), days, now);

    out.calibration.curve = calibrator.map(|c| c.x.into_iter().zip(c.y).collect());
    out.calibration.history = store::artifact_history(conn, KIND_CALIBRATION, HISTORY)?;
    let category = Field::Category.as_str();
    let last_trained = store::last_trained_at(conn, category)?.unwrap_or(0);
    out.models = ModelReport {
        category: store::artifact_history(conn, category, HISTORY)?,
        project: store::artifact_history(conn, Field::Project.as_str(), HISTORY)?,
        new_labels: store::labeled_since(conn, last_trained)?,
        retrain_after: RETRAIN_AFTER_LABELS,
    };
    let reset = store::learning_since(conn)?;
    out.learning_since = (reset > 0).then_some(reset);
    Ok(out)
}

fn latest_rows(conn: &Connection, since: u64) -> Result<Vec<Row>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT s.entry_id, s.confidence, s.raw_confidence, s.engine, s.tier, s.outcome,
               s.updated_at, e.status, e.approved_by
             FROM suggestions s JOIN time_entries e ON e.id = s.entry_id
             WHERE s.deleted_at IS NULL AND e.deleted_at IS NULL AND s.created_at >= ?1
               AND s.created_at = (
                 SELECT MAX(created_at) FROM suggestions
                 WHERE entry_id = s.entry_id AND field = s.field AND deleted_at IS NULL
               );",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![since as i64], |row| {
            Ok(Row {
                entry_id: row.get(0)?,
                confidence: row.get(1)?,
                raw_confidence: row.get(2)?,
                engine: row.get(3)?,
                tier: row.get(4)?,
                outcome: row.get(5)?,
                updated_at: row.get::<_, i64>(6)? as u64,
                status: row.get(7)?,
                approved_by: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())
}

fn summarize(rows: &[Row], calibrator: Option<&Calibrator>, days: u32, now: u64) -> AiMetrics {
    let mut by_entry: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for row in rows {
        by_entry.entry(row.entry_id.as_str()).or_default().push(row);
    }

    let (mut decided, mut auto_approved) = (0, 0);
    let mut entry_confidence: Vec<(f64, bool)> = Vec::with_capacity(by_entry.len());
    for fields in by_entry.values() {
        let first = fields[0];
        let approved = first.status == "approved";
        let rejected = fields
            .iter()
            .any(|r| r.outcome.as_deref() == Some("rejected"));
        if approved || rejected {
            decided += 1;
        }
        if approved && matches!(first.approved_by.as_deref(), Some("auto" | "rule")) {
            auto_approved += 1;
        }
        let weakest = fields
            .iter()
            .map(|r| r.confidence_now(calibrator))
            .fold(f64::INFINITY, f64::min);
        entry_confidence.push((weakest, fields.iter().any(|r| r.wrong())));
    }

    let count = |outcome: &str| {
        rows.iter()
            .filter(|r| r.outcome.as_deref() == Some(outcome))
            .count() as u32
    };
    let (accepted, changed, rejected) = (count("accepted"), count("changed"), count("rejected"));

    let mut tiers = TierSplit::default();
    for row in rows {
        match row.tier() {
            TIER_RULE => tiers.rule += 1,
            TIER_PERSONAL => tiers.personal += 1,
            _ => tiers.model += 1,
        }
    }

    let shown: Vec<(f64, bool)> = rows
        .iter()
        .filter(|r| !r.is_rule())
        .filter_map(|r| r.verdict(now).map(|accepted| (r.confidence, accepted)))
        .collect();
    let bins = calibration::reliability(&shown, RELIABILITY_BINS);

    let thresholds = (MIN_AUTO_ACCEPT_PERCENT..=MAX_AUTO_ACCEPT_PERCENT)
        .map(|percent| {
            let threshold = f64::from(percent) / 100.0;
            let clearing = entry_confidence.iter().filter(|(p, _)| *p >= threshold);
            ThresholdPreview {
                percent,
                auto_approved: clearing.clone().count() as u32,
                wrong: clearing.filter(|(_, wrong)| *wrong).count() as u32,
            }
        })
        .collect();

    AiMetrics {
        days,
        entries: by_entry.len() as u32,
        decided,
        auto_approved,
        reviewed: accepted + changed + rejected,
        accepted,
        changed,
        rejected,
        tiers,
        calibration: CalibrationReport {
            samples: shown.len() as u32,
            expected_error: calibration::expected_error(&bins),
            bins,
            curve: None,
            history: Vec::new(),
        },
        thresholds,
        models: ModelReport {
            category: Vec::new(),
            project: Vec::new(),
            new_labels: 0,
            retrain_after: RETRAIN_AFTER_LABELS,
        },
        learning_since: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 100 * DAY_MS;

    fn row(entry: &str, confidence: f64, outcome: Option<&str>) -> Row {
        Row {
            entry_id: entry.to_string(),
            confidence,
            raw_confidence: Some(confidence),
            engine: "full".to_string(),
            tier: Some(TIER_MODEL.to_string()),
            outcome: outcome.map(str::to_string),
            updated_at: NOW - 2 * DAY_MS,
            status: if outcome.is_some() && outcome != Some("rejected") {
                "approved"
            } else {
                "pending"
            }
            .to_string(),
            approved_by: match outcome {
                Some("auto") => Some("auto".to_string()),
                Some("accepted" | "changed") => Some("user".to_string()),
                _ => None,
            },
        }
    }

    fn preview(metrics: &AiMetrics, percent: u8) -> &ThresholdPreview {
        metrics
            .thresholds
            .iter()
            .find(|t| t.percent == percent)
            .unwrap()
    }

    #[test]
    fn rates_count_people_verdicts_and_auto_approvals() {
        let rows = vec![
            row("a", 0.97, Some("auto")),
            row("b", 0.8, Some("accepted")),
            row("c", 0.7, Some("changed")),
            row("d", 0.4, Some("rejected")),
            row("e", 0.9, None),
        ];
        let m = summarize(&rows, None, 30, NOW);
        assert_eq!(m.entries, 5);
        assert_eq!(m.decided, 4);
        assert_eq!(m.auto_approved, 1);
        assert_eq!(
            (m.reviewed, m.accepted, m.changed, m.rejected),
            (3, 1, 1, 1)
        );
        assert_eq!(m.tiers.model, 5);
    }

    #[test]
    fn the_tier_falls_back_to_the_engine_on_old_rows() {
        let mut rule = row("a", 1.0, Some("accepted"));
        rule.engine = "rules".into();
        rule.tier = None;
        let mut fallback = row("b", 0.7, None);
        fallback.engine = "fallback".into();
        fallback.tier = None;
        let m = summarize(&[rule, fallback], None, 30, NOW);
        assert_eq!(
            m.tiers,
            TierSplit {
                rule: 1,
                personal: 1,
                model: 0
            }
        );
        // Rule hits are certain by definition, so they stay off the chart.
        assert_eq!(m.calibration.samples, 0);
    }

    #[test]
    fn the_threshold_preview_uses_todays_calibration_and_counts_mistakes() {
        let rows = vec![
            row("a", 0.99, Some("accepted")),
            row("b", 0.96, Some("changed")),
            row("c", 0.92, Some("accepted")),
            row("d", 0.5, None),
        ];
        // Cold start: every model score is capped at 90%, so nothing clears
        // 95% and the preview says so.
        let cold = summarize(&rows, None, 30, NOW);
        assert_eq!(preview(&cold, 95).auto_approved, 0);
        assert_eq!(preview(&cold, 90).auto_approved, 3);
        assert_eq!(preview(&cold, 90).wrong, 1);

        let identity = Calibrator {
            x: vec![0.0, 1.0],
            y: vec![0.0, 1.0],
            samples: 50,
        };
        let warm = summarize(&rows, Some(&identity), 30, NOW);
        assert_eq!(preview(&warm, 95).auto_approved, 2);
        assert_eq!(preview(&warm, 95).wrong, 1);
        assert_eq!(preview(&warm, 97).auto_approved, 1);
        assert_eq!(preview(&warm, 97).wrong, 0);
        assert_eq!(warm.thresholds.len(), 51);
    }

    #[test]
    fn an_entry_clears_only_when_its_weakest_field_does() {
        let rows = vec![
            row("a", 0.99, Some("accepted")),
            row("a", 0.8, Some("accepted")),
        ];
        let identity = Calibrator {
            x: vec![0.0, 1.0],
            y: vec![0.0, 1.0],
            samples: 50,
        };
        let m = summarize(&rows, Some(&identity), 30, NOW);
        assert_eq!(m.entries, 1);
        assert_eq!(preview(&m, 80).auto_approved, 1);
        assert_eq!(preview(&m, 81).auto_approved, 0);
    }

    #[test]
    fn fresh_auto_approvals_wait_a_day_before_they_count_as_accepted() {
        let mut fresh = row("a", 0.96, Some("auto"));
        fresh.updated_at = NOW - 60_000;
        let settled = row("b", 0.96, Some("auto"));
        let m = summarize(&[fresh, settled], None, 30, NOW);
        assert_eq!(m.calibration.samples, 1);
        assert_eq!(m.calibration.bins[0].actual, 1.0);
    }
}
