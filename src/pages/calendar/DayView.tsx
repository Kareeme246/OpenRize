import { useEffect, useState } from "react";
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

const HOUR_HEIGHT_PX = 88;

interface DayViewProps {
  dayStart: number;
  entries: TimeEntry[];
  segments: ActivitySegment[];
  loading: boolean;
  selectedId?: string;
  categoryById: Map<string, Category>;
  projectById: Map<string, Project>;
  onSelect: (id: string) => void;
}

/** A clock that ticks once a minute, for the "now" line. */
export function useMinuteClock(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);
  return now;
}

/**
 * The Day timeline: the time gutter, the Activity strip (raw segments
 * coloured by their entry's category, gray where uncategorized), and the
 * Entries column.
 */
export function DayView({
  dayStart,
  entries,
  segments,
  loading,
  selectedId,
  categoryById,
  projectById,
  onSelect,
}: DayViewProps) {
  const now = useMinuteClock();
  const hoursInDay = dayLength(dayStart);
  const dayEnd = dayStart + hoursInDay * 3_600_000;
  const timeline = timelineFor(
    entries.map((entry) => ({
      start: entry.startedAt,
      end: entry.endedAt,
      dayStart,
    })),
    "elapsed",
    HOUR_HEIGHT_PX,
    hoursInDay,
  );
  const entryById = new Map(entries.map((entry) => [entry.id, entry]));
  const nowOffset = hourOffset(now, dayStart, "elapsed");
  const showNow =
    now >= dayStart &&
    now < dayEnd &&
    nowOffset >= timeline.startHour &&
    nowOffset <= timeline.endHour;

  return (
    <div className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-4">
      <div
        className="relative grid grid-cols-[48px_16px_minmax(0,1fr)] gap-2"
        style={{ height: `${timeline.height}px` }}
      >
        <div className="relative select-none text-right font-medium text-[11px] text-fg-faint">
          {timeline.hours.map((hour) => (
            <div
              key={`gutter-${hour}`}
              className="absolute right-2 -translate-y-2 whitespace-nowrap"
              style={{
                top: `${(hour - timeline.startHour) * HOUR_HEIGHT_PX}px`,
              }}
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
            const { top, height } = place(
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
                  height: `${height}px`,
                  backgroundColor: category?.color ?? "var(--fg-faint)",
                }}
              />
            );
          })}
        </div>

        <div className="relative">
          {timeline.hours.map((hour) => (
            <div
              key={`line-${hour}`}
              className="pointer-events-none absolute right-0 left-0 border-line-soft border-b"
              style={{
                top: `${(hour - timeline.startHour) * HOUR_HEIGHT_PX}px`,
              }}
            />
          ))}

          {entries.map((entry) => {
            const { top, height } = place(
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
                height={height}
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

          {showNow && (
            <NowLine top={(nowOffset - timeline.startHour) * HOUR_HEIGHT_PX} />
          )}

          {entries.length === 0 && !loading && (
            <div className="absolute inset-x-0 top-16">
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
