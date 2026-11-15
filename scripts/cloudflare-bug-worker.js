/**
 * Cloudflare Worker: OpenRize bug report intake.
 *
 * Receives the report a user sends from the in-app form (shape in
 * src-tauri/src/bug_report.rs) and files it as an issue in Kareeme246/OpenRize.
 *
 * The app holds no secret, so anyone can post here. What stands between this
 * address and the issue tracker:
 * - Limits per hour, charged together or not at all: 5 reports per client
 *   address (an IPv6 address counts as its /64), 5 per install id, and 30
 *   across all callers. The limiter is required; without it every report is
 *   refused.
 * - Only JSON posted by a non-browser client is read, up to 64 KB.
 * - The report is rebuilt from the fields the app sends. The user's text is
 *   placed in code blocks, so it cannot mention, link or reference anything,
 *   and diagnostics keep only known keys with known kinds of values.
 * - The address and the install id reach the limiter only as keyed hashes,
 *   and neither appears in the issue.
 *
 * Deploying (needs wrangler and a Cloudflare login):
 * 1. Set the secret `GITHUB_TOKEN`:
 *    `wrangler secret put GITHUB_TOKEN --config scripts/wrangler.bug-worker.jsonc`
 *    Use a token that can only write issues on Kareeme246/OpenRize, and that
 *    belongs to a dedicated bot account or a GitHub App rather than a personal
 *    account: every issue is filed in its owner's name.
 * 2. `wrangler deploy --config scripts/wrangler.bug-worker.jsonc`
 *    The config binds the limiter (`LIMITER`, a Durable Object), so it cannot
 *    be left out.
 * 3. The app posts to the address compiled into it (ENDPOINT in
 *    src-tauri/src/bug_report.rs). A fork points it at its own Worker by
 *    building with OPENRIZE_BUG_ENDPOINT set to
 *    `https://<worker>.workers.dev/report`.
 */

import {
  charge,
  readBody,
  refund,
  refusal,
  reply,
} from "./cloudflare-worker-shared.js";

export { RateLimiter } from "./cloudflare-worker-shared.js";

const REPORTS_PER_ADDRESS = 5;
const REPORTS_PER_INSTALL = 5;
const REPORTS_PER_HOUR = 30;

/**
 * Same cap as MAX_FIELD_CHARS in src-tauri/src/bug_report.rs, counted the same
 * way (UTF-16 units), so the app refuses whatever this would.
 */
export const MAX_FIELD_CHARS = 8000;
/** GitHub refuses an issue body longer than this. */
export const MAX_ISSUE_BODY_CHARS = 65536;
const MAX_TITLE_CHARS = 70;
const MAX_CRASHES = 5;
const MAX_CRASH_MESSAGE_CHARS = 300;

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const TOKEN = /^[0-9A-Za-z.+_-]{1,32}$/;
// Windows reports its version as "25H2 (build 26300)".
const OS_VERSION = /^[0-9A-Za-z.+_() -]{1,64}$/;
const LOCALE = /^[0-9A-Za-z_-]{1,35}$/;
const CRASH_ID = /^[0-9a-fA-F-]{8,64}$/;

const matches = (pattern) => (value) =>
  typeof value === "string" && pattern.test(value) ? value : undefined;
const oneOf =
  (...known) =>
  (value) =>
    known.includes(value) ? value : undefined;
const flag = (value) => (typeof value === "boolean" ? value : undefined);
const integer = (max) => (value) =>
  Number.isInteger(value) && value >= 0 && value <= max ? value : undefined;
const cut = (max) => (value) =>
  typeof value === "string" ? [...value].slice(0, max).join("") : undefined;

// What the app can attach, mirroring Environment, Preferences and CrashSummary
// in src-tauri/src/bug_report.rs. Enum settings arrive as lower-case names.
const ENVIRONMENT = {
  appVersion: matches(TOKEN),
  os: matches(TOKEN),
  osVersion: matches(OS_VERSION),
  arch: matches(TOKEN),
  locale: matches(LOCALE),
  dataFolder: oneOf("local", "legacyRoaming", "other"),
};
const PREFERENCES = {
  theme: oneOf("system", "light", "dark"),
  accent: oneOf("green", "blue", "purple", "orange"),
  shape: oneOf("rounded", "sharper"),
  sizeMode: oneOf("compact", "normal", "relaxed", "veryrelaxed"),
  closeBehavior: oneOf("quit", "hide"),
  trayEnabled: flag,
  retentionDays: integer(0xffffffff),
  aiSuggest: oneOf("category", "categoryproject"),
  autoAccept: flag,
  autoAcceptPercent: integer(0xff),
  weeklyTargetHours: integer(0xffff),
  trackingHoursEnabled: flag,
  breaksEnabled: flag,
  stopwatchReminderEnabled: flag,
  notificationPlacement: oneOf(
    "topleft",
    "topright",
    "topcenter",
    "bottomleft",
    "bottomright",
  ),
  crashReporting: flag,
};
const CRASH = {
  id: matches(CRASH_ID),
  timestampMs: integer(Number.MAX_SAFE_INTEGER),
  appVersion: matches(TOKEN),
  source: oneOf("backend_panic", "frontend_error", "unhandled_rejection"),
  message: cut(MAX_CRASH_MESSAGE_CHARS),
};

const isRecord = (value) =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * The known keys of `value` whose values pass their check. Everything else is
 * left behind, so nothing the app does not send today can reach an issue.
 */
function pick(value, shape) {
  if (!isRecord(value)) return undefined;
  const kept = {};
  for (const [key, check] of Object.entries(shape)) {
    const checked = check(value[key]);
    if (checked !== undefined) kept[key] = checked;
  }
  return Object.keys(kept).length > 0 ? kept : undefined;
}

function freeText(value) {
  if (typeof value !== "string") return undefined;
  const text = value.trim();
  return text && text.length <= MAX_FIELD_CHARS ? text : undefined;
}

/**
 * The report as the app defines it, rebuilt field by field, or null when a
 * field is missing, of the wrong type or too long.
 */
export function parseReport(payload) {
  if (!isRecord(payload)) return null;
  if (typeof payload.installId !== "string" || !UUID.test(payload.installId)) {
    return null;
  }
  const report = {
    installId: payload.installId.toLowerCase(),
    whatHappened: freeText(payload.whatHappened),
    whatExpected: freeText(payload.whatExpected),
    stepsToReproduce: freeText(payload.stepsToReproduce),
  };
  if (Object.values(report).includes(undefined)) return null;

  if (payload.additionalNotes !== undefined) {
    if (typeof payload.additionalNotes !== "string") return null;
    if (payload.additionalNotes.trim()) {
      report.additionalNotes = freeText(payload.additionalNotes);
      if (!report.additionalNotes) return null;
    }
  }

  if (payload.diagnostics !== undefined) {
    if (!isRecord(payload.diagnostics)) return null;
    const { environment, preferences, recentCrashes } = payload.diagnostics;
    report.diagnostics = {
      environment: pick(environment, ENVIRONMENT),
      preferences: pick(preferences, PREFERENCES),
      recentCrashes: (Array.isArray(recentCrashes) ? recentCrashes : [])
        .slice(0, MAX_CRASHES)
        .map((crash) => pick(crash, CRASH))
        .filter(Boolean),
    };
  }
  return report;
}

/** Text that is safe inside an inline code span: no backticks or line breaks. */
function inline(value, fallback = "unknown") {
  const text = String(value ?? "")
    .replace(/[`\r\n]+/g, " ")
    .trim()
    .slice(0, 200);
  return text || fallback;
}

/**
 * A fenced code block whose fence is longer than any run of backticks in the
 * text, so the text cannot close the block and write Markdown around it.
 */
function fenced(text, language = "") {
  const longest = Math.max(
    0,
    ...(text.match(/`+/g) ?? []).map((run) => run.length),
  );
  const fence = "`".repeat(Math.max(3, longest + 1));
  return [`${fence}${language}`, text, fence];
}

function details(summary, text) {
  return [
    "<details>",
    `<summary>${summary}</summary>`,
    "",
    ...fenced(text, "json"),
    "</details>",
    "",
  ];
}

function render(report, fieldChars) {
  const section = (heading, text) => {
    const chars = [...text];
    const shown =
      chars.length > fieldChars
        ? `${chars.slice(0, fieldChars).join("")}\n[cut short]`
        : text;
    return [heading, ...fenced(shown), ""];
  };

  const parts = [
    ...section("### What happened?", report.whatHappened),
    ...section("### What did you expect to happen?", report.whatExpected),
    ...section("### Steps to reproduce", report.stepsToReproduce),
  ];
  if (report.additionalNotes) {
    parts.push(
      ...section("### Additional context / notes", report.additionalNotes),
    );
  }
  parts.push("---", "", "### Diagnostics");

  const {
    environment,
    preferences,
    recentCrashes = [],
  } = report.diagnostics ?? {};
  if (environment || preferences || recentCrashes.length > 0) {
    if (environment) {
      const { appVersion, os, osVersion, arch, locale, dataFolder } =
        environment;
      parts.push(
        `- **App Version:** \`v${inline(appVersion)}\``,
        `- **OS:** \`${inline(os)} ${inline(osVersion, "")} (${inline(arch, "")})\``,
        `- **Locale:** \`${inline(locale)}\``,
        `- **Data folder:** \`${inline(dataFolder)}\``,
      );
    }
    parts.push("");
    if (preferences) {
      parts.push(
        ...details("App Settings", JSON.stringify(preferences, null, 2)),
      );
    }
    if (recentCrashes.length > 0) {
      parts.push(
        ...details(
          `Recent Local Crashes (${recentCrashes.length})`,
          JSON.stringify(recentCrashes, null, 2),
        ),
      );
    }
  } else {
    parts.push("*Diagnostics omitted by user.*", "");
  }

  parts.push("> *Reported via OpenRize in-app bug report.*");
  return parts.join("\n");
}

/**
 * The issue body for a report from `parseReport`. The field and size caps keep
 * it well inside GitHub's limit; should it not fit anyway, the user's text is
 * shortened before it is fenced, so every code block still closes.
 */
export function formatIssueBody(report) {
  let fieldChars = MAX_FIELD_CHARS;
  let body = render(report, fieldChars);
  while (body.length > MAX_ISSUE_BODY_CHARS && fieldChars > 0) {
    fieldChars = Math.floor(fieldChars / 2);
    body = render(report, fieldChars);
  }
  return body;
}

/** GitHub renders no Markdown in a title; this only keeps it tidy. */
export function issueTitle(report) {
  const firstLine = report.whatHappened
    .split(/[\r\n]/)[0]
    .replace(/[`@#<>[\]]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  const title = [...firstLine].slice(0, MAX_TITLE_CHARS).join("").trim();
  return `[Bug Report]: ${title || "Issue reported in OpenRize"}`;
}

/** An IPv4 address as it is; an IPv6 address as the /64 its network owns. */
function addressBucket(address) {
  if (!address) return "unknown";
  if (!address.includes(":")) return address;
  const [head, tail] = address.toLowerCase().split("::");
  const groups = head.split(":").filter(Boolean);
  if (tail !== undefined) {
    const rest = tail.split(":").filter(Boolean);
    const gap = Math.max(0, 8 - groups.length - rest.length);
    groups.push(...Array(gap).fill("0"), ...rest);
  }
  return groups
    .slice(0, 4)
    .map((group) => Number.parseInt(group, 16).toString(16))
    .join(":");
}

/**
 * Limiter keys that name no one: an HMAC under the token the Worker already
 * holds, so stored keys cannot be turned back into an address or install id.
 */
async function limiterKeys(request, report, secret) {
  const encoder = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const hashed = async (text) =>
    Array.from(
      new Uint8Array(
        await crypto.subtle.sign("HMAC", key, encoder.encode(text)),
      ),
      (byte) => byte.toString(16).padStart(2, "0"),
    ).join("");
  // Only Cloudflare sets this header; a forwarded-for value is the caller's.
  const address = addressBucket(request.headers.get("cf-connecting-ip"));
  return [
    { key: await hashed(`address:${address}`), limit: REPORTS_PER_ADDRESS },
    {
      key: await hashed(`install:${report.installId}`),
      limit: REPORTS_PER_INSTALL,
    },
    { key: "all", limit: REPORTS_PER_HOUR },
  ];
}

async function createIssue(report, token) {
  const response = await fetch(
    "https://api.github.com/repos/Kareeme246/OpenRize/issues",
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: "application/vnd.github+json",
        "User-Agent": "OpenRize-Bug-Report-Worker",
        "X-GitHub-Api-Version": "2022-11-28",
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        title: issueTitle(report),
        body: formatIssueBody(report),
        labels: ["bug", "in-app-report"],
      }),
    },
  );
  if (response.status !== 201) {
    const detail = await response.text().catch(() => "");
    throw new Error(
      `GitHub replied ${response.status}: ${detail.slice(0, 500)}`,
    );
  }
  return response;
}

async function handle(request, env) {
  const refused = refusal(request);
  if (refused) return refused;

  let payload;
  try {
    const body = await readBody(request);
    if (body === null) {
      return reply(413, {
        error: "Bug report exceeds maximum size limit (64 KB).",
      });
    }
    payload = JSON.parse(body);
  } catch {
    return reply(400, { error: "Invalid JSON payload." });
  }

  const report = parseReport(payload);
  if (!report) {
    return reply(400, {
      error: `Invalid bug report: installId, whatHappened, whatExpected and stepsToReproduce are required, and each text field can hold at most ${MAX_FIELD_CHARS} characters.`,
    });
  }

  if (!env?.GITHUB_TOKEN) {
    console.error("GITHUB_TOKEN is not set");
    return reply(500, { error: "Bug reporting is not set up." });
  }

  let keys;
  try {
    keys = await limiterKeys(request, report, env.GITHUB_TOKEN);
    if (!(await charge(env, keys))) {
      return reply(429, {
        error:
          "Rate limit exceeded. Please wait before submitting another report, or copy your report to the clipboard.",
      });
    }
  } catch (error) {
    console.error("Rate limiter unavailable:", error);
    return reply(503, {
      error: "Bug reporting is unavailable right now. Please try again later.",
    });
  }

  let created;
  try {
    created = await createIssue(report, env.GITHUB_TOKEN);
  } catch (error) {
    console.error("Could not create the issue:", error);
    // Nothing was filed, so the attempt does not count against the caller.
    // The cap across all callers keeps its charge: it also bounds how often
    // this Worker calls GitHub while GitHub is refusing.
    await refund(env, keys.slice(0, 2)).catch((refundError) =>
      console.error("Rate limiter refund failed:", refundError),
    );
    return reply(502, {
      error:
        "GitHub issue creation failed. Please copy your report to the clipboard instead.",
    });
  }

  const issue = await created.json();
  return reply(201, {
    status: "created",
    issueNumber: issue.number,
    issueUrl: issue.html_url,
  });
}

export default {
  async fetch(request, env) {
    try {
      return await handle(request, env);
    } catch (error) {
      console.error("Unhandled error:", error);
      return reply(500, { error: "Internal error." });
    }
  },
};
