import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { InlineError } from "../../components/Page";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import type { DayThreads } from "../../lib/agents";
import * as api from "../../lib/api";
import type { BreakEntry } from "../../lib/breaks";
import { calendarDayEdges } from "../../lib/dates";
import { entryEnd } from "../../lib/entries";
import type {
  CalendarScale,
  Category,
  Project,
  TimeEntry,
} from "../../lib/types";
import { DayView, type ProjectRail } from "./DayView";
import { FocusStrip, ThreadsView } from "./ThreadsView";
import { WeekThreads } from "./WeekThreads";
import { WeekView } from "./WeekView";
import {
  useWorkflowLayout,
  WorkflowLayoutControl,
} from "./WorkflowLayoutControl";

/** The same workflow data and renderers as Calendar, beside the review table. */
export function TimesheetWorkflow({
  scale,
  start,
  end,
  entries,
  categoryById,
  projectById,
  selectedId,
  onSelect,
  onCreated,
  onOpenDay,
}: {
  scale: CalendarScale;
  start: Date;
  end: Date;
  entries: TimeEntry[];
  categoryById: Map<string, Category>;
  projectById: Map<string, Project>;
  selectedId?: string;
  onSelect: (id?: string) => void;
  onCreated: () => Promise<void>;
  onOpenDay: (date: string) => void;
}) {
  const { layout, choose } = useWorkflowLayout();
  const [days, setDays] = useState<DayThreads[]>([]);
  const [agents, setAgents] = useState<TimeEntry[]>([]);
  const [breaks, setBreaks] = useState<BreakEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [now, setNow] = useState(Date.now);
  const startMs = start.getTime();
  const endMs = end.getTime();
  const rangeKey = `${startMs}:${endMs}`;
  const shown = useRef(rangeKey);
  shown.current = rangeKey;
  const boundaries = useMemo(() => calendarDayEdges(start, end), [start, end]);
  const refresh = useCallback(async (): Promise<void> => {
    const key = `${startMs}:${endMs}`;
    try {
      const [nextDays, nextAgents, nextBreaks] = await Promise.all([
        api.threadDays(boundaries),
        api.listAgentEntries(startMs, endMs - 1),
        api.listBreaks(startMs, endMs),
      ]);
      if (shown.current !== key) return;
      setDays(nextDays);
      setAgents(nextAgents);
      setBreaks(nextBreaks);
      setNow(Date.now());
      setError(null);
    } catch (cause) {
      if (shown.current === key) setError(api.describeError(cause));
    } finally {
      if (shown.current === key) setLoading(false);
    }
  }, [startMs, endMs, boundaries]);
  useEffect(() => {
    setLoading(true);
    void refresh();
  }, [refresh]);
  useTauriEvent(api.ENTRIES_CHANGED, refresh);
  useTauriEvent(api.AGENTS_CHANGED, refresh);
  useTauriEvent(api.BREAK_STATE_CHANGED, refresh);
  useTauriEvent(api.ACTIVITY_CHANGED, refresh);
  useEffect(() => {
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible" && document.hasFocus())
        void refresh();
    }, 15_000);
    return () => window.clearInterval(timer);
  }, [refresh]);
  const rails: ProjectRail[] = days.flatMap((day) =>
    day.threads.flatMap((thread) =>
      thread.rails.map((rail) => ({ ...rail, projectId: thread.projectId })),
    ),
  );
  const visible = [...entries, ...agents].filter(
    (entry) => entry.startedAt < endMs && entryEnd(entry, now) > startMs,
  );
  return (
    <section
      aria-label="Multi-project workflow tracking"
      className={`mb-4 flex min-h-0 flex-col overflow-hidden rounded-xl border border-line bg-panel sharper:rounded-none ${layout === "lanes" && scale !== "month" ? "h-[440px]" : "h-[280px]"}`}
    >
      <div className="flex items-center justify-between gap-3 border-line border-b px-3 py-2">
        <span className="text-[11.5px] text-fg-soft">
          Your time and parallel agent work · agent time stays separate
        </span>
        {scale !== "month" && (
          <WorkflowLayoutControl
            name="timesheet-workflow-layout"
            layout={layout}
            onChange={choose}
          />
        )}
      </div>
      {error && <InlineError message={error} onRetry={refresh} />}
      {scale === "day" && layout === "lanes" && (
        <>
          {days[0] && <FocusStrip day={days[0]} projectById={projectById} />}
          <DayView
            key={startMs}
            viewportKey={`timesheet:day:${startMs}`}
            dayStart={startMs}
            showAgents
            rails={rails}
            entries={visible}
            segments={[]}
            breaks={breaks}
            loading={loading}
            selectedId={selectedId}
            categoryById={categoryById}
            projectById={projectById}
            onSelect={onSelect}
            onEmpty={() => onSelect(undefined)}
            onCreate={async (startedAt, endedAt) => {
              try {
                await api.createTimeEntry({
                  startedAt,
                  endedAt,
                  description: "Untitled session",
                  review: true,
                });
                await onCreated();
              } catch (cause) {
                setError(api.describeError(cause));
              }
            }}
            now={now}
          />
        </>
      )}
      {scale === "day" && layout === "timeline" && (
        <ThreadsView
          day={days[0]}
          dayStart={startMs}
          layout="timeline"
          projectById={projectById}
          selectedId={selectedId}
          loading={loading}
          onSelect={onSelect}
          now={now}
          viewportKey={`timesheet:timeline:${startMs}`}
        />
      )}
      {scale === "week" && layout === "lanes" && (
        <WeekView
          weekStart={start}
          viewportKey={`timesheet:week:${startMs}`}
          entries={visible}
          parallel
          breaks={breaks}
          loading={loading}
          selectedId={selectedId}
          categoryById={categoryById}
          projectById={projectById}
          onSelect={onSelect}
          onOpenDay={onOpenDay}
          agentMsByDay={days.map((day) => day.agentsMs)}
          now={now}
        />
      )}
      {(scale === "month" || (scale === "week" && layout === "timeline")) && (
        <WeekThreads
          weekStart={start}
          days={days}
          projectById={projectById}
          loading={loading}
          onOpenDay={onOpenDay}
          now={now}
        />
      )}
    </section>
  );
}
