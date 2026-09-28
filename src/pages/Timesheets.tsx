import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  DateStepper,
  EmptyState,
  InlineError,
  PageHeader,
  SkeletonRows,
} from "../components/Page";
import { useCatalog } from "../hooks/useCatalog";
import { useTauriEvent } from "../hooks/useTauriEvent";
import * as api from "../lib/api";
import {
  addDays,
  dayEdges,
  localDateString,
  rangeFor,
  rangeLabel,
} from "../lib/dates";
import { formatDuration } from "../lib/format";
import { projectWeek } from "../lib/timesheetGrid";
import type { Route, TimeEntry } from "../lib/types";

export function Timesheets({ navigate }: { navigate: (route: Route) => void }) {
  const [week, setWeek] = useState(() => new Date());
  const [loaded, setLoaded] = useState<{
    start: number;
    edges: number[];
    entries: TimeEntry[];
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const catalog = useCatalog();
  const range = useMemo(() => rangeFor("week", week), [week]);
  const edges = useMemo(() => dayEdges(range.start, range.end), [range]);
  const start = range.start.getTime();
  const end = range.end.getTime();
  const shownStart = useRef(start);
  shownStart.current = start;
  const refresh = useCallback(async () => {
    try {
      const next = await api.listTimeEntries(start, end - 1);
      if (shownStart.current !== start) return;
      setLoaded({ start, edges, entries: next });
      setError(null);
    } catch (cause) {
      if (shownStart.current === start) setError(api.describeError(cause));
    }
  }, [start, end, edges]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useTauriEvent(api.ENTRIES_CHANGED, () => void refresh());
  const displayEdges = loaded?.edges ?? edges;
  const rows = useMemo(
    () => (loaded ? projectWeek(loaded.entries, loaded.edges) : []),
    [loaded],
  );
  const totals = displayEdges
    .slice(1)
    .map((_, day) => rows.reduce((sum, row) => sum + row.days[day], 0));
  const total = totals.reduce((sum, ms) => sum + ms, 0);

  return (
    <div className="flex h-full min-h-0 flex-col bg-base">
      <PageHeader title="Timesheets" crumb={rangeLabel("week", week)}>
        <DateStepper
          unit="week"
          onStep={(direction) =>
            setWeek((date) => addDays(date, direction * 7))
          }
          onToday={() => setWeek(new Date())}
        />
      </PageHeader>
      <div className="min-h-0 flex-1 overflow-auto p-5">
        <p className="mb-4 text-xs text-fg-soft">
          Project time by day · Select a day to review or approve its entries.
        </p>
        {(error || catalog.error) && (
          <div className="mb-4">
            <InlineError
              message={error || catalog.error || ""}
              onRetry={() => {
                void refresh();
                void catalog.reload();
              }}
            />
          </div>
        )}
        {!loaded ? (
          <SkeletonRows />
        ) : rows.length === 0 ? (
          <EmptyState
            title="No project time this week"
            hint="Assign a project to a time entry to see it here."
          />
        ) : (
          <div className="overflow-x-auto rounded-xl border border-line bg-panel">
            <table className="w-full min-w-[750px] border-collapse text-left text-xs tabular-nums">
              <thead className="border-b border-line bg-surface text-fg-soft">
                <tr>
                  <th scope="col" className="min-w-48 px-4 py-3 font-medium">
                    Project / client
                  </th>
                  {displayEdges.slice(0, -1).map((edge) => (
                    <th
                      scope="col"
                      key={edge}
                      className="px-2 py-3 text-right font-medium"
                    >
                      {new Date(edge).toLocaleDateString(undefined, {
                        weekday: "short",
                        day: "numeric",
                      })}
                    </th>
                  ))}
                  <th scope="col" className="px-4 py-3 text-right font-medium">
                    Total
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const project = catalog.projectById.get(row.projectId);
                  const client = project?.clientId
                    ? catalog.clientById.get(project.clientId)
                    : undefined;
                  return (
                    <tr
                      key={row.projectId}
                      className="border-b border-line/60 last:border-0"
                    >
                      <th scope="row" className="px-4 py-3 font-medium text-fg">
                        <span className="flex items-center gap-2">
                          <span
                            className="size-2 shrink-0 rounded-full"
                            style={{
                              backgroundColor:
                                project?.color ?? "var(--fg-soft)",
                            }}
                          />
                          {project?.name ?? "Unknown project"}
                        </span>
                        {client && (
                          <span className="ml-4 text-[11px] font-normal text-fg-faint">
                            {client.name}
                          </span>
                        )}
                      </th>
                      {row.days.map((ms, day) => (
                        <td
                          key={displayEdges[day]}
                          className="px-2 py-3 text-right"
                        >
                          {ms ? (
                            <button
                              type="button"
                              className="rounded px-1 text-fg hover:bg-accent-soft hover:text-accent"
                              onClick={() =>
                                navigate({
                                  name: "timesheet",
                                  scale: "day",
                                  date: localDateString(
                                    new Date(displayEdges[day]),
                                  ),
                                })
                              }
                              title="Review this day"
                            >
                              {formatDuration(ms)}
                            </button>
                          ) : (
                            <span className="text-fg-faint">–</span>
                          )}
                        </td>
                      ))}
                      <td
                        className="px-4 py-3 text-right font-semibold text-fg-strong"
                        title={`${formatDuration(row.approvedMs)} approved · ${formatDuration(row.billableMs)} approved billable`}
                      >
                        {formatDuration(
                          row.days.reduce((sum, ms) => sum + ms, 0),
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
              <tfoot className="border-t border-line bg-surface font-semibold text-fg">
                <tr>
                  <th scope="row" className="px-4 py-3">
                    All projects
                  </th>
                  {totals.map((ms, day) => (
                    <td
                      key={displayEdges[day]}
                      className="px-2 py-3 text-right"
                    >
                      {ms ? formatDuration(ms) : "–"}
                    </td>
                  ))}
                  <td className="px-4 py-3 text-right">
                    {formatDuration(total)}
                  </td>
                </tr>
              </tfoot>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
