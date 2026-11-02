/**
 * Cloudflare Worker: OpenRize crash report intake.
 *
 * Self-contained: paste this file into a Worker, no build step or packages.
 * It accepts the reports the app sends only after the user turned crash
 * reporting on (src-tauri/src/crashes.rs, shape in src-tauri/core/src/crashes.rs)
 * and keeps exactly the fields the app sends, and no others.
 *
 * Privacy rules this Worker holds to:
 * - It stores and logs only the allow-listed fields below, each type-checked
 *   and length-capped. Anything else in a request is dropped, never kept.
 * - It never reads or stores the sender's IP address, headers or cookies. The
 *   one thing it logs per report is the report's own id, source and version.
 * - Reports expire from KV after 30 days.
 * - It sets no CORS headers: only the desktop app posts here, and a web page
 *   must not be able to.
 *
 * Setup (Cloudflare dashboard):
 * 1. Workers & Pages -> Create -> Create Worker, name it `openrize-crashes`.
 * 2. Edit code, replace the contents with this file, Deploy.
 * 3. The app posts to the address compiled into it (CRASH_ENDPOINT in
 *    src-tauri/src/crashes.rs). A fork points it at its own Worker by building
 *    with OPENRIZE_CRASH_ENDPOINT set to `https://<worker>.workers.dev/report`.
 *
 * Optional storage and alerts (Worker Settings -> Bindings / Variables):
 * - KV namespace bound as `CRASHES_KV`: each report is kept 30 days.
 * - R2 bucket bound as `CRASHES_BUCKET`: each report is kept as JSON, until
 *   you delete it.
 * - Secret `WEBHOOK_URL` (a Discord or Slack-compatible webhook): a short alert
 *   with the report's version, platform and failure description.
 */

const MAX_BODY_BYTES = 64 * 1024;
const KV_TTL_SECONDS = 60 * 60 * 24 * 30;
const ID_PATTERN = /^[0-9a-fA-F-]{8,64}$/;
const SOURCES = new Set(["backend_panic", "frontend_error", "unhandled_rejection"]);

/** A string field, trimmed to `max` characters, or null when it is not one. */
function text(value, max) {
  return typeof value === "string" ? value.slice(0, max) : null;
}

/**
 * The report as the app defines it, rebuilt field by field. Returns null when
 * a required field is missing or malformed.
 */
function parseReport(payload) {
  if (typeof payload !== "object" || payload === null) return null;
  const id = text(payload.id, 64);
  const source = text(payload.source, 32);
  const message = text(payload.message, 500);
  const appVersion = text(payload.appVersion, 32);
  if (!id || !ID_PATTERN.test(id) || !SOURCES.has(source) || !message || !appVersion) {
    return null;
  }
  const timestampMs = Number.isFinite(payload.timestampMs) ? payload.timestampMs : Date.now();
  return {
    id,
    timestampMs,
    appVersion,
    os: text(payload.os, 32) ?? "",
    osVersion: text(payload.osVersion, 64) ?? "",
    arch: text(payload.arch, 32) ?? "",
    source,
    message,
    backtrace: text(payload.backtrace, 16 * 1024),
    componentStack: text(payload.componentStack, 8 * 1024),
  };
}

function reply(status, body) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

/** Text safe to put in a Discord or Slack message: no formatting, no mentions. */
function plain(value) {
  return value.replace(/[`*_~<>@|\\]/g, " ");
}

export default {
  async fetch(request, env, ctx) {
    if (request.method !== "POST") {
      return reply(405, { error: "Use POST." });
    }

    const declared = Number(request.headers.get("Content-Length") ?? 0);
    if (declared > MAX_BODY_BYTES) {
      return reply(413, { error: "Report too large." });
    }
    const body = await request.text();
    if (body.length > MAX_BODY_BYTES) {
      return reply(413, { error: "Report too large." });
    }

    let report;
    try {
      report = parseReport(JSON.parse(body));
    } catch {
      return reply(400, { error: "Invalid JSON body." });
    }
    if (!report) {
      return reply(400, { error: "Invalid crash report." });
    }

    const timestamp = new Date(report.timestampMs).toISOString();
    console.log(`crash ${report.id} ${report.source} v${report.appVersion}`);

    if (env.CRASHES_KV) {
      ctx.waitUntil(
        env.CRASHES_KV.put(`crash:${report.id}`, JSON.stringify(report), {
          expirationTtl: KV_TTL_SECONDS,
        }),
      );
    }

    if (env.CRASHES_BUCKET) {
      ctx.waitUntil(
        env.CRASHES_BUCKET.put(`${timestamp.slice(0, 10)}/${report.id}.json`, JSON.stringify(report), {
          httpMetadata: { contentType: "application/json" },
        }),
      );
    }

    if (env.WEBHOOK_URL) {
      const alert = [
        "OpenRize crash report",
        `ID: ${report.id}`,
        `Source: ${report.source}`,
        `Version: v${plain(report.appVersion)} (${plain(report.os)} ${plain(report.osVersion)})`,
        `Failure: ${plain(report.message)}`,
      ].join("\n");
      ctx.waitUntil(
        fetch(env.WEBHOOK_URL, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ content: alert }),
        }).catch(() => console.error("Webhook notification failed")),
      );
    }

    return reply(200, { status: "received", id: report.id });
  },
};
