import assert from "node:assert/strict";
import { test } from "node:test";
import { entryShortcut } from "../src/lib/entryShortcuts.ts";
import {
  MERGE_GAP_MS,
  mergeBlocks,
  mergeCandidates,
} from "../src/lib/merge.ts";
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

test("an entry that overlaps by seconds is a neighbour with no gap", () => {
  const after = entry("after", at(10, 40, 0) - 5_000, at(11, 0));
  const before = entry("before", at(9, 30), at(10, 20, 8));
  const candidates = mergeCandidates([after, open, before], open, open.endedAt);
  assert.equal(candidates.next?.id, "after");
  assert.equal(candidates.nextGapMs, 0);
  assert.equal(candidates.prev?.id, "before");
  assert.equal(candidates.prevGapMs, 0);
});

test("an entry that overlaps by a minute is a neighbour on its own side", () => {
  const after = entry("after", at(10, 39), at(11, 0));
  const before = entry("before", at(9, 30), at(10, 21));
  const candidates = mergeCandidates([after, open, before], open, open.endedAt);
  assert.equal(candidates.next?.id, "after");
  assert.equal(candidates.prev?.id, "before");
});

test("an overlap is a neighbour from either entry's point of view", () => {
  const other = entry("other", at(10, 39), at(11, 0));
  assert.equal(
    mergeCandidates([open, other], open, open.endedAt).next?.id,
    "other",
  );
  assert.equal(
    mergeCandidates([open, other], other, other.endedAt).prev?.id,
    "open",
  );
});

test("an entry inside another, or around it, is a neighbour", () => {
  const inside = entry("inside", at(10, 25), at(10, 30));
  const around = entry("around", at(10, 0), at(11, 0));
  assert.ok(mergeCandidates([open, inside], open, open.endedAt).next);
  assert.ok(mergeCandidates([open, around], open, open.endedAt).prev);
});

test("an overlap on one side does not widen the other side's tolerance", () => {
  const overlapping = entry("overlapping", at(10, 30), at(11, 0));
  const tooFar = entry("tooFar", at(9, 0), at(10, 20) - MERGE_GAP_MS - 1);
  const candidates = mergeCandidates(
    [overlapping, tooFar, open],
    open,
    open.endedAt,
  );
  assert.equal(candidates.next?.id, "overlapping");
  assert.equal(candidates.prev, undefined);
});

test("an overlap beats a gap on the same side", () => {
  const gapped = entry("gapped", at(10, 43), at(11, 0));
  const overlapping = entry("overlapping", at(10, 39), at(11, 0));
  const candidates = mergeCandidates(
    [gapped, overlapping, open],
    open,
    open.endedAt,
  );
  assert.equal(candidates.next?.id, "overlapping");
  assert.equal(candidates.nextGapMs, 0);
});

test("an overlap across midnight is not a neighbour", () => {
  const late = entry("late", midnight - 5 * MINUTE, midnight + 5 * MINUTE);
  const early = entry("early", midnight - 2 * MINUTE, midnight + 10 * MINUTE);
  assert.equal(
    mergeCandidates([late, early], late, late.endedAt).next,
    undefined,
  );
  assert.equal(
    mergeCandidates([late, early], early, early.endedAt).prev,
    undefined,
  );
});

test("an overlap that ends exactly at midnight is one day's work", () => {
  const evening = entry("evening", midnight - 60 * MINUTE, midnight);
  const late = entry("late", midnight - 10 * MINUTE, midnight - MINUTE);
  const candidates = mergeCandidates([evening, late], evening, evening.endedAt);
  assert.equal(candidates.next?.id, "late");
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

/** The locked rule the sheet uses: approved entries are frozen. */
const approved = (e: TimeEntry): boolean => e.status === "approved";

test("a lone session has both merge sides present and unavailable", () => {
  const candidates = mergeCandidates([open], open, open.endedAt);
  assert.deepEqual(mergeBlocks(candidates, approved), {
    prev: "none",
    next: "none",
  });
});

test("the first, middle and last of a run enable the sides that have a neighbour", () => {
  const top = entry("top", at(10, 0), at(10, 20));
  const middle = entry("middle", at(10, 20), at(10, 40));
  const bottom = entry("bottom", at(10, 40), at(11, 0));
  const run = [top, middle, bottom];
  const sides = (current: TimeEntry) =>
    mergeBlocks(mergeCandidates(run, current, current.endedAt), approved);
  assert.deepEqual(sides(top), { prev: "none", next: undefined });
  assert.deepEqual(sides(middle), { prev: undefined, next: undefined });
  assert.deepEqual(sides(bottom), { prev: undefined, next: "none" });
});

test("two sessions leave the far side of each unavailable", () => {
  const top = entry("top", at(10, 0), at(10, 20));
  const bottom = entry("bottom", at(10, 20), at(10, 40));
  const sides = (current: TimeEntry) =>
    mergeBlocks(
      mergeCandidates([top, bottom], current, current.endedAt),
      approved,
    );
  assert.deepEqual(sides(top), { prev: "none", next: undefined });
  assert.deepEqual(sides(bottom), { prev: undefined, next: "none" });
});

test("an approved neighbour is blocked, not absent", () => {
  const before = entry("before", at(10, 0), at(10, 20));
  before.status = "approved";
  const after = entry("after", at(10, 40), at(11, 0));
  const candidates = mergeCandidates([before, open, after], open, open.endedAt);
  assert.deepEqual(mergeBlocks(candidates, approved), {
    prev: "locked",
    next: undefined,
  });
});

test("no merge shortcut fires", () => {
  const key = (
    name: string,
    modifiers: { metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean } = {},
  ) =>
    entryShortcut({
      key: name,
      metaKey: false,
      ctrlKey: false,
      altKey: false,
      ...modifiers,
    });
  for (const name of ["m", "M"]) {
    assert.equal(key(name), undefined);
    assert.equal(key(name, { altKey: true }), undefined);
    assert.equal(key(name, { metaKey: true }), undefined);
    assert.equal(key(name, { ctrlKey: true }), undefined);
  }
});

test("the other entry shortcuts are unchanged", () => {
  const key = (
    name: string,
    modifiers: { metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean } = {},
  ) =>
    entryShortcut({
      key: name,
      metaKey: false,
      ctrlKey: false,
      altKey: false,
      ...modifiers,
    });
  assert.equal(key("s"), "split");
  assert.equal(key("S"), "split");
  assert.equal(key("s", { metaKey: true }), undefined);
  assert.equal(key("s", { altKey: true }), undefined);
  assert.equal(key("Escape"), "close");
  assert.equal(key("Enter", { metaKey: true }), "accept");
  assert.equal(key("Enter", { ctrlKey: true }), "accept");
  assert.equal(key("Enter"), undefined);
  assert.equal(key("Backspace", { metaKey: true }), "reject");
  assert.equal(key("Backspace"), undefined);
});
