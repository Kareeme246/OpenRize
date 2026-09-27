/**
 * Productivity metrics for the Calendar panel's metrics tab. Pure functions
 * over what the page already loaded, so switching tabs costs no query.
 */

import { durationOf } from "../../lib/entries";
import { formatDuration } from "../../lib/format";
import type {
  ActivitySegment,
  CalendarScale,
  RollupCell,
  TimeEntry,
} from "../../lib/types";

export interface Metric {
  label: string;
  value: string;
  sub?: string;
}

/** App changes between consecutive non-break segments. */
export function appSwitches(segments: ActivitySegment[]): number {
  let switches = 0;
  let previous: string | undefined;
  for (const segment of segments) {
    if (segment.kind === "break") continue;
    if (previous !== undefined && segment.app !== previous) switches++;
    previous = segment.app;
  }
  return switches;
}

/** Days with any tracked time, from per-day rollup cells. */
function daysTracked(cells: RollupCell[], firstDay: number, days: number) {
  const tracked = new Set<number>();
  for (const cell of cells) {
    if (cell.bucket < firstDay || cell.bucket >= firstDay + days) continue;
    if (cell.ms > 0) tracked.add(cell.bucket);
  }
  return tracked.size;
}

/** Days with any tracked time, from entries (by local start day). */
function entryDays(entries: TimeEntry[]): number {
  return new Set(
    entries.map((entry) => new Date(entry.startedAt).toDateString()),
  ).size;
}

function average(totalMs: number, count: number): string {
  return count > 0 ? formatDuration(totalMs / count) : "–";
}

/**
 * Four headline numbers per scale. A day adds app switches (how scattered
 * the day was); a week or month adds how many days had any time, and the
 * average across those days.
 */
export function rangeMetrics({
  scale,
  entries,
  segments,
  cells,
  firstDay,
  days,
  workMs,
  now,
}: {
  scale: CalendarScale;
  entries: TimeEntry[];
  segments: ActivitySegment[];
  /** Month only: per-day rollup cells, and where the month starts in them. */
  cells: RollupCell[];
  firstDay: number;
  days: number;
  workMs: number;
  now: number;
}): Metric[] {
  if (scale === "month") {
    let totalMs = 0;
    let count = 0;
    for (const cell of cells) {
      if (cell.bucket < firstDay || cell.bucket >= firstDay + days) continue;
      totalMs += cell.ms;
      count += cell.entries;
    }
    const tracked = daysTracked(cells, firstDay, days);
    return [
      { label: "Sessions", value: String(count) },
      { label: "Avg session", value: average(totalMs, count) },
      { label: "Days tracked", value: String(tracked), sub: `of ${days}` },
      { label: "Daily average", value: average(workMs, tracked), sub: "work" },
    ];
  }

  const durations = entries.map((entry) => durationOf(entry, now));
  const totalMs = durations.reduce((sum, ms) => sum + ms, 0);
  const longest = durations.length > 0 ? Math.max(...durations) : 0;
  const sessions = { label: "Sessions", value: String(entries.length) };
  const longestSession = {
    label: "Longest session",
    value: longest > 0 ? formatDuration(longest) : "–",
  };

  if (scale === "day") {
    const switches = appSwitches(segments);
    const hours = totalMs / 3_600_000;
    return [
      sessions,
      { label: "Avg session", value: average(totalMs, entries.length) },
      longestSession,
      {
        label: "App switches",
        value: String(switches),
        sub:
          hours >= 1 ? `${Math.round(switches / hours)} per hour` : undefined,
      },
    ];
  }

  const tracked = entryDays(entries);
  return [
    sessions,
    longestSession,
    { label: "Days tracked", value: String(tracked), sub: `of ${days}` },
    { label: "Daily average", value: average(workMs, tracked), sub: "work" },
  ];
}
