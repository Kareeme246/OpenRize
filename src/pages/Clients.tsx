import { useCallback, useEffect, useMemo, useState } from "react";
import {
  BUTTON_PRIMARY,
  EmptyState,
  InlineError,
  PageHeader,
  StatCard,
  Tabs,
} from "../components/Page";
import { Picker } from "../components/Picker";
import { Tooltip } from "../components/Tooltip";
import { useCatalog } from "../hooks/useCatalog";
import { useTauriEvent } from "../hooks/useTauriEvent";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { rollupClients } from "../lib/clients";
import { startOfMonth } from "../lib/dates";
import {
  formatDuration,
  formatMoney,
  formatRelative,
  plural,
} from "../lib/format";
import { formatUsd } from "../lib/invoices";
import type {
  Client,
  ClientsTab,
  InvoiceSummary,
  ProjectStats,
  ProjectsRange,
  Route,
} from "../lib/types";
import { ClientDetail } from "./clients/ClientDetail";
import { ClientSheet } from "./clients/ClientSheet";
import { RANGE_OPTIONS, rangeBounds } from "./projects/range";

type ClientsRoute = Extract<Route, { name: "clients" }>;

interface ClientsProps {
  route: ClientsRoute;
  navigate: (route: Route) => void;
  replace: (route: Route) => void;
}

/** Editing state for the sheet: a client, or a new one. */
type Editing = { client: Client } | "new" | null;

const TEMPLATE =
  "minmax(0,1.5fr) 80px 110px 90px minmax(100px,1fr) minmax(100px,1fr) 100px 16px";

/**
 * Clients, the Projects page's counterpart: a scannable list with each
 * client's projects, time, ready-to-bill time, and invoiced total, and a
 * detail view per client. Assignment stays project-to-client: a client's page
 * writes `Project.clientId`.
 */
export function Clients({ route, navigate, replace }: ClientsProps) {
  const catalog = useCatalog();
  const tab: ClientsTab = route.tab ?? "active";
  const range: ProjectsRange = route.range ?? "month";

  const [stats, setStats] = useState<Map<string, ProjectStats>>(new Map());
  const [invoices, setInvoices] = useState<InvoiceSummary[]>([]);
  const [search, setSearch] = useState("");
  const [editing, setEditing] = useState<Editing>(null);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const now = Date.now();

  const loadNumbers = useCallback(async (): Promise<void> => {
    const bounds = rangeBounds(range);
    try {
      const [list, issued] = await Promise.all([
        api.projectStats(
          bounds.start,
          bounds.end,
          startOfMonth(new Date()).getTime(),
        ),
        api.listInvoices(),
      ]);
      setStats(new Map(list.map((entry) => [entry.projectId, entry])));
      setInvoices(issued);
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setLoaded(true);
    }
  }, [range]);

  useEffect(() => {
    loadNumbers();
  }, [loadNumbers]);

  useTauriEvent(api.ENTRIES_CHANGED, () => void loadNumbers());

  const reloadAll = useCallback(async (): Promise<void> => {
    await Promise.all([catalog.reload(), loadNumbers()]);
  }, [catalog, loadNumbers]);

  const setRoute = (patch: Partial<ClientsRoute>): void =>
    replace({ ...route, ...patch });

  const rollups = useMemo(
    () => rollupClients(catalog.clients, catalog.projects, stats, invoices),
    [catalog.clients, catalog.projects, stats, invoices],
  );

  const counts = useMemo(
    () => ({
      active: catalog.clients.filter((client) => !client.archivedAt).length,
      archived: catalog.clients.filter((client) => client.archivedAt).length,
    }),
    [catalog.clients],
  );

  const shown = useMemo(() => {
    const query = search.trim().toLowerCase();
    return catalog.clients
      .filter((client) => (client.archivedAt ? "archived" : "active") === tab)
      .filter(
        (client) =>
          query === "" ||
          client.name.toLowerCase().includes(query) ||
          client.email?.toLowerCase().includes(query) ||
          rollups
            .get(client.id)
            ?.projects.some((project) =>
              project.name.toLowerCase().includes(query),
            ),
      )
      .sort(
        (a, b) =>
          (rollups.get(b.id)?.lastActivity ?? 0) -
            (rollups.get(a.id)?.lastActivity ?? 0) ||
          a.name.localeCompare(b.name),
      );
  }, [catalog.clients, rollups, tab, search]);

  // The summary above the list covers the clients in the current tab.
  const summary = useMemo(() => {
    const totals = { rangeMs: 0, unbilledMs: 0, outstandingCents: 0 };
    for (const client of catalog.clients) {
      if ((client.archivedAt ? "archived" : "active") !== tab) continue;
      const rollup = rollups.get(client.id);
      if (!rollup) continue;
      totals.rangeMs += rollup.rangeMs;
      totals.unbilledMs += rollup.unbilledMs;
      totals.outstandingCents += rollup.outstandingCents;
    }
    return totals;
  }, [catalog.clients, rollups, tab]);

  const rangeLabel =
    RANGE_OPTIONS.find((option) => option.value === range)?.label ?? "";
  const open = route.clientId
    ? catalog.clients.find((client) => client.id === route.clientId)
    : undefined;
  const openRollup = open ? rollups.get(open.id) : undefined;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-canvas text-fg">
      <PageHeader title="Clients" crumb={open?.name}>
        <button
          type="button"
          onClick={() => setEditing("new")}
          className={BUTTON_PRIMARY}
        >
          + New client
        </button>
      </PageHeader>

      {open && openRollup ? (
        <div className="flex min-h-0 flex-1 flex-col">
          {(error || catalog.error) && (
            <div className="px-5 pt-4">
              <InlineError
                message={error ?? catalog.error ?? ""}
                onRetry={reloadAll}
              />
            </div>
          )}
          <ClientDetail
            key={open.id}
            client={open}
            rollup={openRollup}
            catalog={catalog}
            stats={stats}
            invoices={invoices}
            rangeLabel={rangeLabel}
            onBack={() => navigate({ name: "clients", tab, range })}
            onEdit={() => setEditing({ client: open })}
            onOpenProject={(project) =>
              navigate({ name: "projects", projectId: project.id })
            }
            onOpenInvoice={(invoiceId) =>
              navigate({ name: "invoices", invoiceId })
            }
            onInvoice={() =>
              navigate({ name: "invoices", compose: { clientId: open.id } })
            }
            onChanged={() => void reloadAll()}
          />
        </div>
      ) : (
        <main className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          <Tabs
            label="Client status"
            tabs={[
              {
                value: "active" as const,
                label: "Active",
                count: counts.active,
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

          <div className="shape-strip mt-3 grid grid-cols-4 gap-3">
            <StatCard
              label={tab === "active" ? "Active clients" : "Archived clients"}
              value={String(counts[tab])}
            />
            <StatCard
              label="Time tracked"
              value={formatDuration(summary.rangeMs)}
              sub={rangeLabel}
            />
            <StatCard
              label="Ready to invoice"
              value={formatDuration(summary.unbilledMs)}
              tone={summary.unbilledMs > 0 ? "accent" : undefined}
              sub="Approved billable time"
            />
            <StatCard
              label="Outstanding"
              value={formatUsd(summary.outstandingCents)}
              tone={summary.outstandingCents > 0 ? "review" : undefined}
              sub="Open invoices"
            />
          </div>

          <div className="mt-3 flex flex-wrap items-center gap-2">
            <input
              type="search"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              aria-label="Search clients"
              placeholder="⌕ Search clients"
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

          <div className="shape-bleed-table mt-3 overflow-hidden rounded-xl border border-line bg-panel">
            <div
              className="grid items-center gap-3 border-line border-b bg-surface px-4 py-2 font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider"
              style={{ gridTemplateColumns: TEMPLATE }}
            >
              <span>Client</span>
              <span className="text-right">Projects</span>
              <span>Last activity</span>
              <Tooltip content={rangeLabel}>
                <span className="text-right">Time</span>
              </Tooltip>
              <span className="text-right">Unbilled</span>
              <span className="text-right">Invoiced</span>
              <span className="text-right">Rate</span>
              <span />
            </div>
            {shown.length === 0 && loaded ? (
              catalog.clients.length === 0 ? (
                <EmptyState
                  title="Add your first client"
                  hint="Clients own the projects you bill. Give one a default rate and currency and its projects fall back to it."
                  action={
                    <button
                      type="button"
                      onClick={() => setEditing("new")}
                      className={BUTTON_PRIMARY}
                    >
                      + New client
                    </button>
                  }
                />
              ) : (
                <EmptyState
                  title={search ? "No clients match" : `No ${tab} clients`}
                />
              )
            ) : (
              shown.map((client) => {
                const rollup = rollups.get(client.id);
                const currency = client.currency || "USD";
                return (
                  <button
                    key={client.id}
                    type="button"
                    onClick={() =>
                      navigate({
                        name: "clients",
                        tab,
                        range,
                        clientId: client.id,
                      })
                    }
                    className="grid min-h-[48px] w-full items-center gap-3 border-line-soft border-b px-4 py-2.5 text-left text-[12.5px] transition-colors last:border-b-0 hover:bg-surface"
                    style={{ gridTemplateColumns: TEMPLATE }}
                  >
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate font-medium text-fg-strong">
                        {client.name}
                      </span>
                      {client.email && (
                        <span className="truncate text-[11px] text-fg-faint">
                          {client.email}
                        </span>
                      )}
                    </span>
                    <Tooltip
                      content={`${plural(rollup?.activeProjects ?? 0, "active project")}`}
                    >
                      <span className="text-right font-mono text-fg-muted tabular-nums">
                        {rollup?.projects.length ?? 0}
                      </span>
                    </Tooltip>
                    <span className="text-[12px] text-fg-soft">
                      {rollup?.lastActivity
                        ? formatRelative(rollup.lastActivity, now)
                        : "–"}
                    </span>
                    <span className="text-right font-mono text-fg-muted tabular-nums">
                      {formatDuration(rollup?.rangeMs ?? 0)}
                    </span>
                    <span className="flex flex-col items-end">
                      {rollup && rollup.unbilledMs > 0 ? (
                        <>
                          <span className="font-mono text-accent tabular-nums">
                            {formatDuration(rollup.unbilledMs)}
                          </span>
                          {rollup.unbilledAmount > 0 && (
                            <span className="font-mono text-[11px] text-fg-faint tabular-nums">
                              {formatMoney(rollup.unbilledAmount, currency)}
                            </span>
                          )}
                        </>
                      ) : (
                        <span className="text-fg-faint">–</span>
                      )}
                    </span>
                    <span className="flex flex-col items-end">
                      {rollup && rollup.invoicedCents > 0 ? (
                        <>
                          <span className="font-mono text-fg-muted tabular-nums">
                            {formatUsd(rollup.invoicedCents)}
                          </span>
                          {rollup.outstandingCents > 0 && (
                            <span className="font-mono text-[11px] text-review tabular-nums">
                              {formatUsd(rollup.outstandingCents)} due
                            </span>
                          )}
                        </>
                      ) : (
                        <span className="text-fg-faint">–</span>
                      )}
                    </span>
                    <span className="text-right font-mono text-fg-muted tabular-nums">
                      {client.defaultRate
                        ? `${formatMoney(client.defaultRate, currency)}/h`
                        : "–"}
                    </span>
                    <span className="text-fg-faint">›</span>
                  </button>
                );
              })
            )}
          </div>
        </main>
      )}

      {editing && (
        <ClientSheet
          client={editing === "new" ? undefined : editing.client}
          onClose={() => setEditing(null)}
          onSaved={() => void reloadAll()}
        />
      )}
    </div>
  );
}
