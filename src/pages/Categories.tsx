import { useCallback, useEffect, useMemo, useState } from "react";
import {
  BUTTON_PRIMARY,
  Dot,
  EmptyState,
  InlineError,
  PageHeader,
  StatCard,
  Tabs,
} from "../components/Page";
import { Picker } from "../components/Picker";
import { Toggle } from "../components/SegmentedControl";
import { Tooltip } from "../components/Tooltip";
import { useCatalog } from "../hooks/useCatalog";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { useToast } from "../hooks/useToast";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { formatDuration } from "../lib/format";
import type {
  CategoriesTab,
  Category,
  CategoryStats,
  ProjectsRange,
  Route,
} from "../lib/types";
import { CategorySheet } from "./categories/CategorySheet";
import { RANGE_OPTIONS, rangeBounds } from "./projects/range";

type CategoriesRoute = Extract<Route, { name: "categories" }>;

interface CategoriesProps {
  route: CategoriesRoute;
  navigate: (route: Route) => void;
  replace: (route: Route) => void;
}

type Editing = { category?: Category } | null;

const TEMPLATE = "32px minmax(180px,1.2fr) minmax(200px,2fr) 110px 140px 100px";

export function Categories({ route, replace }: CategoriesProps) {
  const catalog = useCatalog();
  const showToast = useToast();
  const tab: CategoriesTab = route.tab ?? "active";
  const range: ProjectsRange = route.range ?? "month";

  const [stats, setStats] = useState<Map<string, CategoryStats>>(new Map());
  const [search, setSearch] = useState("");
  const [editing, setEditing] = useState<Editing>(null);
  const [error, setError] = useState<string | null>(null);
  const [draggedId, setDraggedId] = useState<string | null>(null);

  const loadStats = useCallback(async (): Promise<void> => {
    const bounds = rangeBounds(range);
    try {
      const list = await api.categoryStats(bounds.start, bounds.end);
      setStats(new Map(list.map((entry) => [entry.categoryId, entry])));
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [range]);

  useEffect(() => {
    loadStats();
  }, [loadStats]);

  useTauriEvent(api.ENTRIES_CHANGED, () => {
    loadStats();
  });

  const setRoute = (patch: Partial<CategoriesRoute>): void =>
    replace({ ...route, ...patch });

  const activeCategories = useMemo(
    () =>
      catalog.categories
        .filter((c) => !c.archived && !c.deletedAt)
        .sort((a, b) => a.sort - b.sort || a.name.localeCompare(b.name)),
    [catalog.categories],
  );

  const archivedCategories = useMemo(
    () =>
      catalog.categories
        .filter((c) => c.archived && !c.deletedAt)
        .sort((a, b) => a.sort - b.sort || a.name.localeCompare(b.name)),
    [catalog.categories],
  );

  const counts = useMemo(
    () => ({
      active: activeCategories.length,
      archived: archivedCategories.length,
    }),
    [activeCategories, archivedCategories],
  );

  const totalRangeMs = useMemo(() => {
    let sum = 0;
    for (const cat of activeCategories) {
      const s = stats.get(cat.id);
      if (s) sum += s.rangeMs;
    }
    return sum;
  }, [activeCategories, stats]);

  const workCategoriesCount = useMemo(
    () => activeCategories.filter((c) => c.countsAsWork).length,
    [activeCategories],
  );

  const currentList = tab === "active" ? activeCategories : archivedCategories;

  const shown = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return currentList;
    return currentList.filter(
      (cat) =>
        cat.name.toLowerCase().includes(q) ||
        cat.description?.toLowerCase().includes(q) ||
        cat.aiHints?.toLowerCase().includes(q),
    );
  }, [currentList, search]);

  const handleDragStart = (id: string) => {
    setDraggedId(id);
  };

  const handleDragOver = (e: React.DragEvent, targetId: string) => {
    e.preventDefault();
    if (
      !draggedId ||
      draggedId === targetId ||
      tab !== "active" ||
      search.trim()
    )
      return;
  };

  const handleDrop = async (targetId: string) => {
    if (
      !draggedId ||
      draggedId === targetId ||
      tab !== "active" ||
      search.trim()
    )
      return;
    const currentIds = activeCategories.map((c) => c.id);
    const fromIndex = currentIds.indexOf(draggedId);
    const toIndex = currentIds.indexOf(targetId);
    if (fromIndex === -1 || toIndex === -1) return;

    const nextIds = [...currentIds];
    nextIds.splice(fromIndex, 1);
    nextIds.splice(toIndex, 0, draggedId);
    setDraggedId(null);

    try {
      await api.reorderCategories(nextIds);
      await catalog.reload();
    } catch (cause) {
      setError(describeError(cause));
    }
  };

  const toggleWork = async (cat: Category, nextVal: boolean) => {
    try {
      await api.updateCategory(cat.id, { countsAsWork: nextVal });
      await catalog.reload();
    } catch (cause) {
      setError(describeError(cause));
    }
  };

  const restoreCategory = async (cat: Category) => {
    try {
      await api.updateCategory(cat.id, { archived: false });
      showToast({
        message: `Restored "${cat.name}"`,
        tone: "success",
      });
      await catalog.reload();
    } catch (cause) {
      setError(describeError(cause));
    }
  };

  const rangeLabel =
    RANGE_OPTIONS.find((opt) => opt.value === range)?.label ?? "This month";

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
      <PageHeader title="Categories">
        <button
          type="button"
          onClick={() => setEditing({})}
          className={BUTTON_PRIMARY}
        >
          + New category
        </button>
      </PageHeader>

      <div className="flex-1 overflow-y-auto px-5 py-4">
        {error && (
          <div className="mb-4">
            <InlineError message={error} onRetry={loadStats} />
          </div>
        )}

        <Tabs
          label="Categories"
          value={tab}
          onChange={(next) => setRoute({ tab: next })}
          tabs={[
            { value: "active", label: "Active", count: counts.active },
            { value: "archived", label: "Archived", count: counts.archived },
          ]}
        />

        <div className="shape-strip mt-3 grid grid-cols-3 gap-3">
          <StatCard
            label={
              tab === "active" ? "Active categories" : "Archived categories"
            }
            value={String(counts[tab])}
          />
          <StatCard
            label="Time tracked"
            value={formatDuration(totalRangeMs)}
            sub={rangeLabel}
          />
          <StatCard
            label="Count as work"
            value={`${workCategoriesCount} of ${counts.active}`}
            sub="Included in daily and weekly targets"
          />
        </div>

        <div className="mt-4 flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <input
              type="search"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              aria-label="Search categories"
              placeholder="⌕ Search categories"
              className="h-7 w-56 rounded-md border border-line bg-panel px-2.5 text-[12px] text-fg outline-hidden placeholder:text-fg-faint focus:border-accent"
            />
            <Picker
              ariaLabel="Time range"
              value={range}
              onChange={(val) => setRoute({ range: val as ProjectsRange })}
              options={RANGE_OPTIONS}
              variant="compact"
            />
          </div>
          {tab === "active" && !search.trim() && (
            <span className="text-[11px] text-fg-faint">
              Drag rows to reorder · Top 3 get review shortcut keys 1–3
            </span>
          )}
        </div>

        <div className="shape-bleed-table mt-3 overflow-hidden rounded-xl border border-line bg-panel">
          <div
            className="grid items-center gap-3 border-line border-b bg-surface px-4 py-2 font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider"
            style={{ gridTemplateColumns: TEMPLATE }}
          >
            <span className="text-center">#</span>
            <span>Category</span>
            <span>What counts</span>
            <Tooltip content={rangeLabel}>
              <span className="text-right">Time</span>
            </Tooltip>
            <span className="text-center">Counts as work</span>
            <span />
          </div>

          {shown.length === 0 ? (
            <EmptyState
              title={
                search.trim()
                  ? `No categories match "${search}"`
                  : tab === "active"
                    ? "No active categories"
                    : "No archived categories"
              }
              hint={
                search.trim()
                  ? "Try another search term"
                  : tab === "active"
                    ? "Create your first custom category to start tracking."
                    : "Categories you archive will appear here."
              }
              action={
                tab === "active" && !search.trim() ? (
                  <button
                    type="button"
                    onClick={() => setEditing({})}
                    className={BUTTON_PRIMARY}
                  >
                    + New category
                  </button>
                ) : undefined
              }
            />
          ) : (
            <ul className="divide-y divide-line">
              {shown.map((cat, index) => {
                const stat = stats.get(cat.id);
                const isKeyed = tab === "active" && !search.trim() && index < 3;
                const keyNum = index + 1;
                return (
                  <li
                    key={cat.id}
                    draggable={tab === "active" && !search.trim()}
                    onDragStart={() => handleDragStart(cat.id)}
                    onDragOver={(e) => handleDragOver(e, cat.id)}
                    onDrop={() => handleDrop(cat.id)}
                    className={`grid items-center gap-3 px-4 py-2.5 transition-colors hover:bg-surface/50 ${
                      draggedId === cat.id ? "opacity-40" : ""
                    }`}
                    style={{ gridTemplateColumns: TEMPLATE }}
                  >
                    <div className="flex items-center justify-center">
                      {isKeyed ? (
                        <Tooltip
                          content={`Keyboard shortcut ${keyNum} in review panel`}
                        >
                          <span className="inline-flex size-5 items-center justify-center rounded border border-line bg-surface font-mono text-[10.5px] font-semibold text-accent">
                            {keyNum}
                          </span>
                        </Tooltip>
                      ) : tab === "active" && !search.trim() ? (
                        <span className="cursor-grab text-fg-faint hover:text-fg select-none">
                          ⋮⋮
                        </span>
                      ) : (
                        <span className="font-mono text-[11px] text-fg-faint">
                          {index + 1}
                        </span>
                      )}
                    </div>

                    <div className="flex min-w-0 items-center gap-2">
                      <Dot color={cat.color} size={10} />
                      <button
                        type="button"
                        onClick={() => setEditing({ category: cat })}
                        className="truncate text-left font-medium text-[13px] text-fg-strong hover:text-accent hover:underline"
                      >
                        {cat.name}
                      </button>
                    </div>

                    <div className="truncate text-[12px] text-fg-soft">
                      {cat.description || (
                        <span className="text-fg-faint italic">
                          No description
                        </span>
                      )}
                    </div>

                    <div className="text-right font-mono text-[12px] text-fg">
                      {formatDuration(stat?.rangeMs ?? 0)}
                    </div>

                    <div className="flex justify-center">
                      <Toggle
                        checked={cat.countsAsWork}
                        onChange={(val) => void toggleWork(cat, val)}
                        label={`Count ${cat.name} toward work hours`}
                      />
                    </div>

                    <div className="flex items-center justify-end gap-2">
                      {tab === "archived" ? (
                        <button
                          type="button"
                          onClick={() => void restoreCategory(cat)}
                          className="rounded px-2 py-1 text-[11.5px] font-medium text-accent hover:bg-accent-soft"
                        >
                          Restore
                        </button>
                      ) : (
                        <button
                          type="button"
                          onClick={() => setEditing({ category: cat })}
                          className="rounded px-2 py-1 text-[11.5px] text-fg-soft hover:bg-surface hover:text-fg"
                        >
                          Edit
                        </button>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </div>

      {editing && (
        <CategorySheet
          category={editing.category}
          categories={catalog.categories}
          stats={editing.category ? stats.get(editing.category.id) : undefined}
          onClose={() => setEditing(null)}
          onSaved={async () => {
            await Promise.all([catalog.reload(), loadStats()]);
          }}
          onRemoved={async () => {
            await Promise.all([catalog.reload(), loadStats()]);
          }}
        />
      )}
    </div>
  );
}
