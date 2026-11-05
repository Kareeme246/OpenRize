/**
 * A stretch of a timer's time as an entry defines it; `durationMs` is derived
 * by Rust, never edited.
 */
export interface TimerSegment {
  id: number;
  startedAt: number;
  endedAt: number;
  durationMs: number;
}

export interface SegmentEdit {
  startedAt?: number;
  endedAt?: number;
}

/** A snapshot taken on demand; `savedAt` is epoch milliseconds. */
export interface TimerSavedState {
  id: number;
  timerId: string;
  label: string;
  elapsedMs: number;
  savedAt: number;
}

export interface Timer {
  id: string;
  label: string;
  accumulatedMs: number;
  segments: TimerSegment[];
  startedAt: number | null;
  createdAt: number;
  ordinal: number;
  /**
   * The tag this timer carries; timers sharing one start and pause together
   * (Rust links them).
   */
  tag: string | null;
  /** The section it sits under, or null for the trackers above every section. */
  sectionId: string | null;
}

export interface TimerSection {
  id: string;
  name: string;
  ordinal: number;
  createdAt: number;
}

export interface TimerTag {
  name: string;
  timers: number;
  running: number;
  totalMs: number;
}

/**
 * The tags among `timers` in first-appearance order, compared
 * case-insensitively as Rust does.
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

/** Lists in reading order: unsectioned trackers first, then each section's. */
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
 * Rust owns the timestamps; the run in progress is `now - startedAt`, as
 * `Timer::run_span_at`, so `startedAt` must come from the wall clock `now` is
 * read from.
 */
export function elapsedMs(timer: Timer, now: number): number {
  if (timer.startedAt === null) return timer.accumulatedMs;
  return timer.accumulatedMs + Math.max(0, now - timer.startedAt);
}

/** `1:02:03` above an hour, `02:03` below, never a width-jumping string. */
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
