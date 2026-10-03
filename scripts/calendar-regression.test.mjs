import assert from "node:assert/strict";
import test from "node:test";
import { packOverlaps } from "../src/pages/calendar/overlap.ts";
import { readViewport, rememberViewport } from "../src/pages/calendar/viewport.ts";

const span = (id, startedAt, endedAt) => ({ id, startedAt, endedAt });

test("parallel entries use distinct subcolumns under one time axis", () => {
  const placed = packOverlaps([span("human", 0, 100), span("agent-a", 10, 70), span("agent-b", 20, 80)]);
  assert.deepEqual(placed.map(({ item, column, columns }) => [item.id, column, columns]), [["human", 0, 3], ["agent-a", 1, 3], ["agent-b", 2, 3]]);
});

test("adjacent entries do not overlap and later isolated entries reclaim width", () => {
  const placed = packOverlaps([span("b", 20, 30), span("a", 0, 20), span("c", 30, 50)]);
  assert.deepEqual(placed.map(({ column, columns }) => [column, columns]), [[0, 1], [0, 1], [0, 1]]);
});

test("connected overlap groups share width even when a column can be reused", () => {
  const placed = packOverlaps([span("long", 0, 100), span("first", 10, 30), span("second", 30, 80), span("next", 200, 250)]);
  assert.deepEqual(placed.map(({ column, columns }) => [column, columns]), [[0, 2], [1, 2], [1, 2], [0, 1]]);
});

test("equal starts pack stably and an empty range stays empty", () => {
  assert.deepEqual(packOverlaps([]), []);
  assert.deepEqual(packOverlaps([span("a", 0, 20), span("b", 0, 20)]).map(({ item, column }) => [item.id, column]), [["a", 0], ["b", 1]]);
});

test("day time range/zoom and scroll survive page remount without replacing other dates or views", () => {
  rememberViewport("calendar:day:1", { top: 450, left: 0, hourHeight: 90 });
  rememberViewport("calendar:day:2", { top: 850, left: 0, hourHeight: 120 });
  rememberViewport("timesheet:day:1", { top: 10, left: 0, hourHeight: 45 });
  assert.deepEqual(readViewport("calendar:day:1"), { top: 450, left: 0, hourHeight: 90 });
  assert.deepEqual(readViewport("calendar:day:2"), { top: 850, left: 0, hourHeight: 120 });
  assert.equal(readViewport("calendar:day:3"), undefined);
});

test("timeline restores horizontal and vertical scrolling together", () => {
  rememberViewport("calendar:timeline:1", { top: 42, left: 320 });
  assert.deepEqual(readViewport("calendar:timeline:1"), { top: 42, left: 320 });
});
