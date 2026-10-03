import { useMemo } from "react";
import { EmptyState } from "../../components/Page";
import type { DayThreads } from "../../lib/agents";
import {
  addDays,
  currentCalendarDay,
  isSameDay,
  localDateString,
} from "../../lib/dates";
import { formatDuration as spaced } from "../../lib/format";
import type { Project } from "../../lib/types";
import { useRememberedScroll } from "./useRememberedScroll";

/** `1h40m`: the cells are narrow, so the space goes. */
function formatDuration(ms: number): string {
  return spaced(ms).replace(" ", "");
}

interface WeekThreadsProps {
  weekStart: Date;
  days: DayThreads[];
  projectById: Map<string, Project>;
  loading: boolean;
  onOpenDay: (date: string) => void;
  now: number;
}

interface Row {
  key: string;
  projectId: string | null;
  youMs: number;
  agentsMs: number;
}

/**
 * Thread rows by day: what you spent on each project each day, with the agent
 * time that ran beside it as a ghost. Only your own time adds up.
 */
export function WeekThreads({
  weekStart,
  days,
  projectById,
  loading,
  onOpenDay,
  now,
}: WeekThreadsProps) {
  const { viewport, onScroll } = useRememberedScroll(
    `calendar:week-timeline:${weekStart.getTime()}`,
  );
  const columns = Array.from({ length: Math.max(7, days.length) }, (_, index) =>
    addDays(weekStart, index),
  );
  const today = currentCalendarDay(new Date(now));
  const rows = useMemo(() => {
    const byKey = new Map<string, Row>();
    for (const day of days) {
      for (const thread of day.threads) {
        const key = thread.projectId ?? "none";
        const row = byKey.get(key) ?? {
          key,
          projectId: thread.projectId,
          youMs: 0,
          agentsMs: 0,
        };
        row.youMs += thread.youMs;
        row.agentsMs += thread.agentsMs;
        byKey.set(key, row);
      }
    }
    return [...byKey.values()].sort(
      (a, b) =>
        Number(a.projectId === null) - Number(b.projectId === null) ||
        b.youMs + b.agentsMs - (a.youMs + a.agentsMs),
    );
  }, [days]);

  if (rows.length === 0) {
    return (
      <div className="min-h-0 flex-1 overflow-y-auto">
        {!loading && (
          <EmptyState
            title="No project activity in this range"
            hint="Projects you work on show up here, one row each."
          />
        )}
      </div>
    );
  }

  const cell = (dayIndex: number, key: string) =>
    days[dayIndex]?.threads.find((t) => (t.projectId ?? "none") === key);

  return (
    <div
      ref={viewport}
      onScroll={onScroll}
      className="min-h-0 flex-1 overflow-auto"
    >
      <div
        className="grid px-4 py-2 text-[12px]"
        style={{
          gridTemplateColumns: `96px repeat(${columns.length}, minmax(64px, 1fr)) 64px`,
        }}
      >
        <div />
        {columns.map((day, index) => {
          const focus = days[index]?.focus;
          return (
            <button
              key={day.getTime()}
              type="button"
              onClick={() => onOpenDay(localDateString(day))}
              title={
                focus && focus.switches > 0
                  ? `${focus.switches} switches · longest stretch ${formatDuration(focus.longestMs)}`
                  : undefined
              }
              className="rounded-md px-1 py-1 text-left transition-colors hover:bg-surface"
            >
              <div className="flex flex-col items-start leading-tight">
                <span className="font-medium text-[10px] text-fg-soft uppercase">
                  {day.toLocaleDateString(undefined, { weekday: "short" })}
                </span>
                <span
                  className={`font-semibold text-[14px] tabular-nums ${
                    isSameDay(day, today)
                      ? "rounded-full bg-accent px-1.5 text-accent-fg"
                      : "text-fg-strong"
                  }`}
                >
                  {day.getDate()}
                </span>
              </div>
            </button>
          );
        })}
        <div className="px-2 py-1 text-right font-medium text-[11px] text-fg-soft uppercase">
          Week
        </div>

        {rows.map((row) => {
          const project = row.projectId
            ? projectById.get(row.projectId)
            : undefined;
          const color = project?.color ?? "var(--fg-faint)";
          return (
            <div key={row.key} className="contents">
              <div className="flex min-w-0 items-center gap-2 border-line-soft border-t px-2 py-2">
                <i
                  className="size-2 shrink-0 rounded-full"
                  style={{ backgroundColor: color }}
                />
                <span className="truncate font-medium text-fg">
                  {row.projectId
                    ? (project?.name ?? "Unknown project")
                    : "No project"}
                </span>
              </div>
              {columns.map((day, index) => {
                const found = cell(index, row.key);
                return (
                  <div
                    key={day.getTime()}
                    className="border-line-soft border-t px-1 py-2 tabular-nums"
                  >
                    {found && found.youMs > 0 ? (
                      <span
                        className="inline-block whitespace-nowrap rounded-md px-1 py-0.5 font-semibold text-[11px]"
                        style={{
                          backgroundColor: `color-mix(in srgb, ${color} 22%, var(--bg-panel))`,
                          color,
                        }}
                      >
                        {formatDuration(found.youMs)}
                      </span>
                    ) : (
                      <span className="text-fg-ghost">–</span>
                    )}
                    {found && found.agentsMs > 0 && (
                      <div className="text-[10.5px] text-fg-faint opacity-80">
                        +{formatDuration(found.agentsMs)}
                      </div>
                    )}
                  </div>
                );
              })}
              <div className="border-line-soft border-t px-2 py-2 text-right tabular-nums">
                <div className="font-semibold text-fg-strong">
                  {formatDuration(row.youMs)}
                </div>
                {row.agentsMs > 0 && (
                  <div className="text-[10.5px] text-fg-faint opacity-80">
                    +{formatDuration(row.agentsMs)}
                  </div>
                )}
              </div>
            </div>
          );
        })}

        <div className="border-line border-t px-2 py-2 font-semibold text-fg-strong">
          Work time
        </div>
        {columns.map((day, index) => (
          <div
            key={day.getTime()}
            className="border-line border-t px-1 py-2 tabular-nums"
          >
            <div className="font-semibold text-fg-strong">
              {days[index]?.workMs ? formatDuration(days[index].workMs) : "–"}
            </div>
            {(days[index]?.agentsMs ?? 0) > 0 && (
              <div className="text-[10.5px] text-fg-faint opacity-80">
                +{formatDuration(days[index].agentsMs)}
              </div>
            )}
          </div>
        ))}
        <div className="border-line border-t px-2 py-2 text-right font-semibold text-fg-strong tabular-nums">
          {formatDuration(days.reduce((sum, day) => sum + day.workMs, 0))}
        </div>
      </div>
      <p className="px-4 pb-3 text-[11px] text-fg-faint">
        The faint +time is what agents ran beside you. It is never part of your
        work time.
      </p>
    </div>
  );
}
