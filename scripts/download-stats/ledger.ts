import {
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
import { dirname } from "node:path";
import { type DailyTallies, emptyTally } from "./classify.ts";
import type { ReleaseCounts } from "./github.ts";

/**
 * A local, git-ignored record that outlives Cloudflare's 31 days of history. It holds only counts:
 * one entry per UTC day for the website and one per UTC day for GitHub's cumulative counters. Writing
 * a day replaces it, so running the command again never counts anything twice.
 */
export interface Ledger {
  version: 1;
  days: DailyTallies;
  /** Days whose counts are Cloudflare estimates, with the sampling interval; absent means exact. */
  sampled: Record<string, number>;
  github: Record<string, ReleaseCounts[]>;
}

export const emptyLedger = (): Ledger => ({
  version: 1,
  days: {},
  sampled: {},
  github: {},
});

/** A missing file is a fresh ledger; an unreadable one is an error, never silently replaced. */
export function readLedger(path: string): Ledger {
  if (!existsSync(path)) return emptyLedger();
  const ledger = JSON.parse(readFileSync(path, "utf8")) as Ledger;
  if (ledger.version !== 1) {
    throw new Error(`${path}: unsupported ledger version ${ledger.version}`);
  }
  return { ...emptyLedger(), ...ledger };
}

/** `YYYY-MM-DD` for every UTC day from `from` through `to`, inclusive. */
export function daysBetween(from: string, to: string): string[] {
  const days: string[] = [];
  for (
    let t = Date.parse(`${from}T00:00:00Z`);
    t <= Date.parse(`${to}T00:00:00Z`);
    t += 86_400_000
  ) {
    days.push(new Date(t).toISOString().slice(0, 10));
  }
  return days;
}

/**
 * Days worth asking Cloudflare about. A finished day does not change, so one already in the ledger is
 * kept (and the rate limiter spared); yesterday is asked again because late events can still land in
 * it, and today is always incomplete.
 */
export function daysToFetch(
  window: readonly string[],
  ledger: Ledger,
  today: string,
  refresh: boolean,
): string[] {
  const yesterday = new Date(Date.parse(`${today}T00:00:00Z`) - 86_400_000)
    .toISOString()
    .slice(0, 10);
  return window.filter(
    (day) => refresh || day >= yesterday || !(day in ledger.days),
  );
}

/** A day Cloudflare answered with no installer downloads is recorded as zeros, so it is not asked again. */
export function withEmptyDays(
  days: DailyTallies,
  asked: readonly string[],
): DailyTallies {
  const filled = { ...days };
  for (const day of asked) {
    filled[day] ??= { macos: emptyTally(), windows: emptyTally() };
  }
  return filled;
}

export function recordSnapshot(
  ledger: Ledger,
  days: DailyTallies,
  sampled: Record<string, number>,
  github: ReleaseCounts[] | undefined,
  today: string,
): Ledger {
  const kept = Object.entries(ledger.sampled).filter(([day]) => !(day in days));
  return {
    version: 1,
    days: { ...ledger.days, ...days },
    sampled: { ...Object.fromEntries(kept), ...sampled },
    github: github ? { ...ledger.github, [today]: github } : ledger.github,
  };
}

export function writeLedger(path: string, ledger: Ledger): void {
  mkdirSync(dirname(path), { recursive: true });
  const sorted: Ledger = {
    version: 1,
    days: Object.fromEntries(Object.entries(ledger.days).sort()),
    sampled: Object.fromEntries(Object.entries(ledger.sampled).sort()),
    github: Object.fromEntries(Object.entries(ledger.github).sort()),
  };
  const temp = `${path}.tmp`;
  writeFileSync(temp, `${JSON.stringify(sorted, null, 2)}\n`);
  renameSync(temp, path);
}
