import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type Slice, StackedBar } from "./components/Charts";
import { useCatalog } from "./hooks/useCatalog";
import { SettingsProvider, useSettings } from "./hooks/useSettings";
import { useTauriEvent } from "./hooks/useTauriEvent";
import { useTimers } from "./hooks/useTimers";
import {
  type ActivitySnapshot,
  type ActivityTick,
  startOfToday,
} from "./lib/activity";
import * as api from "./lib/api";
import { describeError } from "./lib/api";
import { addDays, currentCalendarDay } from "./lib/dates";
import {
  countsAsWork,
  durationOf,
  isReviewable,
  recordingEntry,
} from "./lib/entries";
import { formatDuration, formatTime } from "./lib/format";
import {
  dailyTargetMs,
  formatTargetHours,
  nextTrackingStart,
} from "./lib/settings";
import {
  elapsedMs,
  formatDuration as formatStopwatch,
  type Timer,
} from "./lib/timers";
import type { Category, Project, TimeEntry } from "./lib/types";

const DAY_MS = 86_400_000;
const MAX_TIMERS = 3;
const MAX_LEGEND = 4;

/**
 * The menu-bar Pulse panel (design board option A): is capture on, and how
 * is today going. Live state only - anything historical, list-shaped, or
 * editable stays in the main window, and the review count is a door to it.
 */
export default function Pulse() {
  return (
    <SettingsProvider>
      <PulsePanel />
    </SettingsProvider>
  );
}

function PulsePanel() {
  const { settings } = useSettings();
  const catalog = useCatalog();
  const { categoryById, projectById } = catalog;
  const timers = useTimers();
  const { now } = timers;

  // Only the live fields are read; a full snapshot is a tick plus segments.
  const [live, setLive] = useState<ActivityTick | null>(null);
  const [entries, setEntries] = useState<TimeEntry[]>([]);
  const [entriesLoaded, setEntriesLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The Calendar's day (it turns at 5 AM). Changing it reloads, so the
  // panel never shows yesterday.
  const day = currentCalendarDay(new Date(now));
  const dayStart = day.getTime();
  const loadEntries = useCallback(async (): Promise<void> => {
    try {
      const start = new Date(dayStart);
      setEntries(
        await api.listTimeEntries(dayStart, addDays(start, 1).getTime() - 1),
      );
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setEntriesLoaded(true);
    }
  }, [dayStart]);

  const loadLive = useCallback(async (): Promise<void> => {
    try {
      setLive(await api.fetchActivitySnapshot(startOfToday()));
    } catch (cause) {
      setError(describeError(cause));
    }
  }, []);

  useEffect(() => {
    void loadEntries();
  }, [loadEntries]);
  useEffect(() => {
    void loadLive();
  }, [loadLive]);

  useTauriEvent<ActivityTick>(api.ACTIVITY_TICK, setLive);
  useTauriEvent<ActivitySnapshot>(api.ACTIVITY_CHANGED, (snapshot) => {
    setLive(snapshot);
    void loadEntries();
  });
  useTauriEvent(api.ENTRIES_CHANGED, () => void loadEntries());
  useTauriEvent(api.SUGGESTION_READY, () => void loadEntries());

  // The panel stays loaded between opens; catch up on whatever changed while
  // it was hidden (a renamed category, a new project).
  const { reload } = catalog;
  useEffect(() => {
    const onFocus = (): void => {
      void reload();
      void loadEntries();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload, loadEntries]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") {
        event.preventDefault();
        void api.hidePulsePanel();
      } else if (event.metaKey && event.key.toLowerCase() === "o") {
        event.preventDefault();
        void api.openMainWindow(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // The window is sized to the card, so it grows and shrinks with content.
  // The first open waits for this first report, so hold it until the data
  // is in: the panel must not appear short and then grow. A failed read
  // counts as loaded, or an error would keep the panel from ever opening.
  const card = useRef<HTMLDivElement>(null);
  const ready =
    (live !== null || error !== null) && entriesLoaded && !timers.loading;
  useEffect(() => {
    const element = card.current;
    if (!element || !ready) return;
    let reported = 0;
    const observer = new ResizeObserver(() => {
      const height = Math.ceil(element.getBoundingClientRect().height);
      if (height === reported) return;
      reported = height;
      void api.resizePulsePanel(height);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [ready]);

  const toReview = useMemo(
    () => entries.filter(isReviewable).length,
    [entries],
  );

  return (
    <div
      ref={card}
      className="flex select-none flex-col overflow-hidden rounded-xl border border-line bg-panel text-[12px] text-fg"
    >
      <section className="px-3.5 py-3">
        <CaptureStatus
          live={live}
          recording={recordingEntry(entries, now)}
          categoryById={categoryById}
          projectById={projectById}
        />
      </section>
      <section className="border-line border-t px-3.5 py-3">
        <TodaySummary
          entries={entries}
          categoryById={categoryById}
          targetMs={dailyTargetMs(settings)}
          now={now}
        />
      </section>
      {timers.timers.length > 0 && (
        <section className="border-line border-t px-3.5 pt-2.5 pb-2">
          <TimerRows
            timers={timers.timers}
            now={now}
            onStart={timers.start}
            onPause={timers.pause}
          />
        </section>
      )}
      {error && (
        <p className="border-line border-t px-3.5 py-2 text-[11px] text-danger">
          {error}
        </p>
      )}
      <footer className="flex items-center gap-2 border-line border-t px-3.5 py-2.5">
        {toReview > 0 ? (
          <button
            type="button"
            onClick={() => void api.openMainWindow(true)}
            className="inline-flex items-center gap-1.5 rounded-lg bg-review/15 px-2.5 py-1.5 font-semibold text-[11.5px] text-review transition-colors hover:bg-review/25"
          >
            {toReview} to review →
          </button>
        ) : (
          <span className="px-1 text-[11.5px] text-fg-faint">
            {entries.length > 0 ? "All caught up ✓" : "Nothing to review"}
          </span>
        )}
        <button
          type="button"
          onClick={() => void api.openMainWindow(false)}
          className="ml-auto inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 font-semibold text-[11.5px] text-fg-soft transition-colors hover:bg-surface-strong hover:text-fg"
        >
          Open OpenRize
          <kbd className="font-mono text-[10px] text-fg-faint">⌘O</kbd>
        </button>
      </footer>
    </div>
  );
}

interface StatusView {
  dot: string;
  text: string;
  detail?: string;
  /** What the button does: pause capture, or resume it. */
  action?: "pause" | "resume";
}

/** The sidebar's live capture strip, plus one line of context under it. */
function CaptureStatus({
  live,
  recording,
  categoryById,
  projectById,
}: {
  live: ActivityTick | null;
  recording: TimeEntry | undefined;
  categoryById: Map<string, Category>;
  projectById: Map<string, Project>;
}) {
  const { settings } = useSettings();
  const view = statusView(live, recording, categoryById, projectById, () =>
    nextTrackingStart(settings.trackingHours),
  );
  const toggle = (): void => {
    if (!live) return;
    void api.setCaptureEnabled(!trackingActive(live));
  };

  return (
    <>
      <div className="flex min-h-[38px] items-center gap-2 rounded-lg bg-surface py-1.5 pr-1.5 pl-2.5">
        <span className={`size-2 shrink-0 rounded-full ${view.dot}`} />
        <span className="min-w-0 flex-1 truncate font-medium text-fg-muted">
          {view.text}
        </span>
        {view.action && (
          <button
            type="button"
            onClick={toggle}
            title={
              view.action === "pause" ? "Pause tracking" : "Resume tracking"
            }
            className={`grid size-6 shrink-0 place-items-center rounded-md transition-colors hover:bg-surface-strong ${
              view.action === "resume"
                ? "text-accent"
                : "text-fg-soft hover:text-fg"
            }`}
          >
            {view.action === "pause" ? <PauseIcon /> : <PlayIcon />}
          </button>
        )}
      </div>
      {view.detail && (
        <p className="mx-0.5 mt-1.5 truncate text-[10.5px] text-fg-faint">
          {view.detail}
        </p>
      )}
    </>
  );
}

/** Older payloads may lack the flag; capture alone was the answer then. */
function trackingActive(live: ActivityTick): boolean {
  return live.trackingActive ?? live.captureEnabled;
}

function statusView(
  live: ActivityTick | null,
  recording: TimeEntry | undefined,
  categoryById: Map<string, Category>,
  projectById: Map<string, Project>,
  nextStart: () => Date | null,
): StatusView {
  if (!live) return { dot: "bg-fg-ghost", text: "Checking capture…" };

  if (trackingActive(live)) {
    if (live.idleMs >= live.idleThresholdMs) {
      return {
        dot: "bg-fg-ghost",
        text: `Idle ${formatDuration(live.idleMs)} · not counting`,
        detail: "Counting resumes when you're back",
        action: "pause",
      };
    }
    const app = live.current?.app;
    const name = recording
      ? (projectById.get(recording.projectId ?? "")?.name ??
        categoryById.get(recording.categoryId ?? "")?.name)
      : undefined;
    const detail = recording
      ? `${name ?? recording.description} · since ${formatTime(recording.startedAt)}`
      : live.current?.title || undefined;
    return {
      dot: "bg-accent animate-pulse",
      text: app ? `Tracking · ${app}` : "Tracking active",
      detail,
      action: "pause",
    };
  }

  if (!live.captureEnabled) {
    return {
      dot: "bg-review",
      text: "Tracking paused",
      detail: "Nothing is recorded until you resume",
      action: "resume",
    };
  }

  const next = nextStart();
  return {
    dot: "bg-review",
    text: "Outside tracking hours",
    detail: next ? `Starts ${startsAt(next)}` : undefined,
    action: "resume",
  };
}

/** `at 7:00 AM` today, `tomorrow at 7:00 AM`, else `Mon at 7:00 AM`. */
function startsAt(date: Date): string {
  const days = Math.round(
    (startOfToday(date.getTime()) - startOfToday()) / DAY_MS,
  );
  const time = formatTime(date.getTime());
  if (days <= 0) return `at ${time}`;
  if (days === 1) return `tomorrow at ${time}`;
  return `${date.toLocaleDateString(undefined, { weekday: "short" })} at ${time}`;
}

const RING = 64;
const RING_STROKE = 6;
const RING_RADIUS = (RING - RING_STROKE) / 2;
const RING_LENGTH = 2 * Math.PI * RING_RADIUS;

/** Work hours against the day's target, and where the time went. */
function TodaySummary({
  entries,
  categoryById,
  targetMs,
  now,
}: {
  entries: TimeEntry[];
  categoryById: Map<string, Category>;
  targetMs: number;
  now: number;
}) {
  const { workMs, slices } = useMemo(() => {
    const byCategory = new Map<string | null, number>();
    let work = 0;
    for (const entry of entries) {
      const ms = durationOf(entry, now);
      const key = entry.categoryId ?? null;
      byCategory.set(key, (byCategory.get(key) ?? 0) + ms);
      if (countsAsWork(key, categoryById)) work += ms;
    }
    const sorted: Slice[] = [...byCategory.entries()]
      .map(([id, ms]) => {
        const category = id ? categoryById.get(id) : undefined;
        return {
          key: id ?? "none",
          label: category?.name ?? "Uncategorized",
          color: category?.color ?? "var(--fg-ghost)",
          ms,
        };
      })
      .filter((slice) => slice.ms > 0)
      .sort((a, b) => b.ms - a.ms);
    return { workMs: work, slices: sorted };
  }, [entries, categoryById, now]);

  const progress = targetMs > 0 ? workMs / targetMs : 0;
  const percent = Math.round(progress * 100);
  const left = targetMs - workMs;

  return (
    <>
      <div className="grid grid-cols-[64px_minmax(0,1fr)] items-center gap-3">
        <svg
          viewBox={`0 0 ${RING} ${RING}`}
          className="size-16"
          role="img"
          aria-label={`${percent}% of today's target`}
        >
          <circle
            cx={RING / 2}
            cy={RING / 2}
            r={RING_RADIUS}
            fill="none"
            strokeWidth={RING_STROKE}
            className="stroke-line"
          />
          {/* A round cap draws a dot even for a sliver; "0%" gets no arc. */}
          {percent > 0 && (
            <circle
              cx={RING / 2}
              cy={RING / 2}
              r={RING_RADIUS}
              fill="none"
              strokeWidth={RING_STROKE}
              strokeLinecap="round"
              strokeDasharray={RING_LENGTH}
              strokeDashoffset={RING_LENGTH * (1 - Math.min(1, progress))}
              transform={`rotate(-90 ${RING / 2} ${RING / 2})`}
              className="stroke-accent"
            />
          )}
          <text
            x={RING / 2}
            y={RING / 2 + 4}
            textAnchor="middle"
            className="fill-fg-strong font-bold text-[11px] tabular-nums"
          >
            {percent}%
          </text>
        </svg>
        <div className="min-w-0">
          <div className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
            Today
          </div>
          <div className="font-bold text-[20px] text-fg-strong tabular-nums leading-tight tracking-tight">
            {formatDuration(workMs)}
          </div>
          <div className="truncate text-[11px] text-fg-faint">
            of {formatTargetHours(targetMs)} target ·{" "}
            {left > 0 ? `${formatDuration(left)} to go` : "target reached"}
          </div>
        </div>
      </div>
      {slices.length > 0 ? (
        <>
          <div className="mt-2.5">
            <StackedBar slices={slices} />
          </div>
          <ul className="mt-2 grid grid-cols-2 gap-x-3 gap-y-0.5 text-[10.5px] text-fg-soft">
            {slices.slice(0, MAX_LEGEND).map((slice) => (
              <li key={slice.key} className="flex min-w-0 items-center gap-1.5">
                <i
                  className="size-[7px] shrink-0 rounded-[2px]"
                  style={{ backgroundColor: slice.color }}
                />
                <span className="min-w-0 flex-1 truncate">{slice.label}</span>
                <span className="shrink-0 text-fg-muted tabular-nums">
                  {formatDuration(slice.ms)}
                </span>
              </li>
            ))}
          </ul>
        </>
      ) : (
        <p className="mt-2 text-[11px] text-fg-faint">
          Nothing tracked yet today
        </p>
      )}
    </>
  );
}

/** Manual timers, running first; one click pauses or resumes each. */
function TimerRows({
  timers,
  now,
  onStart,
  onPause,
}: {
  timers: Timer[];
  now: number;
  onStart: (id: string) => void;
  onPause: (id: string) => void;
}) {
  const running = (timer: Timer): boolean => timer.startedAt !== null;
  const ordered = [...timers].sort(
    (a, b) => Number(running(b)) - Number(running(a)),
  );
  const shown = ordered.slice(0, MAX_TIMERS);
  const hidden = ordered.length - shown.length;

  return (
    <>
      <div className="mb-0.5 flex items-baseline justify-between font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
        <span>Timers</span>
        {hidden > 0 && (
          <span className="font-normal normal-case tracking-normal">
            +{hidden} more
          </span>
        )}
      </div>
      {shown.map((timer) => {
        const isRunning = running(timer);
        return (
          <div key={timer.id} className="flex items-center gap-2 py-1">
            <span
              className={`size-2 shrink-0 rounded-full ${
                isRunning ? "bg-accent animate-pulse" : "bg-fg-ghost"
              }`}
            />
            <span className="min-w-0 flex-1 truncate">{timer.label}</span>
            <span
              className={`tabular-nums ${
                isRunning ? "font-semibold text-accent" : "text-fg-soft"
              }`}
            >
              {formatStopwatch(elapsedMs(timer, now))}
            </span>
            <button
              type="button"
              onClick={() =>
                isRunning ? onPause(timer.id) : onStart(timer.id)
              }
              title={
                isRunning ? `Pause ${timer.label}` : `Resume ${timer.label}`
              }
              className={`grid size-6 shrink-0 place-items-center rounded-md transition-colors hover:bg-surface-strong ${
                isRunning ? "text-accent" : "text-fg-soft hover:text-fg"
              }`}
            >
              {isRunning ? <PauseIcon /> : <PlayIcon />}
            </button>
          </div>
        );
      })}
    </>
  );
}

function PauseIcon() {
  return (
    <svg viewBox="0 0 24 24" className="size-3.5 fill-current" aria-hidden>
      <rect x="6" y="4" width="4" height="16" rx="1" />
      <rect x="14" y="4" width="4" height="16" rx="1" />
    </svg>
  );
}

function PlayIcon() {
  return (
    <svg viewBox="0 0 24 24" className="size-3.5 fill-current" aria-hidden>
      <polygon points="5 3 19 12 5 21 5 3" />
    </svg>
  );
}
