import type { Client, InvoiceSummary, Project, ProjectStats } from "./types";

const HOUR_MS = 3_600_000;

/** The clients a project can be assigned to: not archived, plus the one it already has. */
export function assignableClients(clients: Client[], keep?: string): Client[] {
  return clients.filter((client) => !client.archivedAt || client.id === keep);
}

/** A client's name as pickers list it. */
export function clientLabel(client: Client): string {
  return client.archivedAt ? `${client.name} (archived)` : client.name;
}

/** A project's hourly rate, falling back to its client's default. */
function projectRate(project: Project, client: Client): number | undefined {
  return project.hourlyRate ?? client.defaultRate ?? undefined;
}

/** One client's projects, time, ready-to-bill time, and invoices. */
export interface ClientRollup {
  client: Client;
  projects: Project[];
  activeProjects: number;
  totalMs: number;
  /** Time in the page's selected range. */
  rangeMs: number;
  unbilledMs: number;
  unbilledEntries: number;
  /** What the ready-to-bill time comes to at each project's rate, in the client's currency; projects with no rate are left out. */
  unbilledAmount: number;
  /** Some ready-to-bill time has no rate to price it. */
  unpriced: boolean;
  lastActivity?: number;
  invoiceCount: number;
  /** Issued (open or paid) invoices, in USD cents. */
  invoicedCents: number;
  /** Issued but not yet paid, in USD cents. */
  outstandingCents: number;
}

/** Rolls project stats and invoices up to their client. Deleted projects and invoice drafts and voids are not counted as work billed. */
export function rollupClients(
  clients: Client[],
  projects: Project[],
  stats: Map<string, ProjectStats>,
  invoices: InvoiceSummary[],
): Map<string, ClientRollup> {
  const rollups = new Map<string, ClientRollup>();
  for (const client of clients) {
    const own = projects.filter((project) => project.clientId === client.id);
    const rollup: ClientRollup = {
      client,
      projects: own,
      activeProjects: own.filter((project) => project.status === "active")
        .length,
      totalMs: 0,
      rangeMs: 0,
      unbilledMs: 0,
      unbilledEntries: 0,
      unbilledAmount: 0,
      unpriced: false,
      invoiceCount: 0,
      invoicedCents: 0,
      outstandingCents: 0,
    };
    for (const project of own) {
      const projectStats = stats.get(project.id);
      if (!projectStats) continue;
      rollup.totalMs += projectStats.totalMs;
      rollup.rangeMs += projectStats.rangeMs;
      rollup.unbilledMs += projectStats.unbilledMs;
      rollup.unbilledEntries += projectStats.unbilledEntries;
      const rate = projectRate(project, client);
      if (projectStats.unbilledMs > 0) {
        if (rate === undefined) rollup.unpriced = true;
        else
          rollup.unbilledAmount += (projectStats.unbilledMs / HOUR_MS) * rate;
      }
      if (projectStats.lastActivity) {
        rollup.lastActivity = Math.max(
          rollup.lastActivity ?? 0,
          projectStats.lastActivity,
        );
      }
    }
    rollups.set(client.id, rollup);
  }
  for (const invoice of invoices) {
    const rollup = rollups.get(invoice.clientId);
    if (!rollup) continue;
    rollup.invoiceCount += 1;
    if (invoice.status === "open" || invoice.status === "paid") {
      rollup.invoicedCents += invoice.totalCents;
    }
    if (invoice.status === "open")
      rollup.outstandingCents += invoice.totalCents;
  }
  return rollups;
}
