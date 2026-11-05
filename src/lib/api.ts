import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  AccountsState,
  DataSummary,
  PausedOnSwitch,
  SwitchPreview,
} from "./accounts";
import type { BreakEntry, BreakState } from "./breaks";
import type { LoginItemState, Settings, StoragePaths } from "./settings";
import type {
  SegmentEdit,
  Timer,
  TimerSavedState,
  TimerSection,
} from "./timers";
import type {
  AccessibilityState,
  ActivitySnapshot,
  AiMetrics,
  AiStatus,
  AppRecord,
  BillableEntry,
  Category,
  CategoryRemoval,
  CategoryStats,
  Client,
  DevToolsInfo,
  EnergySummary,
  EntryDetail,
  EntryQuery,
  ExportResult,
  HintPreview,
  ImportSummary,
  Invoice,
  InvoiceDraftInput,
  InvoiceProfile,
  InvoiceProfileInput,
  InvoiceSummary,
  NewCategory,
  NewClient,
  NewProject,
  NewTimeEntry,
  PermissionInfo,
  Project,
  ProjectRule,
  ProjectStats,
  ProjectSuggestion,
  ReleaseNotes,
  RollupCell,
  RollupGroup,
  RuleSuggestion,
  TimeEntry,
  UpdateCategory,
  UpdateClient,
  UpdateProject,
  UpdateStatus,
  UpdateTimeEntry,
} from "./types";

export const ACTIVITY_CHANGED = "activity-changed";
export const ACTIVITY_TICK = "activity-tick";
export const SETTINGS_CHANGED = "settings-changed";
export const ENTRIES_CHANGED = "entries-changed";
export const TIMERS_CHANGED = "timers-changed";
export const TIMER_SECTIONS_CHANGED = "timer-sections-changed";
/** Payload: `{ entryId }`. */
export const SUGGESTION_READY = "suggestion-ready";
/** Payload: `AiStatus`. */
export const AI_STATUS_CHANGED = "ai-status-changed";
export const ENERGY_CHANGED = "energy-changed";
/** Main window only: the Pulse panel asked for today's review queue. */
export const OPEN_REVIEW = "open-review";
/** Payload: `UpdateStatus`. */
export const UPDATE_STATUS = "update-status";
export const PERMISSIONS_CHANGED = "permissions-changed";
/** The first-run permissions screen was re-armed or reset (developer tools). No payload. */
export const ONBOARDING_CHANGED = "onboarding-changed";
/** Main window only: the reminder asked for Settings > Notifications. */
export const OPEN_BREAK_SETTINGS = "open-break-settings";
/** Payload: `BreakState`. */
export const BREAK_STATE_CHANGED = "break-state-changed";
/** The accounts list changed without a restart (a rename or a delete). */
export const ACCOUNTS_CHANGED = "accounts-changed";

/** Tauri rejects with a string; React errors are Error objects. Handle both. */
export function describeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

export async function getSettings(): Promise<Settings> {
  return await invoke<Settings>("get_settings");
}

export async function updateSettings(settings: Settings): Promise<Settings> {
  return await invoke<Settings>("update_settings", { settings });
}

export async function storagePaths(): Promise<StoragePaths> {
  return await invoke<StoragePaths>("storage_paths");
}

/** What macOS says about launching at login; it can change behind the app. */
export async function launchAtLogin(): Promise<LoginItemState> {
  return await invoke<LoginItemState>("launch_at_login");
}

export async function setLaunchAtLogin(
  enabled: boolean,
): Promise<LoginItemState> {
  return await invoke<LoginItemState>("set_launch_at_login", { enabled });
}

export async function checkPermissions(): Promise<PermissionInfo[]> {
  return await invoke<PermissionInfo[]>("check_permissions");
}

export async function accessibilityState(): Promise<AccessibilityState> {
  return await invoke<AccessibilityState>("accessibility_state");
}

export async function requestAccessibility(): Promise<void> {
  await invoke("request_accessibility");
}

export async function onboardingNeeded(): Promise<boolean> {
  return await invoke<boolean>("onboarding_needed");
}

export async function finishOnboarding(): Promise<void> {
  await invoke("finish_onboarding");
}

export async function devToolsInfo(): Promise<DevToolsInfo> {
  return await invoke<DevToolsInfo>("dev_tools_info");
}

export async function devSimulateAccessibility(
  state: AccessibilityState | null,
): Promise<void> {
  await invoke("dev_simulate_accessibility", { state });
}

export async function devSimulateAutomation(
  granted: boolean | null,
): Promise<void> {
  await invoke("dev_simulate_automation", { granted });
}

export async function devRerunOnboarding(): Promise<void> {
  await invoke("dev_rerun_onboarding");
}

export async function devReset(): Promise<void> {
  await invoke("dev_reset");
}

export async function relaunchApp(): Promise<void> {
  await invoke("relaunch_app");
}

export async function accountsState(): Promise<AccountsState> {
  return await invoke<AccountsState>("accounts_state");
}

export async function existingDataSummary(): Promise<DataSummary | null> {
  return await invoke<DataSummary | null>("existing_data_summary");
}

export async function switchPreview(): Promise<SwitchPreview> {
  return await invoke<SwitchPreview>("switch_preview");
}

/**
 * Restarts into another account (null: No account); resolves at once, the
 * restart follows after 3s+.
 */
export async function switchAccount(to: string | null): Promise<void> {
  await invoke("switch_account", { to });
}

/**
 * `moveExisting` moves the data that belongs to No account into the new one.
 */
export async function createAccount(
  name: string,
  moveExisting: boolean,
): Promise<Account> {
  return await invoke<Account>("create_account", { name, moveExisting });
}

export async function renameAccount(
  id: string,
  name: string,
): Promise<Account> {
  return await invoke<Account>("rename_account", { id, name });
}

export async function deleteAccount(id: string): Promise<void> {
  await invoke("delete_account", { id });
}

export async function dismissMissingAccount(): Promise<void> {
  await invoke("dismiss_missing_account");
}

export async function pausedOnSwitch(): Promise<PausedOnSwitch | null> {
  return await invoke<PausedOnSwitch | null>("paused_on_switch");
}

export async function resumePaused(): Promise<void> {
  await invoke("resume_paused");
}

export async function dismissPaused(): Promise<void> {
  await invoke("dismiss_paused");
}

export async function resizePulsePanel(height: number): Promise<void> {
  await invoke("resize_pulse_panel", { height });
}

export async function hidePulsePanel(): Promise<void> {
  await invoke("hide_pulse_panel");
}

export async function openMainWindow(
  review: boolean,
  timerId?: string,
): Promise<void> {
  await invoke("open_main_window", { review, timerId });
}

/**
 * Undoes (or redoes) the last entry or catalog change; resolves to what it was
 * ("Split", ...), or null if none.
 */
export async function undo(redo = false): Promise<string | null> {
  return await invoke<string | null>("undo", { redo });
}

export async function breakState(): Promise<BreakState> {
  return await invoke<BreakState>("break_state");
}

export async function dismissStopwatchReminder(
  id: string,
  startedAt: number,
): Promise<BreakState> {
  return await invoke<BreakState>("dismiss_stopwatch_reminder", {
    id,
    startedAt,
  });
}

export async function startBreak(): Promise<BreakState> {
  return await invoke<BreakState>("start_break");
}

export async function endBreak(): Promise<BreakState> {
  return await invoke<BreakState>("end_break");
}

export async function snoozeBreak(minutes: number): Promise<BreakState> {
  return await invoke<BreakState>("snooze_break", { minutes });
}

export async function skipBreak(): Promise<BreakState> {
  return await invoke<BreakState>("skip_break");
}

export async function expandBreakReminder(): Promise<BreakState> {
  return await invoke<BreakState>("expand_break_reminder");
}

export async function extendBreak(): Promise<BreakState> {
  return await invoke<BreakState>("extend_break");
}

/** Silences reminders until `until` (epoch ms); null turns them back on. */
export async function pauseBreakReminders(
  until: number | null,
): Promise<BreakState> {
  return await invoke<BreakState>("pause_break_reminders", { until });
}

export async function listBreaks(
  sinceMs: number,
  untilMs: number,
): Promise<BreakEntry[]> {
  return await invoke<BreakEntry[]>("list_breaks", { sinceMs, untilMs });
}

export async function resizeReminderPanel(
  width: number,
  height: number,
): Promise<void> {
  await invoke("resize_reminder_panel", { width, height });
}

export async function openBreakSettings(): Promise<void> {
  await invoke("open_break_settings");
}

export async function devSampleBreakReminder(): Promise<BreakState> {
  return await invoke<BreakState>("dev_sample_break_reminder");
}

export async function previewBreakChime(): Promise<void> {
  await invoke("preview_break_chime");
}

export async function fetchActivitySnapshot(
  sinceMs: number,
): Promise<ActivitySnapshot> {
  return await invoke<ActivitySnapshot>("activity_snapshot", { sinceMs });
}

export async function setCaptureEnabled(enabled: boolean): Promise<void> {
  await invoke("set_capture_enabled", { enabled });
}

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

export async function removeCategory(id: string): Promise<CategoryRemoval> {
  return await invoke<CategoryRemoval>("remove_category", { id });
}

export async function reorderCategories(ids: string[]): Promise<void> {
  await invoke("reorder_categories", { ids });
}

export async function categoryStats(
  rangeStart: number,
  rangeEnd: number,
): Promise<CategoryStats[]> {
  return await invoke<CategoryStats[]>("category_stats", {
    rangeStart,
    rangeEnd,
  });
}

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

export async function listInvoices(): Promise<InvoiceSummary[]> {
  return await invoke<InvoiceSummary[]>("list_invoices");
}

export async function getInvoice(id: string): Promise<Invoice> {
  return await invoke<Invoice>("get_invoice", { id });
}

export async function listBillableEntries(
  clientId: string,
  startMs: number,
  endMs: number,
  invoiceId?: string,
): Promise<BillableEntry[]> {
  return await invoke<BillableEntry[]>("list_billable_entries", {
    clientId,
    startMs,
    endMs,
    invoiceId: invoiceId ?? null,
  });
}

export async function quoteInvoice(draft: InvoiceDraftInput): Promise<Invoice> {
  return await invoke<Invoice>("quote_invoice", { draft });
}

export async function renderInvoicePreview(
  draft: InvoiceDraftInput,
): Promise<Uint8Array> {
  const bytes = await invoke<ArrayBuffer>("render_invoice_preview", { draft });
  return new Uint8Array(bytes);
}

export async function getInvoicePdf(id: string): Promise<Uint8Array> {
  const bytes = await invoke<ArrayBuffer>("get_invoice_pdf", { id });
  return new Uint8Array(bytes);
}

export async function saveInvoiceDraft(
  draft: InvoiceDraftInput,
): Promise<Invoice> {
  return await invoke<Invoice>("save_invoice_draft", { draft });
}

export async function finalizeInvoice(id: string): Promise<Invoice> {
  return await invoke<Invoice>("finalize_invoice", { id });
}

export async function setInvoicePaid(id: string, paid: boolean): Promise<void> {
  await invoke("set_invoice_paid", { id, paid });
}

export async function voidInvoice(id: string): Promise<void> {
  await invoke("void_invoice", { id });
}

export async function deleteDraftInvoice(id: string): Promise<void> {
  await invoke("delete_draft_invoice", { id });
}

export async function deleteVoidInvoice(id: string): Promise<void> {
  await invoke("delete_void_invoice", { id });
}

/**
 * Pass `id` for a finalized invoice or `draft` for a watermarked render.
 * Resolves to the saved path, or null on cancel.
 */
export async function exportInvoicePdf(
  target: { id: string } | { draft: InvoiceDraftInput },
): Promise<string | null> {
  return await invoke<string | null>("export_invoice_pdf", {
    id: "id" in target ? target.id : null,
    draft: "draft" in target ? target.draft : null,
  });
}

export async function getInvoiceProfile(): Promise<InvoiceProfile> {
  return await invoke<InvoiceProfile>("get_invoice_profile");
}

export async function updateInvoiceProfile(
  profile: InvoiceProfileInput,
): Promise<InvoiceProfile> {
  return await invoke<InvoiceProfile>("update_invoice_profile", { profile });
}

export async function getInvoiceLogo(): Promise<Uint8Array> {
  return new Uint8Array(await invoke<ArrayBuffer>("get_invoice_logo"));
}

/** Sends the image bytes as the raw request body; Rust validates and resizes. */
export async function setInvoiceLogo(image: Uint8Array): Promise<void> {
  await invoke("set_invoice_logo", image);
}

export async function clearInvoiceLogo(): Promise<void> {
  await invoke("clear_invoice_logo");
}

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

export async function unapproveTimeEntries(
  ids: string[],
): Promise<TimeEntry[]> {
  return await invoke<TimeEntry[]>("unapprove_time_entries", { ids });
}

export async function updateTimeEntries(
  ids: string[],
  patch: UpdateTimeEntry,
): Promise<TimeEntry[]> {
  return await invoke<TimeEntry[]>("update_time_entries", { ids, patch });
}

export async function queryTimeEntries(
  filter: EntryQuery,
  limit?: number,
): Promise<TimeEntry[]> {
  return await invoke<TimeEntry[]>("query_time_entries", { filter, limit });
}

/** `boundaries` holds n + 1 ascending local-time edges for n buckets. */
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

export async function mergeTimeEntries(
  primaryId: string,
  secondaryId: string,
): Promise<TimeEntry> {
  return await invoke<TimeEntry>("merge_time_entries", {
    primaryId,
    secondaryId,
  });
}

export async function deleteTimeEntry(id: string): Promise<void> {
  await invoke("delete_time_entry", { id });
}

export async function deleteTimeEntries(ids: string[]): Promise<void> {
  await invoke("delete_time_entries", { ids });
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

export function setTimerTag(id: string, tag: string | null): Promise<Timer[]> {
  return invoke<Timer[]>("set_timer_tag", { id, tag });
}

export function startTimerTag(tag: string): Promise<Timer[]> {
  return invoke<Timer[]>("start_timer_tag", { tag });
}

export function pauseTimerTag(tag: string): Promise<Timer[]> {
  return invoke<Timer[]>("pause_timer_tag", { tag });
}

/** `index` is the place among the section's timers; left out, the end. */
export function moveTimerToSection(
  id: string,
  sectionId: string | null,
  index?: number,
): Promise<Timer[]> {
  return invoke<Timer[]>("move_timer_to_section", { id, sectionId, index });
}

export function listTimerSections(): Promise<TimerSection[]> {
  return invoke<TimerSection[]>("list_timer_sections");
}

export function createTimerSection(
  name: string,
  timerId?: string,
): Promise<TimerSection[]> {
  return invoke<TimerSection[]>("create_timer_section", { name, timerId });
}

export function renameTimerSection(
  id: string,
  name: string,
): Promise<TimerSection[]> {
  return invoke<TimerSection[]>("rename_timer_section", { id, name });
}

export function deleteTimerSection(id: string): Promise<TimerSection[]> {
  return invoke<TimerSection[]>("delete_timer_section", { id });
}

export function reorderTimerSections(ids: string[]): Promise<TimerSection[]> {
  return invoke<TimerSection[]>("reorder_timer_sections", { ids });
}

export function addTimerSegment(
  id: string,
  startedAt: number,
  endedAt: number,
): Promise<Timer[]> {
  return invoke<Timer[]>("add_timer_segment", { id, startedAt, endedAt });
}

export function updateTimerSegment(
  id: string,
  segmentId: number,
  edit: SegmentEdit,
): Promise<Timer[]> {
  return invoke<Timer[]>("update_timer_segment", {
    id,
    segmentId,
    startedAt: edit.startedAt ?? null,
    endedAt: edit.endedAt ?? null,
  });
}

export function deleteTimerSegment(
  id: string,
  segmentId: number,
): Promise<Timer[]> {
  return invoke<Timer[]>("delete_timer_segment", { id, segmentId });
}

export function saveTimerState(id: string): Promise<TimerSavedState> {
  return invoke<TimerSavedState>("save_timer_state", { id });
}

export function listTimerSavedStates(id: string): Promise<TimerSavedState[]> {
  return invoke<TimerSavedState[]>("list_timer_saved_states", { id });
}

export function deleteTimer(id: string): Promise<Timer[]> {
  return invoke<Timer[]>("delete_timer", { id });
}

export function reorderTimers(ids: string[]): Promise<Timer[]> {
  return invoke<Timer[]>("reorder_timers", { ids });
}

export function getEnergySummary(days?: number): Promise<EnergySummary> {
  return invoke<EnergySummary>("get_energy_summary", { days });
}

export function resetEnergyHistory(): Promise<EnergySummary> {
  return invoke<EnergySummary>("reset_energy_history");
}

export function updateStatus(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("update_status");
}

export function checkForUpdates(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("check_for_updates");
}

export function installedReleaseNotes(): Promise<ReleaseNotes[]> {
  return invoke<ReleaseNotes[]>("installed_release_notes");
}

/** Resolves only if the install fails to restart; otherwise the app relaunches. */
export function installUpdate(): Promise<void> {
  return invoke<void>("install_update");
}
