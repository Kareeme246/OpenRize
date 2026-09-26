/**
 * Mirror of the activity types in src-tauri/src/activity.rs. The Rust structs
 * carry #[serde(rename_all = "camelCase")], so the JSON keys arriving over IPC
 * match these field names exactly.
 */

export type SessionKind = "activity" | "focus" | "break";

export interface ActivitySegment {
  id: number;
  app: string;
  title: string;
  kind: SessionKind;
  label: string | null;
  /** Epoch milliseconds. */
  startedAt: number;
  /** Null while the segment is still running. */
  endedAt: number | null;
  reviewed: boolean;
}

export interface ActivitySnapshot {
  current: ActivitySegment | null;
  segments: ActivitySegment[];
  /** Activity + Focus. Breaks are counted separately. */
  trackedMs: number;
  focusMs: number;
  breakMs: number;
  unreviewed: number;
  /**
   * How long the user has been idle right now: since their last input, or
   * since the app in front last kept the display awake to play video.
   */
  idleMs: number;
  idleThresholdMs: number;
  captureEnabled: boolean;
  inTrackingHours?: boolean;
  trackingActive?: boolean;
}

/**
 * The lightweight push (1Hz focused / 30s heartbeat backgrounded) — same
 * numbers as `ActivitySnapshot`, minus `segments`. A structural change
 * always arrives as a full `ActivitySnapshot` instead, so the segment list
 * only ever comes from that path.
 */
export type ActivityTick = Omit<ActivitySnapshot, "segments">;

export function segmentDuration(segment: ActivitySegment, now: number): number {
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

/** `14:05` — a clock time, distinct from the durations in lib/timers.ts. */
export function formatClock(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export const KIND_STYLES: Record<
  SessionKind,
  { dot: string; text: string; label: string }
> = {
  activity: { dot: "bg-fg-faint", text: "text-fg-muted", label: "Activity" },
  focus: { dot: "bg-accent", text: "text-accent", label: "Focus" },
  break: { dot: "bg-break", text: "text-break", label: "Break" },
};
