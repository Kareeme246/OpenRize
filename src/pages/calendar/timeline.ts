/**
 * Vertical timeline geometry shared by the Day and Week views.
 *
 * The Day view places time by *elapsed* hours since local midnight and
 * labels its gutter from real timestamps, so a DST day renders as 23 or 25
 * hours (the repeated 1 AM shows twice). The Week view shares one gutter
 * across seven columns, so it places time by wall-clock hour instead.
 */

export type Placement = "elapsed" | "wall";

const HOUR_MS = 3_600_000;
const DEFAULT_START = 8;
const DEFAULT_END = 20;

/** Hours from `dayStart` to `ms`, by the chosen placement. */
export function hourOffset(
  ms: number,
  dayStart: number,
  placement: Placement,
): number {
  if (placement === "elapsed") return (ms - dayStart) / HOUR_MS;
  const date = new Date(ms);
  const midnight = new Date(date);
  midnight.setHours(0, 0, 0, 0);
  // An entry ending at the next midnight ends at hour 24, not hour 0.
  const dayShift = Math.round((midnight.getTime() - dayStart) / (24 * HOUR_MS));
  return (
    dayShift * 24 +
    date.getHours() +
    date.getMinutes() / 60 +
    date.getSeconds() / 3600
  );
}

export interface Timeline {
  startHour: number;
  endHour: number;
  hourHeight: number;
  height: number;
  /** Whole hours from start to end, for gutter lines. */
  hours: number[];
}

/**
 * 8 AM to 8 PM, stretched to fit anything earlier or later, and capped at
 * the length of the day.
 */
export function timelineFor(
  spans: { start: number; end: number; dayStart: number }[],
  placement: Placement,
  hourHeight: number,
  dayHours = 24,
): Timeline {
  let startHour = DEFAULT_START;
  let endHour = DEFAULT_END;
  for (const span of spans) {
    const from = hourOffset(span.start, span.dayStart, placement);
    const to = hourOffset(span.end, span.dayStart, placement);
    startHour = Math.min(startHour, Math.floor(Math.max(0, from)));
    endHour = Math.max(endHour, Math.ceil(Math.min(dayHours, to)));
  }
  const hours: number[] = [];
  for (let hour = startHour; hour <= endHour; hour += 1) hours.push(hour);
  return {
    startHour,
    endHour,
    hourHeight,
    height: (endHour - startHour) * hourHeight,
    hours,
  };
}

/** Top and height in px for a span, clipped to the timeline. */
export function place(
  timeline: Timeline,
  start: number,
  end: number,
  dayStart: number,
  placement: Placement,
  minHeight = 4,
): { top: number; height: number } {
  const from = Math.max(
    timeline.startHour,
    hourOffset(start, dayStart, placement),
  );
  const to = Math.min(timeline.endHour, hourOffset(end, dayStart, placement));
  const top = (from - timeline.startHour) * timeline.hourHeight;
  // Less a hairline so neighbours don't touch.
  const height = Math.max(minHeight, (to - from) * timeline.hourHeight - 2);
  return { top, height };
}

/** `9 AM`, from a real timestamp. */
export function hourLabel(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "numeric" });
}

/** The gutter label for an hour line. */
export function gutterLabel(
  hour: number,
  dayStart: number,
  placement: Placement,
): string {
  if (placement === "elapsed") return hourLabel(dayStart + hour * HOUR_MS);
  const date = new Date(dayStart);
  date.setHours(hour, 0, 0, 0);
  return hourLabel(date.getTime());
}

/** Hours in a local day: 24, or 23/25 across a DST change. */
export function dayLength(dayStart: number): number {
  const next = new Date(dayStart);
  next.setDate(next.getDate() + 1);
  return Math.round((next.getTime() - dayStart) / HOUR_MS);
}
