import assert from "node:assert/strict";
import test from "node:test";
import { readViewport, rememberViewport } from "../src/pages/calendar/viewport.ts";

test("day time range/zoom and scroll survive page remount without replacing other dates or views", () => {
  rememberViewport("calendar:day:1", { top: 450, left: 0, hourHeight: 90 });
  rememberViewport("calendar:day:2", { top: 850, left: 0, hourHeight: 120 });
  rememberViewport("calendar:week:1", { top: 10, left: 0, hourHeight: 45 });
  assert.deepEqual(readViewport("calendar:day:1"), { top: 450, left: 0, hourHeight: 90 });
  assert.deepEqual(readViewport("calendar:day:2"), { top: 850, left: 0, hourHeight: 120 });
  assert.equal(readViewport("calendar:day:3"), undefined);
});

test("a view restores horizontal and vertical scrolling together", () => {
  rememberViewport("calendar:week:2", { top: 42, left: 320 });
  assert.deepEqual(readViewport("calendar:week:2"), { top: 42, left: 320 });
});
