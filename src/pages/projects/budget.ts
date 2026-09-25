import { formatMoney } from "../../lib/format";
import { PALETTE } from "../../lib/palette";
import type { Client, Project, ProjectStats } from "../../lib/types";

const HOUR_MS = 3_600_000;

/** The muted palette offered for new projects. */
export const PROJECT_COLORS = PALETTE;

export interface BudgetUsage {
  /** "38.5 / 50h", "$4,620 / $6,000", or "12.0h · no rate". */
  label: string;
  /** Used share of the budget; undefined when it can't be measured. */
  ratio?: number;
}

/** A project's rate, falling back to its client's default. */
export function rateFor(project: Project, client?: Client): number | undefined {
  return project.hourlyRate ?? client?.defaultRate ?? undefined;
}

export function currencyFor(client?: Client): string {
  return client?.currency || "USD";
}

/**
 * Budget used: hours against an hours budget, or billable hours times the
 * rate against an amount budget. A monthly budget counts this month only.
 */
export function budgetUsage(
  project: Project,
  stats: ProjectStats | undefined,
  client?: Client,
): BudgetUsage | null {
  if (project.budgetKind === "none" || !project.budgetValue) return null;
  const monthly = project.budgetPeriod === "monthly";
  const ms = (monthly ? stats?.monthMs : stats?.totalMs) ?? 0;
  const billableMs =
    (monthly ? stats?.billableMonthMs : stats?.billableMs) ?? 0;
  const suffix = monthly ? " /mo" : "";
  if (project.budgetKind === "hours") {
    const used = ms / HOUR_MS;
    return {
      label: `${used.toFixed(1)} / ${project.budgetValue}h${suffix}`,
      ratio: used / project.budgetValue,
    };
  }
  const rate = rateFor(project, client);
  if (rate === undefined) {
    return { label: `${(ms / HOUR_MS).toFixed(1)}h · no rate` };
  }
  const currency = currencyFor(client);
  const spent = (billableMs / HOUR_MS) * rate;
  return {
    label: `${formatMoney(spent, currency)} / ${formatMoney(project.budgetValue, currency)}${suffix}`,
    ratio: spent / project.budgetValue,
  };
}

/** Amber from 80% of the budget. */
export const BUDGET_WARN = 0.8;
