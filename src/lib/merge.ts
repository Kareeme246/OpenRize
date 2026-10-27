/**
 * Which neighbour a merge may take in. A merge joins the open entry with the
 * one entry that shares its border, and capture rarely stops and starts on
 * the same millisecond: a few minutes between two entries are the same
 * border as far as the user is concerned, so they still count as neighbours.
 * Entries that overlap, say because one was dragged over the next, are
 * neighbours too, and are the one thing a merge cannot be refused for being
 * too close.
 *
 * Neighbouring means: either the two overlap, or at most `MERGE_GAP_MS` of
 * untracked time sits between them (the tolerance only measures that gap, so
 * an overlap never adds to it and an overlapping entry on one side does not
 * widen the window on the other). Either way the merge is on one local
 * calendar day - time either side of midnight is a different day's work
 * however close together the two entries are. The merged entry spans the
 * earliest start to the latest end, so an overlap is absorbed, not counted
 * twice. Keep the rule in step with `merge_gap` in
 * `src-tauri/core/src/activity.rs`, which the store refuses merges by.
 */

import type { TimeEntry } from "./types";

/** How far a border may be missed and still be the same border. */
export const MERGE_GAP_MS = 300_000;

export interface MergeCandidate {
  /** The entry before the open one, when it is a neighbour. */
  prev?: TimeEntry;
  /** The entry after the open one, when it is a neighbour. */
  next?: TimeEntry;
  /**
   * Untracked time the merge claims as tracked time, per side. Zero when the
   * two entries touch or overlap; absent when there is no neighbour on that
   * side.
   */
  prevGapMs?: number;
  nextGapMs?: number;
}

/**
 * Why a merge side is off: there is no neighbour on that side, or it is
 * approved. A side that can merge has no reason.
 */
export type MergeBlock = "none" | "locked";

/**
 * Both merge buttons always show; this says which are usable. A side is
 * blocked when it has no neighbour, or when that neighbour is locked.
 */
export function mergeBlocks(
  candidates: MergeCandidate | undefined,
  isLocked: (entry: TimeEntry) => boolean,
): { prev?: MergeBlock; next?: MergeBlock } {
  const block = (entry: TimeEntry | undefined): MergeBlock | undefined =>
    !entry ? "none" : isLocked(entry) ? "locked" : undefined;
  return { prev: block(candidates?.prev), next: block(candidates?.next) };
}

/** Local calendar day of `ms`, as `localDateString` keys dates. */
function localDay(ms: number): [number, number, number] {
  const date = new Date(ms);
  return [date.getFullYear(), date.getMonth(), date.getDate()];
}

function sameLocalDay(left: number, right: number): boolean {
  const [a, b] = [localDay(left), localDay(right)];
  return a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
}

/**
 * Whether `other` sorts before `current`, which puts it on the earlier side.
 * Start decides, then end, then id, so an overlapping entry lands on exactly
 * one side however the two overlap.
 */
function startsBefore(other: TimeEntry, current: TimeEntry): boolean {
  if (other.startedAt !== current.startedAt) {
    return other.startedAt < current.startedAt;
  }
  if (other.endedAt !== current.endedAt) return other.endedAt < current.endedAt;
  return other.id < current.id;
}

/**
 * The untracked time between `current` (ending at `endMs`) and `other`, 0 when
 * they overlap, or `undefined` when they are too far apart or not on one
 * local day.
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
 * The neighbours `current` may merge with, drawn from the entries a view has
 * loaded. `endMs` is the entry's live edge for a session still recording.
 * A merge takes in the closest candidate on a side; entries further than
 * `MERGE_GAP_MS` away, or on another local day, are never candidates.
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
