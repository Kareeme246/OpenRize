/**
 * Which installer downloads were plausibly a person. The rule is deliberately strict, so the number
 * errs low: a successful full-file GET of an installer, from a user agent that is a known browser and
 * not a verified bot, counted once per (client IP, platform, UTC day).
 */

export type Platform = "macos" | "windows";

export const PLATFORMS: readonly Platform[] = ["macos", "windows"];

/** One Cloudflare group: requests sharing a day, installer type, user agent, bot category and client. */
export interface DownloadRow {
  /** UTC day, `YYYY-MM-DD`. */
  date: string;
  /** Cloudflare's `edgeResponseContentTypeName`: `dmg` is the macOS installer, `bin` the Windows one. */
  contentType: string;
  /** Cloudflare's `userAgentBrowser`, such as `Chrome`, `Curl` or `Unknown`. */
  browser: string;
  /** Cloudflare's `verifiedBotCategory`; empty when the client is not a verified bot. */
  botCategory: string;
  /** Used only to dedupe inside `classify`; never returned, stored or printed. */
  clientIP: string;
  count: number;
}

/**
 * Every request lands in exactly one bucket, so `raw` always equals the sum of the other four and the
 * reader can see what each filter removed.
 */
export interface Tally {
  raw: number;
  likelyHuman: number;
  /** Cloudflare flags the client as a verified bot (search engines, AI crawlers, SEO tools). */
  verifiedBot: number;
  /** Not a bot Cloudflare vouches for, but not a known browser either: curl, scripts, unknown clients. */
  nonBrowser: number;
  /** A browser that already counted once for the same IP, platform and day. */
  repeat: number;
}

export type DailyTallies = Record<string, Record<Platform, Tally>>;

/**
 * Cloudflare `userAgentBrowser` names that mean a person's web browser, compared without case or
 * separators. An allowlist, not a denylist: a client Cloudflare names that is not listed here is
 * excluded rather than flattered into the human count.
 */
const BROWSERS = new Set([
  "chrome",
  "chromemobile",
  "chromeios",
  "firefox",
  "firefoxmobile",
  "firefoxios",
  "safari",
  "mobilesafari",
  "edge",
  "edgemobile",
  "opera",
  "operamobile",
  "samsungbrowser",
  "brave",
  "vivaldi",
]);

export function platformOf(contentType: string): Platform {
  if (contentType === "dmg") return "macos";
  if (contentType === "bin") return "windows";
  throw new Error(`unexpected installer content type "${contentType}"`);
}

export function isKnownBrowser(browser: string): boolean {
  return BROWSERS.has(browser.toLowerCase().replace(/[^a-z]/g, ""));
}

export const emptyTally = (): Tally => ({
  raw: 0,
  likelyHuman: 0,
  verifiedBot: 0,
  nonBrowser: 0,
  repeat: 0,
});

export function addTally(a: Tally, b: Tally): Tally {
  return {
    raw: a.raw + b.raw,
    likelyHuman: a.likelyHuman + b.likelyHuman,
    verifiedBot: a.verifiedBot + b.verifiedBot,
    nonBrowser: a.nonBrowser + b.nonBrowser,
    repeat: a.repeat + b.repeat,
  };
}

export function classify(rows: readonly DownloadRow[]): DailyTallies {
  const days: DailyTallies = {};
  // Browser requests per (IP, platform, day), counted once and the rest as repeats.
  const browserRequests = new Map<string, number>();
  for (const row of rows) {
    const platform = platformOf(row.contentType);
    days[row.date] ??= { macos: emptyTally(), windows: emptyTally() };
    const tally = (days[row.date] as Record<Platform, Tally>)[platform];
    tally.raw += row.count;
    if (row.botCategory !== "") tally.verifiedBot += row.count;
    else if (!isKnownBrowser(row.browser)) tally.nonBrowser += row.count;
    else {
      const key = JSON.stringify([row.clientIP, platform, row.date]);
      browserRequests.set(key, (browserRequests.get(key) ?? 0) + row.count);
    }
  }
  for (const [key, count] of browserRequests) {
    const [, platform, date] = JSON.parse(key) as [string, Platform, string];
    const tally = (days[date] as Record<Platform, Tally>)[platform];
    tally.likelyHuman += 1;
    tally.repeat += count - 1;
  }
  return days;
}

export function totalByPlatform(days: DailyTallies): Record<Platform, Tally> {
  const totals = { macos: emptyTally(), windows: emptyTally() };
  for (const day of Object.values(days)) {
    for (const platform of PLATFORMS) {
      totals[platform] = addTally(totals[platform], day[platform]);
    }
  }
  return totals;
}
