import type { TimeEntry, TimesheetFilters } from "./types";

/** Local-midnight edges (including DST changes) partition overlapping entries. */
function overlap(entry: TimeEntry, start: number, end: number): number {
  return Math.max(
    0,
    Math.min(entry.endedAt, end) - Math.max(entry.startedAt, start),
  );
}

/** What the Analyze > Timesheets matrix puts on its row axis. */
export type SheetRows = "project" | "category";

/** The filter value for "No client", "No project", and "Uncategorized". */
export const NONE = "none";

export function hasTimesheetFilters(filters: TimesheetFilters): boolean {
  return Boolean(
    filters.clientIds?.length ||
      filters.projectIds?.length ||
      filters.categoryIds?.length,
  );
}

function matches(list: string[] | undefined, id: string | undefined): boolean {
  return !list?.length || list.includes(id ?? NONE);
}

/** Entries every active filter admits; a project's client comes from `clientOf`. */
export function filterEntries(
  entries: TimeEntry[],
  filters: TimesheetFilters,
  clientOf: (projectId: string) => string | undefined,
): TimeEntry[] {
  if (!hasTimesheetFilters(filters)) return entries;
  return entries.filter(
    (entry) =>
      matches(filters.projectIds, entry.projectId) &&
      matches(filters.categoryIds, entry.categoryId) &&
      matches(
        filters.clientIds,
        entry.projectId ? clientOf(entry.projectId) : undefined,
      ),
  );
}

/** One cell: its time, and how much of it still waits on review. */
export interface SheetCell {
  ms: number;
  reviewMs: number;
  /** Entries overlapping the cell that wait on review. */
  review: number;
}

export interface SheetRow {
  /** Project or category id; `NONE` for "No project" / "Uncategorized". */
  key: string;
  days: SheetCell[];
  total: SheetCell;
  /** Entries in the row, oldest first. */
  entries: TimeEntry[];
  /** The row broken down by the other dimension (its expanded view). */
  children: SheetRow[];
}

export interface SheetGroup {
  /** Client id or `NONE`; category rows sit in one `all` group. */
  key: string;
  rows: SheetRow[];
  days: SheetCell[];
  total: SheetCell;
}

export interface Sheet {
  groups: SheetGroup[];
  days: SheetCell[];
  total: SheetCell;
}

interface Builder {
  key: string;
  days: SheetCell[];
  reviewIds: Set<string>;
  ms: number;
  reviewMs: number;
  entries: TimeEntry[];
}

const emptyCell = (): SheetCell => ({ ms: 0, reviewMs: 0, review: 0 });

function builder(key: string, width: number): Builder {
  return {
    key,
    days: Array.from({ length: width }, emptyCell),
    reviewIds: new Set(),
    ms: 0,
    reviewMs: 0,
    entries: [],
  };
}

/** Adds the entry's time to each day it overlaps. */
function add(
  target: Builder,
  entry: TimeEntry,
  edges: number[],
  review: boolean,
): void {
  target.entries.push(entry);
  if (review) target.reviewIds.add(entry.id);
  for (let day = 0; day < edges.length - 1; day++) {
    const ms = overlap(entry, edges[day], edges[day + 1]);
    if (ms === 0) continue;
    const cell = target.days[day];
    cell.ms += ms;
    target.ms += ms;
    if (review) {
      cell.reviewMs += ms;
      cell.review += 1;
      target.reviewMs += ms;
    }
  }
}

const totalOf = (target: Builder): SheetCell => ({
  ms: target.ms,
  reviewMs: target.reviewMs,
  review: target.reviewIds.size,
});

/** Largest first; the "none" bucket always last; ids break ties. */
function byTotal(a: Builder, b: Builder): number {
  if ((a.key === NONE) !== (b.key === NONE)) return a.key === NONE ? 1 : -1;
  return b.ms - a.ms || a.key.localeCompare(b.key);
}

/**
 * The timesheet matrix: rows of projects or of categories, one cell per
 * bucket between `edges`, with row, group, and column totals. With
 * `clientOf`, project rows group under their client; without it every row
 * sits in one `all` group. Every entry lands in exactly one row, so the footer matches
 * the tracked time; entries on no project or category get a `NONE` row.
 * An entry spanning midnight counts in each day's review count but once in
 * a total's.
 */
export function buildSheet(
  entries: TimeEntry[],
  edges: number[],
  rows: SheetRows,
  needsReview: (entry: TimeEntry) => boolean,
  clientOf?: (projectId: string) => string | undefined,
): Sheet {
  const width = edges.length - 1;
  const groups = new Map<
    string,
    {
      group: Builder;
      rows: Map<string, Builder & { children: Map<string, Builder> }>;
    }
  >();
  const sheet = builder("sheet", width);
  for (const entry of entries) {
    if (
      entry.endedAt <= entry.startedAt ||
      entry.endedAt <= edges[0] ||
      entry.startedAt >= edges[width]
    )
      continue;
    const rowKey =
      (rows === "project" ? entry.projectId : entry.categoryId) ?? NONE;
    const childKey =
      (rows === "project" ? entry.categoryId : entry.projectId) ?? NONE;
    const groupKey =
      rows === "project" && clientOf
        ? ((entry.projectId && clientOf(entry.projectId)) ?? NONE)
        : "all";
    let group = groups.get(groupKey);
    if (!group) {
      group = { group: builder(groupKey, width), rows: new Map() };
      groups.set(groupKey, group);
    }
    let row = group.rows.get(rowKey);
    if (!row) {
      row = { ...builder(rowKey, width), children: new Map() };
      group.rows.set(rowKey, row);
    }
    let child = row.children.get(childKey);
    if (!child) {
      child = builder(childKey, width);
      row.children.set(childKey, child);
    }
    const review = needsReview(entry);
    for (const target of [sheet, group.group, row, child]) {
      add(target, entry, edges, review);
    }
  }
  const finish = (target: Builder, children: Builder[] = []): SheetRow => ({
    key: target.key,
    days: target.days,
    total: totalOf(target),
    entries: [...target.entries].sort((a, b) => a.startedAt - b.startedAt),
    children: children.sort(byTotal).map((child) => finish(child)),
  });
  return {
    groups: [...groups.values()]
      .sort((a, b) => byTotal(a.group, b.group))
      .map(({ group, rows: groupRows }) => ({
        key: group.key,
        days: group.days,
        total: totalOf(group),
        rows: [...groupRows.values()]
          .sort(byTotal)
          .map((row) => finish(row, [...row.children.values()])),
      })),
    days: sheet.days,
    total: totalOf(sheet),
  };
}

/** `4.6`, `14`, or `<0.1`: Rise's bare decimal hours, blank for none. */
export function decimalHours(ms: number): string {
  if (ms <= 0) return "";
  const hours = ms / 3_600_000;
  if (hours < 0.05) return "<0.1";
  const rounded = Math.round(hours * 10) / 10;
  return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
}

export interface SheetTally {
  ms: number;
  entries: number;
}

/** The Day view's cards: time waiting on review, approved, and in all. */
export interface SheetSummary {
  review: SheetTally;
  approved: SheetTally;
  total: SheetTally;
}

/** Time inside [start, end) per review state, counting each entry once. */
export function summarize(
  entries: TimeEntry[],
  start: number,
  end: number,
  needsReview: (entry: TimeEntry) => boolean,
): SheetSummary {
  const summary: SheetSummary = {
    review: { ms: 0, entries: 0 },
    approved: { ms: 0, entries: 0 },
    total: { ms: 0, entries: 0 },
  };
  for (const entry of entries) {
    const ms = overlap(entry, start, end);
    if (ms === 0) continue;
    const tallies = [summary.total];
    if (entry.status === "approved") tallies.push(summary.approved);
    else if (needsReview(entry)) tallies.push(summary.review);
    for (const tally of tallies) {
      tally.ms += ms;
      tally.entries += 1;
    }
  }
  return summary;
}
