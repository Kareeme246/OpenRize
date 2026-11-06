/** A local account: a name and its own data folder. No password. */
export interface Account {
  id: string;
  name: string;
  createdAt: number;
}

export interface AccountView extends Account {
  sizeBytes: number;
  /** Listed, but its data folder was moved or deleted outside OpenRize. */
  missing: boolean;
}

export interface AccountsState {
  current: string | null;
  accounts: AccountView[];
  /** The account this launch could not open because its data is gone. */
  missing: Account | null;
  /** True once, on the first read after existing data moved into an account. */
  justAdopted: boolean;
  /** A newer release wrote the accounts: they can be seen but not changed. */
  readOnly: boolean;
}

export interface DataSummary {
  entries: number;
  projects: number;
  clients: number;
  invoices: number;
  stopwatches: number;
  segments: number;
  since: number | null;
}

export interface Labeled {
  id: string;
  label: string;
}

export interface BreakPreview {
  label: string;
  remainingMs: number;
}

export interface SwitchPreview {
  stopwatches: Labeled[];
  break: BreakPreview | null;
}

/** Work an account's last switch away paused, offered back on return. */
export interface PausedOnSwitch {
  at: number;
  stopwatches: Labeled[];
  break: BreakPreview | null;
}

export const MAX_NAME_LENGTH = 40;

const countFormat = new Intl.NumberFormat();

export function formatDataSummary(summary: DataSummary): string {
  const counts: [number, string, string][] = [
    [summary.entries, "time entry", "time entries"],
    [summary.projects, "project", "projects"],
    [summary.clients, "client", "clients"],
    [summary.invoices, "invoice", "invoices"],
    [summary.stopwatches, "stopwatch", "stopwatches"],
  ];
  const parts = counts
    .filter(([count]) => count > 0)
    .map(
      ([count, one, many]) =>
        `${countFormat.format(count)} ${count === 1 ? one : many}`,
    );
  const since =
    summary.since === null
      ? ""
      : `, going back to ${new Date(summary.since).toLocaleDateString(
          undefined,
          { month: "short", day: "numeric", year: "numeric" },
        )}`;
  if (parts.length === 0) {
    return `You have tracked activity from before accounts${since}.`;
  }
  const list =
    parts.length === 1
      ? parts[0]
      : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
  return `You have ${list} from before accounts${since}.`;
}

export function listNames(names: string[], shown = 2): string {
  if (names.length <= shown) {
    return names.length <= 1
      ? (names[0] ?? "")
      : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  }
  return `${names.slice(0, shown).join(", ")} and ${names.length - shown} more`;
}

export function describeBreak(item: BreakPreview): string {
  const minutes = Math.ceil(item.remainingMs / 60_000);
  return minutes < 1 ? item.label : `${item.label} (${minutes} min left)`;
}

export function describePausedWork(
  stopwatches: Labeled[],
  breakItem: BreakPreview | null,
): string {
  const stopwatchNames = stopwatches.map((stopwatch) => stopwatch.label);
  if (breakItem === null) return listNames(stopwatchNames);
  const withBreak = `your ${describeBreak(breakItem)}`;
  if (stopwatchNames.length === 0) return withBreak;
  return `${listNames(stopwatchNames)} and ${withBreak}`;
}

export function accountColor(id: string): string {
  let hash = 0;
  for (const char of id) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return `var(--cat-${(hash % 12) + 1})`;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1_000_000) return `${Math.max(1, Math.round(bytes / 1_000))} KB`;
  if (bytes < 1_000_000_000) return `${Math.round(bytes / 1_000_000)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

/**
 * localStorage key for the Time Entries saved views; per account since they
 * hold its ids. No account keeps the pre-accounts key.
 */
export function entriesViewsKey(accountId: string | null): string {
  return accountId === null
    ? "openrize.entries.views"
    : `openrize.entries.views.${accountId}`;
}
