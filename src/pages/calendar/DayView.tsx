import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { EmptyState } from "../../components/Page";
import type {
  ActivitySegment,
  Category,
  Project,
  TimeEntry,
} from "../../lib/types";
import { EntryBlock } from "./EntryBlock";
import {
  dayLength,
  gutterLabel,
  hourOffset,
  place,
  timelineFor,
} from "./timeline";

const QUARTER = 900_000;

interface DayViewProps {
  dayStart: number;
  entries: TimeEntry[];
  segments: ActivitySegment[];
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
  const column = useRef<HTMLButtonElement>(null);
  const [height, setHeight] = useState(600);
  const [hourHeight, setHourHeight] = useState(120);
  const [draft, setDraft] = useState<{ start: number; end: number } | null>(
    null,
  );
  const press = useRef<number | null>(null);
  const hourHeightRef = useRef(hourHeight);
  hourHeightRef.current = hourHeight;
  const hoursInDay = dayLength(dayStart);
  const dayEnd = dayStart + hoursInDay * 3_600_000;
  const timeline = timelineFor([], "elapsed", hourHeight, hoursInDay, true);
  const entryById = new Map(entries.map((entry) => [entry.id, entry]));
  const nowOffset = hourOffset(now, dayStart, "elapsed");

  useLayoutEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const resize = (): void => setHeight(element.clientHeight - 32);
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    resize();
    return () => observer.disconnect();
  }, []);

  // Open at 9 AM to 5 PM; zoom changes only scale, not the 5 AM day bounds.
  useLayoutEffect(() => {
    const element = viewport.current;
    if (!element || height <= 0) return;
    const size = height / 8;
    setHourHeight(size);
    element.scrollTop =
      hourOffset(new Date(dayStart).setHours(9), dayStart, "elapsed") * size;
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
    Math.min(dayEnd - QUARTER, Math.floor(time / QUARTER) * QUARTER);
  const endAt = (time: number): number =>
    Math.max(
      dayStart + QUARTER,
      Math.min(dayEnd, Math.round(time / QUARTER) * QUARTER),
    );
  const dragRange = (
    start: number,
    end: number,
  ): { start: number; end: number } => {
    const from = Math.min(start, end);
    return { start: from, end: Math.max(from + QUARTER, Math.max(start, end)) };
  };

  return (
    <div
      ref={viewport}
      onPointerDown={(event) => {
        if (!(event.target as HTMLElement).closest(".calendar-entry"))
          onEmpty();
      }}
      className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-4"
    >
      <div
        className="relative grid grid-cols-[48px_16px_minmax(0,1fr)] gap-2"
        style={{ height: `${timeline.height}px` }}
      >
        <div className="relative select-none text-right font-medium text-[11px] text-fg-faint">
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
          className="relative overflow-hidden rounded-full bg-surface"
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
            const category = owner?.categoryId
              ? categoryById.get(owner.categoryId)
              : undefined;
            return (
              <div
                key={segment.id}
                title={`${segment.app}: ${segment.title}`}
                className="absolute right-0 left-0 rounded-xs"
                style={{
                  top: `${top}px`,
                  height: `${segmentHeight}px`,
                  backgroundColor: category?.color ?? "var(--fg-faint)",
                }}
              />
            );
          })}
        </div>
        <div className="relative">
          <button
            ref={column}
            type="button"
            aria-label="Drag to add a session"
            title="Drag to add a session"
            className="absolute inset-0 w-full cursor-cell touch-none"
            onPointerDown={(event) => {
              if (event.button !== 0) return;
              const start = startAt(timeAt(event.clientY));
              press.current = start;
              setDraft({ start, end: start + QUARTER });
              event.currentTarget.setPointerCapture(event.pointerId);
            }}
            onPointerMove={(event) => {
              if (press.current === null) return;
              setDraft(dragRange(press.current, endAt(timeAt(event.clientY))));
            }}
            onPointerUp={(event) => {
              if (press.current === null) return;
              const range = dragRange(
                press.current,
                endAt(timeAt(event.clientY)),
              );
              press.current = null;
              setDraft(null);
              onCreate(range.start, range.end);
            }}
            onPointerCancel={() => {
              press.current = null;
              setDraft(null);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                const start = startAt(
                  Math.max(dayStart, Math.min(now, dayEnd - QUARTER)),
                );
                onCreate(start, start + QUARTER);
              }
            }}
          />
          {timeline.hours.map((hour) => (
            <div
              key={`line-${hour}`}
              className="pointer-events-none absolute right-0 left-0 border-line-soft border-b"
              style={{ top: `${hour * hourHeight}px` }}
            />
          ))}
          {entries.map((entry) => {
            const { top, height: entryHeight } = place(
              timeline,
              entry.startedAt,
              entry.endedAt,
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
                  entry.projectId ? projectById.get(entry.projectId) : undefined
                }
                onSelect={onSelect}
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
          {now >= dayStart && now < dayEnd && (
            <NowLine top={nowOffset * hourHeight} />
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
      </div>
    </div>
  );
}

export function NowLine({ top }: { top: number }) {
  return (
    <div
      className="pointer-events-none absolute right-0 left-0 z-30 flex items-center"
      style={{ top: `${top}px` }}
      aria-hidden="true"
    >
      <span className="-ml-1 size-2 rounded-full bg-danger" />
      <span className="h-px flex-1 bg-danger/70" />
    </div>
  );
}
