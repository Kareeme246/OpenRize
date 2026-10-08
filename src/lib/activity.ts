import type { ActivitySegment } from "./types";

function segmentDuration(segment: ActivitySegment, now: number): number {
  return Math.max(0, (segment.endedAt ?? now) - segment.startedAt);
}

/** Local midnight — the day boundary is a browser concern, not Rust's. */
export function startOfToday(now: number = Date.now()): number {
  const date = new Date(now);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

/** App name -> total milliseconds, longest first. Breaks are excluded. */
export function timeByApp(
  segments: ActivitySegment[],
  now: number,
): { app: string; ms: number }[] {
  const totals = new Map<string, number>();
  for (const segment of segments) {
    if (segment.kind === "break") continue;
    totals.set(
      segment.app,
      (totals.get(segment.app) ?? 0) + segmentDuration(segment, now),
    );
  }
  return [...totals.entries()]
    .map(([app, ms]) => ({ app, ms }))
    .sort((left, right) => right.ms - left.ms);
}
