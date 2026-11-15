/**
 * Cloudflare Worker: OpenRize crash report intake.
 *
 * It accepts the reports the app sends only after the user turned crash
 * reporting on (src-tauri/src/crashes.rs, shape in src-tauri/core/src/crashes.rs)
 * and keeps exactly the fields the app sends, and no others.
 *
 * Privacy rules this Worker holds to:
 * - It stores and logs only the allow-listed fields below, each type-checked
 *   and length-capped. Anything else in a request is dropped, never kept.
 * - It never reads the sender's IP address or cookies, and stores no header.
 *   The one thing it logs per report is the report's own id, source and version.
 * - Reports expire from KV after 30 days.
 *
 * The app holds no secret, so anyone can post here. What bounds that:
 * - Limits per hour across all callers, since the sender is never identified:
 *   300 reports stored and 10 alerts sent. The limiter is required; without it
 *   every report is refused.
 * - Only JSON posted by a non-browser client is read, up to 64 KB. A request
 *   carrying `Origin`, which a browser adds to every cross-origin POST, is
 *   refused, and no CORS header is ever sent.
 * - A report is stored under a key made here and dated by this Worker's clock,
 *   so no request can replace another report or choose where it is filed.
 *
 * A refusal the app should retry later (limit reached, limiter or storage
 * failing) is a 503; a report that will never be accepted is a 4xx.
 *
 * Deploying (needs wrangler and a Cloudflare login):
 * 1. In scripts/wrangler.crash-worker.jsonc, fill in the KV namespace id and
 *    the R2 bucket name, or remove the store you do not use: each report is
 *    kept in every store that is bound.
 * 2. Set a lifecycle rule on the R2 bucket that deletes objects after a fixed
 *    time, for example 90 days. Without one, reports are kept until deleted.
 * 3. Optional secret `WEBHOOK_URL` (a Discord or Slack webhook) for a short
 *    alert per report:
 *    `wrangler secret put WEBHOOK_URL --config scripts/wrangler.crash-worker.jsonc`
 * 4. `wrangler deploy --config scripts/wrangler.crash-worker.jsonc`
 *    The config binds the limiter (`LIMITER`, a Durable Object), so it cannot
 *    be left out.
 * 5. The app posts to the address compiled into it (CRASH_ENDPOINT in
 *    src-tauri/src/crashes.rs). A fork points it at its own Worker by building
 *    with OPENRIZE_CRASH_ENDPOINT set to `https://<worker>.workers.dev/report`.
 */

import {
  charge,
  readBody,
  refusal,
  reply,
} from "./cloudflare-worker-shared.js";

export { RateLimiter } from "./cloudflare-worker-shared.js";

const REPORTS_PER_HOUR = 300;
const ALERTS_PER_HOUR = 10;
const KV_TTL_SECONDS = 60 * 60 * 24 * 30;
const DAY_MS = 24 * 60 * 60 * 1000;
// A report can wait on disk a long time before the user turns sending on.
const MAX_AGE_MS = 365 * DAY_MS;
const MAX_CLOCK_SKEW_MS = DAY_MS;

const ID = /^[0-9a-fA-F-]{8,64}$/;
const TOKEN = /^[0-9A-Za-z.+_-]{1,32}$/;
// Windows bug reports word the version as "25H2 (build 26300)".
const OS_VERSION = /^[0-9A-Za-z.+_() -]{1,64}$/;
const SOURCES = new Set([
  "backend_panic",
  "frontend_error",
  "unhandled_rejection",
]);

/** A string field, trimmed to `max` characters, or null when it is not one. */
function text(value, max) {
  return typeof value === "string" ? value.slice(0, max) : null;
}

function matching(value, pattern) {
  return typeof value === "string" && pattern.test(value) ? value : "";
}

/**
 * The report as the app defines it, rebuilt field by field. Returns null when
 * a required field is missing or malformed.
 */
export function parseReport(payload, now = Date.now()) {
  if (typeof payload !== "object" || payload === null) return null;
  const id = matching(payload.id, ID);
  const appVersion = matching(payload.appVersion, TOKEN);
  const message = text(payload.message, 500);
  if (!id || !appVersion || !SOURCES.has(payload.source) || !message) {
    return null;
  }
  const sent = payload.timestampMs;
  const plausible =
    Number.isInteger(sent) &&
    sent >= now - MAX_AGE_MS &&
    sent <= now + MAX_CLOCK_SKEW_MS;
  return {
    id,
    timestampMs: plausible ? sent : now,
    appVersion,
    os: matching(payload.os, TOKEN),
    osVersion: matching(payload.osVersion, OS_VERSION),
    arch: matching(payload.arch, TOKEN),
    source: payload.source,
    message,
    backtrace: text(payload.backtrace, 16 * 1024),
    componentStack: text(payload.componentStack, 8 * 1024),
  };
}

/**
 * Posts a short alert, unless this hour's alerts are used up. It carries only
 * fields matched against a pattern above, never the report's free text.
 */
async function alert(env, report) {
  try {
    if (!(await charge(env, [{ key: "alerts", limit: ALERTS_PER_HOUR }]))) {
      return;
    }
    const message = [
      "OpenRize crash report",
      `ID: ${report.id}`,
      `Source: ${report.source}`,
      `Version: v${report.appVersion} (${report.os} ${report.osVersion} ${report.arch})`,
    ].join("\n");
    const response = await fetch(env.WEBHOOK_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      // Discord reads `content`, Slack reads `text`; each ignores the other.
      body: JSON.stringify({ content: message, text: message }),
    });
    if (!response.ok) {
      console.error(`Webhook notification failed (${response.status})`);
    }
  } catch {
    console.error("Webhook notification failed");
  }
}

async function handle(request, env, ctx) {
  const refused = refusal(request);
  if (refused) return refused;

  let payload;
  try {
    const body = await readBody(request);
    if (body === null) return reply(413, { error: "Report too large." });
    payload = JSON.parse(body);
  } catch {
    return reply(400, { error: "Invalid JSON body." });
  }

  const now = Date.now();
  const report = parseReport(payload, now);
  if (!report) {
    return reply(400, { error: "Invalid crash report." });
  }

  try {
    if (!(await charge(env, [{ key: "reports", limit: REPORTS_PER_HOUR }]))) {
      return reply(503, { error: "Too many reports. Try again later." });
    }
  } catch (error) {
    console.error("Rate limiter unavailable:", error);
    return reply(503, { error: "Crash reporting is unavailable right now." });
  }

  const key = crypto.randomUUID();
  const record = JSON.stringify({ ...report, receivedAtMs: now });
  try {
    await env.CRASHES_KV?.put(`crash:${key}`, record, {
      expirationTtl: KV_TTL_SECONDS,
    });
    const day = new Date(now).toISOString().slice(0, 10);
    await env.CRASHES_BUCKET?.put(`${day}/${key}.json`, record, {
      httpMetadata: { contentType: "application/json" },
    });
  } catch (error) {
    // The app marks a report sent on any 2xx, so a lost write must not be one.
    console.error("Could not store the report:", error);
    return reply(503, { error: "Could not store the report." });
  }

  console.log(`crash ${report.id} ${report.source} v${report.appVersion}`);
  if (env.WEBHOOK_URL) ctx.waitUntil(alert(env, report));

  return reply(200, { status: "received", id: report.id });
}

export default {
  async fetch(request, env, ctx) {
    try {
      return await handle(request, env, ctx);
    } catch (error) {
      console.error("Unhandled error:", error);
      return reply(500, { error: "Internal error." });
    }
  },
};
