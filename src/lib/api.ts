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
import type { Timer, TimerSavedState } from "./timers";
import type {
  ActivitySnapshot,
  AiMetrics,
  AiStatus,
  AppRecord,
  BillableEntry,
  Category,
  CategoryRemoval,
  CategoryStats,
  Client,
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
/** Payload: `{ entryId }`. */
export const SUGGESTION_READY = "suggestion-ready";
/** Payload: `AiStatus`. */
export const AI_STATUS_CHANGED = "ai-status-changed";
export const ENERGY_CHANGED = "energy-changed";
/** Main window only: the Pulse panel asked for today's review queue. */
export const OPEN_REVIEW = "open-review";
/** Payload: `UpdateStatus`. */
export const UPDATE_STATUS = "update-status";
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

/** What macOS says about launching at login; it can change behind the app. */
export async function launchAtLogin(): Promise<LoginItemState> {
  return await invoke<LoginItemState>("launch_at_login");
}

export async function setLaunchAtLogin(
  enabled: boolean,
): Promise<LoginItemState> {
  return await invoke<LoginItemState>("set_launch_at_login", { enabled });
}

/** Reads authorization status of system permissions (Accessibility, Automation, etc.). */
export async function checkPermissions(): Promise<PermissionInfo[]> {
  return await invoke<PermissionInfo[]>("check_permissions");
}

// --- Accounts ---

export async function accountsState(): Promise<AccountsState> {
  return await invoke<AccountsState>("accounts_state");
}

/** What No account holds, or null when an account is open or it is empty. */
export async function existingDataSummary(): Promise<DataSummary | null> {
  return await invoke<DataSummary | null>("existing_data_summary");
}

/** What switching accounts right now would pause. */
export async function switchPreview(): Promise<SwitchPreview> {
  return await invoke<SwitchPreview>("switch_preview");
}

/**
 * Opens another account, or No account for null, by restarting the app.
 * Resolves at once; the restart follows after at least three seconds.
 */
export async function switchAccount(to: string | null): Promise<void> {
  await invoke("switch_account", { to });
}

/**
 * Adds an account and opens it by restarting. `moveExisting` moves the data
 * that belongs to No account into it.
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

/** Moves the account's folder to the Trash and forgets it. */
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

// --- Pulse panel ---

/** Sizes the menu-bar panel to its content, keeping it under the icon. */
export async function resizePulsePanel(height: number): Promise<void> {
  await invoke("resize_pulse_panel", { height });
}

export async function hidePulsePanel(): Promise<void> {
  await invoke("hide_pulse_panel");
}

/** Closes the panel and brings the main window forward. */
export async function openMainWindow(
  review: boolean,
  timerId?: string,
): Promise<void> {
  await invoke("open_main_window", { review, timerId });
}

/**
 * Undoes (or redoes) the last change to entries or the catalog. Resolves to
 * what it was ("Split", "Accept", ...), or null when there was nothing.
 */
export async function undo(redo = false): Promise<string | null> {
  return await invoke<string | null>("undo", { redo });
}

// --- Break reminders ---

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

/** Starts the pending reminder's break, or a manual one when none is pending. */
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

/** Re-expands a reminder that collapsed to the corner capsule. */
export async function expandBreakReminder(): Promise<BreakState> {
  return await invoke<BreakState>("expand_break_reminder");
}

/** Adds five minutes to the running break. */
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

/** Sizes the top-right reminder panel to its card. */
export async function resizeReminderPanel(
  width: number,
  height: number,
): Promise<void> {
  await invoke("resize_reminder_panel", { width, height });
}

/** Brings the main window forward on Settings > Notifications. */
export async function openBreakSettings(): Promise<void> {
  await invoke("open_break_settings");
}

/** Dev builds only: raises a sample reminder. */
export async function devSampleBreakReminder(): Promise<BreakState> {
  return await invoke<BreakState>("dev_sample_break_reminder");
}

export async function previewBreakChime(): Promise<void> {
  await invoke("preview_break_chime");
}

// --- Activity & Capture ---

export async function fetchActivitySnapshot(
  sinceMs: number,
): Promise<ActivitySnapshot> {
  return await invoke<ActivitySnapshot>("activity_snapshot", { sinceMs });
}

export async function setCaptureEnabled(enabled: boolean): Promise<void> {
  await invoke("set_capture_enabled", { enabled });
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

// --- Invoices ---

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

/** The draft priced and validated by Rust, without storing it. */
export async function quoteInvoice(draft: InvoiceDraftInput): Promise<Invoice> {
  return await invoke<Invoice>("quote_invoice", { draft });
}

/** The DRAFT-watermarked PDF for a draft, straight from the Rust renderer. */
export async function renderInvoicePreview(
  draft: InvoiceDraftInput,
): Promise<Uint8Array> {
  const bytes = await invoke<ArrayBuffer>("render_invoice_preview", { draft });
  return new Uint8Array(bytes);
}

/** The archived PDF of a finalized invoice. */
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
 * Asks where to save an invoice PDF. Pass `id` for a finalized invoice or
 * `draft` for a watermarked draft render. Resolves to the saved path, or null
 * when the user cancels.
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

/** The stored logo (PNG), or an empty array when none is set. */
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

/** Returns approved entries to pending for editing; no AI re-run. */
export async function unapproveTimeEntries(
  ids: string[],
): Promise<TimeEntry[]> {
  return await invoke<TimeEntry[]>("unapprove_time_entries", { ids });
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

export function addTimerSegment(
  id: string,
  durationMs: number,
): Promise<Timer[]> {
  return invoke<Timer[]>("add_timer_segment", { id, durationMs });
}

export function updateTimerSegment(
  id: string,
  segmentId: number,
  durationMs: number,
): Promise<Timer[]> {
  return invoke<Timer[]>("update_timer_segment", {
    id,
    segmentId,
    durationMs,
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

export function moveTimer(id: string, delta: number): Promise<Timer[]> {
  return invoke<Timer[]>("move_timer", { id, delta });
}

// --- Battery & Energy ---

export function getEnergySummary(days?: number): Promise<EnergySummary> {
  return invoke<EnergySummary>("get_energy_summary", { days });
}

export function resetEnergyHistory(): Promise<EnergySummary> {
  return invoke<EnergySummary>("reset_energy_history");
}

// --- Updates ---

export function updateStatus(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("update_status");
}

export function checkForUpdates(): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("check_for_updates");
}

/** Resolves only if the install fails to restart; otherwise the app relaunches. */
export function installUpdate(): Promise<void> {
  return invoke<void>("install_update");
}

// --- Crash diagnostics ---

export function recordClientCrash(
  source: string,
  message: string,
  componentStack?: string,
  metadata?: Record<string, unknown>,
): Promise<void> {
  return invoke<void>("record_client_crash", {
    source,
    message,
    componentStack,
    metadata,
  });
}
