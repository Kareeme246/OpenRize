import assert from "node:assert/strict";
import { test } from "node:test";
import { MERGE_GAP_MS, mergeCandidates } from "../src/lib/merge.ts";
import type { TimeEntry } from "../src/lib/types";

const MINUTE = 60_000;

/** A local wall-clock time on an ordinary mid-month day. */
const at = (hour: number, minute = 0, second = 0): number =>
  new Date(2026, 5, 15, hour, minute, second, 0).getTime();

/** Local midnight, so the day-boundary cases hold in any timezone. */
const midnight = new Date(2026, 5, 16, 0, 0, 0, 0).getTime();

const entry = (id: string, startedAt: number, endedAt: number): TimeEntry => ({
  id,
  startedAt,
  endedAt,
  description: id,
  status: "pending",
  source: "auto",
  billable: false,
  createdAt: startedAt,
  updatedAt: endedAt,
  descriptionOrigin: "template",
});

const open = entry("open", at(10, 20), at(10, 40));

test("the tolerance is five minutes", () => {
  assert.equal(MERGE_GAP_MS, 5 * MINUTE);
});

test("an entry that abuts on either side is a neighbour", () => {
  const before = entry("before", at(10, 0), at(10, 20));
  const after = entry("after", at(10, 40), at(11, 0));
  const candidates = mergeCandidates([before, open, after], open, open.endedAt);
  assert.equal(candidates.prev?.id, "before");
  assert.equal(candidates.prevGapMs, 0);
  assert.equal(candidates.next?.id, "after");
  assert.equal(candidates.nextGapMs, 0);
});

test("a gap inside the tolerance is still the same border", () => {
  const before = entry("before", at(9, 55), at(10, 17));
  const after = entry("after", at(10, 43), at(11, 0));
  const candidates = mergeCandidates([before, open, after], open, open.endedAt);
  assert.equal(candidates.prev?.id, "before");
  assert.equal(candidates.prevGapMs, 3 * MINUTE);
  assert.equal(candidates.next?.id, "after");
  assert.equal(candidates.nextGapMs, 3 * MINUTE);
});

test("a gap of exactly the tolerance is still a neighbour", () => {
  const after = entry("after", at(10, 40) + MERGE_GAP_MS, at(11, 0));
  const candidates = mergeCandidates([open, after], open, open.endedAt);
  assert.equal(candidates.next?.id, "after");
  assert.equal(candidates.nextGapMs, MERGE_GAP_MS);
});

test("a gap beyond the tolerance is not a neighbour", () => {
  const before = entry("before", at(10, 0), at(10, 20) - MERGE_GAP_MS - 1);
  const after = entry("after", at(10, 40) + MERGE_GAP_MS + 1, at(11, 0));
  const candidates = mergeCandidates([before, open, after], open, open.endedAt);
  assert.equal(candidates.prev, undefined);
  assert.equal(candidates.prevGapMs, undefined);
  assert.equal(candidates.next, undefined);
  assert.equal(candidates.nextGapMs, undefined);
});

test("an overlapping entry is never a neighbour", () => {
  const after = entry("after", at(10, 30), at(11, 0));
  const before = entry("before", at(9, 30), at(10, 25));
  const candidates = mergeCandidates([after, open, before], open, open.endedAt);
  assert.equal(candidates.prev, undefined);
  assert.equal(candidates.next, undefined);
});

test("a neighbour on the other side of midnight is not a neighbour", () => {
  // 23:58 to 23:59 sits a minute before midnight, and the neighbour starts
  // 00:01: close enough to count as the same border, but a different day.
  const late = entry("late", midnight - 2 * MINUTE, midnight - MINUTE);
  const early = entry("early", midnight + MINUTE, midnight + 5 * MINUTE);
  const forwards = mergeCandidates([late, early], late, late.endedAt);
  assert.equal(forwards.next, undefined);
  assert.equal(forwards.nextGapMs, undefined);
  const backwards = mergeCandidates([late, early], early, early.endedAt);
  assert.equal(backwards.prev, undefined);
  assert.equal(backwards.prevGapMs, undefined);
});

test("a neighbour on the same side of midnight still counts", () => {
  const earlier = entry(
    "earlier",
    midnight - 8 * MINUTE,
    midnight - 6 * MINUTE,
  );
  const late = entry("late", midnight - 5 * MINUTE, midnight - 3 * MINUTE);
  const candidates = mergeCandidates([earlier, late], late, late.endedAt);
  assert.equal(candidates.prev?.id, "earlier");
  assert.equal(candidates.prevGapMs, MINUTE);
});

test("the closest neighbour on a side wins", () => {
  const near = entry("near", at(10, 42), at(10, 50));
  const far = entry("far", at(10, 44), at(10, 50));
  const candidates = mergeCandidates([far, near, open], open, open.endedAt);
  assert.equal(candidates.next?.id, "near");
  assert.equal(candidates.nextGapMs, 2 * MINUTE);
});

test("a recording entry is measured from its live edge", () => {
  const live = entry("live", at(10, 0), at(10, 10));
  live.status = "building";
  const after = entry("after", at(10, 22), at(10, 30));
  // Ended-at trails the clock; only the live edge makes this a neighbour.
  const candidates = mergeCandidates([live, after], live, at(10, 20));
  assert.equal(candidates.next?.id, "after");
  assert.equal(candidates.nextGapMs, 2 * MINUTE);
});
