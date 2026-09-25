import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AddTimeSheet } from "../components/AddTimeSheet";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EntryReviewSheet } from "../components/EntryReviewSheet";
import {
  BUTTON_PRIMARY,
  BUTTON_SECONDARY,
  DateStepper,
  Dot,
  EmptyState,
  FilterSelect,
  InlineError,
  PageHeader,
  Progress,
  ScaleControl,
  SkeletonRows,
  StatCard,
  Tabs,
} from "../components/Page";
import { useCatalog } from "../hooks/useCatalog";
import { useEntryReview } from "../hooks/useEntryReview";
import { useSettings } from "../hooks/useSettings";
import { useTauriEvent } from "../hooks/useTauriEvent";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { BAND_TONE, band, percent } from "../lib/confidence";
import {
  localDateString,
  parseLocalDate,
  rangeFor,
  rangeLabel,
  stepDate,
} from "../lib/dates";
import {
  blockState,
  durationOf,
  isInFlight,
  isReviewable,
  isTyping,
  splitPoint,
  totalDuration,
  workDuration,
} from "../lib/entries";
import { formatDuration, formatTime, plural } from "../lib/format";
import { formatTargetHours, targetMsFor } from "../lib/settings";
import type {
  CalendarScale,
  Category,
  Project,
  Route,
  TimeEntry,
  TimesheetGroup,
  TimesheetTab,
} from "../lib/types";

type TimesheetRoute = Extract<Route, { name: "timesheet" }>;

interface MyTimesheetProps {
  route: TimesheetRoute;
  navigate: (route: Route) => void;
  replace: (route: Route) => void;
}

const GROUP_OPTIONS: { value: TimesheetGroup; label: string }[] = [
  { value: "project", label: "Project" },
  { value: "category", label: "Category" },
  { value: "app", label: "App" },
  { value: "none", label: "None" },
];

interface Columns {
  project: boolean;
  category: boolean;
  billable: boolean;
}

const COLUMNS_KEY = "openrize.timesheet.columns";
const DEFAULT_COLUMNS: Columns = {
  project: true,
  category: true,
  billable: true,
};

/** Column visibility is a per-viewer convenience, so it lives in localStorage. */
function readColumns(): Columns {
  try {
    const raw = window.localStorage.getItem(COLUMNS_KEY);
    return raw ? { ...DEFAULT_COLUMNS, ...JSON.parse(raw) } : DEFAULT_COLUMNS;
  } catch {
    return DEFAULT_COLUMNS;
  }
}

function writeColumns(columns: Columns): void {
  try {
    window.localStorage.setItem(COLUMNS_KEY, JSON.stringify(columns));
  } catch {
    // Storage can be unavailable; the choice just won't be remembered.
  }
}

interface Group {
  key: string;
  label: string;
  color: string;
  entries: TimeEntry[];
  /** Sorts last: "No project", "Needs you", "Unknown app". */
  trailing: boolean;
}

function inTab(entry: TimeEntry, tab: TimesheetTab): boolean {
  if (tab === "all") return true;
  if (tab === "approved") return entry.status === "approved";
  if (tab === "processing") return isInFlight(entry);
  return isReviewable(entry);
}

function groupEntries(
  entries: TimeEntry[],
  by: TimesheetGroup,
  categoryById: Map<string, Category>,
  projectById: Map<string, Project>,
): Group[] {
  const groups = new Map<string, Group>();
  const add = (
    key: string,
    make: () => Omit<Group, "entries">,
    entry: TimeEntry,
  ) => {
    const group = groups.get(key) ?? { ...make(), entries: [] };
    group.entries.push(entry);
    groups.set(key, group);
  };
  for (const entry of entries) {
    if (by === "none") {
      add(
        "all",
        () => ({
          key: "all",
          label: "All entries",
          color: "",
          trailing: false,
        }),
        entry,
      );
    } else if (by === "project") {
      const project = entry.projectId
        ? projectById.get(entry.projectId)
        : undefined;
      add(
        project?.id ?? "none",
        () => ({
          key: project?.id ?? "none",
          label: project?.name ?? "No project",
          color: project?.color ?? "var(--fg-ghost)",
          trailing: !project,
        }),
        entry,
      );
    } else if (by === "category") {
      const state = blockState(entry);
      const category =
        entry.categoryId && state !== "needsYou"
          ? categoryById.get(entry.categoryId)
          : undefined;
      add(
        category?.id ?? "needs",
        () => ({
          key: category?.id ?? "needs",
          label: category?.name ?? "Needs you",
          color: category?.color ?? "var(--review)",
          trailing: !category,
        }),
        entry,
      );
    } else {
      const app = entry.dominantApp;
      add(
        app ?? "unknown",
        () => ({
          key: app ?? "unknown",
          label: app ?? "No app activity",
          color: "var(--fg-soft)",
          trailing: !app,
        }),
        entry,
      );
    }
  }
  return [...groups.values()].sort((a, b) => {
    if (a.trailing !== b.trailing) return a.trailing ? 1 : -1;
    return totalDuration(b.entries) - totalDuration(a.entries);
  });
}

/** Keeps the list-only fields a single-entry command doesn't return. */
function adopt(list: TimeEntry[], updated: TimeEntry[]): TimeEntry[] {
  const byId = new Map(updated.map((entry) => [entry.id, entry]));
  return list.map((entry) => {
    const next = byId.get(entry.id);
    return next
      ? { ...next, ai: entry.ai, dominantApp: entry.dominantApp }
      : entry;
  });
}

export function MyTimesheet({ route, navigate, replace }: MyTimesheetProps) {
  const { settings } = useSettings();
  const catalog = useCatalog();
  const { categoryById, projectById } = catalog;
  const review = useEntryReview();

  const scale: CalendarScale = route.scale ?? "day";
  const tab: TimesheetTab = route.tab ?? "review";
  const groupBy: TimesheetGroup = route.groupBy ?? "project";
  const date = useMemo(() => parseLocalDate(route.date), [route.date]);
  const range = useMemo(() => rangeFor(scale, date), [scale, date]);
  const startMs = range.start.getTime();
  const endMs = range.end.getTime();

  const [entries, setEntries] = useState<TimeEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [categoryFilter, setCategoryFilter] = useState("");
  const [projectFilter, setProjectFilter] = useState("");
  const [columns, setColumns] = useState<Columns>(readColumns);
  const [adding, setAdding] = useState(false);
  const [confirmApprove, setConfirmApprove] = useState<TimeEntry[] | null>(
    null,
  );
  const [confirmDelete, setConfirmDelete] = useState<TimeEntry | null>(null);

  // Responses for a range the user already left are dropped.
  const shownStart = useRef(startMs);
  shownStart.current = startMs;

  const refresh = useCallback(async (): Promise<void> => {
    try {
      const list = await api.listTimeEntries(startMs, endMs - 1);
      if (shownStart.current === startMs) setEntries(list);
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [startMs, endMs]);

  const load = useCallback(async (): Promise<void> => {
    setLoading(true);
    try {
      const list = await api.rebuildTimeEntries(startMs, endMs - 1);
      if (shownStart.current !== startMs) return;
      setEntries(list);
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setLoading(false);
    }
  }, [startMs, endMs]);

  useEffect(() => {
    load();
    setSelected(new Set());
  }, [load]);

  useTauriEvent(api.ENTRIES_CHANGED, () => void refresh());
  useTauriEvent(api.SUGGESTION_READY, () => void refresh());

  const inRange = useMemo(
    () =>
      entries.filter(
        (entry) => entry.startedAt >= startMs && entry.startedAt < endMs,
      ),
    [entries, startMs, endMs],
  );
  const toReview = useMemo(() => inRange.filter(isReviewable), [inRange]);
  const processing = useMemo(() => inRange.filter(isInFlight), [inRange]);
  const approved = useMemo(
    () => inRange.filter((entry) => entry.status === "approved"),
    [inRange],
  );
  const autoApproved = approved.filter(
    (entry) => entry.approvedBy === "auto" || entry.approvedBy === "rule",
  ).length;
  const totalMs = totalDuration(inRange);
  const approvedMs = totalDuration(approved);
  // Same measure as the Calendar summary: work time (breaks excluded)
  // against the expected hours.
  const workMs = workDuration(inRange, categoryById);
  const targetMs = targetMsFor(
    settings,
    scale,
    Math.round((endMs - startMs) / 86_400_000),
  );

  const rows = useMemo(
    () =>
      inRange.filter(
        (entry) =>
          inTab(entry, tab) &&
          (categoryFilter === "" ||
            (categoryFilter === "none"
              ? !entry.categoryId
              : entry.categoryId === categoryFilter)) &&
          (projectFilter === "" ||
            (projectFilter === "none"
              ? !entry.projectId
              : entry.projectId === projectFilter)),
      ),
    [inRange, tab, categoryFilter, projectFilter],
  );
  const groups = useMemo(
    () => groupEntries(rows, groupBy, categoryById, projectById),
    [rows, groupBy, categoryById, projectById],
  );
  /** Approve all: everything reviewable that has a category to approve. */
  const approvable = useMemo(
    () => toReview.filter((entry) => entry.categoryId),
    [toReview],
  );

  const setRoute = (patch: Partial<TimesheetRoute>): void =>
    replace({ ...route, ...patch });
  const go = (patch: Partial<TimesheetRoute>): void => {
    review.select(undefined);
    navigate({ ...route, ...patch });
  };

  const act = useCallback(
    async (action: () => Promise<TimeEntry[] | TimeEntry | undefined>) => {
      try {
        const result = await action();
        if (result) {
          setEntries((list) =>
            adopt(list, Array.isArray(result) ? result : [result]),
          );
        }
        setActionError(null);
      } catch (cause) {
        setActionError(describeError(cause));
      }
    },
    [],
  );

  const approveIds = (list: TimeEntry[]): void => {
    const ids = list
      .filter((entry) => entry.categoryId && !isInFlight(entry))
      .map((entry) => entry.id);
    if (ids.length === 0) return;
    void act(() => api.approveTimeEntries(ids));
    setSelected(new Set());
  };

  const openApproveAll = useCallback((): void => {
    if (approvable.length > 0) setConfirmApprove(approvable);
  }, [approvable]);

  // ⌘⇧↵ approves everything confirmable, after asking.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (isTyping(event.target)) return;
      if (
        (event.metaKey || event.ctrlKey) &&
        event.shiftKey &&
        event.key === "Enter"
      ) {
        event.preventDefault();
        openApproveAll();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openApproveAll]);

  const selectedEntries = inRange.filter((entry) => selected.has(entry.id));
  const toggleSelected = (id: string): void =>
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const splitEntry = async (entry: TimeEntry): Promise<void> => {
    await act(async () => {
      const detail = await api.getEntryDetail(entry.id);
      await api.splitTimeEntry(entry.id, splitPoint(entry, detail.segments));
      return undefined;
    });
  };

  const tabs = [
    { value: "review" as const, label: "To review", count: toReview.length },
    {
      value: "processing" as const,
      label: "Processing",
      count: processing.length,
    },
    { value: "approved" as const, label: "Approved", count: approved.length },
    { value: "all" as const, label: "All", count: inRange.length },
  ];
  const dayName =
    scale === "day"
      ? date.toLocaleDateString(undefined, { weekday: "short" })
      : scale === "week"
        ? "this week"
        : "this month";

  const categoryOptions = [
    { value: "none", label: "Uncategorized" },
    ...catalog.categories
      .filter((category) => !category.archived)
      .map((category) => ({ value: category.id, label: category.name })),
  ];
  const projectOptions = [
    { value: "none", label: "No project" },
    ...catalog.projects.map((project) => ({
      value: project.id,
      label: project.name,
    })),
  ];

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-canvas text-fg">
      <PageHeader title="My Timesheet" crumb={rangeLabel(scale, date)}>
        <DateStepper
          unit={scale}
          onStep={(direction) =>
            go({ date: localDateString(stepDate(scale, date, direction)) })
          }
          onToday={() => go({ date: localDateString(new Date()) })}
        />
        <ScaleControl
          name="timesheet-scale"
          value={scale}
          onChange={(next) => go({ scale: next })}
        />
      </PageHeader>

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <main className="min-h-0 min-w-0 flex-1 overflow-y-auto px-5 py-4">
          <div className="grid grid-cols-3 gap-3">
            <StatCard
              label="Pending review"
              value={formatDuration(totalDuration(toReview))}
              tone={toReview.length > 0 ? "review" : undefined}
              sub={plural(toReview.length, "entry", "entries")}
            />
            <StatCard
              label="Approved"
              value={formatDuration(approvedMs)}
              sub={`${plural(approved.length, "entry", "entries")}${
                autoApproved > 0 ? ` · ${autoApproved} auto` : ""
              }`}
            />
            <StatCard
              label="Total"
              value={formatDuration(totalMs)}
              sub={`${formatDuration(workMs)} work · ${Math.round((workMs / targetMs) * 100)}% of ${formatTargetHours(targetMs)} target`}
            />
          </div>
          <div className="mt-3 flex items-center gap-3">
            <Progress
              value={totalMs > 0 ? approvedMs / totalMs : 0}
              label="Reviewed share of tracked time"
            />
            <span className="shrink-0 text-[11px] text-fg-soft tabular-nums">
              {totalMs > 0 ? Math.round((approvedMs / totalMs) * 100) : 0}%
              reviewed
            </span>
          </div>

          <div className="mt-5">
            <Tabs
              label="Entry status"
              tabs={tabs}
              value={tab}
              onChange={(next) => setRoute({ tab: next })}
            />
          </div>

          <div className="mt-3 flex flex-wrap items-center gap-2">
            <label className="flex items-center gap-1.5 text-[12px] text-fg-soft">
              Group:
              <select
                value={groupBy}
                onChange={(event) =>
                  setRoute({ groupBy: event.target.value as TimesheetGroup })
                }
                className="h-7 rounded-md border border-line bg-panel px-2 font-medium text-[12px] text-fg outline-hidden focus:border-accent"
              >
                {GROUP_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
            <FilterSelect
              label="Category"
              value={categoryFilter}
              options={categoryOptions}
              onChange={setCategoryFilter}
            />
            <FilterSelect
              label="Project"
              value={projectFilter}
              options={projectOptions}
              onChange={setProjectFilter}
            />
            <ColumnsMenu
              columns={columns}
              onChange={(next) => {
                setColumns(next);
                writeColumns(next);
              }}
            />
            <div className="ml-auto flex items-center gap-2">
              <button
                type="button"
                onClick={() => setAdding(true)}
                className={BUTTON_SECONDARY}
              >
                + Add time
              </button>
              {toReview.length > 0 && (
                <button
                  type="button"
                  onClick={() =>
                    navigate({
                      name: "calendar",
                      scale: scale === "month" ? "week" : scale,
                      date: localDateString(date),
                      review: true,
                    })
                  }
                  className="rounded-md border border-review/30 bg-review/15 px-3 py-1.5 font-semibold text-[12px] text-review hover:bg-review/25"
                >
                  Review {toReview.length}
                </button>
              )}
              <button
                type="button"
                onClick={openApproveAll}
                disabled={approvable.length === 0}
                className={BUTTON_PRIMARY}
                title="Approve all (⌘⇧↵)"
              >
                Approve all <span className="text-[10px] opacity-70">⌘⇧↵</span>
              </button>
            </div>
          </div>

          {selectedEntries.length > 0 && (
            <BulkBar
              entries={selectedEntries}
              categories={catalog.categories}
              onApprove={() => approveIds(selectedEntries)}
              onSetCategory={(categoryId) =>
                void act(() =>
                  api.updateTimeEntries(
                    selectedEntries.map((entry) => entry.id),
                    { categoryId },
                  ),
                )
              }
              onSetBillable={(billable) =>
                void act(() =>
                  api.updateTimeEntries(
                    selectedEntries.map((entry) => entry.id),
                    { billable },
                  ),
                )
              }
              onClear={() => setSelected(new Set())}
            />
          )}

          {(error || actionError) && (
            <div className="mt-3">
              <InlineError
                message={error ?? actionError ?? ""}
                onRetry={error ? load : undefined}
              />
            </div>
          )}

          <div className="mt-3 overflow-hidden rounded-xl border border-line bg-panel">
            <div
              className="grid items-center gap-3 border-line border-b bg-surface px-3 py-2 font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider"
              style={{ gridTemplateColumns: gridColumns(columns) }}
            >
              <input
                type="checkbox"
                aria-label="Select all shown"
                checked={
                  rows.length > 0 &&
                  rows.every((entry) => selected.has(entry.id))
                }
                onChange={(event) =>
                  setSelected(
                    event.target.checked
                      ? new Set(rows.map((entry) => entry.id))
                      : new Set(),
                  )
                }
                className="accent-(--accent)"
              />
              <span>Time</span>
              <span>Description</span>
              {columns.project && <span>Project</span>}
              {columns.category && <span>Category</span>}
              <span className="text-right">Dur.</span>
              {columns.billable && <span className="text-center">$</span>}
              <span />
            </div>

            {loading && entries.length === 0 ? (
              <SkeletonRows />
            ) : rows.length === 0 ? (
              <EmptyState
                title={
                  tab === "review"
                    ? `All caught up for ${dayName}`
                    : tab === "processing"
                      ? "Nothing is being categorized"
                      : tab === "approved"
                        ? "Nothing approved yet"
                        : "No entries in this range"
                }
                hint={
                  tab === "review" && processing.length > 0
                    ? `${plural(processing.length, "entry", "entries")} still categorizing.`
                    : tab === "all"
                      ? "Tracked time turns into entries here as you work. Add time for anything the tracker missed."
                      : undefined
                }
              />
            ) : (
              groups.map((group) => {
                const open = !collapsed.has(group.key);
                return (
                  <div key={group.key}>
                    {groupBy !== "none" && (
                      <button
                        type="button"
                        onClick={() =>
                          setCollapsed((current) => {
                            const next = new Set(current);
                            if (next.has(group.key)) next.delete(group.key);
                            else next.add(group.key);
                            return next;
                          })
                        }
                        aria-expanded={open}
                        className="flex w-full items-center gap-2 border-line-soft border-b bg-inset-soft px-3 py-1.5 text-left font-semibold text-[12px] text-fg-muted hover:bg-surface"
                      >
                        <span className="w-3 text-fg-faint">
                          {open ? "▾" : "▸"}
                        </span>
                        {group.color && <Dot color={group.color} />}
                        <span className="text-fg-strong">{group.label}</span>
                        <span className="font-normal text-fg-soft">
                          · {formatDuration(totalDuration(group.entries))}
                        </span>
                        <span className="ml-auto font-normal text-[11px] text-fg-faint">
                          {plural(group.entries.length, "entry", "entries")}
                        </span>
                      </button>
                    )}
                    {open &&
                      group.entries.map((entry) => (
                        <TimesheetRow
                          key={entry.id}
                          entry={entry}
                          columns={columns}
                          checked={selected.has(entry.id)}
                          active={review.selectedId === entry.id}
                          categories={catalog.categories}
                          projects={catalog.projects}
                          categoryById={categoryById}
                          projectById={projectById}
                          showDate={scale !== "day"}
                          onCheck={() => toggleSelected(entry.id)}
                          onOpen={() => review.select(entry.id)}
                          onUpdate={(patch) =>
                            void act(() => api.updateTimeEntry(entry.id, patch))
                          }
                          onAccept={() => approveIds([entry])}
                          onOpenInCalendar={() =>
                            navigate({
                              name: "calendar",
                              scale: "day",
                              date: localDateString(new Date(entry.startedAt)),
                              entryId: entry.id,
                            })
                          }
                          onSplit={() => void splitEntry(entry)}
                          onDelete={() => setConfirmDelete(entry)}
                        />
                      ))}
                  </div>
                );
              })
            )}
          </div>
        </main>

        {review.detail && (
          <aside className="flex w-[320px] shrink-0 flex-col overflow-hidden border-line border-l bg-panel">
            <EntryReviewSheet
              review={review}
              categories={catalog.categories}
              projects={catalog.projects}
            />
          </aside>
        )}
      </div>

      {adding && (
        <AddTimeSheet
          date={date}
          categories={catalog.categories}
          projects={catalog.projects}
          onClose={() => setAdding(false)}
          onCreated={refresh}
        />
      )}
      {confirmApprove && (
        <ConfirmDialog
          title={`Approve ${plural(confirmApprove.length, "entry", "entries")}, ${formatDuration(totalDuration(confirmApprove))}?`}
          body={
            toReview.length > confirmApprove.length
              ? `${plural(toReview.length - confirmApprove.length, "entry", "entries")} marked "Needs you" stay pending until you choose a category.`
              : "Each entry keeps the category and project it has now."
          }
          confirmLabel="Approve"
          tone="affirmative"
          onCancel={() => setConfirmApprove(null)}
          onConfirm={() => {
            approveIds(confirmApprove);
            setConfirmApprove(null);
          }}
        />
      )}
      {confirmDelete && (
        <ConfirmDialog
          title="Delete this entry?"
          body={`"${confirmDelete.description}" (${formatDuration(durationOf(confirmDelete))}) is removed. Its activity stays in the log, unassigned.`}
          confirmLabel="Delete"
          onCancel={() => setConfirmDelete(null)}
          onConfirm={() => {
            const id = confirmDelete.id;
            setConfirmDelete(null);
            if (review.selectedId === id) review.select(undefined);
            void act(async () => {
              await api.deleteTimeEntry(id);
              return undefined;
            });
          }}
        />
      )}
    </div>
  );
}

function gridColumns(columns: Columns): string {
  return [
    "18px",
    "64px",
    "minmax(0,1fr)",
    columns.project ? "minmax(0,150px)" : "",
    columns.category ? "minmax(0,150px)" : "",
    "56px",
    columns.billable ? "28px" : "",
    "108px",
  ]
    .filter(Boolean)
    .join(" ");
}

function ColumnsMenu({
  columns,
  onChange,
}: {
  columns: Columns;
  onChange: (columns: Columns) => void;
}) {
  const options: { key: keyof Columns; label: string }[] = [
    { key: "project", label: "Project" },
    { key: "category", label: "Category" },
    { key: "billable", label: "Billable" },
  ];
  return (
    <details className="relative">
      <summary className="flex h-7 cursor-pointer list-none items-center rounded-md border border-line bg-panel px-2 font-medium text-[12px] text-fg-soft hover:text-fg">
        Columns ▾
      </summary>
      <div className="absolute z-30 mt-1 w-40 space-y-1 rounded-lg border border-line bg-panel p-2 shadow-xl">
        {options.map((option) => (
          <label
            key={option.key}
            className="flex items-center gap-2 rounded px-1 py-0.5 text-[12px] text-fg-muted hover:bg-surface"
          >
            <input
              type="checkbox"
              checked={columns[option.key]}
              onChange={(event) =>
                onChange({ ...columns, [option.key]: event.target.checked })
              }
              className="accent-(--accent)"
            />
            {option.label}
          </label>
        ))}
      </div>
    </details>
  );
}

function BulkBar({
  entries,
  categories,
  onApprove,
  onSetCategory,
  onSetBillable,
  onClear,
}: {
  entries: TimeEntry[];
  categories: Category[];
  onApprove: () => void;
  onSetCategory: (categoryId: string) => void;
  onSetBillable: (billable: boolean) => void;
  onClear: () => void;
}) {
  const approvable = entries.filter(
    (entry) =>
      entry.categoryId && !isInFlight(entry) && entry.status !== "approved",
  );
  return (
    <div className="mt-3 flex flex-wrap items-center gap-2 rounded-lg border border-accent/30 bg-accent-soft px-3 py-2 text-[12px]">
      <span className="font-semibold text-fg-strong">
        {entries.length} selected · {formatDuration(totalDuration(entries))}
      </span>
      <button
        type="button"
        onClick={onApprove}
        disabled={approvable.length === 0}
        className={BUTTON_PRIMARY}
        title={
          approvable.length < entries.length
            ? "Entries without a category, or still categorizing, are skipped"
            : undefined
        }
      >
        Approve {approvable.length}
      </button>
      <select
        aria-label="Set category"
        value=""
        onChange={(event) => onSetCategory(event.target.value)}
        className="h-7 rounded-md border border-line bg-panel px-2 text-[12px] text-fg outline-hidden focus:border-accent"
      >
        <option value="" disabled>
          Set category…
        </option>
        {categories
          .filter((category) => !category.archived)
          .map((category) => (
            <option key={category.id} value={category.id}>
              {category.name}
            </option>
          ))}
      </select>
      <button
        type="button"
        onClick={() => onSetBillable(true)}
        className={BUTTON_SECONDARY}
      >
        Billable
      </button>
      <button
        type="button"
        onClick={() => onSetBillable(false)}
        className={BUTTON_SECONDARY}
      >
        Not billable
      </button>
      <button
        type="button"
        onClick={onClear}
        className="ml-auto text-fg-soft hover:text-fg"
      >
        Clear
      </button>
    </div>
  );
}

interface TimesheetRowProps {
  entry: TimeEntry;
  columns: Columns;
  checked: boolean;
  active: boolean;
  categories: Category[];
  projects: Project[];
  categoryById: Map<string, Category>;
  projectById: Map<string, Project>;
  showDate: boolean;
  onCheck: () => void;
  onOpen: () => void;
  onUpdate: (patch: {
    description?: string;
    categoryId?: string;
    projectId?: string;
    billable?: boolean;
  }) => void;
  onAccept: () => void;
  onOpenInCalendar: () => void;
  onSplit: () => void;
  onDelete: () => void;
}

function Confidence({ value }: { value?: number }) {
  if (value === undefined) return null;
  return (
    <span
      className={`shrink-0 font-mono text-[10.5px] tabular-nums ${BAND_TONE[band(value)]}`}
    >
      {percent(value)}
    </span>
  );
}

/** A chip-styled select: the value's colour dot, name, and confidence. */
function ChipSelect({
  label,
  value,
  color,
  placeholder,
  confidence,
  disabled,
  options,
  onChange,
}: {
  label: string;
  value: string;
  color?: string;
  placeholder: string;
  confidence?: number;
  disabled?: boolean;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
}) {
  return (
    <span className="flex min-w-0 items-center gap-1.5">
      <span className="relative flex min-w-0 items-center">
        {color && (
          <span className="pointer-events-none absolute left-1.5">
            <Dot color={color} size={7} />
          </span>
        )}
        <select
          aria-label={label}
          value={value}
          disabled={disabled}
          onChange={(event) => onChange(event.target.value)}
          onClick={(event) => event.stopPropagation()}
          className={`h-6 min-w-0 max-w-full cursor-pointer appearance-none truncate rounded-md border py-0 pr-5 text-[11.5px] outline-hidden focus:border-accent disabled:cursor-default ${
            color ? "pl-4.5" : "pl-1.5"
          } ${
            value === ""
              ? "border-dashed border-review/50 bg-transparent text-review"
              : "border-line-soft bg-transparent text-fg-muted hover:border-line"
          }`}
        >
          {value === "" && <option value="">{placeholder}</option>}
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <svg
          viewBox="0 0 24 24"
          aria-hidden="true"
          fill="none"
          stroke="currentColor"
          strokeWidth={2.2}
          strokeLinecap="round"
          strokeLinejoin="round"
          className="pointer-events-none absolute right-1.5 size-2.5 text-fg-faint"
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </span>
      <Confidence value={confidence} />
    </span>
  );
}

function TimesheetRow({
  entry,
  columns,
  checked,
  active,
  categories,
  projects,
  categoryById,
  projectById,
  showDate,
  onCheck,
  onOpen,
  onUpdate,
  onAccept,
  onOpenInCalendar,
  onSplit,
  onDelete,
}: TimesheetRowProps) {
  const state = blockState(entry);
  const inFlight = isInFlight(entry);
  const isApproved = entry.status === "approved";
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(entry.description);
  const category = entry.categoryId
    ? categoryById.get(entry.categoryId)
    : undefined;
  const project = entry.projectId
    ? projectById.get(entry.projectId)
    : undefined;
  const unconfirmed = !isApproved;

  const saveDescription = (): void => {
    setEditing(false);
    const text = draft.trim();
    if (text && text !== entry.description) onUpdate({ description: text });
  };

  const categoryOptions = categories
    .filter((c) => !c.archived || c.id === entry.categoryId)
    .map((c) => ({ value: c.id, label: c.name }));
  const projectOptions = [
    { value: "", label: "No project" },
    ...projects
      .filter((p) => p.status === "active" || p.id === entry.projectId)
      .map((p) => ({ value: p.id, label: p.name })),
  ];

  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: the row's controls are all keyboard reachable; clicking empty row space is a mouse shortcut for opening the panel, which J/K and the ⋯ menu also reach.
    // biome-ignore lint/a11y/noStaticElementInteractions: see above.
    <div
      onClick={onOpen}
      className={`grid cursor-pointer items-center gap-3 border-line-soft border-b px-3 py-1.5 text-[12px] transition-colors last:border-b-0 ${
        active ? "bg-accent/8" : "hover:bg-surface"
      }`}
      style={{ gridTemplateColumns: gridColumns(columns) }}
    >
      <input
        type="checkbox"
        aria-label={`Select ${entry.description}`}
        checked={checked}
        onChange={onCheck}
        onClick={(event) => event.stopPropagation()}
        className="accent-(--accent)"
      />
      <span className="font-mono text-[11px] text-fg-soft tabular-nums leading-tight">
        {showDate && (
          <span className="block text-[10px] text-fg-faint">
            {new Date(entry.startedAt).toLocaleDateString(undefined, {
              weekday: "short",
              day: "numeric",
            })}
          </span>
        )}
        {formatTime(entry.startedAt)}
      </span>
      <span className="flex min-w-0 items-center gap-1.5">
        {inFlight && (
          <span
            className="size-3 shrink-0 animate-spin rounded-full border-2 border-fg-ghost border-t-fg-soft"
            aria-hidden="true"
          />
        )}
        {editing ? (
          <input
            // biome-ignore lint/a11y/noAutofocus: opened by a click to type into.
            autoFocus
            value={draft}
            aria-label="Description"
            onChange={(event) => setDraft(event.target.value)}
            onClick={(event) => event.stopPropagation()}
            onBlur={saveDescription}
            onKeyDown={(event) => {
              if (event.key === "Enter") saveDescription();
              if (event.key === "Escape") {
                event.stopPropagation();
                setDraft(entry.description);
                setEditing(false);
              }
            }}
            className="min-w-0 flex-1 rounded border border-accent bg-canvas px-1.5 py-0.5 text-[12px] text-fg outline-hidden"
          />
        ) : (
          <button
            type="button"
            onClick={(event) => {
              event.stopPropagation();
              setDraft(entry.description);
              setEditing(true);
            }}
            title="Edit description"
            className={`min-w-0 truncate text-left hover:text-accent ${
              inFlight ? "text-fg-soft" : "text-fg-strong"
            }`}
          >
            {state === "processing"
              ? "Categorizing…"
              : state === "building"
                ? `${entry.description} · recording`
                : entry.description}
          </button>
        )}
      </span>
      {columns.project && (
        <ChipSelect
          label="Project"
          value={entry.projectId ?? ""}
          color={project?.color}
          placeholder="No project"
          confidence={unconfirmed ? entry.ai?.projectConfidence : undefined}
          options={projectOptions}
          onChange={(projectId) => onUpdate({ projectId })}
        />
      )}
      {columns.category && (
        <ChipSelect
          label="Category"
          value={entry.categoryId ?? ""}
          color={category?.color}
          placeholder="Choose…"
          confidence={unconfirmed ? entry.ai?.categoryConfidence : undefined}
          options={categoryOptions}
          onChange={(categoryId) => onUpdate({ categoryId })}
        />
      )}
      <span className="text-right font-mono text-[11.5px] text-fg-muted tabular-nums">
        {formatDuration(durationOf(entry))}
      </span>
      {columns.billable && (
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onUpdate({ billable: !entry.billable });
          }}
          aria-pressed={entry.billable}
          title={entry.billable ? "Billable" : "Not billable"}
          className={`mx-auto flex size-6 items-center justify-center rounded font-semibold text-[11px] transition-colors ${
            entry.billable
              ? "bg-accent/20 text-accent"
              : "text-fg-ghost hover:bg-surface hover:text-fg-soft"
          }`}
        >
          {entry.billable ? "$" : "–"}
        </button>
      )}
      <span className="flex items-center justify-end gap-1">
        {isApproved ? (
          <span
            className="text-[11px] text-accent"
            title={`Approved by ${entry.approvedBy ?? "you"}`}
          >
            {entry.approvedBy === "auto" || entry.approvedBy === "rule"
              ? `✓ ${entry.approvedBy}`
              : "✓ Approved"}
          </span>
        ) : (
          <button
            type="button"
            disabled={inFlight || !entry.categoryId}
            onClick={(event) => {
              event.stopPropagation();
              onAccept();
            }}
            title={
              inFlight
                ? "Still categorizing"
                : entry.categoryId
                  ? "Accept"
                  : "Choose a category first"
            }
            className="rounded-md border border-accent/40 px-2 py-0.5 font-semibold text-[11px] text-accent transition-colors hover:bg-accent-soft disabled:cursor-not-allowed disabled:border-line disabled:text-fg-ghost disabled:hover:bg-transparent"
          >
            ✓ Accept
          </button>
        )}
        <RowMenu
          onOpenInCalendar={onOpenInCalendar}
          onSplit={onSplit}
          onDelete={onDelete}
          canSplit={!inFlight}
        />
      </span>
    </div>
  );
}

function RowMenu({
  onOpenInCalendar,
  onSplit,
  onDelete,
  canSplit,
}: {
  onOpenInCalendar: () => void;
  onSplit: () => void;
  onDelete: () => void;
  canSplit: boolean;
}) {
  const item =
    "block w-full rounded px-2 py-1 text-left text-[12px] hover:bg-surface disabled:opacity-40";
  const close = (element: HTMLElement): void => {
    element.closest("details")?.removeAttribute("open");
  };
  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: only stops the row's click-to-open; the menu's own controls handle keys.
    <details className="relative" onClick={(event) => event.stopPropagation()}>
      <summary
        aria-label="More actions"
        className="flex size-6 cursor-pointer list-none items-center justify-center rounded text-fg-soft hover:bg-surface-strong hover:text-fg"
      >
        ⋯
      </summary>
      <div className="absolute right-0 z-30 mt-1 w-40 rounded-lg border border-line bg-panel p-1 shadow-xl">
        <button
          type="button"
          className={`${item} text-fg-muted`}
          onClick={(event) => {
            close(event.currentTarget);
            onOpenInCalendar();
          }}
        >
          Open in Calendar
        </button>
        <button
          type="button"
          disabled={!canSplit}
          className={`${item} text-fg-muted`}
          onClick={(event) => {
            close(event.currentTarget);
            onSplit();
          }}
        >
          Split
        </button>
        <button
          type="button"
          className={`${item} text-danger`}
          onClick={(event) => {
            close(event.currentTarget);
            onDelete();
          }}
        >
          Delete
        </button>
      </div>
    </details>
  );
}
