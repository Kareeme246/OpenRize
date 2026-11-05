/**
 * Mirror of `Timer` in src-tauri/src/timers.rs.
 * The Rust struct carries #[serde(rename_all = "camelCase")], so the JSON keys
 * arriving over IPC match these field names exactly.
 */
/**
 * A stretch of a timer's time, defined as a time entry is: a start and an end
 * in epoch milliseconds. `durationMs` is `endedAt - startedAt`, derived by
 * Rust, never edited.
 */
export interface TimerSegment {
  id: number;
  startedAt: number;
  endedAt: number;
  durationMs: number;
}

/** The boundaries of a segment to change; one left out stays as it is. */
export interface SegmentEdit {
  startedAt?: number;
  endedAt?: number;
}

/** A snapshot taken on demand; `savedAt` is epoch milliseconds. */
export interface TimerSavedState {
  id: number;
  timerId: string;
  /** The timer's label when it was saved. */
  label: string;
  elapsedMs: number;
  savedAt: number;
}

export interface Timer {
  id: string;
  label: string;
  /** Sum of `segments`. */
  accumulatedMs: number;
  segments: TimerSegment[];
  startedAt: number | null;
  createdAt: number;
  ordinal: number;
  /**
   * The tag this timer carries. Timers sharing a tag are linked: Rust starts
   * or pauses them all when any one of them starts or pauses.
   */
  tag: string | null;
  /** The section it sits under, or null for the trackers above every section. */
  sectionId: string | null;
}

/** Mirror of `TimerSection`: a subheading that only arranges the page. */
export interface TimerSection {
  id: string;
  name: string;
  ordinal: number;
  createdAt: number;
}

/** The timers that share one tag, read as a set. */
export interface TimerTag {
  name: string;
  timers: number;
  running: number;
  totalMs: number;
}

/**
 * The tags among `timers`, in the order their first timer appears. Names are
 * compared without regard to case, as Rust does when it links them.
 */
export function timerTags(timers: Timer[], now: number): TimerTag[] {
  const tags = new Map<string, TimerTag>();
  for (const timer of timers) {
    if (timer.tag === null) continue;
    const key = timer.tag.toLowerCase();
    const tag = tags.get(key) ?? {
      name: timer.tag,
      timers: 0,
      running: 0,
      totalMs: 0,
    };
    tag.timers += 1;
    if (timer.startedAt !== null) tag.running += 1;
    tag.totalMs += elapsedMs(timer, now);
    tags.set(key, tag);
  }
  return [...tags.values()];
}

/**
 * The page's lists in reading order: the trackers in no section first, then
 * each section's, every list in the timers' own order.
 */
export function timerLists(
  timers: Timer[],
  sections: TimerSection[],
): { section: TimerSection | null; timers: Timer[] }[] {
  const known = new Set(sections.map((section) => section.id));
  return [
    {
      section: null,
      timers: timers.filter(
        (timer) => timer.sectionId === null || !known.has(timer.sectionId),
      ),
    },
    ...sections.map((section) => ({
      section,
      timers: timers.filter((timer) => timer.sectionId === section.id),
    })),
  ];
}

/**
 * The frontend needs elapsed time to draw a number; Rust owns the timestamps
 * that make it correct. The run in progress is `now - startedAt`, the same
 * span `Timer::run_span_at` hands the pause and the save in Rust, which is
 * why `startedAt` must come from the wall clock `now` is read from. Keep the
 * two consistent if this ever gains a second caller.
 */
export function elapsedMs(timer: Timer, now: number): number {
  if (timer.startedAt === null) return timer.accumulatedMs;
  return timer.accumulatedMs + Math.max(0, now - timer.startedAt);
}

/** `1:02:03` above an hour, `02:03` below it — never a width-jumping string. */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const pad = (value: number): string => String(value).padStart(2, "0");
  return hours > 0
    ? `${hours}:${pad(minutes)}:${pad(seconds)}`
    : `${pad(minutes)}:${pad(seconds)}`;
}
