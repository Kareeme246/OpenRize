import { useMemo, useState } from "react";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import {
  BUTTON_SECONDARY,
  Dot,
  EmptyState,
  InlineError,
  StatCard,
} from "../../components/Page";
import { Picker } from "../../components/Picker";
import type { Catalog } from "../../hooks/useCatalog";
import * as api from "../../lib/api";
import { describeError } from "../../lib/api";
import type { ClientRollup } from "../../lib/clients";
import {
  formatDuration,
  formatMoney,
  formatRelative,
  formatShortDate,
  plural,
} from "../../lib/format";
import { displayStatus, formatUsd } from "../../lib/invoices";
import type {
  Client,
  InvoiceSummary,
  Project,
  ProjectStats,
} from "../../lib/types";
import { StatusPill } from "../invoices/StatusPill";

const RECENT_INVOICES = 6;

interface ClientDetailProps {
  client: Client;
  rollup: ClientRollup;
  catalog: Catalog;
  stats: Map<string, ProjectStats>;
  invoices: InvoiceSummary[];
  rangeLabel: string;
  onBack: () => void;
  onEdit: () => void;
  onOpenProject: (project: Project) => void;
  onOpenInvoice: (id: string) => void;
  /** Start an invoice for this client. */
  onInvoice: () => void;
  onChanged: () => void;
}

/**
 * One client: its contact and billing details, its numbers, the projects
 * assigned to it (attach an existing one, detach another), its invoices, and
 * the Edit / Archive / Delete actions.
 */
export function ClientDetail({
  client,
  rollup,
  catalog,
  stats,
  invoices,
  rangeLabel,
  onBack,
  onEdit,
  onOpenProject,
  onOpenInvoice,
  onInvoice,
  onChanged,
}: ClientDetailProps) {
  const [actionError, setActionError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  // Rust sends `null` for a client that is not archived.
  const archived = Boolean(client.archivedAt);
  const currency = client.currency || "USD";
  const now = Date.now();

  const run = async (action: () => Promise<unknown>): Promise<boolean> => {
    try {
      await action();
      setActionError(null);
      onChanged();
      return true;
    } catch (cause) {
      setActionError(describeError(cause));
      return false;
    }
  };

  const remove = async (): Promise<void> => {
    setConfirmDelete(false);
    if (await run(() => api.deleteClient(client.id))) onBack();
  };

  const attachable = useMemo(
    () =>
      catalog.projects
        .filter((project) => project.clientId !== client.id)
        .map((project) => {
          const other = project.clientId
            ? catalog.clientById.get(project.clientId)
            : undefined;
          return {
            value: project.id,
            // A project has one client: choosing one that has another moves it.
            label: other ? `${project.name} (now ${other.name})` : project.name,
            color: project.color,
          };
        }),
    [catalog.projects, catalog.clientById, client.id],
  );

  const recentInvoices = useMemo(
    () =>
      invoices
        .filter((invoice) => invoice.clientId === client.id)
        .sort((a, b) => b.createdAt - a.createdAt)
        .slice(0, RECENT_INVOICES),
    [invoices, client.id],
  );

  const projects = useMemo(
    () =>
      [...rollup.projects].sort(
        (a, b) =>
          (stats.get(b.id)?.lastActivity ?? 0) -
          (stats.get(a.id)?.lastActivity ?? 0),
      ),
    [rollup.projects, stats],
  );

  const details: { label: string; value?: string }[] = [
    { label: "Email", value: client.email },
    { label: "Address", value: client.address },
    {
      label: "Default rate",
      value:
        client.defaultRate !== undefined && client.defaultRate !== null
          ? `${formatMoney(client.defaultRate, currency)}/h`
          : undefined,
    },
    { label: "Currency", value: client.currency },
    { label: "Notes", value: client.notes },
  ];

  return (
    <main className="min-h-0 min-w-0 flex-1 overflow-y-auto px-5 py-4">
      <button
        type="button"
        onClick={onBack}
        className="text-[12px] text-fg-soft hover:text-fg"
      >
        ‹ Clients
      </button>
      <div className="mt-2 flex flex-wrap items-center gap-3">
        <h2 className="font-semibold text-[18px] text-fg-strong">
          {client.name}
        </h2>
        <span className="rounded-full border border-line px-2 py-0.5 text-[10.5px] text-fg-soft">
          {archived ? "Archived" : "Active"}
        </span>
        <span className="text-[12px] text-fg-soft">
          {[
            client.email,
            client.defaultRate
              ? `${formatMoney(client.defaultRate, currency)}/h`
              : undefined,
          ]
            .filter(Boolean)
            .join(" · ")}
        </span>
        <div className="ml-auto flex flex-wrap gap-2">
          <button type="button" onClick={onEdit} className={BUTTON_SECONDARY}>
            Edit
          </button>
          <button
            type="button"
            onClick={() =>
              void run(() =>
                api.updateClient(client.id, { archived: !archived }),
              )
            }
            className={BUTTON_SECONDARY}
          >
            {archived ? "Restore" : "Archive"}
          </button>
          <button
            type="button"
            onClick={() => setConfirmDelete(true)}
            disabled={rollup.invoiceCount > 0}
            title={
              rollup.invoiceCount > 0
                ? "This client has invoices, so it can only be archived"
                : "Delete client"
            }
            className={`${BUTTON_SECONDARY} text-danger`}
          >
            Delete
          </button>
        </div>
      </div>
      {archived && (
        <p className="mt-2 rounded-lg border border-line bg-surface px-3 py-2 text-[12px] text-fg-soft">
          Archived clients are left out of the client pickers on projects and
          invoices, and can't take new projects. Their projects stay assigned
          and visible. Restore the client to use it again.
        </p>
      )}
      {actionError && (
        <div className="mt-3">
          <InlineError message={actionError} />
        </div>
      )}

      <div className="mt-4 grid grid-cols-4 gap-3">
        <StatCard
          label="Projects"
          value={String(rollup.projects.length)}
          sub={`${rollup.activeProjects} active`}
        />
        <StatCard
          label="Time tracked"
          value={formatDuration(rollup.totalMs)}
          sub={`${formatDuration(rollup.rangeMs)} ${rangeLabel.toLowerCase()}`}
        />
        <StatCard
          label="Ready to invoice"
          value={formatDuration(rollup.unbilledMs)}
          tone={rollup.unbilledMs > 0 ? "accent" : undefined}
          sub={
            rollup.unbilledMs === 0
              ? "No approved billable time"
              : `${plural(rollup.unbilledEntries, "entry", "entries")}${
                  rollup.unbilledAmount > 0
                    ? ` · ${formatMoney(rollup.unbilledAmount, currency)}`
                    : ""
                }${rollup.unpriced ? " · some unpriced" : ""}`
          }
        />
        <StatCard
          label="Invoiced"
          value={formatUsd(rollup.invoicedCents)}
          tone={rollup.outstandingCents > 0 ? "review" : undefined}
          sub={
            rollup.invoiceCount === 0
              ? "No invoices yet"
              : `${formatUsd(rollup.outstandingCents)} outstanding`
          }
        />
      </div>

      {rollup.unbilledEntries > 0 && (
        <div className="mt-3 flex flex-wrap items-center gap-3 rounded-xl border border-accent/30 bg-accent-soft px-4 py-3 text-[12px]">
          <p className="min-w-0 flex-1 text-fg">
            <span className="font-semibold">
              {formatDuration(rollup.unbilledMs)} ready to invoice
            </span>{" "}
            across{" "}
            {plural(
              rollup.unbilledEntries,
              "approved entry",
              "approved entries",
            )}
          </p>
          <button
            type="button"
            onClick={onInvoice}
            className="rounded-md bg-accent px-3 py-1.5 font-semibold text-[12px] text-accent-fg hover:opacity-90"
          >
            Create invoice
          </button>
        </div>
      )}

      <div className="mt-4 grid grid-cols-1 gap-3 xl:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)]">
        <section className="rounded-xl border border-line bg-panel">
          <div className="flex items-center gap-3 border-line border-b px-4 py-2">
            <h3 className="min-w-0 flex-1 font-semibold text-[12.5px] text-fg-strong">
              Projects
            </h3>
            <Picker
              ariaLabel="Attach a project"
              value=""
              placeholder="+ Attach project"
              disabled={archived || attachable.length === 0}
              options={attachable}
              onChange={(projectId) =>
                void run(() =>
                  api.updateProject(projectId, { clientId: client.id }),
                )
              }
              variant="compact"
            />
          </div>
          {projects.length === 0 ? (
            <EmptyState
              title="No projects assigned"
              hint={
                archived
                  ? "Restore this client to attach projects."
                  : "Attach an existing project. A project has one client, set here or when you edit the project."
              }
            />
          ) : (
            <div>
              <div className="grid grid-cols-[minmax(0,1.5fr)_72px_84px_84px_64px] items-center gap-3 border-line-soft border-b px-4 py-1.5 font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
                <span>Project</span>
                <span>Status</span>
                <span className="text-right">Time</span>
                <span className="text-right">Ready</span>
                <span />
              </div>
              {projects.map((project) => {
                const projectStats = stats.get(project.id);
                return (
                  <div
                    key={project.id}
                    className="grid min-h-[44px] grid-cols-[minmax(0,1.5fr)_72px_84px_84px_64px] items-center gap-3 border-line-soft border-b px-4 py-2 text-[12.5px] last:border-b-0 hover:bg-surface"
                  >
                    <button
                      type="button"
                      onClick={() => onOpenProject(project)}
                      className="flex min-w-0 flex-col text-left"
                    >
                      <span className="flex min-w-0 items-center gap-2">
                        <Dot color={project.color} size={9} />
                        <span className="truncate font-medium text-fg-strong">
                          {project.name}
                        </span>
                      </span>
                      <span className="truncate pl-[17px] text-[11px] text-fg-faint">
                        {projectStats?.lastActivity
                          ? `Active ${formatRelative(projectStats.lastActivity, now)}`
                          : "No activity yet"}
                        {project.dueDate
                          ? ` · Due ${formatShortDate(project.dueDate, now)}`
                          : ""}
                      </span>
                    </button>
                    <span className="text-[11.5px] text-fg-soft capitalize">
                      {project.status}
                    </span>
                    <span className="text-right font-mono text-fg-muted tabular-nums">
                      {formatDuration(projectStats?.totalMs ?? 0)}
                    </span>
                    <span className="text-right font-mono text-fg-muted tabular-nums">
                      {projectStats?.unbilledMs
                        ? formatDuration(projectStats.unbilledMs)
                        : "–"}
                    </span>
                    <button
                      type="button"
                      onClick={() =>
                        void run(() =>
                          api.updateProject(project.id, { clientId: null }),
                        )
                      }
                      title={`Remove ${project.name} from ${client.name}`}
                      className="justify-self-end text-[11.5px] text-fg-soft hover:text-danger"
                    >
                      Detach
                    </button>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <div className="space-y-3">
          <section className="rounded-xl border border-line bg-panel p-4">
            <h3 className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
              Details
            </h3>
            <dl className="mt-2 space-y-2 text-[12px]">
              {details.map((detail) => (
                <div key={detail.label} className="flex gap-3">
                  <dt className="w-24 shrink-0 text-fg-faint">
                    {detail.label}
                  </dt>
                  <dd
                    className={`min-w-0 flex-1 whitespace-pre-line break-words ${
                      detail.value ? "text-fg" : "text-fg-faint"
                    }`}
                  >
                    {detail.value || "–"}
                  </dd>
                </div>
              ))}
            </dl>
          </section>

          <section className="rounded-xl border border-line bg-panel">
            <h3 className="border-line border-b px-4 py-2.5 font-semibold text-[12.5px] text-fg-strong">
              Invoices
            </h3>
            {recentInvoices.length === 0 ? (
              <EmptyState
                title="No invoices yet"
                hint="Invoices for this client's approved billable time show up here."
              />
            ) : (
              <div className="p-1.5">
                {recentInvoices.map((invoice) => (
                  <button
                    key={invoice.id}
                    type="button"
                    onClick={() => onOpenInvoice(invoice.id)}
                    className="flex w-full items-center gap-3 rounded px-2.5 py-1.5 text-left text-[12px] hover:bg-surface"
                  >
                    <span className="min-w-0 flex-1 truncate text-fg-strong">
                      {invoice.number ?? "Draft"}
                    </span>
                    <StatusPill status={displayStatus(invoice)} />
                    <span className="w-20 shrink-0 text-right font-mono text-fg-muted tabular-nums">
                      {formatUsd(invoice.totalCents)}
                    </span>
                  </button>
                ))}
              </div>
            )}
          </section>
        </div>
      </div>

      {confirmDelete && (
        <ConfirmDialog
          title={`Delete ${client.name}?`}
          body={
            rollup.projects.length > 0
              ? `${plural(rollup.projects.length, "project")} will stay, with their time and rules intact, but will no longer have a client. Archive the client instead to keep them assigned. This cannot be undone.`
              : "This client has no projects. This cannot be undone."
          }
          confirmLabel="Delete"
          onCancel={() => setConfirmDelete(false)}
          onConfirm={() => void remove()}
        />
      )}
    </main>
  );
}
