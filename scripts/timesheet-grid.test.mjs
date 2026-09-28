import assert from "node:assert/strict";
import { test } from "node:test";
import { projectWeek } from "../src/lib/timesheetGrid.ts";

test("project-day totals split at local edges and only approved billable time is billable", () => {
  const edges = [0, 3_600_000, 5_400_000];
  const entry = (projectId, startedAt, endedAt, status, billable) => ({
    projectId,
    startedAt,
    endedAt,
    status,
    billable,
  });
  const rows = projectWeek(
    [
      entry("a", 1_800_000, 4_500_000, "approved", true),
      entry("a", 0, 900_000, "pending", true),
      entry("b", 0, 3_600_000, "approved", false),
      entry(undefined, 0, 3_600_000, "approved", true),
      entry("previous-week", -3_600_000, 0, "approved", true),
    ],
    edges,
  );
  assert.deepEqual(rows, [
    { projectId: "a", days: [2_700_000, 900_000], approvedMs: 2_700_000, billableMs: 2_700_000 },
    { projectId: "b", days: [3_600_000, 0], approvedMs: 3_600_000, billableMs: 0 },
  ]);
});

test("blank week produces empty project rows", () => {
  const edges = [0, 86_400_000, 172_800_000];
  const rows = projectWeek([], edges);
  assert.deepEqual(rows, []);
});

test("entries entirely outside edges produce empty project rows", () => {
  const edges = [86_400_000, 172_800_000];
  const rows = projectWeek(
    [
      {
        id: "e1",
        projectId: "p1",
        startedAt: 0,
        endedAt: 3_600_000,
        status: "approved",
        billable: true,
        source: "manual",
        createdAt: 0,
        updatedAt: 0,
        description: "work",
        descriptionOrigin: "user",
      },
    ],
    edges,
  );
  assert.deepEqual(rows, []);
});
