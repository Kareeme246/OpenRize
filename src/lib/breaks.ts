/**
 * Mirror of the break types in src-tauri/src/breaks; Rust owns the state
 * machine.
 */

export type BreakSource = "interval" | "scheduled" | "manual" | "idle";
export type BreakPhase = "idle" | "due" | "nudge" | "onBreak" | "over";

export interface ReminderView {
  source: BreakSource;
  scheduleId: string | null;
  label: string;
  plannedMs: number;
  dueAt: number;
  workedMs: number;
  snoozes: number;
  snoozeMinutes: number;
  message: string | null;
  canSnooze: boolean;
}

export interface BreakView {
  id: string | null;
  source: BreakSource;
  label: string;
  plannedMs: number;
  startedAt: number;
  endedAt: number | null;
  pausedTimers: string[];
  resumedTimers: string[];
  /** Why the app ended the break, e.g. "The break was cut short". */
  note: string | null;
}

export interface NextBreak {
  at: number;
  source: BreakSource;
  label: string;
}

export interface BreakState {
  phase: BreakPhase;
  reminder: ReminderView | null;
  current: BreakView | null;
  next: NextBreak | null;
  pausedUntil: number | null;
  snoozedUntil: number | null;
  stopwatch: StopwatchReminderView | null;
}

export interface StopwatchReminderView {
  id: string;
  label: string;
  startedAt: number;
}

export const IDLE_BREAK_STATE: BreakState = {
  phase: "idle",
  reminder: null,
  current: null,
  next: null,
  pausedUntil: null,
  snoozedUntil: null,
  stopwatch: null,
};

export type BreakStatus = "taken" | "skipped" | "missed";

export interface BreakEntry {
  id: string;
  source: BreakSource;
  scheduleId: string | null;
  dueAt: number | null;
  plannedMs: number;
  status: BreakStatus;
  snoozes: number;
  startedAt: number | null;
  endedAt: number | null;
  segmentId: number | null;
}

export const SNOOZE_CHOICES = [5, 10, 15] as const;

const MINUTE_MS = 60_000;

/** `4:05` - a countdown or elapsed readout. */
export function formatCountdown(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

/** `5-minute`, `1h 30m`: a planned length inside a sentence. */
export function formatPlanned(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / MINUTE_MS));
  if (minutes < 60) return `${minutes}-minute`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours}-hour` : `${hours}h ${rest}m`;
}

/** `50 min`, `1h 05m`: time worked. */
export function formatWorked(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / MINUTE_MS));
  if (minutes < 60) return `${minutes} min`;
  const rest = minutes % 60;
  return `${Math.floor(minutes / 60)}h${rest === 0 ? "" : ` ${String(rest).padStart(2, "0")}m`}`;
}

/** `in 18m`, `in 1h 5m`, `now`. */
export function formatUntil(ms: number): string {
  const minutes = Math.ceil(ms / MINUTE_MS);
  if (minutes <= 0) return "now";
  if (minutes < 60) return `in ${minutes}m`;
  const rest = minutes % 60;
  return `in ${Math.floor(minutes / 60)}h${rest === 0 ? "" : ` ${rest}m`}`;
}

/**
 * A break taken on purpose (Take a break now, or Start break on a reminder).
 * Rest credited from idle time resets the work clock but is not one.
 */
export function isOfficialBreak(entry: BreakEntry): boolean {
  switch (entry.source) {
    case "manual":
    case "interval":
      return true;
    case "scheduled":
      return entry.dueAt !== null;
    case "idle":
      return false;
  }
}

/**
 * Official breaks actually taken, as drawn on the Calendar; inactivity draws
 * nothing.
 */
export function takenBreaks(entries: BreakEntry[]): BreakEntry[] {
  return entries.filter(
    (entry) =>
      entry.status === "taken" &&
      entry.startedAt !== null &&
      isOfficialBreak(entry),
  );
}

export function breakEnd(entry: BreakEntry, now: number): number {
  return entry.endedAt ?? Math.max(entry.startedAt ?? now, now);
}
