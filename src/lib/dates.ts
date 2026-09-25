/**
 * Local-calendar arithmetic. The day boundary is a browser concern, not
 * Rust's: every range and bucket edge is computed here with `Date` setters,
 * so a DST day is 23 or 25 hours long instead of a fixed 86 400 000 ms.
 */

import type { CalendarScale } from "./types";

/** Local `YYYY-MM-DD` (toISOString would give the UTC date). */
export function localDateString(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

/** Local midnight of a `YYYY-MM-DD`, or of today when absent or invalid. */
export function parseLocalDate(value?: string): Date {
  if (value && /^\d{4}-\d{2}-\d{2}$/.test(value)) {
    const [year, month, day] = value.split("-").map(Number);
    const date = new Date(year, month - 1, day);
    if (!Number.isNaN(date.getTime())) return date;
  }
  return startOfDay(new Date());
}

export function startOfDay(date: Date): Date {
  const copy = new Date(date);
  copy.setHours(0, 0, 0, 0);
  return copy;
}

export function addDays(date: Date, days: number): Date {
  const copy = new Date(date);
  copy.setDate(copy.getDate() + days);
  return copy;
}

export function addMonths(date: Date, months: number): Date {
  return new Date(date.getFullYear(), date.getMonth() + months, 1);
}

/** Weeks start on Monday, as ISO weeks do. */
export function startOfWeek(date: Date): Date {
  const day = startOfDay(date);
  const offset = (day.getDay() + 6) % 7;
  return addDays(day, -offset);
}

export function startOfMonth(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), 1);
}

export function isSameDay(left: Date, right: Date): boolean {
  return localDateString(left) === localDateString(right);
}

/** ISO 8601 week number (the one containing the week's Thursday). */
export function isoWeek(date: Date): number {
  const thursday = addDays(startOfWeek(date), 3);
  const firstThursday = addDays(
    startOfWeek(new Date(thursday.getFullYear(), 0, 4)),
    3,
  );
  return (
    1 +
    Math.round(
      (thursday.getTime() - firstThursday.getTime()) / (7 * 86_400_000),
    )
  );
}

export interface DateRange {
  start: Date;
  /** Exclusive. */
  end: Date;
}

/** The day, Monday-to-Sunday week, or month containing `date`. */
export function rangeFor(scale: CalendarScale, date: Date): DateRange {
  if (scale === "day") {
    const start = startOfDay(date);
    return { start, end: addDays(start, 1) };
  }
  if (scale === "week") {
    const start = startOfWeek(date);
    return { start, end: addDays(start, 7) };
  }
  const start = startOfMonth(date);
  return { start, end: addMonths(start, 1) };
}

/** Calendar day begins at 5 AM local, including across DST transitions. */
export function calendarDay(date: Date): Date {
  const start = startOfDay(date);
  start.setHours(5);
  return start;
}

/** Most recent calendar day for the current clock time. */
export function currentCalendarDay(now: Date): Date {
  const start = calendarDay(now);
  return now < start ? addDays(start, -1) : start;
}

export function calendarRange(scale: CalendarScale, date: Date): DateRange {
  const range = rangeFor(scale, date);
  return { start: calendarDay(range.start), end: calendarDay(range.end) };
}

export function calendarDayEdges(start: Date, end: Date): number[] {
  const edges: number[] = [];
  for (let day = calendarDay(start); day <= end; day = addDays(day, 1)) {
    edges.push(day.getTime());
  }
  return edges;
}

/** Moves by one unit of the scale. */
export function stepDate(
  scale: CalendarScale,
  date: Date,
  direction: 1 | -1,
): Date {
  if (scale === "day") return addDays(date, direction);
  if (scale === "week") return addDays(date, 7 * direction);
  return addMonths(date, direction);
}

/** Every local midnight from `start` up to and including `end`. */
export function dayEdges(start: Date, end: Date): number[] {
  const edges: number[] = [];
  for (let day = startOfDay(start); day <= end; day = addDays(day, 1)) {
    edges.push(day.getTime());
  }
  return edges;
}

export function daysIn(range: DateRange): Date[] {
  const days: Date[] = [];
  for (let day = range.start; day < range.end; day = addDays(day, 1)) {
    days.push(day);
  }
  return days;
}

/** `Wed, Sep 23`, `Week 39 · Sep 21–27`, or `September 2026`. */
export function rangeLabel(scale: CalendarScale, date: Date): string {
  if (scale === "day") {
    return date.toLocaleDateString(undefined, {
      weekday: "short",
      month: "short",
      day: "numeric",
    });
  }
  if (scale === "week") {
    const { start, end } = rangeFor("week", date);
    const last = addDays(end, -1);
    const first = start.toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
    });
    const tail =
      last.getMonth() === start.getMonth()
        ? String(last.getDate())
        : last.toLocaleDateString(undefined, {
            month: "short",
            day: "numeric",
          });
    return `Week ${isoWeek(date)} · ${first}–${tail}`;
  }
  return date.toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

/**
 * Bucket edges for a pivot: each day, Monday week, or month overlapping the
 * range, clipped to it (n + 1 edges for n buckets).
 */
export function stackEdges(
  range: DateRange,
  stack: "day" | "week" | "month",
): number[] {
  const edges = [range.start.getTime()];
  let cursor =
    stack === "day"
      ? addDays(range.start, 1)
      : stack === "week"
        ? addDays(startOfWeek(range.start), 7)
        : addMonths(startOfMonth(range.start), 1);
  while (cursor < range.end) {
    edges.push(cursor.getTime());
    cursor =
      stack === "day"
        ? addDays(cursor, 1)
        : stack === "week"
          ? addDays(cursor, 7)
          : addMonths(cursor, 1);
  }
  edges.push(range.end.getTime());
  return edges;
}

/** `Mon 21`, `Sep 21`, or `Sep` for a bucket starting at `ms`. */
export function bucketLabel(
  ms: number,
  stack: "day" | "week" | "month",
): string {
  const date = new Date(ms);
  if (stack === "day") {
    return date.toLocaleDateString(undefined, {
      weekday: "short",
      day: "numeric",
    });
  }
  if (stack === "week") {
    return date.toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
    });
  }
  return date.toLocaleDateString(undefined, { month: "short" });
}
