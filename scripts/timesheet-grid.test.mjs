import assert from "node:assert/strict";
import { test } from "node:test";
import {
  buildSheet,
  decimalHours,
  filterEntries,
  hasTimesheetFilters,
  NONE,
  summarize,
} from "../src/lib/timesheetGrid.ts";

const HOUR = 3_600_000;
const entry = (id, fields) => ({
  id,
  startedAt: 0,
  endedAt: HOUR,
  status: "pending",
  billable: false,
  ...fields,
});
const clients = { a: "acme", b: "acme", c: "globex" };
const clientOf = (projectId) => clients[projectId];
const pending = (e) => e.status === "pending";
const cells = (list) => list.map((cell) => cell.ms);

test("time splits at local edges, and a midnight-spanning entry counts once in totals", () => {
  const edges = [0, HOUR, 1.5 * HOUR];
  const sheet = buildSheet(
    [
      entry("span", { projectId: "a", startedAt: 0.5 * HOUR, endedAt: 1.25 * HOUR }),
      entry("done", { projectId: "a", endedAt: 0.25 * HOUR, status: "approved" }),
      entry("previous-week", { projectId: "a", startedAt: -HOUR, endedAt: 0 }),
    ],
    edges,
    "project",
    pending,
    clientOf,
  );
  const [row] = sheet.groups[0].rows;
  assert.deepEqual(cells(row.days), [0.75 * HOUR, 0.25 * HOUR]);
  assert.deepEqual(
    row.days.map((cell) => cell.review),
    [1, 1],
  );
  assert.deepEqual(row.total, { ms: HOUR, reviewMs: 0.75 * HOUR, review: 1 });
  assert.deepEqual(
    row.entries.map((e) => e.id),
    ["done", "span"],
  );
});

test("project rows group under their client, largest first, with none last", () => {
  const sheet = buildSheet(
    [
      entry("1", { projectId: "a", categoryId: "code" }),
      entry("2", { projectId: "b", endedAt: 2 * HOUR, categoryId: "code" }),
      entry("3", { projectId: "b", categoryId: "meet" }),
      entry("4", { projectId: "c", endedAt: 5 * HOUR }),
      entry("5", { categoryId: "code", status: "approved" }),
    ],
    [0, 24 * HOUR],
    "project",
    pending,
    clientOf,
  );
  assert.deepEqual(
    sheet.groups.map((group) => [group.key, group.rows.map((row) => row.key)]),
    [
      ["globex", ["c"]],
      ["acme", ["b", "a"]],
      [NONE, [NONE]],
    ],
  );
  const acme = sheet.groups[1];
  assert.deepEqual(acme.total, { ms: 4 * HOUR, reviewMs: 4 * HOUR, review: 3 });
  assert.deepEqual(
    acme.rows[0].children.map((child) => [child.key, child.total.ms]),
    [
      ["code", 2 * HOUR],
      ["meet", HOUR],
    ],
  );
  assert.deepEqual(sheet.total, { ms: 10 * HOUR, reviewMs: 9 * HOUR, review: 4 });
  assert.deepEqual(cells(sheet.days), [10 * HOUR]);
});

test("without clientOf, project rows form one flat list, largest first", () => {
  const sheet = buildSheet(
    [
      entry("1", { projectId: "a" }),
      entry("2", { projectId: "c", endedAt: 3 * HOUR }),
      entry("3", { endedAt: 5 * HOUR }),
      entry("4", { projectId: "b", endedAt: 2 * HOUR }),
    ],
    [0, 24 * HOUR],
    "project",
    pending,
  );
  assert.deepEqual(
    sheet.groups.map((group) => [group.key, group.rows.map((row) => row.key)]),
    [["all", ["c", "b", "a", NONE]]],
  );
  assert.equal(sheet.groups[0].total.ms, 11 * HOUR);
});

test("category rows sit in one group and break down by project", () => {
  const sheet = buildSheet(
    [
      entry("1", { projectId: "a", categoryId: "code" }),
      entry("2", { categoryId: "code" }),
      entry("3", { projectId: "a" }),
    ],
    [0, 24 * HOUR],
    "category",
    pending,
    clientOf,
  );
  assert.equal(sheet.groups.length, 1);
  assert.equal(sheet.groups[0].key, "all");
  assert.deepEqual(
    sheet.groups[0].rows.map((row) => [
      row.key,
      row.children.map((child) => child.key),
    ]),
    [
      ["code", ["a", NONE]],
      [NONE, ["a"]],
    ],
  );
});

test("blank or out-of-range weeks produce an empty sheet", () => {
  const edges = [24 * HOUR, 48 * HOUR];
  for (const entries of [[], [entry("early", { projectId: "a" })]]) {
    const sheet = buildSheet(entries, edges, "project", pending, clientOf);
    assert.deepEqual(sheet.groups, []);
    assert.deepEqual(sheet.total, { ms: 0, reviewMs: 0, review: 0 });
  }
});

test("filters narrow by client, project, and category, including none", () => {
  const entries = [
    entry("1", { projectId: "a", categoryId: "code" }),
    entry("2", { projectId: "c", categoryId: "meet" }),
    entry("3", { categoryId: "code" }),
    entry("4", { projectId: "b" }),
  ];
  const ids = (filters) =>
    filterEntries(entries, filters, clientOf).map((e) => e.id);
  assert.deepEqual(ids({}), ["1", "2", "3", "4"]);
  assert.deepEqual(ids({ clientIds: ["acme"] }), ["1", "4"]);
  assert.deepEqual(ids({ clientIds: [NONE] }), ["3"]);
  assert.deepEqual(ids({ projectIds: ["c", NONE] }), ["2", "3"]);
  assert.deepEqual(ids({ categoryIds: [NONE] }), ["4"]);
  assert.deepEqual(ids({ clientIds: ["acme"], categoryIds: ["code"] }), ["1"]);
  assert.equal(hasTimesheetFilters({ projectIds: [] }), false);
  assert.equal(hasTimesheetFilters({ categoryIds: ["code"] }), true);
});

test("decimal hours read like Rise's cells", () => {
  assert.equal(decimalHours(0), "");
  assert.equal(decimalHours(60_000), "<0.1");
  assert.equal(decimalHours(4 * HOUR), "4");
  assert.equal(decimalHours(4.6 * HOUR), "4.6");
  assert.equal(decimalHours(13.97 * HOUR), "14");
});

test("summary tallies clipped time per review state, once per entry", () => {
  const summary = summarize(
    [
      entry("1", { startedAt: -HOUR, endedAt: HOUR }),
      entry("2", { status: "approved", endedAt: 2 * HOUR }),
      entry("3", { status: "processing" }),
      entry("4", { startedAt: 30 * HOUR, endedAt: 31 * HOUR }),
    ],
    0,
    24 * HOUR,
    pending,
  );
  assert.deepEqual(summary, {
    review: { ms: HOUR, entries: 1 },
    approved: { ms: 2 * HOUR, entries: 1 },
    total: { ms: 4 * HOUR, entries: 3 },
  });
});
