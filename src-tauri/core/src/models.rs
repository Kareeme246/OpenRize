use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySegment {
    pub id: i64,
    pub app: String,
    pub title: String,
    pub kind: String,
    pub label: Option<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub reviewed: bool,
    pub app_id: Option<String>,
    pub bundle_id: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
    pub entry_id: Option<String>,
    /// Captured in a private browsing window. Its title, URL and domain are erased once its entry
    /// is classified (see `activity::redact`), leaving `PRIVATE_TITLE`.
    #[serde(default)]
    pub private: bool,
}

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
    /// The category's "Apps & sites": its hint rules' patterns, comma
    /// separated. Not a column: the rules are the source of truth.
    #[serde(default)]
    pub ai_hints: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCategory {
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub ai_prompt: Option<String>,
    pub ai_hints: Option<String>,
    pub billable_default: Option<bool>,
    pub counts_as_work: Option<bool>,
    pub sort: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCategory {
    pub name: Option<String>,
    pub color: Option<String>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub ai_prompt: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub ai_hints: Option<Option<String>>,
    pub billable_default: Option<bool>,
    pub counts_as_work: Option<bool>,
    pub archived: Option<bool>,
    pub sort: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CategoryRemoval {
    Archived,
    Deleted,
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
    pub notes: Option<String>,
    pub archived_at: Option<u64>,
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
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClient {
    pub name: Option<String>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub email: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub address: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_rate: Option<Option<f64>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub currency: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub notes: Option<Option<String>>,
    pub archived: Option<bool>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProject {
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_id: Option<Option<String>>,
    pub name: Option<String>,
    pub color: Option<String>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub ai_hints: Option<Option<String>>,
    pub status: Option<String>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub due_date: Option<Option<u64>>,
    pub budget_kind: Option<String>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub budget_value: Option<Option<f64>>,
    pub budget_period: Option<String>,
    pub billable_default: Option<bool>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub hourly_rate: Option<Option<f64>>,
}

/// Tells "absent" (`None`) from an explicit `null` (`Some(None)`). Serde maps
/// both to `None` for a plain `Option`, which would make clearing impossible.
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
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
    pub description_origin: String,
    /// Classification state and headline confidences, for list views. Only
    /// `list_time_entries` fills it; single-entry mutations return `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<EntryAi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dominant_app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntryAi {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Leaves a labeled entry pending for the user to review instead of
    /// approving it, without queueing AI classification (Calendar's
    /// drag-to-create placeholder).
    #[serde(default)]
    pub review: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    pub segments: Vec<ActivitySegment>,
    pub apps: Vec<AppContribution>,
    pub titles: Vec<TitleItem>,
    pub events: Vec<EntryEvent>,
    pub suggestions: Vec<FieldSuggestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_suggestion: Option<RuleSuggestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<ClassifyJob>,
}

/// One piece of evidence behind a suggestion. The "Why" line is built only
/// from signals that actually fired, never from the model's own rationale.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Dominant {
    pub kind: String,
    pub key: String,
    pub label: String,
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Alternative {
    /// `None` on the project field means "No project".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_id: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldSuggestion {
    pub id: String,
    pub entry_id: String,
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_id: Option<String>,
    pub confidence: f64,
    pub rationale: String,
    pub signals: Vec<Signal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dominant: Option<Dominant>,
    pub alternatives: Vec<Alternative>,
    pub engine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuleSuggestion {
    pub field: String,
    pub match_kind: String,
    pub pattern: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_id: Option<String>,
    pub corrections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassifyJob {
    pub state: String,
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}
