/**
 * How many people downloaded the OpenRize installer from the website, and how many GitHub downloads there were.
 *
 *   pnpm downloads [--days N] [--refresh] [--json] [--ledger PATH]
 *
 * Website numbers come from Cloudflare's zone analytics (the last --days UTC days, default and maximum
 * 30). They are shown raw and as "likely-human" side by side; classify.ts defines the filter. Client IPs
 * are used in memory to dedupe and are never printed or stored.
 *
 * Auth: CLOUDFLARE_API_TOKEN when set, otherwise the Wrangler login on this machine.
 *
 * Every run also updates a local, git-ignored ledger (default `.download-stats/ledger.json` in the repo
 * root) with one entry per UTC day, so history survives Cloudflare's 31 days. Days already in the
 * ledger are not asked about again (Cloudflare rate-limits this API; --refresh asks about all of them),
 * and re-running replaces a day instead of adding to it. The tables read from the ledger.
 */
import { resolve } from "node:path";
import { parseArgs } from "node:util";
import {
  classify,
  type DailyTallies,
  PLATFORMS,
  type Platform,
  type Tally,
  totalByPlatform,
} from "./classify.ts";
import {
  CloudflareError,
  fetchDailyInstallerGets,
  MAX_DAYS,
  resolveToken,
} from "./cloudflare.ts";
import { fetchReleaseCounts, type ReleaseCounts } from "./github.ts";
import {
  daysBetween,
  daysToFetch,
  readLedger,
  recordSnapshot,
  withEmptyDays,
  writeLedger,
} from "./ledger.ts";

const { values } = parseArgs({
  options: {
    days: { type: "string", default: String(MAX_DAYS) },
    refresh: { type: "boolean", default: false },
    json: { type: "boolean", default: false },
    ledger: {
      type: "string",
      default: resolve(
        import.meta.dirname,
        "../../.download-stats/ledger.json",
      ),
    },
  },
});

const DAY_MS = 86_400_000;
const isoDay = (date: Date) => date.toISOString().slice(0, 10);

function table(rows: string[][]): string {
  const widths = rows[0]?.map((_, i) =>
    Math.max(...rows.map((row) => (row[i] ?? "").length)),
  );
  return rows
    .map((row) =>
      row
        .map((cell, i) =>
          i === 0
            ? cell.padEnd(widths?.[i] ?? 0)
            : cell.padStart(widths?.[i] ?? 0),
        )
        .join("  "),
    )
    .join("\n");
}

const platformRow = (
  label: string,
  totals: Record<Platform, Tally>,
  pick: (tally: Tally) => number,
) => [
  label,
  ...PLATFORMS.map((p) => String(pick(totals[p]))),
  String(PLATFORMS.reduce((sum, p) => sum + pick(totals[p]), 0)),
];

function websiteTable(title: string, days: DailyTallies): string {
  const totals = totalByPlatform(days);
  return table([
    [title, "macOS", "Windows", "Total"],
    platformRow("  likely-human downloads", totals, (t) => t.likelyHuman),
    platformRow("  raw installer GETs (200)", totals, (t) => t.raw),
    platformRow("  excluded: verified bots", totals, (t) => t.verifiedBot),
    platformRow(
      "  excluded: curl, scripts, unknown",
      totals,
      (t) => t.nonBrowser,
    ),
    platformRow("  excluded: repeat clicks", totals, (t) => t.repeat),
  ]);
}

function githubTable(releases: ReleaseCounts[]): string {
  return table([
    [
      "GitHub releases (cumulative)",
      "dmg",
      "exe",
      "updates (tar.gz)",
      "update checks (latest.json)",
    ],
    ...releases.map((r) => [
      `  ${r.tag}`,
      String(r.dmg),
      String(r.exe),
      String(r.updates),
      String(r.updateChecks),
    ]),
  ]);
}

try {
  const days = Number(values.days);
  if (!Number.isInteger(days) || days < 1 || days > MAX_DAYS) {
    throw new CloudflareError(
      `--days must be a whole number from 1 to ${MAX_DAYS}`,
    );
  }
  const ledgerPath = values.ledger as string;
  const now = new Date();
  const today = isoDay(now);
  const from = new Date(`${today}T00:00:00Z`);
  from.setTime(from.getTime() - (days - 1) * DAY_MS);

  const stored = readLedger(ledgerPath);
  const windowDays = daysBetween(isoDay(from), today);
  const asked = daysToFetch(windowDays, stored, today, values.refresh);

  const token = await resolveToken();
  const releases = await fetchReleaseCounts();
  let ledger = recordSnapshot(stored, {}, {}, releases, today);
  // One day at a time, saving after each, so a rate-limited run keeps its progress.
  for (const day of asked) {
    const got = await fetchDailyInstallerGets(token, [day], now);
    ledger = recordSnapshot(
      ledger,
      withEmptyDays(classify(got.rows), [day]),
      got.sampleIntervals,
      undefined,
      today,
    );
    writeLedger(ledgerPath, ledger);
  }
  writeLedger(ledgerPath, ledger);

  const window = Object.fromEntries(
    windowDays.flatMap((day) =>
      ledger.days[day] ? [[day, ledger.days[day]]] : [],
    ),
  ) as DailyTallies;
  const sampled = windowDays.filter((day) => day in ledger.sampled);
  const ledgerDays = Object.keys(ledger.days).sort();
  const windowLabel = `${isoDay(from)} to ${today}`;

  if (values.json) {
    console.log(
      JSON.stringify(
        {
          window: { from: isoDay(from), to: today },
          website: totalByPlatform(window),
          ledger: {
            since: ledgerDays[0],
            website: totalByPlatform(ledger.days),
          },
          sampledDays: Object.fromEntries(
            sampled.map((day) => [day, ledger.sampled[day]]),
          ),
          github: releases,
        },
        null,
        2,
      ),
    );
  } else {
    console.log(
      websiteTable(`Website (Cloudflare, ${windowLabel} UTC)`, window),
    );
    if (sampled.length) {
      console.log(
        `  note: Cloudflare sampled ${sampled.join(", ")}, so those days are estimates`,
      );
    }
    console.log(
      `\n${websiteTable(`Ledger (all days kept since ${ledgerDays[0]})`, ledger.days)}`,
    );
    console.log(`\n${githubTable(releases)}`);
    console.log(
      "\nlikely-human = browser user agent, no verified-bot category, one per IP, platform and day." +
        "\nA download is not an install; GitHub dmg/exe counts overlap the website only before 2026-10-15." +
        `\nLedger: ${ledgerPath}`,
    );
  }
} catch (error) {
  console.error(`error: ${error instanceof Error ? error.message : error}`);
  process.exitCode = 1;
}
