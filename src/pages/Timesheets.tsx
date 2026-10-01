import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  BUTTON_SECONDARY,
  DateStepper,
  InlineError,
  PageHeader,
  SkeletonRows,
} from "../components/Page";
import {
  SegmentedControl,
  type SegmentedOption,
} from "../components/SegmentedControl";
import { useCatalog } from "../hooks/useCatalog";
import { useSettings } from "../hooks/useSettings";
import { useTauriEvent } from "../hooks/useTauriEvent";
import type { AgentReport } from "../lib/agents";
import * as api from "../lib/api";
import {
  dayEdges,
  isoWeek,
  localDateString,
  parseLocalDate,
  rangeFor,
  stepDate,
} from "../lib/dates";
import { isReviewable } from "../lib/entries";
import { dailyTargetMs, weeklyTargetMs } from "../lib/settings";
import {
  buildSheet,
  filterEntries,
  hasTimesheetFilters,
  summarize,
} from "../lib/timesheetGrid";
import type { Route, TimeEntry, TimesheetFilters } from "../lib/types";
import { AgentSplit } from "./timesheets/AgentSplit";
import { DaySummary } from "./timesheets/DaySummary";
import { FilterBar } from "./timesheets/FilterBar";
import { SheetGrid } from "./timesheets/SheetGrid";

type TimesheetsRoute = Extract<Route, { name: "timesheets" }>;
type Scale = NonNullable<TimesheetsRoute["scale"]>;

const NO_FILTERS: TimesheetFilters = {};

const SCALES: SegmentedOption<Scale>[] = [
  { value: "day", label: "Day" },
  { value: "week", label: "Week" },
];

/** `Week 39 - September 21, 2026` or `Wednesday, September 23`, as Rise titles them. */
function crumbFor(scale: Scale, date: Date, start: Date): string {
  if (scale === "day") {
    return date.toLocaleDateString(undefined, {
      weekday: "long",
      month: "long",
      day: "numeric",
    });
  }
  return `Week ${isoWeek(date)} - ${start.toLocaleDateString(undefined, {
    month: "long",
    day: "numeric",
    year: "numeric",
  })}`;
}

export function Timesheets({
  route,
  navigate,
  replace,
}: {
  route: TimesheetsRoute;
  navigate: (route: Route) => void;
  replace: (route: Route) => void;
}) {
  const { settings } = useSettings();
  const catalog = useCatalog();
  const scale: Scale = route.scale ?? "week";
  const rows = route.rows ?? "project";
  const filters = route.filters ?? NO_FILTERS;
  const date = useMemo(() => parseLocalDate(route.date), [route.date]);
  const range = useMemo(() => rangeFor(scale, date), [scale, date]);
  const edges = useMemo(() => dayEdges(range.start, range.end), [range]);
  const start = range.start.getTime();
  const end = range.end.getTime();

  const [loaded, setLoaded] = useState<{
    start: number;
    end: number;
    entries: TimeEntry[];
  } | null>(null);
  const [agents, setAgents] = useState<AgentReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  // Responses for a range the user already left are dropped.
  const shown = useRef(`${start}-${end}`);
  shown.current = `${start}-${end}`;
  const refresh = useCallback(async (): Promise<void> => {
    try {
      const [next, report] = await Promise.all([
        api.listTimeEntries(start, end - 1),
        // The agent figures are a bonus: a failure never blocks the sheet.
        api.agentReport(start, end).catch(() => null),
      ]);
      if (shown.current !== `${start}-${end}`) return;
      setLoaded({ start, end, entries: next });
      setAgents(report);
      setError(null);
    } catch (cause) {
      if (shown.current === `${start}-${end}`) {
        setError(api.describeError(cause));
      }
    }
  }, [start, end]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useTauriEvent(api.ENTRIES_CHANGED, () => void refresh());
  useTauriEvent(api.SUGGESTION_READY, () => void refresh());
  useTauriEvent(api.AGENTS_CHANGED, () => void refresh());

  // Until the new range arrives, the grid keeps drawing the last one.
  const shownEdges = useMemo(
    () =>
      loaded && (loaded.start !== start || loaded.end !== end)
        ? dayEdges(new Date(loaded.start), new Date(loaded.end))
        : edges,
    [loaded, start, end, edges],
  );
  const clientOf = useCallback(
    (projectId: string) => catalog.projectById.get(projectId)?.clientId,
    [catalog.projectById],
  );
  const filtered = hasTimesheetFilters(filters);
  const entries = useMemo(
    () => (loaded ? filterEntries(loaded.entries, filters, clientOf) : []),
    [loaded, filters, clientOf],
  );
  const sheet = useMemo(
    // Week stays one flat list until the user narrows by client, project,
    // or category; Day always groups project rows under their client.
    () =>
      buildSheet(
        entries,
        shownEdges,
        rows,
        isReviewable,
        scale === "day" || filtered ? clientOf : undefined,
      ),
    [entries, shownEdges, rows, clientOf, scale, filtered],
  );
  const summary = useMemo(
    () =>
      summarize(
        entries,
        shownEdges[0],
        shownEdges[shownEdges.length - 1],
        isReviewable,
      ),
    [entries, shownEdges],
  );

  const setRoute = (patch: Partial<TimesheetsRoute>): void =>
    replace({ ...route, ...patch });
  const go = (patch: Partial<TimesheetsRoute>): void =>
    navigate({ ...route, ...patch });

  const approve = async (entry: TimeEntry): Promise<void> => {
    try {
      await api.approveTimeEntries([entry.id]);
      setActionError(null);
      await refresh();
    } catch (cause) {
      setActionError(api.describeError(cause));
    }
  };

  const unit = scale === "day" ? "day" : "week";
  const message = error ?? catalog.error ?? actionError;

  return (
    <div className="flex h-full min-h-0 flex-col bg-canvas text-fg">
      <PageHeader title="Timesheets" crumb={crumbFor(scale, date, range.start)}>
        <DateStepper
          unit={unit}
          onStep={(direction) =>
            go({ date: localDateString(stepDate(scale, date, direction)) })
          }
          onToday={() => go({ date: localDateString(new Date()) })}
          isToday={
            Date.now() >= range.start.getTime() &&
            Date.now() < range.end.getTime()
          }
        />
        <SegmentedControl
          name="timesheets-scale"
          value={scale}
          options={SCALES}
          onChange={(next) => go({ scale: next })}
        />
      </PageHeader>
      <div className="shrink-0 px-5 pt-4">
        <FilterBar
          catalog={catalog}
          filters={filters}
          onChange={(next) =>
            setRoute({ filters: hasTimesheetFilters(next) ? next : undefined })
          }
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-5 pt-3 pb-5">
        {message && (
          <div className="mb-3">
            <InlineError
              message={message}
              onRetry={() => {
                void refresh();
                void catalog.reload();
              }}
            />
          </div>
        )}
        {!loaded ? (
          <SkeletonRows />
        ) : (
          <div className="shape-stack space-y-4">
            {scale === "day" && <DaySummary summary={summary} />}
            <SheetGrid
              sheet={sheet}
              edges={shownEdges}
              scale={scale}
              rows={rows}
              onRowsChange={(next) => setRoute({ rows: next })}
              catalog={catalog}
              targetMs={
                scale === "day"
                  ? dailyTargetMs(settings)
                  : weeklyTargetMs(settings)
              }
              emptyTitle={
                filtered
                  ? "Nothing matches these filters"
                  : `No time tracked this ${unit}`
              }
              emptyHint={
                filtered
                  ? undefined
                  : "Tracked and added time shows up here by project and day."
              }
              emptyAction={
                filtered ? (
                  <button
                    type="button"
                    className={BUTTON_SECONDARY}
                    onClick={() => setRoute({ filters: undefined })}
                  >
                    Clear filters
                  </button>
                ) : undefined
              }
              onOpenDay={(dayStart) =>
                go({ scale: "day", date: localDateString(new Date(dayStart)) })
              }
              onReview={() =>
                navigate({
                  name: "timesheet",
                  scale,
                  date: localDateString(date),
                  tab: "review",
                })
              }
              onApprove={(entry) => void approve(entry)}
            />
            {agents && !filtered && (
              <AgentSplit report={agents} projectById={catalog.projectById} />
            )}
          </div>
        )}
      </div>
    </div>
  );
}
