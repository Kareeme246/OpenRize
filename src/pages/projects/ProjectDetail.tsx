import { useCallback, useEffect, useMemo, useState } from "react";
import { BarRow, Donut, StackedColumns } from "../../components/Charts";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { EntryReviewSheet } from "../../components/EntryReviewSheet";
import {
  BUTTON_SECONDARY,
  Dot,
  EmptyState,
  InlineError,
  Progress,
  SkeletonRows,
  StatCard,
} from "../../components/Page";
import type { Catalog } from "../../hooks/useCatalog";
import { useEntryReview } from "../../hooks/useEntryReview";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import * as api from "../../lib/api";
import { describeError } from "../../lib/api";
import { addDays, bucketLabel, startOfWeek } from "../../lib/dates";
import { durationOf } from "../../lib/entries";
import {
  formatDuration,
  formatMoney,
  formatShortDate,
  formatTime,
} from "../../lib/format";
import type {
  EntryQuery,
  Project,
  ProjectRule,
  ProjectStats,
  RollupCell,
  TimeEntry,
} from "../../lib/types";
import { BUDGET_WARN, budgetUsage, currencyFor, rateFor } from "./budget";

const WEEKS = 12;
const RECENT = 15;

const RULE_KINDS: Record<string, string> = {
  app: "App",
  domain: "Domain",
  title_contains: "Title has",
  title_regex: "Title matches",
  url_prefix: "URL",
  path_prefix: "Folder",
};

const ORIGINS: Record<string, string> = {
  hint: "AI hint",
  suggested: "Learned",
  manual: "Rule",
  app: "Apps page",
};

interface ProjectDetailProps {
  project: Project;
  stats?: ProjectStats;
  catalog: Catalog;
  onBack: () => void;
  onEdit: () => void;
  onChanged: () => void;
}

interface DetailData {
  weekMs: number;
  categories: RollupCell[];
  weeks: RollupCell[];
  apps: RollupCell[];
  recent: TimeEntry[];
  rules: ProjectRule[];
}

/**
 * One project: totals, time by category and by week, recent entries (which
 * open the review panel), the apps and rules that feed it, and the
 * Complete / Archive / Delete actions.
 */
export function ProjectDetail({
  project,
  stats,
  catalog,
  onBack,
  onEdit,
  onChanged,
}: ProjectDetailProps) {
  const review = useEntryReview();
  const [data, setData] = useState<DetailData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const client = project.clientId
    ? catalog.clientById.get(project.clientId)
    : undefined;

  const weekEdges = useMemo(() => {
    const thisWeek = startOfWeek(new Date());
    return Array.from({ length: WEEKS + 1 }, (_, index) =>
      addDays(thisWeek, (index - WEEKS + 1) * 7).getTime(),
    );
  }, []);

  const load = useCallback(async (): Promise<void> => {
    const everything: EntryQuery = {
      startMs: 0,
      endMs: addDays(new Date(), 1).getTime(),
      projectId: project.id,
    };
    const lastWeeks: EntryQuery = {
      ...everything,
      startMs: weekEdges[0],
      endMs: weekEdges[weekEdges.length - 1],
    };
    try {
      const [categories, weeks, apps, recent, rules] = await Promise.all([
        api.entryRollup(
          everything,
          [everything.startMs, everything.endMs],
          "category",
        ),
        api.entryRollup(lastWeeks, weekEdges, "none"),
        api.entryRollup(
          everything,
          [everything.startMs, everything.endMs],
          "app",
        ),
        api.queryTimeEntries(everything, RECENT),
        api.projectRules(project.id),
      ]);
      setData({
        weekMs: weeks.find((cell) => cell.bucket === WEEKS - 1)?.ms ?? 0,
        categories,
        weeks,
        apps: apps.sort((a, b) => b.ms - a.ms).slice(0, 6),
        recent,
        rules,
      });
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [project.id, weekEdges]);

  useEffect(() => {
    load();
  }, [load]);

  useTauriEvent(api.ENTRIES_CHANGED, () => void load());

  const setStatus = async (status: string): Promise<void> => {
    try {
      await api.updateProject(project.id, { status });
      setActionError(null);
      onChanged();
    } catch (cause) {
      setActionError(describeError(cause));
    }
  };

  const remove = async (): Promise<void> => {
    setConfirmDelete(false);
    try {
      await api.deleteProject(project.id);
      onChanged();
      onBack();
    } catch (cause) {
      setActionError(describeError(cause));
    }
  };

  const budget = budgetUsage(project, stats, client);
  const rate = rateFor(project, client);
  const billableAmount =
    rate !== undefined
      ? ((stats?.billableMs ?? 0) / 3_600_000) * rate
      : undefined;
  const hasEntries = (stats?.entries ?? 0) > 0;
  const due = project.dueDate
    ? `Due ${formatShortDate(project.dueDate)}`
    : undefined;

  return (
    <div className="flex min-h-0 flex-1 overflow-hidden">
      <main className="min-h-0 min-w-0 flex-1 overflow-y-auto px-5 py-4">
        <button
          type="button"
          onClick={onBack}
          className="text-[12px] text-fg-soft hover:text-fg"
        >
          ‹ Projects
        </button>
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <Dot color={project.color} size={12} />
          <h2 className="font-semibold text-[18px] text-fg-strong">
            {project.name}
          </h2>
          <span className="rounded-full border border-line px-2 py-0.5 text-[10.5px] text-fg-soft capitalize">
            {project.status}
          </span>
          <span className="text-[12px] text-fg-soft">
            {[client?.name, due].filter(Boolean).join(" · ")}
          </span>
          <div className="ml-auto flex flex-wrap gap-2">
            <button type="button" onClick={onEdit} className={BUTTON_SECONDARY}>
              Edit
            </button>
            {project.status === "active" ? (
              <button
                type="button"
                onClick={() => void setStatus("completed")}
                className={BUTTON_SECONDARY}
              >
                Complete
              </button>
            ) : (
              <button
                type="button"
                onClick={() => void setStatus("active")}
                className={BUTTON_SECONDARY}
              >
                Reopen
              </button>
            )}
            {project.status !== "archived" && (
              <button
                type="button"
                onClick={() => void setStatus("archived")}
                className={BUTTON_SECONDARY}
              >
                Archive
              </button>
            )}
            <button
              type="button"
              onClick={() => setConfirmDelete(true)}
              disabled={hasEntries}
              title={
                hasEntries
                  ? "Only a project without entries can be deleted. Archive it instead."
                  : "Delete project"
              }
              className={`${BUTTON_SECONDARY} text-danger`}
            >
              Delete
            </button>
          </div>
        </div>
        {project.description && (
          <p className="mt-1 text-[12px] text-fg-soft">{project.description}</p>
        )}
        {actionError && (
          <div className="mt-3">
            <InlineError message={actionError} />
          </div>
        )}
        {error && (
          <div className="mt-3">
            <InlineError message={error} onRetry={load} />
          </div>
        )}

        <div className="mt-4 grid grid-cols-4 gap-3">
          <StatCard
            label="Total"
            value={formatDuration(stats?.totalMs ?? 0)}
            sub={`${stats?.entries ?? 0} entries`}
          />
          <StatCard
            label="This week"
            value={formatDuration(data?.weekMs ?? 0)}
          />
          <StatCard
            label="Budget used"
            value={
              budget?.ratio !== undefined
                ? `${Math.round(budget.ratio * 100)}%`
                : "–"
            }
            tone={
              budget?.ratio !== undefined && budget.ratio >= BUDGET_WARN
                ? "review"
                : undefined
            }
            sub={budget?.label ?? "No budget"}
          >
            {budget?.ratio !== undefined && (
              <div className="mt-2">
                <Progress
                  value={budget.ratio}
                  warnAt={BUDGET_WARN}
                  label="Budget used"
                />
              </div>
            )}
          </StatCard>
          <StatCard
            label="Billable"
            value={
              billableAmount !== undefined
                ? formatMoney(billableAmount, currencyFor(client))
                : formatDuration(stats?.billableMs ?? 0)
            }
            sub={
              billableAmount !== undefined
                ? `${formatDuration(stats?.billableMs ?? 0)} at ${formatMoney(rate ?? 0, currencyFor(client))}/h`
                : "No rate set"
            }
          />
        </div>

        {data === null ? (
          <div className="mt-4 rounded-xl border border-line bg-panel">
            <SkeletonRows />
          </div>
        ) : (
          <>
            <div className="mt-4 grid grid-cols-1 gap-3 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
              <div className="rounded-xl border border-line bg-panel p-4">
                <Donut
                  title="Time by category"
                  slices={data.categories.map((cell) => {
                    const category = cell.key
                      ? catalog.categoryById.get(cell.key)
                      : undefined;
                    return {
                      key: cell.key ?? "none",
                      label: category?.name ?? "Uncategorized",
                      color: category?.color ?? "var(--fg-ghost)",
                      ms: cell.ms,
                    };
                  })}
                />
              </div>
              <div className="rounded-xl border border-line bg-panel p-4">
                <StackedColumns
                  title={`Time per week · last ${WEEKS} weeks`}
                  height={140}
                  legend={[
                    {
                      key: project.id,
                      label: project.name,
                      color: project.color,
                      ms: 0,
                    },
                  ]}
                  columns={weekEdges.slice(0, -1).map((edge, index) => ({
                    key: String(edge),
                    label: bucketLabel(edge, "week"),
                    slices: [
                      {
                        key: project.id,
                        label: project.name,
                        color: project.color,
                        ms:
                          data.weeks.find((cell) => cell.bucket === index)
                            ?.ms ?? 0,
                      },
                    ],
                  }))}
                />
              </div>
            </div>

            <div className="mt-4 grid grid-cols-1 gap-3 xl:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
              <section className="rounded-xl border border-line bg-panel">
                <h3 className="border-line border-b px-4 py-2.5 font-semibold text-[12.5px] text-fg-strong">
                  Recent entries
                </h3>
                {data.recent.length === 0 ? (
                  <EmptyState
                    title="No time on this project yet"
                    hint="Entries land here once a rule, an AI hint, or you assign them."
                  />
                ) : (
                  <div className="p-1.5">
                    {data.recent.map((entry) => {
                      const category = entry.categoryId
                        ? catalog.categoryById.get(entry.categoryId)
                        : undefined;
                      return (
                        <button
                          key={entry.id}
                          type="button"
                          onClick={() => review.select(entry.id)}
                          className={`flex w-full items-center gap-3 rounded px-2.5 py-1.5 text-left text-[12px] ${
                            review.selectedId === entry.id
                              ? "bg-accent/10"
                              : "hover:bg-surface"
                          }`}
                        >
                          <span className="w-28 shrink-0 font-mono text-[10.5px] text-fg-soft tabular-nums">
                            {formatShortDate(entry.startedAt)}{" "}
                            {formatTime(entry.startedAt)}
                          </span>
                          <Dot
                            color={category?.color ?? "var(--fg-ghost)"}
                            size={7}
                          />
                          <span className="min-w-0 flex-1 truncate text-fg-strong">
                            {entry.description}
                          </span>
                          {entry.status !== "approved" && (
                            <span className="shrink-0 text-[10.5px] text-review">
                              Pending
                            </span>
                          )}
                          <span className="shrink-0 font-mono text-fg-muted tabular-nums">
                            {formatDuration(durationOf(entry))}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                )}
              </section>

              <section className="space-y-4 rounded-xl border border-line bg-panel p-4">
                <div className="space-y-2">
                  <h3 className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
                    Matched apps
                  </h3>
                  {data.apps.length === 0 && (
                    <p className="text-[11.5px] text-fg-faint">
                      No app activity yet
                    </p>
                  )}
                  {data.apps.map((cell) => (
                    <BarRow
                      key={cell.key ?? "none"}
                      label={cell.key ?? "Unknown"}
                      ms={cell.ms}
                      maxMs={data.apps[0]?.ms ?? 0}
                      color={project.color}
                    />
                  ))}
                </div>
                <div className="space-y-1.5">
                  <h3 className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
                    Rules
                  </h3>
                  {data.rules.length === 0 && (
                    <p className="text-[11.5px] text-fg-faint">
                      No rules yet. Add AI hints to create them.
                    </p>
                  )}
                  {data.rules.map((rule) => (
                    <div
                      key={rule.id}
                      className="flex items-center gap-2 text-[11.5px]"
                    >
                      <span className="w-20 shrink-0 text-fg-faint">
                        {RULE_KINDS[rule.matchKind] ?? rule.matchKind}
                      </span>
                      <span className="min-w-0 flex-1 truncate font-mono text-fg-muted">
                        {rule.pattern}
                      </span>
                      <span className="shrink-0 rounded-full bg-surface px-1.5 text-[10px] text-fg-soft">
                        {ORIGINS[rule.origin] ?? rule.origin}
                      </span>
                    </div>
                  ))}
                </div>
              </section>
            </div>
          </>
        )}
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

      {confirmDelete && (
        <ConfirmDialog
          title={`Delete ${project.name}?`}
          body="The project and its AI-hint rules are removed. It has no entries, so no tracked time changes."
          confirmLabel="Delete"
          onCancel={() => setConfirmDelete(false)}
          onConfirm={() => void remove()}
        />
      )}
    </div>
  );
}
