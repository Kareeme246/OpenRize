/**
 * A neighbour overlaps the open entry or is within `MERGE_GAP_MS`, on one local
 * day. Keep in step with `merge_gap` in src-tauri/core/src/activity.rs.
 */

import type { TimeEntry } from "./types";

export const MERGE_GAP_MS = 300_000;

export interface MergeCandidate {
  prev?: TimeEntry;
  next?: TimeEntry;
  /**
   * Untracked time the merge claims as tracked, per side; zero when touching or
   * overlapping.
   */
  prevGapMs?: number;
  nextGapMs?: number;
}

/** Why a merge side is off: no neighbour, or it is approved. */
export type MergeBlock = "none" | "locked";

/**
 * Which merge buttons are usable: a side is blocked with no neighbour or a
 * locked one.
 */
export function mergeBlocks(
  candidates: MergeCandidate | undefined,
  isLocked: (entry: TimeEntry) => boolean,
): { prev?: MergeBlock; next?: MergeBlock } {
  const block = (entry: TimeEntry | undefined): MergeBlock | undefined =>
    !entry ? "none" : isLocked(entry) ? "locked" : undefined;
  return { prev: block(candidates?.prev), next: block(candidates?.next) };
}

function localDay(ms: number): [number, number, number] {
  const date = new Date(ms);
  return [date.getFullYear(), date.getMonth(), date.getDate()];
}

function sameLocalDay(left: number, right: number): boolean {
  const [a, b] = [localDay(left), localDay(right)];
  return a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
}

/**
 * Start decides, then end, then id, so an overlapping entry lands on exactly
 * one side.
 */
function startsBefore(other: TimeEntry, current: TimeEntry): boolean {
  if (other.startedAt !== current.startedAt) {
    return other.startedAt < current.startedAt;
  }
  if (other.endedAt !== current.endedAt) return other.endedAt < current.endedAt;
  return other.id < current.id;
}

/**
 * Untracked time between the entries; 0 on overlap, `undefined` when too far
 * apart or on another day.
 */
function gapBetween(
  current: TimeEntry,
  endMs: number,
  other: TimeEntry,
): number | undefined {
  const gapAfter = other.startedAt - endMs;
  const gapBefore = current.startedAt - other.endedAt;
  if (gapAfter >= 0) {
    return gapAfter <= MERGE_GAP_MS && sameLocalDay(endMs, other.startedAt)
      ? gapAfter
      : undefined;
  }
  if (gapBefore >= 0) {
    return gapBefore <= MERGE_GAP_MS &&
      sameLocalDay(other.endedAt, current.startedAt)
      ? gapBefore
      : undefined;
  }
  // Overlap: the merged span is one day's work only if it ends on the day it
  // starts. Its last instant is the one before the end, so a span that stops
  // exactly at midnight still belongs to the day before.
  const start = Math.min(current.startedAt, other.startedAt);
  const end = Math.max(endMs, other.endedAt);
  return sameLocalDay(start, Math.max(start, end - 1)) ? 0 : undefined;
}

/**
 * The closest neighbour per side; `endMs` is the live edge for a recording
 * session.
 */
export function mergeCandidates(
  entries: readonly TimeEntry[],
  current: TimeEntry,
  endMs: number,
): MergeCandidate {
  const candidates: MergeCandidate = {};
  for (const other of entries) {
    if (other.id === current.id) continue;
    const gap = gapBetween(current, endMs, other);
    if (gap === undefined) continue;
    if (startsBefore(other, current)) {
      if (candidates.prevGapMs === undefined || gap < candidates.prevGapMs) {
        candidates.prev = other;
        candidates.prevGapMs = gap;
      }
    } else if (
      candidates.nextGapMs === undefined ||
      gap < candidates.nextGapMs
    ) {
      candidates.next = other;
      candidates.nextGapMs = gap;
    }
  }
  return candidates;
}
