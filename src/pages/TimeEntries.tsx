import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Donut, type Slice, StackedColumns } from "../components/Charts";
import {
  BUTTON_SECONDARY,
  DateStepper,
  Dot,
  EmptyState,
  FilterSelect,
  InlineError,
  PageHeader,
  SkeletonRows,
  StatCard,
  Tabs,
} from "../components/Page";
import { Picker } from "../components/Picker";
import { type Catalog, useCatalog } from "../hooks/useCatalog";
import { useTauriEvent } from "../hooks/useTauriEvent";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import {
  addDays,
  addMonths,
  bucketLabel,
  type DateRange,
  dayEdges,
  localDateString,
  parseLocalDate,
  rangeFor,
  rangeLabel,
  stackEdges,
  startOfDay,
} from "../lib/dates";
import { blockState, durationOf } from "../lib/entries";
import {
  formatDuration,
  formatHours,
  formatShortDate,
  formatTime,
  plural,
} from "../lib/format";
import { assignColors } from "../lib/palette";
import type {
  EntriesGroup,
  EntriesRange,
  EntriesStack,
  EntriesView,
  EntryFilters,
  EntryQuery,
  RollupCell,
  Route,
  TimeEntry,
} from "../lib/types";

type EntriesRoute = Extract<Route, { name: "entries" }>;

interface TimeEntriesProps {
  route: EntriesRoute;
  navigate: (route: Route) => void;
  replace: (route: Route) => void;
}

const RANGE_OPTIONS: { value: EntriesRange; label: string }[] = [
  { value: "day", label: "Day" },
  { value: "week", label: "Week" },
  { value: "month", label: "Month" },
  { value: "30d", label: "Last 30 days" },
  { value: "year", label: "Year" },
];

const GROUP_OPTIONS: { value: EntriesGroup; label: string }[] = [
  { value: "category", label: "Category" },
  { value: "project", label: "Project" },
  { value: "client", label: "Client" },
  { value: "app", label: "App" },
  { value: "status", label: "Status" },
];

const STACK_OPTIONS: { value: EntriesStack; label: string }[] = [
  { value: "day", label: "Day" },
  { value: "week", label: "Week" },
  { value: "month", label: "Month" },
];

/** What the pivot's cells count; the stat cards switch it. */
type Metric = "time" | "entries" | "approved";

function entriesRange(range: EntriesRange, date: Date): DateRange {
  if (range === "30d") {
    const end = addDays(startOfDay(date), 1);
    return { start: addDays(end, -30), end };
  }
  if (range === "year") {
    const start = new Date(date.getFullYear(), 0, 1);
    return { start, end: new Date(date.getFullYear() + 1, 0, 1) };
  }
  return rangeFor(range, date);
}

function stepRange(range: EntriesRange, date: Date, direction: 1 | -1): Date {
  if (range === "day") return addDays(date, direction);
  if (range === "week") return addDays(date, 7 * direction);
  if (range === "30d") return addDays(date, 30 * direction);
  if (range === "month") return addMonths(date, direction);
  return new Date(date.getFullYear() + direction, date.getMonth(), 1);
}

function rangeTitle(range: EntriesRange, date: Date, span: DateRange): string {
  if (range === "day" || range === "week" || range === "month") {
    return rangeLabel(range, date);
  }
  if (range === "year") return String(date.getFullYear());
  return `${formatShortDate(span.start.getTime())} – ${formatShortDate(
    addDays(span.end, -1).getTime(),
  )}`;
}

// --- Saved views (a named Route, per viewer) --------------------------------

interface SavedView {
  name: string;
  route: EntriesRoute;
}

const VIEWS_KEY = "openrize.entries.views";

function readViews(): SavedView[] {
  try {
    const raw = window.localStorage.getItem(VIEWS_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed)
      ? parsed.filter(
          (view): view is SavedView =>
            typeof view?.name === "string" && view?.route?.name === "entries",
        )
      : [];
  } catch {
    return [];
  }
}

function writeViews(views: SavedView[]): void {
  try {
    window.localStorage.setItem(VIEWS_KEY, JSON.stringify(views));
  } catch {
    // Storage can be unavailable; the view just won't persist.
  }
}

// --- Group labels -------------------------------------------------------------

interface GroupLabel {
  label: string;
  color: string;
}

/**
 * Name and colour for a group key. Categories and projects carry their own
 * colour; apps and clients take theirs from `colors` (see `assignColors`).
 */
function groupLabel(
  by: EntriesGroup,
  key: string | null | undefined,
  catalog: Catalog,
  colors?: Map<string, string>,
): GroupLabel {
  const assigned = (key && colors?.get(key)) || "var(--fg-soft)";
  if (by === "category") {
    const category = key ? catalog.categoryById.get(key) : undefined;
    return {
      label: category?.name ?? "Uncategorized",
      color: category?.color ?? "var(--fg-ghost)",
    };
  }
  if (by === "project") {
    const project = key ? catalog.projectById.get(key) : undefined;
    return {
      label: project?.name ?? "No project",
      color: project?.color ?? "var(--fg-ghost)",
    };
  }
  if (by === "client") {
    return {
      label: (key && catalog.clientById.get(key)?.name) || "No client",
      color: key ? assigned : "var(--fg-ghost)",
    };
  }
  if (by === "status") {
    return key === "approved"
      ? { label: "Approved", color: "var(--accent)" }
      : { label: "Pending", color: "var(--review)" };
  }
  return key
    ? { label: key, color: assigned }
    : { label: "No app activity", color: "var(--fg-ghost)" };
}

/** Adds the constraint a pivot row stands for to the page's filter. */
function withGroup(
  query: EntryQuery,
  by: EntriesGroup,
  key: string | null | undefined,
): EntryQuery {
  const value = key ?? "none";
  if (by === "category") return { ...query, categoryId: value };
  if (by === "project") return { ...query, projectId: value };
  if (by === "client") return { ...query, clientId: value };
  if (by === "status")
    return { ...query, status: value as "pending" | "approved" };
  return { ...query, app: key ?? undefined };
}

const hasFilters = (filters: EntryFilters): boolean =>
  Object.values(filters).some((value) => value !== undefined && value !== "");

export function TimeEntries({ route, navigate, replace }: TimeEntriesProps) {
  const catalog = useCatalog();

  const rangeKind: EntriesRange = route.range ?? "week";
  const view: EntriesView = route.view ?? "table";
  const groupBy: EntriesGroup = route.groupBy ?? "category";
  const stackBy: EntriesStack =
    route.stackBy ?? (rangeKind === "year" ? "month" : "day");
  const filters = useMemo(() => route.filters ?? {}, [route.filters]);
  const date = useMemo(() => parseLocalDate(route.date), [route.date]);
  const span = useMemo(() => entriesRange(rangeKind, date), [rangeKind, date]);

  const query = useMemo<EntryQuery>(
    () => ({
      ...filters,
      startMs: span.start.getTime(),
      endMs: span.end.getTime(),
    }),
    [filters, span],
  );

  const [metric, setMetric] = useState<Metric>("time");
  const [unit, setUnit] = useState<"h" | "%">("h");
  const [totals, setTotals] = useState<RollupCell | null>(null);
  const [activeDays, setActiveDays] = useState(0);
  const [appKeys, setAppKeys] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [notice, setNotice] = useState<{ text: string; path?: string } | null>(
    null,
  );
  const [views, setViews] = useState<SavedView[]>(readViews);

  const setRoute = useCallback(
    (patch: Partial<EntriesRoute>): void => replace({ ...route, ...patch }),
    [replace, route],
  );
  const setFilter = (patch: Partial<EntryFilters>): void => {
    const next: EntryFilters = { ...filters, ...patch };
    for (const key of Object.keys(next) as (keyof EntryFilters)[]) {
      if (next[key] === undefined || next[key] === "") delete next[key];
    }
    setRoute({ filters: next });
  };

  // Search types into a draft and reaches the route after a pause.
  const [searchDraft, setSearchDraft] = useState(filters.search ?? "");
  useEffect(() => setSearchDraft(filters.search ?? ""), [filters.search]);
  const filterRef = useRef(setFilter);
  filterRef.current = setFilter;
  useEffect(() => {
    if (searchDraft === (filters.search ?? "")) return;
    const timer = window.setTimeout(
      () => filterRef.current({ search: searchDraft.trim() || undefined }),
      250,
    );
    return () => window.clearTimeout(timer);
  }, [searchDraft, filters.search]);

  const [version, setVersion] = useState(0);
  const loadTotals = useCallback(async (): Promise<void> => {
    try {
      const { app: _app, ...withoutApp } = query;
      const [whole, days, apps] = await Promise.all([
        api.entryRollup(query, [query.startMs, query.endMs], "none"),
        api.entryRollup(query, dayEdges(span.start, span.end), "none"),
        // The App filter's choices: every app in range under the other filters.
        api.entryRollup(withoutApp, [query.startMs, query.endMs], "app"),
      ]);
      setTotals(whole[0] ?? null);
      setActiveDays(days.filter((cell) => cell.ms > 0).length);
      setAppKeys(
        apps
          .filter((cell) => cell.key)
          .sort((a, b) => b.ms - a.ms)
          .map((cell) => cell.key as string),
      );
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setLoading(false);
    }
  }, [query, span]);

  useEffect(() => {
    setLoading(true);
    loadTotals();
  }, [loadTotals]);

  // Every mutation elsewhere lands here; refetch shortly after the burst.
  const burst = useRef<number | undefined>(undefined);
  const bump = (): void => {
    window.clearTimeout(burst.current);
    burst.current = window.setTimeout(() => {
      loadTotals();
      setVersion((current) => current + 1);
    }, 300);
  };
  useEffect(() => () => window.clearTimeout(burst.current), []);
  useTauriEvent(api.ENTRIES_CHANGED, bump);
  useTauriEvent(api.SUGGESTION_READY, bump);

  const exportAs = async (format: "csv" | "json"): Promise<void> => {
    try {
      const result = await api.exportTimeEntries(query, format);
      setNotice({
        text: `Exported ${plural(result.count, "entry", "entries")} to ${result.path}`,
        path: result.path,
      });
    } catch (cause) {
      setNotice({ text: `Export failed: ${describeError(cause)}` });
    }
  };

  const saveView = (name: string): void => {
    const next = [
      ...views.filter((saved) => saved.name !== name),
      { name, route },
    ];
    setViews(next);
    writeViews(next);
  };
  const deleteView = (name: string): void => {
    const next = views.filter((saved) => saved.name !== name);
    setViews(next);
    writeViews(next);
  };

  const totalMs = totals?.ms ?? 0;
  const entryCount = totals?.entries ?? 0;
  const approvedShare = totalMs > 0 ? (totals?.approvedMs ?? 0) / totalMs : 0;
  const filtered = hasFilters(filters);

  const option = <T extends string>(value: T, label: string) => ({
    value,
    label,
  });
  const categoryOptions = [
    option("none", "Uncategorized"),
    ...catalog.categories.map((c) => option(c.id, c.name)),
  ];
  const projectOptions = [
    option("none", "No project"),
    ...catalog.projects.map((p) => option(p.id, p.name)),
  ];
  const clientOptions = [
    option("none", "No client"),
    ...catalog.clients.map((c) => option(c.id, c.name)),
  ];
  const appOptions = appKeys.map((key) => option(key, key));
  if (filters.app && !appKeys.includes(filters.app)) {
    appOptions.unshift(option(filters.app, filters.app));
  }

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-canvas text-fg">
      <PageHeader
        title="Time Entries"
        crumb={rangeTitle(rangeKind, date, span)}
      >
        <DateStepper
          unit={rangeKind === "30d" ? "30 days" : rangeKind}
          onStep={(direction) =>
            navigate({
              ...route,
              date: localDateString(stepRange(rangeKind, date, direction)),
            })
          }
          onToday={() =>
            navigate({ ...route, date: localDateString(new Date()) })
          }
        />
        <Picker
          ariaLabel="Date range"
          value={rangeKind}
          onChange={(val) => navigate({ ...route, range: val as EntriesRange })}
          options={RANGE_OPTIONS}
          variant="compact"
        />
      </PageHeader>

      <div className="shrink-0 space-y-2 border-line border-b px-5 py-2.5">
        <div className="flex flex-wrap items-center gap-2">
          <FilterSelect
            label="Category"
            value={filters.categoryId ?? ""}
            options={categoryOptions}
            onChange={(value) => setFilter({ categoryId: value || undefined })}
          />
          {catalog.projects.length > 0 && (
            <FilterSelect
              label="Project"
              value={filters.projectId ?? ""}
              options={projectOptions}
              onChange={(value) => setFilter({ projectId: value || undefined })}
            />
          )}
          <FilterSelect
            label="Client"
            value={filters.clientId ?? ""}
            options={clientOptions}
            onChange={(value) => setFilter({ clientId: value || undefined })}
          />
          <FilterSelect
            label="App"
            value={filters.app ?? ""}
            options={appOptions}
            onChange={(value) => setFilter({ app: value || undefined })}
          />
          <FilterSelect
            label="Status"
            value={filters.status ?? ""}
            options={[
              option("pending", "Pending"),
              option("approved", "Approved"),
            ]}
            onChange={(value) =>
              setFilter({
                status: (value || undefined) as EntryFilters["status"],
              })
            }
          />
          <FilterSelect
            label="Billable"
            value={
              filters.billable === undefined
                ? ""
                : filters.billable
                  ? "yes"
                  : "no"
            }
            options={[option("yes", "Billable"), option("no", "Not billable")]}
            onChange={(value) =>
              setFilter({
                billable: value === "" ? undefined : value === "yes",
              })
            }
          />
          {filtered && (
            <button
              type="button"
              onClick={() => setRoute({ filters: {} })}
              className="text-[12px] text-fg-soft hover:text-fg"
            >
              Clear filters
            </button>
          )}
        </div>
        <div className="flex items-center gap-2">
          <div className="relative min-w-0 flex-1">
            <input
              type="search"
              spellCheck={false}
              autoCorrect="off"
              autoCapitalize="off"
              value={searchDraft}
              onChange={(event) => setSearchDraft(event.target.value)}
              aria-label="Search descriptions and window titles"
              placeholder="Search descriptions and window titles"
              className="h-7 w-full rounded-md border border-line bg-panel pr-2 pl-7 text-[12px] text-fg outline-hidden placeholder:text-fg-faint focus:border-accent"
            />
            <span
              className="pointer-events-none absolute top-1.5 left-2.5 text-[12px] text-fg-faint"
              aria-hidden="true"
            >
              ⌕
            </span>
          </div>
          <ViewsMenu
            views={views}
            onOpen={(saved) => navigate(saved.route)}
            onSave={saveView}
            onDelete={deleteView}
          />
          <Menu label="Export ▾">
            {(close) => (
              <>
                <MenuItem
                  onClick={() => {
                    close();
                    void exportAs("csv");
                  }}
                >
                  CSV
                </MenuItem>
                <MenuItem
                  onClick={() => {
                    close();
                    void exportAs("json");
                  }}
                >
                  JSON
                </MenuItem>
              </>
            )}
          </Menu>
        </div>
      </div>

      {notice && (
        <div className="flex shrink-0 items-center gap-3 border-line border-b bg-accent-soft px-5 py-2 text-[12px]">
          <span className="min-w-0 flex-1 truncate text-fg">{notice.text}</span>
          {notice.path && (
            <button
              type="button"
              onClick={() => notice.path && void revealItemInDir(notice.path)}
              className="shrink-0 font-semibold text-accent hover:underline"
            >
              Show in Finder
            </button>
          )}
          <button
            type="button"
            onClick={() => setNotice(null)}
            aria-label="Dismiss"
            className="shrink-0 text-fg-soft hover:text-fg"
          >
            ✕
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto px-5 py-4">
          {error && (
            <div className="mb-3">
              <InlineError message={error} onRetry={loadTotals} />
            </div>
          )}
          <div className="grid shrink-0 grid-cols-4 gap-3">
            <StatCard
              label="Time"
              value={formatDuration(totalMs)}
              active={metric === "time"}
              onClick={() => setMetric("time")}
            />
            <StatCard
              label="Entries"
              value={String(entryCount)}
              active={metric === "entries"}
              onClick={() => setMetric("entries")}
            />
            <StatCard
              label="Approved"
              value={`${Math.round(approvedShare * 100)}%`}
              active={metric === "approved"}
              onClick={() => setMetric("approved")}
            />
            <StatCard
              label="Daily avg"
              value={formatDuration(activeDays > 0 ? totalMs / activeDays : 0)}
              sub={`over ${plural(activeDays, "active day")}`}
            />
          </div>

          <div className="mt-5 flex shrink-0 flex-wrap items-end justify-between gap-3">
            <Tabs
              label="View"
              tabs={[
                { value: "table" as const, label: "Table" },
                { value: "charts" as const, label: "Charts" },
                { value: "log" as const, label: "Log" },
              ]}
              value={view}
              onChange={(next) => setRoute({ view: next })}
            />
            {/* Hidden rather than removed on Log, so the tabs keep their place. */}
            <div
              aria-hidden={view === "log"}
              className={`flex items-center gap-2 pb-1.5 text-[12px] text-fg-soft ${
                view === "log" ? "invisible" : ""
              }`}
            >
              {view === "table" && (
                <InlineSelect
                  label="Group"
                  value={groupBy}
                  options={GROUP_OPTIONS}
                  onChange={(next) => setRoute({ groupBy: next })}
                />
              )}
              <InlineSelect
                label="Stack"
                value={stackBy}
                options={STACK_OPTIONS}
                onChange={(next) => setRoute({ stackBy: next })}
              />
              {view === "table" && metric === "time" && (
                <span className="inline-flex overflow-hidden rounded-md border border-line">
                  {(["h", "%"] as const).map((value) => (
                    <button
                      key={value}
                      type="button"
                      aria-pressed={unit === value}
                      onClick={() => setUnit(value)}
                      className={`px-2 py-0.5 font-mono text-[11.5px] ${
                        unit === value
                          ? "bg-accent-soft text-fg-strong"
                          : "text-fg-soft hover:bg-surface"
                      }`}
                    >
                      {value}
                    </button>
                  ))}
                </span>
              )}
            </div>
          </div>

          <div className="mt-3 min-h-0 flex-1">
            {!loading && entryCount === 0 && !error ? (
              <div className="rounded-xl border border-line bg-panel">
                <EmptyState
                  title={
                    filtered ? "No entries match" : "No entries in this range"
                  }
                  hint={
                    filtered
                      ? "Try a wider date range or fewer filters."
                      : "Tracked time turns into entries as you work."
                  }
                  action={
                    filtered ? (
                      <button
                        type="button"
                        onClick={() => setRoute({ filters: {} })}
                        className={BUTTON_SECONDARY}
                      >
                        Clear filters
                      </button>
                    ) : undefined
                  }
                />
              </div>
            ) : view === "table" ? (
              <PivotTable
                query={query}
                span={span}
                groupBy={groupBy}
                stackBy={stackBy}
                metric={metric}
                unit={unit}
                catalog={catalog}
                version={version}
              />
            ) : view === "charts" ? (
              <ChartsView
                query={query}
                span={span}
                stackBy={stackBy}
                catalog={catalog}
                version={version}
              />
            ) : (
              <LogView query={query} catalog={catalog} version={version} />
            )}
          </div>
        </main>
      </div>
    </div>
  );
}

function InlineSelect<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <span className="flex items-center gap-1.5">
      {label}:
      <Picker<T>
        ariaLabel={label}
        value={value}
        onChange={onChange}
        options={options}
        variant="compact"
      />
    </span>
  );
}

/** A <details> dropdown; children get a `close` to call after acting. */
function Menu({
  label,
  children,
}: {
  label: string;
  children: (close: () => void) => ReactNode;
}) {
  const ref = useRef<HTMLDetailsElement>(null);
  const close = (): void => ref.current?.removeAttribute("open");
  return (
    <details ref={ref} className="relative">
      <summary className="flex h-7 cursor-pointer list-none items-center rounded-md border border-line bg-panel px-2.5 font-medium text-[12px] text-fg-soft hover:text-fg">
        {label}
      </summary>
      <div className="absolute right-0 z-30 mt-1 min-w-44 rounded-lg border border-line bg-panel p-1 shadow-xl">
        {children(close)}
      </div>
    </details>
  );
}

function MenuItem({
  onClick,
  children,
}: {
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="block w-full rounded px-2 py-1 text-left text-[12px] text-fg-muted hover:bg-surface"
    >
      {children}
    </button>
  );
}

function ViewsMenu({
  views,
  onOpen,
  onSave,
  onDelete,
}: {
  views: SavedView[];
  onOpen: (view: SavedView) => void;
  onSave: (name: string) => void;
  onDelete: (name: string) => void;
}) {
  const [name, setName] = useState("");
  return (
    <Menu
      label={views.length > 0 ? `Views (${views.length}) ▾` : "Save view ▾"}
    >
      {(close) => (
        <div className="w-60">
          {views.map((saved) => (
            <div key={saved.name} className="flex items-center gap-1">
              <MenuItem
                onClick={() => {
                  close();
                  onOpen(saved);
                }}
              >
                {saved.name}
              </MenuItem>
              <button
                type="button"
                onClick={() => onDelete(saved.name)}
                aria-label={`Delete view ${saved.name}`}
                className="rounded px-1.5 text-[11px] text-fg-faint hover:bg-surface hover:text-danger"
              >
                ✕
              </button>
            </div>
          ))}
          {views.length > 0 && <div className="my-1 border-line border-t" />}
          <form
            className="flex gap-1 p-1"
            onSubmit={(event) => {
              event.preventDefault();
              if (!name.trim()) return;
              onSave(name.trim());
              setName("");
              close();
            }}
          >
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              aria-label="View name"
              placeholder="Name this view"
              className="h-7 min-w-0 flex-1 rounded-md border border-line bg-surface px-2 text-[12px] outline-hidden focus:border-accent"
            />
            <button
              type="submit"
              disabled={!name.trim()}
              className="rounded-md bg-accent px-2 font-semibold text-[11.5px] text-accent-fg disabled:opacity-40"
            >
              Save
            </button>
          </form>
        </div>
      )}
    </Menu>
  );
}

// --- Table tab -----------------------------------------------------------------

interface PivotRow {
  key: string | null;
  label: string;
  color: string;
  cells: RollupCell[];
  total: RollupCell;
}

function emptyCell(bucket: number): RollupCell {
  return {
    bucket,
    ms: 0,
    entries: 0,
    approvedMs: 0,
    pending: 0,
    billableMs: 0,
  };
}

function addCell(target: RollupCell, cell: RollupCell): void {
  target.ms += cell.ms;
  target.entries += cell.entries;
  target.approvedMs += cell.approvedMs;
  target.pending += cell.pending;
  target.billableMs += cell.billableMs;
}

function PivotTable({
  query,
  span,
  groupBy,
  stackBy,
  metric,
  unit,
  catalog,
  version,
}: {
  query: EntryQuery;
  span: DateRange;
  groupBy: EntriesGroup;
  stackBy: EntriesStack;
  metric: Metric;
  unit: "h" | "%";
  catalog: Catalog;
  version: number;
}) {
  const edges = useMemo(() => stackEdges(span, stackBy), [span, stackBy]);
  const [cells, setCells] = useState<RollupCell[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` refetches after entries change elsewhere.
  const load = useCallback(async (): Promise<void> => {
    try {
      setCells(await api.entryRollup(query, edges, groupBy));
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [query, edges, groupBy, version]);

  useEffect(() => {
    load();
  }, [load]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new grouping or filter collapses the open row.
  useEffect(() => setExpanded(null), [groupBy, query]);

  const buckets = edges.length - 1;
  const rows = useMemo<PivotRow[]>(() => {
    const byKey = new Map<string, PivotRow>();
    const colors = assignColors(
      (cells ?? []).flatMap((cell) => (cell.key ? [cell.key] : [])),
    );
    for (const cell of cells ?? []) {
      const id = cell.key ?? "\u0000none";
      let row = byKey.get(id);
      if (!row) {
        const label = groupLabel(groupBy, cell.key, catalog, colors);
        row = {
          key: cell.key ?? null,
          ...label,
          cells: Array.from({ length: buckets }, (_, index) =>
            emptyCell(index),
          ),
          total: emptyCell(-1),
        };
        byKey.set(id, row);
      }
      addCell(row.cells[cell.bucket], cell);
      addCell(row.total, cell);
    }
    return [...byKey.values()].sort((a, b) => {
      if ((a.key === null) !== (b.key === null)) return a.key === null ? 1 : -1;
      return b.total.ms - a.total.ms;
    });
  }, [cells, groupBy, catalog, buckets]);

  const columnTotals = useMemo(() => {
    const totals = Array.from({ length: buckets }, (_, index) =>
      emptyCell(index),
    );
    const grand = emptyCell(-1);
    for (const row of rows) {
      for (const [index, cell] of row.cells.entries()) {
        addCell(totals[index], cell);
      }
      addCell(grand, row.total);
    }
    return { totals, grand };
  }, [rows, buckets]);

  const show = (cell: RollupCell, columnMs: number): string => {
    if (metric === "entries")
      return cell.entries > 0 ? String(cell.entries) : "–";
    if (metric === "approved") {
      return cell.ms > 0
        ? `${Math.round((cell.approvedMs / cell.ms) * 100)}%`
        : "–";
    }
    if (unit === "%") {
      return cell.ms > 0 && columnMs > 0
        ? `${Math.round((cell.ms / columnMs) * 100)}%`
        : "–";
    }
    return formatHours(cell.ms);
  };

  if (error) return <InlineError message={error} onRetry={load} />;
  if (cells === null) {
    return (
      <div className="rounded-xl border border-line bg-panel">
        <SkeletonRows />
      </div>
    );
  }

  const groupName = GROUP_OPTIONS.find((g) => g.value === groupBy)?.label;
  return (
    <div className="overflow-x-auto rounded-xl border border-line bg-panel">
      {/* Separate borders on the cells: WebKit drops collapsed borders under
          the sticky first column. */}
      <table className="w-full min-w-max border-separate border-spacing-0 text-[12px]">
        <thead>
          <tr className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider *:border-line *:border-b">
            <th className="sticky left-0 z-10 bg-panel px-3 py-2 text-left">
              {groupName}
            </th>
            {edges.slice(0, -1).map((edge) => (
              <th key={edge} className="px-2 py-2 text-right font-semibold">
                {bucketLabel(edge, stackBy)}
              </th>
            ))}
            <th className="px-3 py-2 text-right">Total</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const id = row.key ?? "none";
            const open = expanded === id;
            return (
              <PivotRowView
                key={id}
                row={row}
                open={open}
                columns={buckets}
                onToggle={() => setExpanded(open ? null : id)}
                cellText={(cell, index) =>
                  show(
                    cell,
                    index === -1
                      ? columnTotals.grand.ms
                      : columnTotals.totals[index].ms,
                  )
                }
                query={withGroup(query, groupBy, row.key)}
                catalog={catalog}
                version={version}
              />
            );
          })}
        </tbody>
        <tfoot>
          <tr className="font-semibold text-fg-strong">
            <td className="sticky left-0 bg-panel px-3 py-2">Total</td>
            {columnTotals.totals.map((cell) => (
              <td
                key={cell.bucket}
                className="px-2 py-2 text-right font-mono tabular-nums"
              >
                {metric === "time" && unit === "%"
                  ? cell.ms > 0
                    ? "100%"
                    : "–"
                  : show(cell, cell.ms)}
              </td>
            ))}
            <td className="px-3 py-2 text-right font-mono tabular-nums">
              {metric === "time" && unit === "%"
                ? "100%"
                : show(columnTotals.grand, columnTotals.grand.ms)}
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
  );
}

function PivotRowView({
  row,
  open,
  columns,
  onToggle,
  cellText,
  query,
  catalog,
  version,
}: {
  row: PivotRow;
  open: boolean;
  columns: number;
  onToggle: () => void;
  cellText: (cell: RollupCell, index: number) => string;
  query: EntryQuery;
  catalog: Catalog;
  version: number;
}) {
  return (
    <>
      <tr className="group hover:bg-surface *:border-line-soft *:border-b">
        {/* Sticky cells stay opaque, so the row tint is layered on top. */}
        <td className="sticky left-0 bg-panel px-3 py-1.5 group-hover:[background-image:linear-gradient(var(--bg-surface-1),var(--bg-surface-1))]">
          <button
            type="button"
            onClick={onToggle}
            aria-expanded={open}
            className="flex max-w-64 items-center gap-2 text-left"
          >
            <span className="w-3 text-fg-faint">{open ? "▾" : "▸"}</span>
            <Dot color={row.color} />
            <span className="truncate font-medium text-fg-strong">
              {row.label}
            </span>
          </button>
        </td>
        {row.cells.map((cell, index) => (
          <td
            key={cell.bucket}
            className={`px-2 py-1.5 text-right font-mono tabular-nums ${
              cell.ms > 0 ? "text-fg-muted" : "text-fg-ghost"
            }`}
          >
            {cellText(cell, index)}
          </td>
        ))}
        <td className="px-3 py-1.5 text-right font-mono font-semibold text-fg-strong tabular-nums">
          {cellText(row.total, -1)}
        </td>
      </tr>
      {open && (
        <tr className="bg-inset-soft *:border-line-soft *:border-b">
          <td colSpan={columns + 2} className="px-3 py-2">
            <RowEntries query={query} catalog={catalog} version={version} />
          </td>
        </tr>
      )}
    </>
  );
}

const ROW_ENTRIES_LIMIT = 50;

function RowEntries({
  query,
  catalog,
  version,
}: {
  query: EntryQuery;
  catalog: Catalog;
  version: number;
}) {
  const [entries, setEntries] = useState<TimeEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` refetches after changes elsewhere.
  useEffect(() => {
    api
      .queryTimeEntries(query, ROW_ENTRIES_LIMIT + 1)
      .then((list) => {
        setEntries(list);
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause)));
  }, [query, version]);

  if (error) return <InlineError message={error} />;
  if (entries === null) return <SkeletonRows rows={2} />;
  return (
    <div className="max-w-3xl space-y-0.5">
      {entries.slice(0, ROW_ENTRIES_LIMIT).map((entry) => (
        <EntryLine key={entry.id} entry={entry} catalog={catalog} />
      ))}
      {entries.length > ROW_ENTRIES_LIMIT && (
        <p className="px-2 pt-1 text-[11px] text-fg-faint">
          Showing the latest {ROW_ENTRIES_LIMIT}. The Log tab lists them all.
        </p>
      )}
    </div>
  );
}

function EntryLine({ entry, catalog }: { entry: TimeEntry; catalog: Catalog }) {
  const category = entry.categoryId
    ? catalog.categoryById.get(entry.categoryId)
    : undefined;
  return (
    <div className="flex w-full items-center gap-3 rounded px-2 py-1 text-left text-[11.5px] hover:bg-surface">
      <span className="w-28 shrink-0 font-mono text-[10.5px] text-fg-soft tabular-nums">
        {formatShortDate(entry.startedAt)} {formatTime(entry.startedAt)}
      </span>
      <Dot color={category?.color ?? "var(--fg-ghost)"} size={7} />
      <span className="min-w-0 flex-1 truncate text-fg-strong">
        {entry.description}
      </span>
      <StatusText entry={entry} />
      <span className="w-14 shrink-0 text-right font-mono text-fg-muted tabular-nums">
        {formatDuration(durationOf(entry))}
      </span>
    </div>
  );
}

function StatusText({ entry }: { entry: TimeEntry }) {
  const state = blockState(entry);
  if (state === "approved") {
    return (
      <span className="shrink-0 text-[10.5px] text-accent">✓ Approved</span>
    );
  }
  return (
    <span className="shrink-0 text-[10.5px] text-review">
      {state === "needsYou"
        ? "Needs you"
        : state === "processing" || state === "building"
          ? "Processing"
          : "Pending"}
    </span>
  );
}

// --- Charts tab ------------------------------------------------------------------

function ChartsView({
  query,
  span,
  stackBy,
  catalog,
  version,
}: {
  query: EntryQuery;
  span: DateRange;
  stackBy: EntriesStack;
  catalog: Catalog;
  version: number;
}) {
  const edges = useMemo(() => stackEdges(span, stackBy), [span, stackBy]);
  const [data, setData] = useState<{
    project: RollupCell[];
    category: RollupCell[];
    app: RollupCell[];
    columns: RollupCell[];
  } | null>(null);
  const [error, setError] = useState<string | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` refetches after entries change elsewhere.
  const load = useCallback(async (): Promise<void> => {
    try {
      const whole = [query.startMs, query.endMs];
      const [project, category, app, columns] = await Promise.all([
        api.entryRollup(query, whole, "project"),
        api.entryRollup(query, whole, "category"),
        api.entryRollup(query, whole, "app"),
        api.entryRollup(query, edges, "category"),
      ]);
      setData({ project, category, app, columns });
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [query, edges, version]);

  useEffect(() => {
    load();
  }, [load]);

  if (error) return <InlineError message={error} onRetry={load} />;
  if (data === null) {
    return (
      <div className="rounded-xl border border-line bg-panel">
        <SkeletonRows />
      </div>
    );
  }

  const slices = (cells: RollupCell[], by: EntriesGroup): Slice[] => {
    const colors = assignColors(
      cells.flatMap((cell) => (cell.key ? [cell.key] : [])),
    );
    return cells.map((cell) => ({
      key: cell.key ?? "none",
      ...groupLabel(by, cell.key, catalog, colors),
      ms: cell.ms,
    }));
  };
  const categoryOrder = (key: string): number =>
    catalog.categoryById.get(key)?.sort ?? Number.MAX_SAFE_INTEGER;
  const legend = slices(data.category, "category").sort(
    (a, b) => categoryOrder(a.key) - categoryOrder(b.key),
  );
  const columns = edges.slice(0, -1).map((edge, index) => ({
    key: String(edge),
    label: bucketLabel(edge, stackBy),
    slices: slices(
      data.columns.filter((cell) => cell.bucket === index),
      "category",
    ).sort((a, b) => categoryOrder(a.key) - categoryOrder(b.key)),
  }));

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 items-start gap-3 lg:grid-cols-3">
        {(
          [
            ["By project", data.project, "project"],
            ["By category", data.category, "category"],
            ["By app", data.app, "app"],
          ] as const
        ).map(([title, cells, by]) => (
          <div
            key={title}
            className="rounded-xl border border-line bg-panel p-4"
          >
            <Donut title={title} slices={slices([...cells], by)} stacked />
          </div>
        ))}
      </div>
      <div className="rounded-xl border border-line bg-panel p-4">
        <StackedColumns
          title={`Time per ${stackBy} by category`}
          columns={columns}
          legend={legend}
        />
      </div>
    </div>
  );
}

// --- Log tab ------------------------------------------------------------------------

type SortKey =
  | "start"
  | "description"
  | "category"
  | "project"
  | "app"
  | "status"
  | "duration";

const ROW_HEIGHT = 34;
const OVERSCAN = 8;

function LogView({
  query,
  catalog,
  version,
}: {
  query: EntryQuery;
  catalog: Catalog;
  version: number;
}) {
  const [entries, setEntries] = useState<TimeEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sort, setSort] = useState<{ key: SortKey; descending: boolean }>({
    key: "start",
    descending: true,
  });
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewport] = useState(600);
  const scroller = useRef<HTMLDivElement>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` refetches after entries change elsewhere.
  const load = useCallback(async (): Promise<void> => {
    try {
      setEntries(await api.queryTimeEntries(query));
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [query, version]);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const observer = new ResizeObserver(() =>
      setViewport(element.clientHeight),
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const name = useCallback(
    (entry: TimeEntry, key: SortKey): string | number => {
      switch (key) {
        case "start":
          return entry.startedAt;
        case "duration":
          return durationOf(entry);
        case "description":
          return entry.description.toLowerCase();
        case "category":
          return (
            (entry.categoryId &&
              catalog.categoryById.get(entry.categoryId)?.name) ||
            "~"
          );
        case "project":
          return (
            (entry.projectId &&
              catalog.projectById.get(entry.projectId)?.name) ||
            "~"
          );
        case "app":
          return entry.dominantApp ?? "~";
        case "status":
          return blockState(entry);
      }
    },
    [catalog],
  );

  const sorted = useMemo(() => {
    const list = [...(entries ?? [])];
    list.sort((a, b) => {
      const left = name(a, sort.key);
      const right = name(b, sort.key);
      const order = left < right ? -1 : left > right ? 1 : 0;
      return sort.descending ? -order : order;
    });
    return list;
  }, [entries, sort, name]);

  if (error) return <InlineError message={error} onRetry={load} />;

  const first = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const last = Math.min(
    sorted.length,
    Math.ceil((scrollTop + viewport) / ROW_HEIGHT) + OVERSCAN,
  );
  // The description takes the most room; the labels share the rest.
  const template =
    "124px minmax(0,2.4fr) minmax(0,1fr) minmax(0,1fr) minmax(0,0.9fr) 84px 56px";
  const header = (key: SortKey, label: string, right = false) => (
    <button
      type="button"
      onClick={() =>
        setSort((current) => ({
          key,
          descending:
            current.key === key
              ? !current.descending
              : key === "start" || key === "duration",
        }))
      }
      className={`flex items-center gap-1 uppercase hover:text-fg ${right ? "justify-end" : ""}`}
      aria-label={`Sort by ${label}`}
    >
      {label}
      {sort.key === key && <span>{sort.descending ? "↓" : "↑"}</span>}
    </button>
  );

  return (
    <div className="flex h-full min-h-[320px] flex-col overflow-hidden rounded-xl border border-line bg-panel">
      <div
        // Reserves the list's scrollbar gutter so the columns line up.
        className="grid shrink-0 items-center gap-3 overflow-y-hidden border-line border-b bg-surface px-3 py-2 font-semibold text-[10.5px] text-fg-faint tracking-wider [scrollbar-gutter:stable]"
        style={{ gridTemplateColumns: template }}
      >
        {header("start", "When")}
        {header("description", "Description")}
        {header("category", "Category")}
        {header("project", "Project")}
        {header("app", "App")}
        {header("status", "Status")}
        {header("duration", "Dur.", true)}
      </div>
      {entries === null ? (
        <SkeletonRows />
      ) : (
        <div
          ref={scroller}
          onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
          className="min-h-0 flex-1 overflow-y-auto [scrollbar-gutter:stable]"
        >
          <div
            className="relative"
            style={{ height: sorted.length * ROW_HEIGHT }}
          >
            {sorted.slice(first, last).map((entry, offset) => {
              const index = first + offset;
              const category = entry.categoryId
                ? catalog.categoryById.get(entry.categoryId)
                : undefined;
              const project = entry.projectId
                ? catalog.projectById.get(entry.projectId)
                : undefined;
              return (
                <div
                  key={entry.id}
                  className="absolute right-0 left-0 grid items-center gap-3 border-line-soft border-b px-3 text-left text-[12px] hover:bg-surface"
                  style={{
                    top: index * ROW_HEIGHT,
                    height: ROW_HEIGHT,
                    gridTemplateColumns: template,
                  }}
                >
                  <span className="font-mono text-[11px] text-fg-soft tabular-nums">
                    {formatShortDate(entry.startedAt)}{" "}
                    {formatTime(entry.startedAt)}
                  </span>
                  <span className="truncate text-fg-strong">
                    {entry.description}
                  </span>
                  <span className="flex min-w-0 items-center gap-1.5 text-fg-muted">
                    <Dot
                      color={category?.color ?? "var(--fg-ghost)"}
                      size={7}
                    />
                    <span className="truncate">
                      {category?.name ?? "Uncategorized"}
                    </span>
                  </span>
                  <span className="flex min-w-0 items-center gap-1.5 text-fg-muted">
                    {project && <Dot color={project.color} size={7} />}
                    <span className="truncate">{project?.name ?? "–"}</span>
                  </span>
                  <span className="truncate text-fg-soft">
                    {entry.dominantApp ?? "–"}
                  </span>
                  <StatusText entry={entry} />
                  <span className="text-right font-mono text-fg-muted tabular-nums">
                    {formatDuration(durationOf(entry))}
                  </span>
                </div>
              );
            })}
          </div>
          {sorted.length >= 5000 && (
            <p className="px-3 py-2 text-[11px] text-fg-faint">
              Showing the latest 5,000 entries. Narrow the range or filters to
              see the rest.
            </p>
          )}
        </div>
      )}
    </div>
  );
}
