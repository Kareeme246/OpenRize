import {
  type ChangeEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  BUTTON_PRIMARY,
  BUTTON_SECONDARY,
  Dot,
  EmptyState,
  FilterSelect,
  InlineError,
  PageHeader,
  Progress,
  Tabs,
} from "../components/Page";
import { Picker } from "../components/Picker";
import { Tooltip } from "../components/Tooltip";
import { type Catalog, useCatalog } from "../hooks/useCatalog";
import { useTauriEvent } from "../hooks/useTauriEvent";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { assignableClients, clientLabel } from "../lib/clients";
import { addDays, startOfMonth } from "../lib/dates";
import {
  formatDuration,
  formatRelative,
  formatShortDate,
  plural,
} from "../lib/format";
import type {
  Project,
  ProjectStats,
  ProjectSuggestion,
  ProjectsRange,
  ProjectsTab,
  Route,
} from "../lib/types";
import { BUDGET_WARN, budgetUsage } from "./projects/budget";
import { ProjectDetail } from "./projects/ProjectDetail";
import { type ProjectDraft, ProjectSheet } from "./projects/ProjectSheet";
import { RANGE_OPTIONS, rangeBounds } from "./projects/range";

type ProjectsRoute = Extract<Route, { name: "projects" }>;

interface ProjectsProps {
  route: ProjectsRoute;
  navigate: (route: Route) => void;
  replace: (route: Route) => void;
}

/** Discovery looks at the last two weeks of titles (design board C§3b). */
const DISCOVERY_DAYS = 14;

/** Editing state for the sheet: a project, or a draft for a new one. */
type Editing = { project: Project } | { draft: ProjectDraft } | null;

export function Projects({ route, navigate, replace }: ProjectsProps) {
  const catalog = useCatalog();
  const tab: ProjectsTab = route.tab ?? "active";
  const range: ProjectsRange = route.range ?? "month";

  const [stats, setStats] = useState<Map<string, ProjectStats>>(new Map());
  const [suggestions, setSuggestions] = useState<ProjectSuggestion[]>([]);
  const [search, setSearch] = useState("");
  const [clientFilter, setClientFilter] = useState("");
  const [editing, setEditing] = useState<Editing>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const now = Date.now();

  const loadStats = useCallback(async (): Promise<void> => {
    const bounds = rangeBounds(range);
    try {
      const [list, found] = await Promise.all([
        api.projectStats(
          bounds.start,
          bounds.end,
          startOfMonth(new Date()).getTime(),
        ),
        api.discoverProjects(addDays(new Date(), -DISCOVERY_DAYS).getTime()),
      ]);
      setStats(new Map(list.map((entry) => [entry.projectId, entry])));
      setSuggestions(found);
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setLoaded(true);
    }
  }, [range]);

  useEffect(() => {
    loadStats();
  }, [loadStats]);

  useTauriEvent(api.ENTRIES_CHANGED, () => void loadStats());

  const reloadAll = useCallback(async (): Promise<void> => {
    await Promise.all([catalog.reload(), loadStats()]);
  }, [catalog, loadStats]);

  const setRoute = (patch: Partial<ProjectsRoute>): void =>
    replace({ ...route, ...patch });

  const counts = useMemo(() => {
    const byStatus = { active: 0, completed: 0, archived: 0 };
    for (const project of catalog.projects) {
      if (project.status in byStatus) {
        byStatus[project.status as keyof typeof byStatus] += 1;
      }
    }
    return byStatus;
  }, [catalog.projects]);

  const shown = useMemo(() => {
    const query = search.trim().toLowerCase();
    return catalog.projects
      .filter((project) => project.status === tab)
      .filter(
        (project) =>
          clientFilter === "" ||
          (clientFilter === "none"
            ? !project.clientId
            : project.clientId === clientFilter),
      )
      .filter(
        (project) =>
          query === "" ||
          project.name.toLowerCase().includes(query) ||
          (project.clientId &&
            catalog.clientById
              .get(project.clientId)
              ?.name.toLowerCase()
              .includes(query)),
      )
      .sort(
        (a, b) =>
          (stats.get(b.id)?.lastActivity ?? 0) -
          (stats.get(a.id)?.lastActivity ?? 0),
      );
  }, [catalog.projects, catalog.clientById, tab, clientFilter, search, stats]);

  const importCsv = async (
    event: ChangeEvent<HTMLInputElement>,
  ): Promise<void> => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    try {
      const summary = await api.importProjectsCsv(await file.text());
      await reloadAll();
      const parts = [`Imported ${plural(summary.created, "project")}`];
      if (summary.clientsCreated > 0) {
        parts.push(`${plural(summary.clientsCreated, "new client")}`);
      }
      setNotice(
        `${parts.join(" and ")}.${
          summary.skipped.length > 0 ? ` ${summary.skipped.join(". ")}.` : ""
        }`,
      );
    } catch (cause) {
      setNotice(`Import failed: ${describeError(cause)}`);
    }
  };

  const dismiss = async (key: string): Promise<void> => {
    setSuggestions((list) => list.filter((entry) => entry.key !== key));
    try {
      await api.dismissProjectSuggestion(key);
    } catch (cause) {
      setError(describeError(cause));
    }
  };

  const open = route.projectId
    ? catalog.projects.find((project) => project.id === route.projectId)
    : undefined;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-canvas text-fg">
      <PageHeader title="Projects" crumb={open?.name}>
        <input
          ref={fileInput}
          type="file"
          accept=".csv,text/csv"
          className="hidden"
          onChange={(event) => void importCsv(event)}
        />
        <Tooltip content="Columns: name, client, budget, due date">
          <button
            type="button"
            onClick={() => fileInput.current?.click()}
            className={BUTTON_SECONDARY}
          >
            Import CSV
          </button>
        </Tooltip>
        <button
          type="button"
          onClick={() => setEditing({ draft: {} })}
          className={BUTTON_PRIMARY}
        >
          + New project
        </button>
      </PageHeader>

      {notice && (
        <div className="flex shrink-0 items-center gap-3 border-line border-b bg-accent-soft px-5 py-2 text-[12px]">
          <span className="min-w-0 flex-1 text-fg">{notice}</span>
          <button
            type="button"
            onClick={() => setNotice(null)}
            aria-label="Dismiss"
            className="text-fg-soft hover:text-fg"
          >
            ✕
          </button>
        </div>
      )}

      {open ? (
        <ProjectDetail
          key={open.id}
          project={open}
          stats={stats.get(open.id)}
          catalog={catalog}
          onBack={() => navigate({ name: "projects", tab, range })}
          onEdit={() => setEditing({ project: open })}
          onInvoice={() =>
            navigate({
              name: "invoices",
              compose: { clientId: open.clientId, projectId: open.id },
            })
          }
          onChanged={() => void reloadAll()}
        />
      ) : (
        <main className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          <Tabs
            label="Project status"
            tabs={[
              {
                value: "active" as const,
                label: "Active",
                count: counts.active,
              },
              {
                value: "completed" as const,
                label: "Completed",
                count: counts.completed,
              },
              {
                value: "archived" as const,
                label: "Archived",
                count: counts.archived,
              },
            ]}
            value={tab}
            onChange={(next) => setRoute({ tab: next })}
          />

          {(error || catalog.error) && (
            <div className="mt-3">
              <InlineError
                message={error ?? catalog.error ?? ""}
                onRetry={reloadAll}
              />
            </div>
          )}

          <div className="mt-3 flex flex-wrap items-center gap-2">
            <input
              type="search"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              aria-label="Search projects"
              placeholder="⌕ Search projects"
              className="h-7 w-56 rounded-md border border-line bg-panel px-2.5 text-[12px] text-fg outline-hidden placeholder:text-fg-faint focus:border-accent"
            />
            <FilterSelect
              label="Client"
              value={clientFilter}
              options={[
                { value: "none", label: "No client" },
                ...catalog.clients.map((client) => ({
                  value: client.id,
                  label: clientLabel(client),
                })),
              ]}
              onChange={setClientFilter}
            />
            <Picker
              ariaLabel="Time range"
              value={range}
              onChange={(val) => setRoute({ range: val as ProjectsRange })}
              options={RANGE_OPTIONS}
              variant="compact"
            />
          </div>

          {tab === "active" &&
            suggestions.map((suggestion) => (
              <SuggestionStrip
                key={suggestion.key}
                suggestion={suggestion}
                onCreate={() =>
                  setEditing({
                    draft: {
                      name: suggestion.name,
                      aiHints: suggestion.evidence.join(", "),
                    },
                  })
                }
                onDismiss={() => void dismiss(suggestion.key)}
              />
            ))}

          <ProjectsTable
            projects={shown}
            stats={stats}
            catalog={catalog}
            now={now}
            loaded={loaded}
            empty={
              catalog.projects.length === 0
                ? "first"
                : search || clientFilter
                  ? "filtered"
                  : "tab"
            }
            tab={tab}
            rangeLabel={
              RANGE_OPTIONS.find((option) => option.value === range)?.label ??
              ""
            }
            onOpen={(project) =>
              navigate({
                name: "projects",
                tab,
                range,
                projectId: project.id,
              })
            }
            onCreate={() => setEditing({ draft: {} })}
          />
        </main>
      )}

      {editing && (
        <ProjectSheet
          project={"project" in editing ? editing.project : undefined}
          draft={"draft" in editing ? editing.draft : undefined}
          clients={assignableClients(
            catalog.clients,
            "project" in editing ? editing.project.clientId : undefined,
          )}
          usedColors={catalog.projects.map((project) => project.color)}
          onClose={() => setEditing(null)}
          onSaved={() => void reloadAll()}
        />
      )}
    </div>
  );
}

function SuggestionStrip({
  suggestion,
  onCreate,
  onDismiss,
}: {
  suggestion: ProjectSuggestion;
  onCreate: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="mt-3 flex items-center gap-3 rounded-lg border border-accent/30 bg-accent-soft px-3 py-2 text-[12px]">
      <span className="text-accent" aria-hidden="true">
        ✦
      </span>
      <span className="min-w-0 flex-1 truncate text-fg">
        Suggested <b className="text-fg-strong">"{suggestion.name}"</b> ·{" "}
        {formatDuration(suggestion.ms)} in the last {DISCOVERY_DAYS} days ·
        matched{" "}
        <span className="font-mono text-fg-muted">
          {suggestion.evidence.join(", ")}
        </span>
      </span>
      <button type="button" onClick={onCreate} className={BUTTON_PRIMARY}>
        Create
      </button>
      <button
        type="button"
        onClick={onDismiss}
        className="text-fg-soft hover:text-fg"
      >
        Dismiss
      </button>
    </div>
  );
}

function ProjectsTable({
  projects,
  stats,
  catalog,
  now,
  loaded,
  empty,
  tab,
  rangeLabel,
  onOpen,
  onCreate,
}: {
  projects: Project[];
  stats: Map<string, ProjectStats>;
  catalog: Catalog;
  now: number;
  loaded: boolean;
  empty: "first" | "filtered" | "tab";
  tab: ProjectsTab;
  rangeLabel: string;
  onOpen: (project: Project) => void;
  onCreate: () => void;
}) {
  const template =
    "minmax(0,1.6fr) minmax(0,1fr) 110px 80px 90px minmax(150px,1fr) 16px";
  return (
    <div className="shape-bleed-table mt-3 overflow-hidden rounded-xl border border-line bg-panel">
      <div
        className="grid items-center gap-3 border-line border-b bg-surface px-4 py-2 font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider"
        style={{ gridTemplateColumns: template }}
      >
        <span>Project</span>
        <span>Client</span>
        <span>Last activity</span>
        <span>Due</span>
        <Tooltip content={rangeLabel}>
          <span className="text-right">Time</span>
        </Tooltip>
        <span>Budget</span>
        <span />
      </div>
      {projects.length === 0 && loaded ? (
        empty === "first" ? (
          <EmptyState
            title="Create your first project"
            hint="Projects say which work time belongs to. Add hints like a folder or repo and OpenRize assigns matching time for you."
            action={
              <button
                type="button"
                onClick={onCreate}
                className={BUTTON_PRIMARY}
              >
                + New project
              </button>
            }
          />
        ) : (
          <EmptyState
            title={
              empty === "filtered" ? "No projects match" : `No ${tab} projects`
            }
          />
        )
      ) : (
        projects.map((project) => {
          const projectStats = stats.get(project.id);
          const client = project.clientId
            ? catalog.clientById.get(project.clientId)
            : undefined;
          const budget = budgetUsage(project, projectStats, client);
          const overdue =
            project.dueDate !== undefined &&
            project.dueDate !== null &&
            project.dueDate < now &&
            project.status === "active";
          return (
            <button
              key={project.id}
              type="button"
              onClick={() => onOpen(project)}
              className="grid min-h-[48px] w-full items-center gap-3 border-line-soft border-b px-4 py-2.5 text-left text-[12.5px] transition-colors last:border-b-0 hover:bg-surface"
              style={{ gridTemplateColumns: template }}
            >
              <span className="flex min-w-0 items-center gap-2">
                <Dot color={project.color} size={9} />
                <span className="truncate font-medium text-fg-strong">
                  {project.name}
                </span>
              </span>
              <span className="truncate text-fg-soft">
                {client?.name ?? "–"}
              </span>
              <span className="text-[12px] text-fg-soft">
                {projectStats?.lastActivity
                  ? formatRelative(projectStats.lastActivity, now)
                  : "–"}
              </span>
              <span
                className={`text-[12px] ${overdue ? "font-semibold text-danger" : "text-fg-soft"}`}
              >
                {project.dueDate ? formatShortDate(project.dueDate, now) : "–"}
              </span>
              <span className="text-right font-mono text-fg-muted tabular-nums">
                {formatDuration(projectStats?.rangeMs ?? 0)}
              </span>
              <span className="min-w-0">
                {budget ? (
                  <span className="flex flex-col gap-1">
                    <span
                      className={`truncate font-mono text-[11px] tabular-nums ${
                        budget.ratio !== undefined &&
                        budget.ratio >= BUDGET_WARN
                          ? "text-danger"
                          : "text-fg-soft"
                      }`}
                    >
                      {budget.label}
                      {budget.ratio !== undefined &&
                        budget.ratio > 1 &&
                        " · over"}
                    </span>
                    {budget.ratio !== undefined && (
                      <Progress
                        value={budget.ratio}
                        warnAt={BUDGET_WARN}
                        label={`${project.name} budget used`}
                      />
                    )}
                  </span>
                ) : (
                  <span className="text-[11.5px] text-fg-faint">no budget</span>
                )}
              </span>
              <span className="text-fg-faint">›</span>
            </button>
          );
        })
      )}
    </div>
  );
}
