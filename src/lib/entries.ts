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
  if (entry.status === "building") return "building";
  const job = entry.ai?.state;
  if (entry.status === "processing" || job === "queued" || job === "running") {
    return "processing";
  }
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

export function durationOf(entry: TimeEntry): number {
  return Math.max(0, entry.endedAt - entry.startedAt);
}

export function totalDuration(entries: TimeEntry[]): number {
  return entries.reduce((sum, entry) => sum + durationOf(entry), 0);
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
): number {
  return entries.reduce(
    (sum, entry) =>
      countsAsWork(entry.categoryId, categoryById)
        ? sum + durationOf(entry)
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

/** Splits at the app switch nearest the middle, else at the midpoint. */
export function splitPoint(
  entry: TimeEntry,
  segments: { app: string; startedAt: number }[],
): number {
  const midpoint = Math.floor((entry.startedAt + entry.endedAt) / 2);
  let splitAt = midpoint;
  let best = Number.POSITIVE_INFINITY;
  segments.forEach((segment, index) => {
    const previous = segments[index - 1];
    if (!previous || previous.app === segment.app) return;
    if (
      segment.startedAt <= entry.startedAt ||
      segment.startedAt >= entry.endedAt
    )
      return;
    const distance = Math.abs(segment.startedAt - midpoint);
    if (distance < best) {
      best = distance;
      splitAt = segment.startedAt;
    }
  });
  return splitAt;
}
