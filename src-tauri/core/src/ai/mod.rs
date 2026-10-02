//! The AI engine's data side: suggestion storage, rules, kNN, calibration
//! and the arbiter. The app's `ai` module runs the worker and the sidecar
//! on top of it.

pub mod arbiter;
pub mod calibration;
pub mod knn;
pub mod rules;
pub mod store;

pub const FIELD_CATEGORY: &str = "category";
pub const FIELD_PROJECT: &str = "project";

/// Suggestions at or above this confidence pre-fill the entry's field. Below
/// it the panel shows "Needs you" with nothing pre-selected.
pub const PREFILL_THRESHOLD: f64 = 0.60;
/// Until this many suggestions have a user outcome, there is no calibration
/// curve and displayed confidence is capped at `COLD_START_CAP`, so nothing
/// auto-approves during the first week except deterministic rule hits.
pub const COLD_START_OUTCOMES: u32 = 50;
pub const COLD_START_CAP: f64 = 0.90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Category,
    Project,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Field::Category => FIELD_CATEGORY,
            Field::Project => FIELD_PROJECT,
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            FIELD_CATEGORY => Ok(Field::Category),
            FIELD_PROJECT => Ok(Field::Project),
            other => Err(format!("unknown suggestion field {other}")),
        }
    }
}
