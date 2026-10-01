import assert from "node:assert/strict";
import { test } from "node:test";
import { isOfficialBreak, takenBreaks } from "../src/lib/breaks.ts";

const MIN = 60_000;
const row = (id, fields) => ({
  id,
  source: "interval",
  scheduleId: null,
  dueAt: 9 * 60 * MIN,
  plannedMs: 5 * MIN,
  status: "taken",
  snoozes: 0,
  startedAt: 9 * 60 * MIN,
  endedAt: 9 * 60 * MIN + 5 * MIN,
  segmentId: 1,
  ...fields,
});
const ids = (list) => list.map((entry) => entry.id);

test("breaks the user started draw on the calendar", () => {
  const rows = [
    row("interval"),
    row("manual", { source: "manual", dueAt: null }),
    row("scheduled", { source: "scheduled", scheduleId: "lunch" }),
  ];
  assert.deepEqual(ids(takenBreaks(rows)), ["interval", "manual", "scheduled"]);
});

test("rest credited from idle time draws nothing", () => {
  const rows = [
    // A short pause at the desk between two pieces of work.
    row("pause", { source: "idle", dueAt: null }),
    // Walked away, the Mac slept overnight: 17:30 to 08:50 the next day.
    row("overnight", {
      source: "idle",
      dueAt: null,
      startedAt: 17.5 * 60 * MIN,
      endedAt: (24 + 8) * 60 * MIN + 50 * MIN,
    }),
    // Away at lunch time: the idle stretch is credited as the scheduled break.
    row("credited-lunch", {
      source: "scheduled",
      scheduleId: "lunch",
      dueAt: null,
    }),
  ];
  assert.deepEqual(takenBreaks(rows), []);
});

test("a backfilled idle stretch stays hidden and the official break beside it stays", () => {
  // 10:00-11:30 was a meeting away from the Mac, later added by hand as an
  // entry; the stored idle credit underneath it is kept for the work clock.
  const backfilled = row("meeting", {
    source: "idle",
    dueAt: null,
    startedAt: 10 * 60 * MIN,
    endedAt: 11.5 * 60 * MIN,
  });
  const official = row("official", {
    startedAt: 11.5 * 60 * MIN,
    endedAt: 11.5 * 60 * MIN + 5 * MIN,
  });
  const rows = [backfilled, official];
  assert.deepEqual(ids(takenBreaks(rows)), ["official"]);
  // The rows themselves are untouched.
  assert.equal(rows.length, 2);
  assert.equal(backfilled.status, "taken");
});

test("skipped, missed, and never-started reminders draw nothing", () => {
  const rows = [
    row("skipped", { status: "skipped", startedAt: null, endedAt: null }),
    row("missed", { status: "missed", startedAt: null, endedAt: null }),
    row("no-start", { startedAt: null }),
    row("scheduled-skipped", {
      source: "scheduled",
      status: "skipped",
      startedAt: null,
      endedAt: null,
    }),
  ];
  assert.deepEqual(takenBreaks(rows), []);
});

test("a running official break still draws", () => {
  assert.deepEqual(ids(takenBreaks([row("running", { endedAt: null })])), [
    "running",
  ]);
});

test("isOfficialBreak by source", () => {
  assert.equal(isOfficialBreak(row("a", { source: "interval" })), true);
  assert.equal(isOfficialBreak(row("b", { source: "manual", dueAt: null })), true);
  assert.equal(isOfficialBreak(row("c", { source: "scheduled" })), true);
  assert.equal(
    isOfficialBreak(row("d", { source: "scheduled", dueAt: null })),
    false,
  );
  assert.equal(isOfficialBreak(row("e", { source: "idle", dueAt: null })), false);
});
