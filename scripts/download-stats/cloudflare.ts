import { execFile } from "node:child_process";
import { promisify } from "node:util";
import type { DownloadRow } from "./classify.ts";

/** The `offlinestudios.net` zone. An identifier, not a secret. */
export const ZONE_ID = "d0eeb20946a56449bc0c25c5054fa64f";

const GRAPHQL_URL = "https://api.cloudflare.com/client/v4/graphql";

/** The most groups one query may return; hitting it would silently drop rows, so it is an error. */
const ROW_LIMIT = 10000;

export class CloudflareError extends Error {}

/** Cloudflare keeps 31 days of this dataset and answers at most 30 days per query. */
export const MAX_DAYS = 30;

export const PERMISSION_HELP = [
  "Create a Cloudflare API token with these permissions and export it as CLOUDFLARE_API_TOKEN:",
  "  Account > Account Analytics > Read",
  "  Zone > Analytics > Read (zone offlinestudios.net)",
  "The R2 token in ~/.config/firstmate-secrets (and the CLOUDFLARE_API_TOKEN the publish workflow uses)",
  "is not that token: it can write the downloads bucket but cannot read analytics.",
].join("\n");

const run = promisify(execFile);

/** `wrangler auth token` prints the logged-in session's token (refreshing it when needed) as its last line. */
async function wranglerLoginToken(): Promise<string | undefined> {
  for (const [command, args] of [
    ["wrangler", ["auth", "token"]],
    ["npx", ["--yes", "wrangler@4", "auth", "token"]],
  ] as const) {
    try {
      const { stdout } = await run(command, [...args], { timeout: 120_000 });
      const token = stdout.trim().split("\n").pop()?.trim();
      if (token && !/\s/.test(token)) return token;
    } catch {
      // Not installed or not logged in; try the next way.
    }
  }
  return undefined;
}

/**
 * CLOUDFLARE_API_TOKEN when set, otherwise the Wrangler login on this machine. The token is only ever
 * sent to Cloudflare and never printed.
 */
export async function resolveToken(
  env: NodeJS.ProcessEnv = process.env,
  wrangler: () => Promise<string | undefined> = wranglerLoginToken,
): Promise<string> {
  const fromEnv = env.CLOUDFLARE_API_TOKEN?.trim();
  if (fromEnv) return fromEnv;
  const fromLogin = await wrangler();
  if (fromLogin) return fromLogin;
  throw new CloudflareError(
    `CLOUDFLARE_API_TOKEN is not set and there is no usable Wrangler login (try \`npx wrangler login\`).\n${PERMISSION_HELP}`,
  );
}

const QUERY = `query ($zone: String!, $from: Time!, $to: Time!) {
  viewer {
    zones(filter: { zoneTag: $zone }) {
      httpRequestsAdaptiveGroups(
        limit: ${ROW_LIMIT}
        filter: {
          datetime_geq: $from
          datetime_lt: $to
          clientRequestPath: "/download/openrize"
          clientRequestHTTPMethodName: "GET"
          edgeResponseStatus: 200
          edgeResponseContentTypeName_in: ["dmg", "bin"]
        }
      ) {
        count
        avg { sampleInterval }
        dimensions {
          date
          edgeResponseContentTypeName
          userAgentBrowser
          verifiedBotCategory
          clientIP
        }
      }
    }
  }
}`;

interface Group {
  count: number;
  avg: { sampleInterval: number };
  dimensions: {
    date: string;
    edgeResponseContentTypeName: string;
    userAgentBrowser: string | null;
    verifiedBotCategory: string | null;
    clientIP: string;
  };
}

interface GraphqlResponse {
  data?: {
    viewer?: {
      zones?: { httpRequestsAdaptiveGroups?: Group[] }[];
    };
  } | null;
  errors?: { message: string; extensions?: { code?: string } }[] | null;
}

export interface InstallerGets {
  rows: DownloadRow[];
  /** Above 1 means Cloudflare sampled some groups, so counts are estimates. */
  maxSampleInterval: number;
}

/** Full-file (200) installer GETs of `/download/openrize` in [from, to), one row per group. */
export async function fetchInstallerGets(
  token: string,
  from: Date,
  to: Date,
  send: typeof fetch = fetch,
): Promise<InstallerGets> {
  const response = await send(GRAPHQL_URL, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      query: QUERY,
      variables: {
        zone: ZONE_ID,
        from: from.toISOString(),
        to: to.toISOString(),
      },
    }),
  });
  const text = await response.text();
  let body: GraphqlResponse = {};
  try {
    body = JSON.parse(text) as GraphqlResponse;
  } catch {
    // Reported below with the HTTP status.
  }
  const errors = body.errors ?? [];
  if (errors.some((e) => /rate limit/i.test(e.message))) {
    throw new CloudflareError(
      "Cloudflare's analytics API rate limit is used up. Wait 5 minutes and run `pnpm downloads` again; days already fetched are saved and are not asked for again.",
    );
  }
  const denied =
    response.status === 401 ||
    response.status === 403 ||
    errors.some(
      (e) =>
        e.extensions?.code === "authz" ||
        /authenticat|authoriz|permission|not allowed/i.test(e.message),
    );
  if (denied) {
    throw new CloudflareError(
      `Cloudflare refused the analytics query (HTTP ${response.status}${errors[0] ? `: ${errors[0].message}` : ""}).\n${PERMISSION_HELP}`,
    );
  }
  if (!response.ok || errors.length) {
    throw new CloudflareError(
      `Cloudflare analytics query failed (HTTP ${response.status}): ${errors[0]?.message ?? text.slice(0, 300)}`,
    );
  }
  const zone = body.data?.viewer?.zones?.[0];
  if (!zone?.httpRequestsAdaptiveGroups) {
    throw new CloudflareError(
      `Cloudflare returned no data for zone ${ZONE_ID}; this is not the same as zero downloads.\n${PERMISSION_HELP}`,
    );
  }
  const groups = zone.httpRequestsAdaptiveGroups;
  if (groups.length >= ROW_LIMIT) {
    throw new CloudflareError(
      `The query hit its ${ROW_LIMIT} group limit, so the counts would be incomplete; use a shorter --days.`,
    );
  }
  return {
    rows: groups.map((g) => ({
      date: g.dimensions.date,
      contentType: g.dimensions.edgeResponseContentTypeName,
      browser: g.dimensions.userAgentBrowser ?? "",
      botCategory: g.dimensions.verifiedBotCategory ?? "",
      clientIP: g.dimensions.clientIP,
      count: g.count,
    })),
    maxSampleInterval: Math.max(1, ...groups.map((g) => g.avg.sampleInterval)),
  };
}

const DAY_MS = 86_400_000;

export interface DailyInstallerGets {
  rows: DownloadRow[];
  /** Days Cloudflare sampled (interval above 1), whose counts are therefore estimates. */
  sampleIntervals: Record<string, number>;
}

/**
 * The same query one UTC day (`YYYY-MM-DD`) at a time, in order, each day ending at `now` at the latest.
 * Cloudflare samples adaptive data more coarsely the wider the time range (one day was exact, three days
 * already estimated at 1 in 10, thirty days at 1 in 100), so a single wide query would invent a smaller,
 * noisier number.
 */
export async function fetchDailyInstallerGets(
  token: string,
  days: readonly string[],
  now: Date,
  send: typeof fetch = fetch,
): Promise<DailyInstallerGets> {
  const all: DailyInstallerGets = { rows: [], sampleIntervals: {} };
  for (const day of days) {
    const start = Date.parse(`${day}T00:00:00Z`);
    const result = await fetchInstallerGets(
      token,
      new Date(start),
      new Date(Math.min(start + DAY_MS, now.getTime())),
      send,
    );
    all.rows.push(...result.rows);
    if (result.maxSampleInterval > 1) {
      all.sampleIntervals[day] = result.maxSampleInterval;
    }
  }
  return all;
}
