use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub ai_prompt: Option<String>,
    pub billable_default: bool,
    pub counts_as_work: bool,
    pub archived: bool,
    pub sort: i64,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCategory {
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub ai_prompt: Option<String>,
    pub billable_default: Option<bool>,
    pub counts_as_work: Option<bool>,
    pub sort: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCategory {
    pub name: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    pub ai_prompt: Option<String>,
    pub billable_default: Option<bool>,
    pub counts_as_work: Option<bool>,
    pub archived: Option<bool>,
    pub sort: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub address: Option<String>,
    pub default_rate: Option<f64>,
    pub currency: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewClient {
    pub name: String,
    pub email: Option<String>,
    pub address: Option<String>,
    pub default_rate: Option<f64>,
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClient {
    pub name: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub default_rate: Option<f64>,
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub client_id: Option<String>,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub ai_hints: Option<String>,
    pub status: String,
    pub due_date: Option<u64>,
    pub budget_kind: String,
    pub budget_value: Option<f64>,
    pub budget_period: String,
    pub billable_default: bool,
    pub hourly_rate: Option<f64>,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProject {
    pub client_id: Option<String>,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub ai_hints: Option<String>,
    pub status: Option<String>,
    pub due_date: Option<u64>,
    pub budget_kind: Option<String>,
    pub budget_value: Option<f64>,
    pub budget_period: Option<String>,
    pub billable_default: Option<bool>,
    pub hourly_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProject {
    pub client_id: Option<String>,
    pub name: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    pub ai_hints: Option<String>,
    pub status: Option<String>,
    pub due_date: Option<u64>,
    pub budget_kind: Option<String>,
    pub budget_value: Option<f64>,
    pub budget_period: Option<String>,
    pub billable_default: Option<bool>,
    pub hourly_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimeEntry {
    pub id: String,
    pub started_at: u64,
    pub ended_at: u64,
    pub description: String,
    pub category_id: Option<String>,
    pub project_id: Option<String>,
    pub status: String,
    pub approved_by: Option<String>,
    pub source: String,
    pub billable: bool,
    pub invoice_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted_at: Option<u64>,
    /// `template` (entry builder), `ai` (Foundation Model sentence), or
    /// `user`. The builder only ever rewrites template text.
    pub description_origin: String,
    /// Classification state and headline confidences, for list views. Only
    /// `list_time_entries` fills it; single-entry mutations return `None`.
    pub ai: Option<EntryAi>,
}

/// What a Calendar block needs to know about an entry's AI suggestion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntryAi {
    /// The classify job: queued | running | done | failed.
    pub state: Option<String>,
    pub category_confidence: Option<f64>,
    pub project_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTimeEntry {
    pub started_at: u64,
    pub ended_at: u64,
    pub description: String,
    pub category_id: Option<String>,
    pub project_id: Option<String>,
    pub billable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTimeEntry {
    pub description: Option<String>,
    pub category_id: Option<String>,
    pub project_id: Option<String>,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub status: Option<String>,
    pub billable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EntryEvent {
    pub id: String,
    pub entry_id: String,
    pub kind: String,
    pub actor: String,
    pub payload: Option<String>,
    pub at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppRecord {
    pub id: String,
    pub kind: String,
    pub identifier: String,
    pub display_name: String,
    pub default_category_id: Option<String>,
    pub default_project_id: Option<String>,
    pub excluded: bool,
    pub first_seen: u64,
    pub last_seen: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppContribution {
    pub app: String,
    pub duration_ms: u64,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TitleItem {
    pub title: String,
    pub app: String,
    pub started_at: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDetail {
    pub entry: TimeEntry,
    pub segments: Vec<crate::activity::ActivitySegment>,
    pub apps: Vec<AppContribution>,
    pub titles: Vec<TitleItem>,
    pub events: Vec<EntryEvent>,
    /// The latest suggestion per field (category, then project).
    pub suggestions: Vec<FieldSuggestion>,
    /// "Always categorize figma.com as Design?", after 3+ consistent
    /// corrections for the entry's dominant app or domain.
    pub rule_suggestion: Option<RuleSuggestion>,
    pub job: Option<ClassifyJob>,
}

/// One piece of evidence behind a suggestion. The "Why" line is built only
/// from signals that actually fired, never from the model's own rationale.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    /// dominant | rule | knn | personal | llm | mention
    pub kind: String,
    pub text: String,
}

/// The app or domain that took most of an entry's active time. Rule
/// suggestions are keyed on it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Dominant {
    /// `app` or `domain`, matching the rule `match_kind` it would create.
    pub kind: String,
    pub key: String,
    pub label: String,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Alternative {
    /// `None` on the project field means "No project".
    pub value_id: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldSuggestion {
    pub id: String,
    pub entry_id: String,
    /// `category` or `project`.
    pub field: String,
    pub value_id: Option<String>,
    pub confidence: f64,
    /// The "Why" line, joined from `signals`.
    pub rationale: String,
    pub signals: Vec<Signal>,
    pub dominant: Option<Dominant>,
    pub alternatives: Vec<Alternative>,
    /// rules | full | fallback
    pub engine: String,
    pub model_version: Option<String>,
    /// accepted | changed | rejected | auto, or `None` while undecided.
    pub outcome: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuleSuggestion {
    pub field: String,
    pub match_kind: String,
    pub pattern: String,
    /// Human label for the pattern (app name or domain).
    pub label: String,
    pub value_id: Option<String>,
    /// How many consistent corrections back this suggestion.
    pub corrections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassifyJob {
    pub state: String,
    pub attempts: u32,
    pub last_error: Option<String>,
}
