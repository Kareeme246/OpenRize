/**
 * How an entry reads in every view: one state machine for the Calendar's
 * blocks, My Timesheet's tabs, and Time Entries' rows (design board, D1).
 */

import { entryConfidence, PREFILL } from "./confidence";
import type { Category, TimeEntry } from "./types";

export type BlockState =
  | "building"
  | "processing"
  | "failed"
  | "needsYou"
  | "pending"
  | "approved";

export function blockState(entry: TimeEntry): BlockState {
  if (entry.status === "approved") return "approved";
  const job = entry.ai?.state;
  if (entry.status === "processing" || job === "queued" || job === "running") {
    return "processing";
  }
  if (entry.status === "building") return "building";
  const confidence = confidenceOf(entry);
  if (job === "failed" && confidence === undefined) return "failed";
  if (!entry.categoryId || (confidence !== undefined && confidence < PREFILL)) {
    return "needsYou";
  }
  return "pending";
}

/** The weaker field's confidence: an entry clears review only on both. */
export function confidenceOf(entry: TimeEntry): number | undefined {
  return entryConfidence(
    entry.ai?.categoryConfidence,
    entry.ai?.projectConfidence,
  );
}

/** The core's refusal text, kept in step with `LOCKED` in activity.rs. */
export const LOCKED_HINT =
  "Approved entries are locked. Unapprove to change them.";

/**
 * Approved entries are frozen: the core refuses every change but unapprove,
 * so views disable their controls instead of offering a doomed action.
 */
export function isLocked(entry: TimeEntry): boolean {
  return entry.status === "approved";
}

/** Invoiced time can be neither unapproved nor deleted. */
export const INVOICED_HINT = "Invoiced entries can't be unapproved or deleted.";

export function isInvoiced(entry: TimeEntry): boolean {
  return entry.invoiceId !== undefined && entry.invoiceId !== null;
}

const REVIEWABLE: ReadonlySet<BlockState> = new Set([
  "pending",
  "needsYou",
  "failed",
]);

/** Waiting on the user (review mode's queue, the "To review" tab). */
export function isReviewable(entry: TimeEntry): boolean {
  return REVIEWABLE.has(blockState(entry));
}

/** Still being recorded or categorized; can't be approved yet. */
export function isInFlight(entry: TimeEntry): boolean {
  const state = blockState(entry);
  return state === "processing" || state === "building";
}

export function entryEnd(entry: TimeEntry, now: number = Date.now()): number {
  return entry.status === "building"
    ? Math.max(entry.endedAt, now)
    : entry.endedAt;
}

/**
 * The session recording right now, if any: the building entry that has
 * started. The Calendar pins its now line to this entry's live edge.
 */
export function recordingEntry(
  entries: TimeEntry[],
  now: number = Date.now(),
): TimeEntry | undefined {
  return entries.find(
    (entry) => entry.status === "building" && entry.startedAt <= now,
  );
}

export function durationOf(entry: TimeEntry, now: number = Date.now()): number {
  return Math.max(0, entryEnd(entry, now) - entry.startedAt);
}

export function totalDuration(
  entries: TimeEntry[],
  now: number = Date.now(),
): number {
  return entries.reduce((sum, entry) => sum + durationOf(entry, now), 0);
}

/** Whether time in this category counts toward work hours. */
export function countsAsWork(
  categoryId: string | null | undefined,
  categoryById: Map<string, Category>,
): boolean {
  if (!categoryId) return true;
  return categoryById.get(categoryId)?.countsAsWork ?? true;
}

/** Tracked time that counts toward work hours (breaks excluded). */
export function workDuration(
  entries: TimeEntry[],
  categoryById: Map<string, Category>,
  now: number = Date.now(),
): number {
  return entries.reduce(
    (sum, entry) =>
      countsAsWork(entry.categoryId, categoryById)
        ? sum + durationOf(entry, now)
        : sum,
    0,
  );
}

/** Keyboard shortcuts stand down while the user types in a field. */
export function isTyping(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  return (
    element?.tagName === "INPUT" ||
    element?.tagName === "TEXTAREA" ||
    element?.tagName === "SELECT" ||
    element?.isContentEditable === true
  );
}

/**
 * Splits at the app switch nearest the middle, else at the midpoint; for the
 * live entry, the middle of what it has recorded until now.
 */
export function splitPoint(
  entry: TimeEntry,
  segments: { app: string; startedAt: number }[],
  now: number = Date.now(),
): number {
  const end = entryEnd(entry, now);
  const midpoint = Math.floor((entry.startedAt + end) / 2);
  let splitAt = midpoint;
  let best = Number.POSITIVE_INFINITY;
  segments.forEach((segment, index) => {
    const previous = segments[index - 1];
    if (!previous || previous.app === segment.app) return;
    if (segment.startedAt <= entry.startedAt || segment.startedAt >= end)
      return;
    const distance = Math.abs(segment.startedAt - midpoint);
    if (distance < best) {
      best = distance;
      splitAt = segment.startedAt;
    }
  });
  return splitAt;
}
