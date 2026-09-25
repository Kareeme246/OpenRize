import { invoke } from "@tauri-apps/api/core";
import type { Settings, StoragePaths } from "./settings";
import type { Timer } from "./timers";
import type {
  ActivitySnapshot,
  AiMetrics,
  AiStatus,
  AppRecord,
  Category,
  Client,
  EntryDetail,
  EntryQuery,
  ExportResult,
  HintPreview,
  ImportSummary,
  NewCategory,
  NewClient,
  NewProject,
  NewTimeEntry,
  Project,
  ProjectRule,
  ProjectStats,
  ProjectSuggestion,
  RollupCell,
  RollupGroup,
  RuleSuggestion,
  TimeEntry,
  UpdateCategory,
  UpdateClient,
  UpdateProject,
  UpdateTimeEntry,
} from "./types";

export const ACTIVITY_CHANGED = "activity-changed";
export const ACTIVITY_TICK = "activity-tick";
export const SETTINGS_CHANGED = "settings-changed";
export const ENTRIES_CHANGED = "entries-changed";
export const TIMERS_CHANGED = "timers-changed";
/** Payload: `{ entryId }`. */
export const SUGGESTION_READY = "suggestion-ready";
/** Payload: `AiStatus`. */
export const AI_STATUS_CHANGED = "ai-status-changed";

/** Tauri rejects with a string; React errors are Error objects. Handle both. */
export function describeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

// --- Preferences ---

export async function getSettings(): Promise<Settings> {
  return await invoke<Settings>("get_settings");
}

export async function updateSettings(settings: Settings): Promise<Settings> {
  return await invoke<Settings>("update_settings", { settings });
}

export async function storagePaths(): Promise<StoragePaths> {
  return await invoke<StoragePaths>("storage_paths");
}

// --- Activity & Capture ---

export async function fetchActivitySnapshot(
  sinceMs: number,
): Promise<ActivitySnapshot> {
  return await invoke<ActivitySnapshot>("activity_snapshot", { sinceMs });
}

export const activitySnapshot = fetchActivitySnapshot;

export async function setCaptureEnabled(enabled: boolean): Promise<void> {
  await invoke("set_capture_enabled", { enabled });
}

export async function setIdleThreshold(minutes: number): Promise<void> {
  await invoke("set_idle_threshold", { minutes });
}

export async function startSession(
  kind: string,
  label?: string,
): Promise<void> {
  await invoke("start_session", { kind, label });
}

export async function stopSession(): Promise<void> {
  await invoke("stop_session");
}

export async function markSegmentReviewed(id: number): Promise<void> {
  await invoke("mark_segment_reviewed", { id });
}

// --- Categories ---

export async function listCategories(): Promise<Category[]> {
  return await invoke<Category[]>("list_categories");
}

export async function createCategory(category: NewCategory): Promise<Category> {
  return await invoke<Category>("create_category", { category });
}

export async function updateCategory(
  id: string,
  patch: UpdateCategory,
): Promise<Category> {
  return await invoke<Category>("update_category", { id, patch });
}

export async function deleteCategory(id: string): Promise<void> {
  await invoke("delete_category", { id });
}

// --- Projects ---

export async function listProjects(): Promise<Project[]> {
  return await invoke<Project[]>("list_projects");
}

export async function createProject(project: NewProject): Promise<Project> {
  return await invoke<Project>("create_project", { project });
}

export async function updateProject(
  id: string,
  patch: UpdateProject,
): Promise<Project> {
  return await invoke<Project>("update_project", { id, patch });
}

export async function deleteProject(id: string): Promise<void> {
  await invoke("delete_project", { id });
}

export async function projectStats(
  rangeStart: number,
  rangeEnd: number,
  monthStart: number,
): Promise<ProjectStats[]> {
  return await invoke<ProjectStats[]>("project_stats", {
    rangeStart,
    rangeEnd,
    monthStart,
  });
}

export async function projectRules(projectId: string): Promise<ProjectRule[]> {
  return await invoke<ProjectRule[]>("project_rules", { projectId });
}

export async function previewProjectHints(
  hints: string,
  sinceMs: number,
): Promise<HintPreview> {
  return await invoke<HintPreview>("preview_project_hints", { hints, sinceMs });
}

export async function discoverProjects(
  sinceMs: number,
): Promise<ProjectSuggestion[]> {
  return await invoke<ProjectSuggestion[]>("discover_projects", { sinceMs });
}

export async function dismissProjectSuggestion(key: string): Promise<void> {
  await invoke("dismiss_project_suggestion", { key });
}

export async function importProjectsCsv(text: string): Promise<ImportSummary> {
  return await invoke<ImportSummary>("import_projects_csv", { text });
}

// --- Clients ---

export async function listClients(): Promise<Client[]> {
  return await invoke<Client[]>("list_clients");
}

export async function createClient(client: NewClient): Promise<Client> {
  return await invoke<Client>("create_client", { client });
}

export async function updateClient(
  id: string,
  patch: UpdateClient,
): Promise<Client> {
  return await invoke<Client>("update_client", { id, patch });
}

export async function deleteClient(id: string): Promise<void> {
  await invoke("delete_client", { id });
}

// --- Time Entries ---

export async function listTimeEntries(
  startMs: number,
  endMs: number,
): Promise<TimeEntry[]> {
  return await invoke<TimeEntry[]>("list_time_entries", { startMs, endMs });
}

export async function getEntryDetail(id: string): Promise<EntryDetail> {
  return await invoke<EntryDetail>("get_entry_detail", { id });
}

export async function updateTimeEntry(
  id: string,
  patch: UpdateTimeEntry,
): Promise<TimeEntry> {
  return await invoke<TimeEntry>("update_time_entry", { id, patch });
}

export async function approveTimeEntries(ids: string[]): Promise<TimeEntry[]> {
  return await invoke<TimeEntry[]>("approve_time_entries", { ids });
}

/** One patch applied to many entries; returns them updated. */
export async function updateTimeEntries(
  ids: string[],
  patch: UpdateTimeEntry,
): Promise<TimeEntry[]> {
  return await invoke<TimeEntry[]>("update_time_entries", { ids, patch });
}

// --- Reports ---

/** Entries matching a filter, newest first. */
export async function queryTimeEntries(
  filter: EntryQuery,
  limit?: number,
): Promise<TimeEntry[]> {
  return await invoke<TimeEntry[]>("query_time_entries", { filter, limit });
}

/**
 * Totals per group per bucket, summed in SQL. `boundaries` holds n + 1
 * ascending local-time edges for n buckets.
 */
export async function entryRollup(
  filter: EntryQuery,
  boundaries: number[],
  groupBy: RollupGroup,
): Promise<RollupCell[]> {
  return await invoke<RollupCell[]>("entry_rollup", {
    filter,
    boundaries,
    groupBy,
  });
}

/** Writes the filtered entries to Downloads; returns the file. */
export async function exportTimeEntries(
  filter: EntryQuery,
  format: "csv" | "json",
): Promise<ExportResult> {
  return await invoke<ExportResult>("export_time_entries", { filter, format });
}

export async function rejectTimeEntry(id: string): Promise<void> {
  await invoke("reject_time_entry", { id });
}

export async function splitTimeEntry(
  id: string,
  atMs: number,
): Promise<[TimeEntry, TimeEntry]> {
  return await invoke<[TimeEntry, TimeEntry]>("split_time_entry", {
    id,
    atMs,
  });
}

export async function deleteTimeEntry(id: string): Promise<void> {
  await invoke("delete_time_entry", { id });
}

export async function createTimeEntry(entry: NewTimeEntry): Promise<TimeEntry> {
  return await invoke<TimeEntry>("create_time_entry", { entry });
}

export async function rebuildTimeEntries(
  startMs: number,
  endMs: number,
): Promise<TimeEntry[]> {
  return await invoke<TimeEntry[]>("rebuild_time_entries", { startMs, endMs });
}

// --- AI suggestions ---

export async function aiStatus(): Promise<AiStatus> {
  return await invoke<AiStatus>("ai_status");
}

export async function retryClassification(id: string): Promise<void> {
  await invoke("retry_classification", { id });
}

export async function resolveRuleSuggestion(
  suggestion: RuleSuggestion,
  accept: boolean,
): Promise<void> {
  await invoke("resolve_rule_suggestion", { suggestion, accept });
}

// --- Learning loop ---

export async function aiMetrics(days: number): Promise<AiMetrics> {
  return await invoke<AiMetrics>("ai_metrics", { days });
}

/** Queues a retrain that skips the idle/power gate; returns the new status. */
export async function aiRetrain(): Promise<AiStatus> {
  return await invoke<AiStatus>("ai_retrain");
}

export async function aiResetLearned(days: number): Promise<AiMetrics> {
  return await invoke<AiMetrics>("ai_reset_learned", { days });
}

// --- Apps ---

export async function listApps(): Promise<AppRecord[]> {
  return await invoke<AppRecord[]>("list_apps");
}

export async function updateApp(
  id: string,
  defaultCategoryId?: string,
  defaultProjectId?: string,
  excluded?: boolean,
): Promise<AppRecord> {
  return await invoke<AppRecord>("update_app", {
    id,
    defaultCategoryId,
    defaultProjectId,
    excluded,
  });
}

// --- Timers (Legacy/Compat) ---

export function listTimers(): Promise<Timer[]> {
  return invoke<Timer[]>("list_timers");
}

export function createTimer(label: string): Promise<Timer[]> {
  return invoke<Timer[]>("create_timer", { label });
}

export function startTimer(id: string): Promise<Timer[]> {
  return invoke<Timer[]>("start_timer", { id });
}

export function pauseTimer(id: string): Promise<Timer[]> {
  return invoke<Timer[]>("pause_timer", { id });
}

export function resetTimer(id: string): Promise<Timer[]> {
  return invoke<Timer[]>("reset_timer", { id });
}

export function renameTimer(id: string, label: string): Promise<Timer[]> {
  return invoke<Timer[]>("rename_timer", { id, label });
}

export function deleteTimer(id: string): Promise<Timer[]> {
  return invoke<Timer[]>("delete_timer", { id });
}
