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

export interface UpdateProject {
  clientId?: string;
  name?: string;
  color?: string;
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

export type Route =
  | {
      name: "calendar";
      scale?: "day" | "week" | "month";
      date?: string;
      entryId?: string;
      review?: boolean;
    }
  | {
      name: "timesheet";
      scale?: "day" | "week" | "month";
      date?: string;
      tab?: "review" | "processing" | "approved" | "all";
      groupBy?: string;
    }
  | {
      name: "apps";
      scale?: "day" | "week" | "month";
      date?: string;
      tab?: "timeline" | "log";
      appId?: string;
    }
  | {
      name: "entries";
      range?: string;
      view?: "table" | "charts" | "log";
      filters?: Record<string, string>;
      groupBy?: string;
      stackBy?: string;
    }
  | {
      name: "timesheets";
      scale?: "week" | "month";
      start?: string;
      rows?: "project" | "category";
    }
  | {
      name: "projects";
      tab?: "active" | "completed" | "archived" | "clients";
      projectId?: string;
      range?: string;
    }
  | {
      name: "invoices";
      invoiceId?: string;
    }
  | {
      name: "settings";
      section?: string;
    };
