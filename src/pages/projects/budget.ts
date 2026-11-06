import { formatMoney } from "../../lib/format";
import { PALETTE } from "../../lib/palette";
import type { Client, Project, ProjectStats } from "../../lib/types";

const HOUR_MS = 3_600_000;

export const PROJECT_COLORS = PALETTE;

export interface BudgetUsage {
  label: string;
  ratio?: number;
}

export function rateFor(project: Project, client?: Client): number | undefined {
  return project.hourlyRate ?? client?.defaultRate ?? undefined;
}

export function currencyFor(client?: Client): string {
  return client?.currency || "USD";
}

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

export const BUDGET_WARN = 0.8;
