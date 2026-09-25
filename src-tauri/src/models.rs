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
}
