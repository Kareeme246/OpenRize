/**
 * Which neighbour a merge may take in. A merge joins the open entry with the
 * one entry that shares its border, and capture rarely stops and starts on
 * the same millisecond: a few minutes between two entries are the same
 * border as far as the user is concerned, so they still count as neighbours.
 *
 * Neighbouring means: no overlap, and at most `MERGE_GAP_MS` of untracked
 * time between them, on the same local calendar day - time either side of
 * midnight is a different day's work however close together the two entries
 * are. Keep the constant in step with `MERGE_GAP_MS` in
 * `src-tauri/core/src/activity.rs`, which refuses anything the panel would
 * not offer.
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
   * two entries touch; absent when there is no neighbour on that side.
   */
  prevGapMs?: number;
  nextGapMs?: number;
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
 * The neighbours `current` may merge with, drawn from the entries a view has
 * loaded. `endMs` is the entry's live edge for a session still recording.
 * A merge takes in the closest candidate on a side; entries that overlap the
 * current one, or sit further than `MERGE_GAP_MS` away, are never candidates.
 */
export function mergeCandidates(
  entries: readonly TimeEntry[],
  current: TimeEntry,
  endMs: number,
): MergeCandidate {
  const candidates: MergeCandidate = {};
  for (const other of entries) {
    if (other.id === current.id) continue;
    const gapAfter = other.startedAt - endMs;
    const gapBefore = current.startedAt - other.endedAt;
    if (gapAfter >= 0 && gapAfter <= MERGE_GAP_MS) {
      if (
        candidates.nextGapMs === undefined ||
        gapAfter < candidates.nextGapMs
      ) {
        candidates.next = other;
        candidates.nextGapMs = gapAfter;
      }
    } else if (gapBefore >= 0 && gapBefore <= MERGE_GAP_MS) {
      if (
        candidates.prevGapMs === undefined ||
        gapBefore < candidates.prevGapMs
      ) {
        candidates.prev = other;
        candidates.prevGapMs = gapBefore;
      }
    }
  }
  if (
    candidates.prev &&
    !sameLocalDay(candidates.prev.endedAt, current.startedAt)
  ) {
    candidates.prev = undefined;
    candidates.prevGapMs = undefined;
  }
  if (candidates.next && !sameLocalDay(endMs, candidates.next.startedAt)) {
    candidates.next = undefined;
    candidates.nextGapMs = undefined;
  }
  return candidates;
}
