import {
  addDays,
  currentCalendarDay,
  isSameDay,
  localDateString,
} from "../../lib/dates";
import { durationOf, entryEnd, isReviewable } from "../../lib/entries";
import { formatDuration } from "../../lib/format";
import type { Category, Project, TimeEntry } from "../../lib/types";
import { NowLine, useMinuteClock } from "./DayView";
import { EntryBlock } from "./EntryBlock";
import { gutterLabel, hourOffset, place, timelineFor } from "./timeline";

const HOUR_HEIGHT_PX = 56;

interface WeekViewProps {
  weekStart: Date;
  entries: TimeEntry[];
  loading: boolean;
  selectedId?: string;
  categoryById: Map<string, Category>;
  projectById: Map<string, Project>;
  onSelect: (id: string) => void;
  onOpenDay: (date: string) => void;
  now?: number;
}

/**
 * Seven day columns with the Day view's blocks. Each header shows the day's
 * total and opens that day; a block opens the same review panel.
 */
export function WeekView({
  weekStart,
  entries,
  loading,
  selectedId,
  categoryById,
  projectById,
  onSelect,
  onOpenDay,
  now: propsNow,
}: WeekViewProps) {
  const clockNow = useMinuteClock();
  const now = propsNow ?? clockNow;
  const days = Array.from({ length: 7 }, (_, index) =>
    addDays(weekStart, index),
  );
  const columns = days.map((day) => {
    const start = day.getTime();
    const end = addDays(day, 1).getTime();
    const list = entries.filter(
      (entry) => entry.startedAt >= start && entry.startedAt < end,
    );
    return {
      day,
      start,
      entries: list,
      totalMs: list.reduce((sum, entry) => sum + durationOf(entry, now), 0),
      pending: list.filter(isReviewable).length,
    };
  });
  const timeline = timelineFor(
    columns.flatMap((column) => [
      {
        start: column.start,
        end: addDays(column.day, 1).getTime(),
        dayStart: column.start,
      },
      ...column.entries.map((entry) => ({
        start: entry.startedAt,
        end: entryEnd(entry, now),
        dayStart: column.start,
      })),
    ]),
    "wall",
    HOUR_HEIGHT_PX,
    29,
  );
  const today = currentCalendarDay(new Date(now));

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="grid shrink-0 grid-cols-[56px_repeat(7,minmax(0,1fr))] border-line border-b px-4 pt-2 pb-1.5">
        <div />
        {columns.map((column) => {
          const isToday = isSameDay(column.day, today);
          return (
            <button
              key={column.start}
              type="button"
              onClick={() => onOpenDay(localDateString(column.day))}
              title="Open this day"
              className="min-w-0 rounded-md px-1.5 py-1 text-left transition-colors hover:bg-surface"
            >
              <div className="flex items-baseline gap-1.5">
                <span className="font-medium text-[11px] text-fg-soft uppercase">
                  {column.day.toLocaleDateString(undefined, {
                    weekday: "short",
                  })}
                </span>
                <span
                  className={`font-semibold text-[14px] tabular-nums ${
                    isToday
                      ? "rounded-full bg-accent px-1.5 text-accent-fg"
                      : "text-fg-strong"
                  }`}
                >
                  {column.day.getDate()}
                </span>
              </div>
              <div className="mt-0.5 flex items-center gap-1.5 truncate font-mono text-[10.5px] text-fg-faint tabular-nums">
                {column.totalMs > 0 ? formatDuration(column.totalMs) : "–"}
                {column.pending > 0 && (
                  <span className="rounded-full bg-review/15 px-1.5 font-sans font-semibold text-review">
                    {column.pending}
                  </span>
                )}
              </div>
            </button>
          );
        })}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-4 py-3">
        <div
          className="relative grid grid-cols-[56px_repeat(7,minmax(0,1fr))]"
          style={{ height: `${timeline.height}px` }}
        >
          <div className="relative select-none text-right font-medium text-[10.5px] text-fg-faint">
            {timeline.hours.map((hour) => (
              <div
                key={`gutter-${hour}`}
                className="absolute right-2 -translate-y-2 whitespace-nowrap"
                style={{
                  top: `${(hour - timeline.startHour) * HOUR_HEIGHT_PX}px`,
                }}
              >
                {gutterLabel(hour, weekStart.getTime(), "wall")}
              </div>
            ))}
          </div>

          {columns.map((column) => {
            const isToday = isSameDay(column.day, today);
            const nowOffset = hourOffset(now, column.start, "wall");
            const weekend =
              column.day.getDay() === 0 || column.day.getDay() === 6;
            return (
              <div
                key={column.start}
                className={`relative border-line-soft border-l ${
                  weekend ? "bg-inset-soft" : ""
                }`}
              >
                {timeline.hours.map((hour) => (
                  <div
                    key={`line-${hour}`}
                    className="pointer-events-none absolute right-0 left-0 border-line-soft border-b"
                    style={{
                      top: `${(hour - timeline.startHour) * HOUR_HEIGHT_PX}px`,
                    }}
                  />
                ))}
                {column.entries.map((entry) => {
                  const end = entryEnd(entry, now);
                  const { top, height } = place(
                    timeline,
                    entry.startedAt,
                    end,
                    column.start,
                    "wall",
                    3,
                  );
                  return (
                    <EntryBlock
                      key={entry.id}
                      entry={entry}
                      top={top}
                      height={height}
                      narrow
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
                    />
                  );
                })}
                {isToday &&
                  nowOffset >= timeline.startHour &&
                  nowOffset <= timeline.endHour && (
                    <NowLine
                      top={(nowOffset - timeline.startHour) * HOUR_HEIGHT_PX}
                    />
                  )}
              </div>
            );
          })}
        </div>
        {entries.length === 0 && !loading && (
          <p className="mt-3 text-center text-[11.5px] text-fg-faint">
            No entries recorded this week.
          </p>
        )}
      </div>
    </div>
  );
}
