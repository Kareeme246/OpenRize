/**
 * Timeline geometry shared by Day and Week. Day places time by elapsed hours
 * since 5 AM (a DST day is 23 or 25 hours); Week shares one gutter across
 * columns, so it uses wall-clock hours.
 */

export type Placement = "elapsed" | "wall";

const HOUR_MS = 3_600_000;
const DEFAULT_START = 8;
const DEFAULT_END = 20;

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
  hours: number[];
}

export function timelineFor(
  spans: { start: number; end: number; dayStart: number }[],
  placement: Placement,
  hourHeight: number,
  dayHours = 24,
  fullDay = false,
): Timeline {
  let startHour = fullDay ? 0 : DEFAULT_START;
  let endHour = fullDay ? dayHours : DEFAULT_END;
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

export function hourLabel(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "numeric" });
}

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

export function dayLength(dayStart: number): number {
  const next = new Date(dayStart);
  next.setDate(next.getDate() + 1);
  return Math.round((next.getTime() - dayStart) / HOUR_MS);
}

export function timeAtOffset(
  timeline: Timeline,
  y: number,
  dayStart: number,
  placement: Placement,
): number {
  const hours = timeline.startHour + y / timeline.hourHeight;
  if (placement === "elapsed") return dayStart + hours * HOUR_MS;
  const midnight = new Date(dayStart);
  midnight.setHours(0, 0, 0, 0);
  return midnight.getTime() + hours * HOUR_MS;
}
