export interface Category {
  id: string;
  name: string;
  color: string;
  description?: string;
  aiPrompt?: string;
  billableDefault: boolean;
  countsAsWork: boolean;
  archived: boolean;
  sort: number;
  createdAt: number;
  updatedAt: number;
  deletedAt?: number;
}

export interface NewCategory {
  name: string;
  color: string;
  description?: string;
  aiPrompt?: string;
  billableDefault?: boolean;
  countsAsWork?: boolean;
  sort?: number;
}

export interface UpdateCategory {
  name?: string;
  color?: string;
  description?: string;
  aiPrompt?: string;
  billableDefault?: boolean;
  countsAsWork?: boolean;
  archived?: boolean;
  sort?: number;
}

export interface Client {
  id: string;
  name: string;
  email?: string;
  address?: string;
  defaultRate?: number;
  currency?: string;
  createdAt: number;
  updatedAt: number;
  deletedAt?: number;
}

export interface NewClient {
  name: string;
  email?: string;
  address?: string;
  defaultRate?: number;
  currency?: string;
}

export interface UpdateClient {
  name?: string;
  email?: string;
  address?: string;
  defaultRate?: number;
  currency?: string;
}

export interface Project {
  id: string;
  clientId?: string;
  name: string;
  color: string;
  description?: string;
  aiHints?: string;
  status: "active" | "completed" | "archived" | string;
  dueDate?: number;
  budgetKind: "none" | "hours" | "amount" | string;
  budgetValue?: number;
  budgetPeriod: "total" | "monthly" | string;
  billableDefault: boolean;
  hourlyRate?: number;
  createdAt: number;
  updatedAt: number;
  deletedAt?: number;
}

export interface NewProject {
  clientId?: string;
  name: string;
  color: string;
  description?: string;
  aiHints?: string;
  status?: string;
  dueDate?: number;
  budgetKind?: string;
  budgetValue?: number;
  budgetPeriod?: string;
  billableDefault?: boolean;
  hourlyRate?: number;
}

/** A patch: omitted fields stay; `null` clears an optional one. */
export interface UpdateProject {
  clientId?: string | null;
  name?: string;
  color?: string;
  description?: string | null;
  aiHints?: string | null;
  status?: string;
  dueDate?: number | null;
  budgetKind?: string;
  budgetValue?: number | null;
  budgetPeriod?: string;
  billableDefault?: boolean;
  hourlyRate?: number | null;
}

export interface TimeEntry {
  id: string;
  startedAt: number;
  endedAt: number;
  description: string;
  categoryId?: string;
  projectId?: string;
  status: "building" | "processing" | "pending" | "approved" | string;
  approvedBy?: "user" | "auto" | "rule" | string;
  source: "auto" | "manual" | "import" | string;
  billable: boolean;
  invoiceId?: string;
  createdAt: number;
  updatedAt: number;
  deletedAt?: number;
  /** Where the description came from; only `template` text is rebuilt. */
  descriptionOrigin: "template" | "ai" | "user" | string;
  /** Classification state and confidences (list views only). */
  ai?: EntryAi;
  /** The app or site with most of the entry's time (list views only). */
  dominantApp?: string;
}

export type ClassifyState = "queued" | "running" | "done" | "failed";

export interface EntryAi {
  state?: ClassifyState;
  categoryConfidence?: number;
  projectConfidence?: number;
}

export type SuggestionField = "category" | "project";

/** One piece of evidence behind a suggestion; the "Why" line joins them. */
export interface Signal {
  kind: "dominant" | "rule" | "knn" | "personal" | "llm" | "mention" | string;
  text: string;
}

export interface Dominant {
  kind: "app" | "domain";
  key: string;
  label: string;
  share: number;
}

export interface Alternative {
  /** Absent on the project field means "No project". */
  valueId?: string;
  confidence: number;
}

export interface FieldSuggestion {
  id: string;
  entryId: string;
  field: SuggestionField;
  valueId?: string;
  confidence: number;
  rationale: string;
  signals: Signal[];
  dominant?: Dominant;
  alternatives: Alternative[];
  engine: "rules" | "full" | "fallback" | string;
  modelVersion?: string;
  outcome?: "accepted" | "changed" | "rejected" | "auto";
  createdAt: number;
}

export interface RuleSuggestion {
  field: SuggestionField;
  matchKind: "app" | "domain";
  pattern: string;
  label: string;
  valueId?: string;
  corrections: number;
}

export interface ClassifyJob {
  state: ClassifyState;
  attempts: number;
  lastError?: string;
}

export interface AiStatus {
  engine: "full" | "fallback" | "rules" | "starting";
  llm:
    | "available"
    | "appleIntelligenceNotEnabled"
    | "modelNotReady"
    | "deviceNotEligible"
    | "unsupportedOS"
    | "unknown";
  embed: boolean;
  personalModel: boolean;
  sidecar: "running" | "stopped" | "unavailable";
  os?: string;
  queued: number;
  /** Verdicts calibration has learned from; the curve starts at 50. */
  outcomes: number;
  /** A calibration curve is active (otherwise the cold-start 90% cap). */
  calibrated: boolean;
  retrain: "idle" | "queued" | "running";
  /** The last retrain's result, or why a due retrain is waiting. */
  retrainNote?: string;
  lastError?: string;
}

/** One trained version in `model_artifacts`. */
export interface ArtifactVersion {
  id: string;
  trainedAt: number;
  examples: number;
  holdoutAccuracy?: number;
  /** False when it lost the holdout comparison, or was superseded. */
  active: boolean;
}

/** Displayed confidence vs actual accept rate for one 10% bucket. */
export interface ReliabilityBin {
  lo: number;
  hi: number;
  count: number;
  predicted: number;
  actual: number;
}

export interface ThresholdPreview {
  percent: number;
  autoApproved: number;
  wrong: number;
}

/** Settings → Categories & AI: how the learning loop is doing. */
export interface AiMetrics {
  days: number;
  /** Entries that got a suggestion in the period. */
  entries: number;
  decided: number;
  autoApproved: number;
  /** Per-field suggestions a person ruled on. */
  reviewed: number;
  accepted: number;
  changed: number;
  rejected: number;
  tiers: { rule: number; personal: number; model: number };
  calibration: {
    bins: ReliabilityBin[];
    samples: number;
    expectedError?: number;
    /** The active curve as [raw, displayed] knots. */
    curve?: [number, number][];
    history: ArtifactVersion[];
  };
  thresholds: ThresholdPreview[];
  models: {
    category: ArtifactVersion[];
    project: ArtifactVersion[];
    newLabels: number;
    retrainAfter: number;
  };
  learningSince?: number;
}

export interface NewTimeEntry {
  startedAt: number;
  endedAt: number;
  description: string;
  categoryId?: string;
  projectId?: string;
  billable?: boolean;
}

export interface UpdateTimeEntry {
  description?: string;
  categoryId?: string;
  projectId?: string;
  startedAt?: number;
  endedAt?: number;
  status?: string;
  billable?: boolean;
}

export type SessionKind = "activity" | "focus" | "break";

export interface ActivitySegment {
  id: number;
  app: string;
  title: string;
  kind: SessionKind;
  label: string | null;
  startedAt: number;
  endedAt: number | null;
  reviewed: boolean;
  appId?: string | null;
  bundleId?: string | null;
  url?: string | null;
  domain?: string | null;
  entryId?: string | null;
}

export interface ActivitySnapshot {
  current: ActivitySegment | null;
  segments: ActivitySegment[];
  trackedMs: number;
  focusMs: number;
  breakMs: number;
  unreviewed: number;
  idleMs: number;
  idleThresholdMs: number;
  captureEnabled: boolean;
}

export interface ActivityTick {
  current: ActivitySegment | null;
  trackedMs: number;
  focusMs: number;
  breakMs: number;
  unreviewed: number;
  idleMs: number;
  idleThresholdMs: number;
  captureEnabled: boolean;
}

export interface AppContribution {
  app: string;
  durationMs: number;
  percentage: number;
}

export interface TitleItem {
  title: string;
  app: string;
  startedAt: number;
  durationMs: number;
}

export interface EntryEvent {
  id: string;
  entryId: string;
  kind: string;
  actor: string;
  payload?: string;
  at: number;
}

export interface EntryDetail {
  entry: TimeEntry;
  segments: ActivitySegment[];
  apps: AppContribution[];
  titles: TitleItem[];
  events: EntryEvent[];
  /** The latest suggestion per field, category first. */
  suggestions: FieldSuggestion[];
  ruleSuggestion?: RuleSuggestion;
  job?: ClassifyJob;
}

export interface AppRecord {
  id: string;
  kind: "app" | "site" | string;
  identifier: string;
  displayName: string;
  defaultCategoryId?: string;
  defaultProjectId?: string;
  excluded: boolean;
  firstSeen: number;
  lastSeen: number;
  createdAt: number;
  updatedAt: number;
  deletedAt?: number;
}

export type CalendarScale = "day" | "week" | "month";
export type TimesheetTab = "review" | "processing" | "approved" | "all";
export type TimesheetGroup = "project" | "category" | "app" | "none";
export type EntriesView = "table" | "charts" | "log";
export type EntriesGroup = "project" | "client" | "category" | "app" | "status";
export type EntriesStack = "day" | "week" | "month";
export type EntriesRange = "day" | "week" | "month" | "30d" | "year";
export type ProjectsTab = "active" | "completed" | "archived" | "clients";
export type ProjectsRange = "week" | "month" | "30d" | "all";

/** Time Entries' filter bar. Values are ids, or `none` for "no …". */
export interface EntryFilters {
  categoryId?: string;
  projectId?: string;
  clientId?: string;
  app?: string;
  status?: "pending" | "approved";
  billable?: boolean;
  search?: string;
}

export type Route =
  | {
      name: "calendar";
      scale?: CalendarScale;
      /** Local `YYYY-MM-DD`; today when absent. */
      date?: string;
      entryId?: string;
      review?: boolean;
    }
  | {
      name: "timesheet";
      scale?: CalendarScale;
      date?: string;
      tab?: TimesheetTab;
      groupBy?: TimesheetGroup;
    }
  | {
      name: "apps";
      scale?: CalendarScale;
      date?: string;
      tab?: "timeline" | "log";
      appId?: string;
    }
  | {
      name: "entries";
      range?: EntriesRange;
      /** Local `YYYY-MM-DD` inside the range; today when absent. */
      date?: string;
      view?: EntriesView;
      filters?: EntryFilters;
      groupBy?: EntriesGroup;
      stackBy?: EntriesStack;
    }
  | {
      name: "timesheets";
      scale?: "week" | "month";
      start?: string;
      rows?: "project" | "category";
    }
  | {
      name: "projects";
      tab?: ProjectsTab;
      projectId?: string;
      range?: ProjectsRange;
    }
  | {
      name: "invoices";
      invoiceId?: string;
    }
  | {
      name: "settings";
      section?: string;
    };

/** The Rust-side filter (src-tauri/src/reports.rs). */
export interface EntryQuery extends EntryFilters {
  startMs: number;
  endMs: number;
}

export type RollupGroup = EntriesGroup | "none";

export interface RollupCell {
  /** Group id or name; absent is "No project", "Uncategorized", etc. */
  key?: string | null;
  bucket: number;
  ms: number;
  entries: number;
  approvedMs: number;
  pending: number;
  billableMs: number;
}

export interface ExportResult {
  path: string;
  count: number;
}

export interface ProjectStats {
  projectId: string;
  entries: number;
  totalMs: number;
  rangeMs: number;
  monthMs: number;
  billableMs: number;
  billableMonthMs: number;
  lastActivity?: number | null;
}

export interface ProjectRule {
  id: string;
  matchKind: string;
  pattern: string;
  origin: "manual" | "suggested" | "hint" | "app" | string;
}

export interface HintMatch {
  hint: string;
  matchKind?: string | null;
  pattern?: string | null;
  matchedMs: number;
}

export interface HintPreview {
  hints: HintMatch[];
  totalMs: number;
}

export interface ProjectSuggestion {
  key: string;
  name: string;
  ms: number;
  evidence: string[];
}

export interface ImportSummary {
  created: number;
  clientsCreated: number;
  skipped: string[];
}
