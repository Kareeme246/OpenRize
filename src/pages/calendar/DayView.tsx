import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { EmptyState } from "../../components/Page";
import { type BreakEntry, breakEnd, takenBreaks } from "../../lib/breaks";
import { recordingEntry } from "../../lib/entries";
import { formatDuration, formatTime } from "../../lib/format";
import type {
  ActivitySegment,
  Category,
  Project,
  TimeEntry,
} from "../../lib/types";
import { clampStyle, EntryBlock } from "./EntryBlock";
import {
  dayLength,
  gutterLabel,
  hourOffset,
  place,
  timelineFor,
} from "./timeline";

const SNAP_MS = 300_000;

interface DayViewProps {
  dayStart: number;
  entries: TimeEntry[];
  segments: ActivitySegment[];
  breaks: BreakEntry[];
  loading: boolean;
  selectedId?: string;
  categoryById: Map<string, Category>;
  projectById: Map<string, Project>;
  onSelect: (id: string) => void;
  onEmpty: () => void;
  onCreate: (startMs: number, endMs: number) => void;
  now: number;
}

/** The week view's now line, updated only while the app is visible. */
export function useMinuteClock(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    let timer: number | undefined;
    const sync = (): void => {
      window.clearInterval(timer);
      if (document.visibilityState === "visible") {
        setNow(Date.now());
        timer = window.setInterval(() => setNow(Date.now()), 15_000);
      }
    };
    document.addEventListener("visibilitychange", sync);
    sync();
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", sync);
    };
  }, []);
  return now;
}

export function DayView({
  dayStart,
  entries,
  segments,
  breaks,
  loading,
  selectedId,
  categoryById,
  projectById,
  onSelect,
  onEmpty,
  onCreate,
  now,
}: DayViewProps) {
  const viewport = useRef<HTMLDivElement>(null);
  const header = useRef<HTMLDivElement>(null);
  const column = useRef<HTMLButtonElement>(null);
  const [height, setHeight] = useState(600);
  const [hourHeight, setHourHeight] = useState(120);
  const [draft, setDraft] = useState<{ start: number; end: number } | null>(
    null,
  );
  const press = useRef<{ y: number; time: number } | null>(null);
  const hourHeightRef = useRef(hourHeight);
  hourHeightRef.current = hourHeight;
  const hoursInDay = dayLength(dayStart);
  const dayEnd = dayStart + hoursInDay * 3_600_000;
  const timeline = timelineFor([], "elapsed", hourHeight, hoursInDay, true);
  const entryById = new Map(entries.map((entry) => [entry.id, entry]));
  const nowOffset = hourOffset(now, dayStart, "elapsed");
  const showNow = now >= dayStart && now < dayEnd;
  const recording = showNow ? recordingEntry(entries, now) : undefined;
  const liveEnd = (entry: TimeEntry): number =>
    entry.status === "building"
      ? Math.min(Math.max(entry.startedAt, now), dayEnd)
      : entry.endedAt;
  // While a session records, the now line rides its live bottom edge (the
  // middle of the hairline gap under the block) instead of floating a few
  // pixels off it.
  const nowTop = recording
    ? (() => {
        const { top, height } = place(
          timeline,
          recording.startedAt,
          liveEnd(recording),
          dayStart,
          "elapsed",
        );
        return top + height + 1;
      })()
    : nowOffset * hourHeight;

  useLayoutEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const resize = (): void =>
      setHeight(
        element.clientHeight - (header.current?.offsetHeight ?? 0) - 32,
      );
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    resize();
    return () => observer.disconnect();
  }, []);

  // Open at 9 AM to 5 PM, or centred on now when today's now line falls
  // outside those hours; zoom changes only scale, not the 5 AM day bounds.
  const nowHours = useRef<number | null>(null);
  nowHours.current = showNow ? nowOffset : null;
  useLayoutEffect(() => {
    const element = viewport.current;
    if (!element || height <= 0) return;
    const size = height / 8;
    setHourHeight(size);
    const nine = hourOffset(
      new Date(dayStart).setHours(9),
      dayStart,
      "elapsed",
    );
    const current = nowHours.current;
    element.scrollTop =
      current !== null && (current < nine || current > nine + 8)
        ? Math.max(0, current * size - height / 2)
        : nine * size;
  }, [dayStart, height]);

  useEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const zoom = (scale: number, clientY: number): void => {
      const old = hourHeightRef.current;
      const next = Math.max(
        height / hoursInDay,
        Math.min(height / 2, old * scale),
      );
      const y = clientY - element.getBoundingClientRect().top;
      const hour = (element.scrollTop + y - 16) / old;
      hourHeightRef.current = next;
      setHourHeight(next);
      // Keep the point under the pointer fixed while zooming.
      element.scrollTop = Math.max(0, hour * next - y + 16);
    };
    const wheel = (event: WheelEvent): void => {
      if (!event.metaKey && !event.ctrlKey) return;
      event.preventDefault();
      zoom(Math.exp(-event.deltaY * 0.01), event.clientY);
    };
    let gestureScale = 1;
    const gestureStart = (event: Event): void => {
      event.preventDefault();
      gestureScale = 1;
    };
    const gestureChange = (event: Event): void => {
      event.preventDefault();
      const gesture = event as Event & { scale: number; clientY: number };
      zoom(gesture.scale / gestureScale, gesture.clientY);
      gestureScale = gesture.scale;
    };
    element.addEventListener("wheel", wheel, { passive: false });
    element.addEventListener("gesturestart", gestureStart, { passive: false });
    element.addEventListener("gesturechange", gestureChange, {
      passive: false,
    });
    return () => {
      element.removeEventListener("wheel", wheel);
      element.removeEventListener("gesturestart", gestureStart);
      element.removeEventListener("gesturechange", gestureChange);
    };
  }, [height, hoursInDay]);

  const timeAt = (clientY: number): number => {
    const bounds = column.current?.getBoundingClientRect();
    const offset = bounds ? (clientY - bounds.top) / hourHeight : 0;
    return Math.max(dayStart, Math.min(dayEnd, dayStart + offset * 3_600_000));
  };
  const startAt = (time: number): number =>
    Math.min(dayEnd - SNAP_MS, Math.floor(time / SNAP_MS) * SNAP_MS);
  const dragRange = (
    startY: number,
    startTime: number,
    currentY: number,
    currentTime: number,
  ): { start: number; end: number } | null => {
    if (Math.abs(currentY - startY) < 3) return null;
    if (currentY >= startY) {
      const start = Math.max(
        dayStart,
        Math.floor(startTime / SNAP_MS) * SNAP_MS,
      );
      const end = Math.min(dayEnd, Math.round(currentTime / SNAP_MS) * SNAP_MS);
      return end - start >= SNAP_MS ? { start, end } : null;
    }
    const end = Math.min(dayEnd, Math.ceil(startTime / SNAP_MS) * SNAP_MS);
    const start = Math.max(
      dayStart,
      Math.round(currentTime / SNAP_MS) * SNAP_MS,
    );
    return end - start >= SNAP_MS ? { start, end } : null;
  };

  return (
    <div
      ref={viewport}
      onPointerDown={(event) => {
        if (!(event.target as HTMLElement).closest(".calendar-entry"))
          onEmpty();
      }}
      className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden"
    >
      <div
        ref={header}
        className="sticky top-0 z-30 flex items-center gap-2 border-line border-b bg-panel px-4 py-2"
      >
        <div className="w-12 shrink-0" />
        <div className="w-4 shrink-0" />
        <div className="flex min-w-0 flex-1">
          <span className="min-w-0 flex-1 truncate font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
            Time Entries
          </span>
          <span className="w-[114px] shrink-0 truncate border-line border-l px-2 font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
            Labels
          </span>
          <span className="w-[114px] shrink-0 truncate border-line border-l px-2 font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
            Projects
          </span>
          <span
            className="flex w-[30px] shrink-0 items-center justify-center border-line border-l"
            title="Productivity metrics"
          >
            <svg
              viewBox="0 0 24 24"
              className="size-3.5 text-fg-faint"
              fill="none"
              stroke="currentColor"
              strokeWidth={1.8}
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-label="Productivity metrics"
            >
              <path d="M3 3v16a2 2 0 0 0 2 2h16" />
              <path d="M18 17V9" />
              <path d="M13 17V5" />
              <path d="M8 17v-3" />
            </svg>
          </span>
        </div>
      </div>

      <div className="relative p-4">
        <div
          className="relative flex items-stretch gap-2"
          style={{ height: `${timeline.height}px` }}
        >
          <div className="relative w-12 shrink-0 select-none text-right font-medium text-[11px] text-fg-faint">
            {timeline.hours.map((hour) => (
              <div
                key={`gutter-${hour}`}
                className="absolute right-2 -translate-y-2 whitespace-nowrap"
                style={{ top: `${hour * hourHeight}px` }}
              >
                {gutterLabel(hour, dayStart, "elapsed")}
              </div>
            ))}
          </div>
          <div
            className="relative w-4 shrink-0 overflow-hidden rounded-full bg-surface"
            role="img"
            aria-label="Activity strip"
          >
            {segments.map((segment) => {
              if (segment.kind === "break") return null;
              const end = segment.endedAt ?? Math.min(now, dayEnd);
              const { top, height: segmentHeight } = place(
                timeline,
                segment.startedAt,
                end,
                dayStart,
                "elapsed",
                3,
              );
              const owner = segment.entryId
                ? entryById.get(segment.entryId)
                : undefined;
              return (
                <div
                  key={segment.id}
                  title={`${segment.app}: ${segment.title}`}
                  className="absolute right-0 left-0 rounded-xs"
                  style={{
                    top: `${top}px`,
                    height: `${segmentHeight}px`,
                    backgroundColor: owner
                      ? "var(--accent)"
                      : "var(--fg-faint)",
                  }}
                />
              );
            })}
          </div>

          {/* Time Entries / Labels / Projects / Productivity lanes, sharing one
              continuous set of hour lines and one now line across the whole
              width instead of a copy per lane. */}
          <div className="relative min-w-0 flex-1">
            {timeline.hours.map((hour) => (
              <div
                key={`line-${hour}`}
                className="pointer-events-none absolute right-0 left-0 border-line-soft border-b"
                style={{ top: `${hour * hourHeight}px` }}
              />
            ))}
            {showNow && (
              <NowLine
                top={nowTop}
                recording={recording && { startedAt: recording.startedAt, now }}
              />
            )}

            <div className="flex h-full items-stretch">
              <div className="relative min-w-0 flex-1">
                <button
                  ref={column}
                  type="button"
                  aria-label="Drag to add a session"
                  title="Drag to add a session"
                  className="absolute inset-0 w-full cursor-cell touch-none"
                  onPointerDown={(event) => {
                    if (event.button !== 0) return;
                    press.current = {
                      y: event.clientY,
                      time: timeAt(event.clientY),
                    };
                    setDraft(null);
                    event.currentTarget.setPointerCapture(event.pointerId);
                  }}
                  onPointerMove={(event) => {
                    if (press.current === null) return;
                    setDraft(
                      dragRange(
                        press.current.y,
                        press.current.time,
                        event.clientY,
                        timeAt(event.clientY),
                      ),
                    );
                  }}
                  onPointerUp={(event) => {
                    if (press.current === null) return;
                    const range = dragRange(
                      press.current.y,
                      press.current.time,
                      event.clientY,
                      timeAt(event.clientY),
                    );
                    press.current = null;
                    setDraft(null);
                    if (range) {
                      onCreate(range.start, range.end);
                    }
                  }}
                  onPointerCancel={() => {
                    press.current = null;
                    setDraft(null);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      const start = startAt(
                        Math.max(dayStart, Math.min(now, dayEnd - SNAP_MS)),
                      );
                      onCreate(start, start + SNAP_MS);
                    }
                  }}
                />
                {takenBreaks(breaks).map((entry) => {
                  const start = entry.startedAt ?? dayStart;
                  const { top, height: bandHeight } = place(
                    timeline,
                    start,
                    Math.min(breakEnd(entry, now), dayEnd),
                    dayStart,
                    "elapsed",
                  );
                  return (
                    <BreakBand
                      key={entry.id}
                      entry={entry}
                      top={top}
                      height={bandHeight}
                      now={now}
                    />
                  );
                })}
                {entries.map((entry) => {
                  const { top, height: entryHeight } = place(
                    timeline,
                    entry.startedAt,
                    liveEnd(entry),
                    dayStart,
                    "elapsed",
                  );
                  return (
                    <EntryBlock
                      key={entry.id}
                      entry={entry}
                      top={top}
                      height={entryHeight}
                      selected={entry.id === selectedId}
                      category={
                        entry.categoryId
                          ? categoryById.get(entry.categoryId)
                          : undefined
                      }
                      project={
                        entry.projectId
                          ? projectById.get(entry.projectId)
                          : undefined
                      }
                      onSelect={onSelect}
                      now={now}
                      recording={entry.id === recording?.id}
                    />
                  );
                })}
                {draft && (
                  <div
                    className="pointer-events-none absolute right-4 left-0 z-20 rounded-md border border-accent bg-accent/20"
                    style={{
                      top: `${((draft.start - dayStart) / 3_600_000) * hourHeight}px`,
                      height: `${((draft.end - draft.start) / 3_600_000) * hourHeight}px`,
                    }}
                  />
                )}
                {entries.length === 0 && !loading && (
                  <div className="pointer-events-none absolute inset-x-0 top-16">
                    <EmptyState
                      title={
                        dayStart > now
                          ? "Nothing here yet"
                          : "No entries recorded for this day"
                      }
                      hint={
                        dayStart > now
                          ? "This day hasn't happened yet."
                          : "Activity appears here automatically as you use your computer."
                      }
                    />
                  </div>
                )}
              </div>

              <div className="relative w-[114px] shrink-0 border-line border-l">
                {entries.map((entry) => {
                  const { top, height: entryHeight } = place(
                    timeline,
                    entry.startedAt,
                    liveEnd(entry),
                    dayStart,
                    "elapsed",
                  );
                  const category = entry.categoryId
                    ? categoryById.get(entry.categoryId)
                    : undefined;
                  return (
                    <LaneBlock
                      key={entry.id}
                      entryId={entry.id}
                      top={top}
                      height={entryHeight}
                      selected={entry.id === selectedId}
                      color={category?.color}
                      label={category?.name ?? "Uncategorized"}
                      onSelect={onSelect}
                    />
                  );
                })}
              </div>

              <div className="relative w-[114px] shrink-0 border-line border-l">
                {entries.map((entry) => {
                  if (!entry.projectId) return null;
                  const { top, height: entryHeight } = place(
                    timeline,
                    entry.startedAt,
                    liveEnd(entry),
                    dayStart,
                    "elapsed",
                  );
                  const project = projectById.get(entry.projectId);
                  return (
                    <LaneBlock
                      key={entry.id}
                      entryId={entry.id}
                      top={top}
                      height={entryHeight}
                      selected={entry.id === selectedId}
                      color={project?.color}
                      label={project?.name ?? "Unknown project"}
                      onSelect={onSelect}
                    />
                  );
                })}
              </div>

              <div className="w-[30px] shrink-0 border-line border-l" />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

interface BreakBandProps {
  entry: BreakEntry;
  top: number;
  height: number;
  now: number;
}

/**
 * A taken break: a neutral gray block in the gap the segment leaves behind.
 * It carries no label or project, only its time and length.
 */
export function BreakBand({ entry, top, height, now }: BreakBandProps) {
  const start = entry.startedAt ?? now;
  const length = formatDuration(Math.max(0, breakEnd(entry, now) - start));
  return (
    <div
      className="pointer-events-none absolute right-4 left-0 z-0 overflow-hidden rounded-md border border-line-strong bg-surface-strong"
      style={{ top: `${top}px`, height: `${height}px` }}
      title={`${formatTime(start)} · ${length}`}
      role="img"
      aria-label={`Break, ${length}`}
    />
  );
}

/** Line height of a lane label (10.5px, leading-tight). */
const LANE_LINE = 13.125;

interface LaneBlockProps {
  entryId: string;
  top: number;
  height: number;
  selected: boolean;
  color?: string;
  label: string;
  onSelect: (id: string) => void;
}

/**
 * A time entry's colour and name in the Labels or Projects lane, aligned to
 * the same top/height as its block in the Time Entries lane.
 */
function LaneBlock({
  entryId,
  top,
  height,
  selected,
  color,
  label,
  onSelect,
}: LaneBlockProps) {
  const tone = color ?? "var(--fg-faint)";
  return (
    <button
      type="button"
      onClick={() => onSelect(entryId)}
      title={label}
      aria-label={label}
      className={`calendar-entry absolute right-1 left-0.5 overflow-hidden rounded-md px-1.5 text-left text-[10.5px] font-medium leading-tight transition-all ${
        selected ? "z-20 ring-2 ring-accent" : "z-10 hover:border-fg-soft/40"
      }`}
      style={{
        top: `${top}px`,
        height: `${height}px`,
        backgroundColor: `color-mix(in srgb, ${tone} 15%, var(--bg-panel))`,
        color: tone,
      }}
    >
      <span
        className="min-w-0"
        style={clampStyle(Math.max(1, Math.floor(height / LANE_LINE)))}
      >
        {label}
      </span>
    </button>
  );
}

interface NowLineProps {
  top: number;
  /**
   * The session being recorded, when there is one: the line then sits on
   * its growing bottom edge, pulses, and (unless `compact`) is tagged with
   * how long the recording has run.
   */
  recording?: { startedAt: number; now: number };
  /** Week columns: no room for the tag, so the pulse alone carries it. */
  compact?: boolean;
}

/** The current time across a timeline column, centred on `top`. */
export function NowLine({ top, recording, compact = false }: NowLineProps) {
  if (!recording) {
    return (
      <div
        className="pointer-events-none absolute right-0 left-0 z-30 flex -translate-y-1/2 items-center"
        style={{ top: `${top}px` }}
        aria-hidden="true"
      >
        <span className="-ml-1 size-2 rounded-full bg-danger" />
        <span className="h-px flex-1 bg-danger/70" />
      </div>
    );
  }
  const elapsed = formatDuration(recording.now - recording.startedAt);
  return (
    <div
      className="pointer-events-none absolute right-0 left-0 z-30 flex -translate-y-1/2 items-center"
      style={{ top: `${top}px` }}
      title={`Recording since ${formatTime(recording.startedAt)}`}
    >
      <span className="recording-dot -ml-1 size-2 shrink-0 rounded-full bg-danger" />
      <span className="h-0.5 flex-1 rounded-full bg-danger" />
      {compact ? (
        <span className="sr-only">Recording, {elapsed}</span>
      ) : (
        <span className="-ml-px flex shrink-0 items-center gap-1 rounded-full bg-danger py-0.5 pr-2 pl-1.5 font-semibold text-[10px] text-canvas leading-none tabular-nums shadow-sm">
          <span className="recording-dot size-1.5 rounded-full bg-canvas" />
          Recording · {elapsed}
        </span>
      )}
    </div>
  );
}
