pub mod arbiter;
pub mod calibration;
pub mod knn;
pub mod prompt;
pub mod rules;
pub mod store;

pub const FIELD_CATEGORY: &str = "category";
pub const FIELD_PROJECT: &str = "project";

pub const PREFILL_THRESHOLD: f64 = 0.60;
/// Until this many outcomes exist there is no calibration curve and confidence
/// is capped at `COLD_START_CAP`, so only rule hits auto-approve.
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
