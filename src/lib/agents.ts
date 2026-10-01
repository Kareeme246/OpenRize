/**
 * Coding agents seen through the Herdr and tmux extensions, mirroring
 * `src-tauri/src/agents` and `src-tauri/src/threads.rs`. Rust owns every
 * total; this file only describes what it sends and how to word it.
 */

export type ExtensionId = "herdr" | "tmux";

/** `off`, `notDetected`, `waiting` (on, no server yet), `connected`, `error`. */
export type ExtensionConnection =
  | "off"
  | "notDetected"
  | "waiting"
  | "connected"
  | "error";

export interface ExtensionStatus {
  id: ExtensionId;
  name: string;
  description: string;
  detected: boolean;
  detail: string | null;
  /** What the person chose by hand; null follows auto-detection. */
  preference: boolean | null;
  enabled: boolean;
  connection: ExtensionConnection;
  agents: number;
  error: string | null;
}

export type PaneState =
  | "running"
  | "needsYou"
  | "ready"
  | "idle"
  | "quiet"
  | "unknown";

export type JobState =
  | "running"
  | "needsYou"
  | "ready"
  | "idle"
  | "stalled"
  | "gone";

export type Confidence = "low" | "medium" | "high";

export interface LiveAgent {
  key: string;
  source: string;
  agent: string;
  state: PaneState;
  projectId: string | null;
  jobId: string | null;
  since: number;
  focused: boolean;
}

export interface JobSpan {
  jobId: string;
  startedAt: number;
  endedAt: number;
}

export interface AgentSegment {
  startedAt: number;
  endedAt: number;
  state: JobState;
}

export type JobPhase = "running" | "needsYou" | "ready" | "reviewed" | "gone";

export interface JobView {
  id: string;
  agent: string;
  source: string;
  projectId: string;
  startedAt: number;
  stoppedAt: number | null;
  reviewedAt: number | null;
  phase: JobPhase;
  confidence: Confidence;
  confirmed: boolean;
  segments: AgentSegment[];
  counted: JobSpan[];
  countedMs: number;
  pendingMs: number;
  waitingMs: number;
}

export interface Board {
  live: LiveAgent[];
  running: number;
  needsYou: number;
  ready: number;
  /** Agent turns today waiting for the person to confirm them. */
  toConfirm: number;
  /** Time agents waited on the person today, before they came. */
  waitedMs: number;
  jobs: JobView[];
  dayStart: number;
  dayEnd: number;
}

export const EMPTY_BOARD: Board = {
  live: [],
  running: 0,
  needsYou: 0,
  ready: 0,
  toConfirm: 0,
  waitedMs: 0,
  jobs: [],
  dayStart: 0,
  dayEnd: 0,
};

export interface ProjectLedger {
  projectId: string;
  /** The person's own time. */
  youMs: number;
  /** Counted agent time that is not already the person's own. */
  agentMs: number;
  billableMs: number;
  waitingMs: number;
  stillWaitingMs: number;
  pendingMs: number;
  overCapMs: number;
  stalledMs: number;
  policyExcludedMs: number;
  outsideSessionMs: number;
  agentSpans: JobSpan[];
}

export interface Ledger {
  projects: ProjectLedger[];
  workMs: number;
  billableMs: number;
  agentMs: number;
  waitingMs: number;
  stillWaitingMs: number;
  pendingMs: number;
  overCapMs: number;
}

export interface AgentReport {
  start: number;
  end: number;
  ledger: Ledger;
  jobs: JobView[];
}

// --- Threads (Stage 2) ---

export interface Visit {
  entryId: string;
  projectId: string | null;
  startedAt: number;
  endedAt: number;
}

export interface Rail {
  jobId: string;
  agent: string;
  startedAt: number;
  endedAt: number;
  state: JobState;
}

export interface Band {
  jobId: string;
  startedAt: number;
  endedAt: number;
}

export interface Thread {
  /** Null is time on no project. */
  projectId: string | null;
  /** The sum of the visits. */
  youMs: number;
  /** Counted agent time, on top of `youMs` and never part of it. */
  agentsMs: number;
  visits: Visit[];
  rails: Rail[];
  bands: Band[];
}

export interface Focus {
  switches: number;
  longestMs: number;
  longestProjectId: string | null;
}

export interface DayThreads {
  start: number;
  end: number;
  /** The person's work time: every visit, counted once. */
  workMs: number;
  agentsMs: number;
  focus: Focus;
  threads: Thread[];
}

/** Words for how an extension is doing, for Settings. */
export function connectionLabel(status: ExtensionStatus): string {
  const agents = `${status.agents} agent${status.agents === 1 ? "" : "s"}`;
  if (status.preference === false) return "Off · you turned it off";
  switch (status.connection) {
    case "notDetected":
      return "Not detected";
    case "waiting":
      return status.preference === null
        ? "Detected · enabled automatically · waiting for a server"
        : "On · waiting for a server";
    case "connected":
      return status.preference === null
        ? `Detected · enabled automatically · ${agents}`
        : `On · ${agents}`;
    case "error":
      return status.error ? `Error · ${status.error}` : "Error";
    case "off":
      return "Off";
  }
}
