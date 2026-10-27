/**
 * Cloudflare Worker: OpenRize Bug Report Endpoint
 *
 * Receives user-initiated bug reports and optional diagnostics from OpenRize
 * desktop clients, validates size and rate limits, and files a GitHub issue
 * in Kareeme246/OpenRize via GitHub REST API.
 *
 * --- PRIVACY & SECURITY MODEL ---
 * • Zero client secrets: OpenRize clients contain NO secrets or GitHub tokens.
 *   The GitHub personal access token resides exclusively as a secret in this Worker.
 * • Abuse controls: Requests are strictly rate-limited per IP and per anonymous install ID
 *   (max 5 reports per hour). Request payloads are capped at 64 KB.
 * • Explicit user consent: Reports are never sent automatically. They transmit ONLY
 *   when the user clicks "Submit" after inspecting what is included.
 *
 * --- DEPLOYMENT INSTRUCTIONS (For the Captain) ---
 * 1. Log in to Cloudflare Dashboard (https://dash.cloudflare.com/)
 * 2. Navigate to Workers & Pages -> Create application -> Create Worker
 * 3. Name the Worker `openrize-bug-report` and click Deploy
 * 4. In Worker settings: Settings -> Variables and Secrets -> Secrets:
 *    • Add secret name: `GITHUB_TOKEN`
 *    • Value: A GitHub fine-grained Personal Access Token with "Issues: Read and Write"
 *      permissions on the `Kareeme246/OpenRize` repository.
 * 5. (Optional) In Settings -> Variables and Secrets -> KV Namespace Bindings:
 *    • Bind a KV namespace variable `RATE_LIMIT_KV` for distributed edge rate limiting.
 *      (If omitted, the Worker falls back to an in-memory sliding window rate limiter).
 * 6. Copy and paste the code from this file into the Worker and deploy.
 * 7. Copy the deployed Worker URL (e.g., https://openrize-bug-report.<subdomain>.workers.dev/report).
 */

const MAX_PAYLOAD_BYTES = 64 * 1024; // 64 KB cap
const RATE_LIMIT_WINDOW_SECONDS = 3600; // 1 hour
const MAX_REPORTS_PER_WINDOW = 5;

// In-memory sliding window fallback for single isolate execution
const memoryRateLimits = new Map();

/**
 * Formats user bug report and diagnostics into markdown for the GitHub issue.
 */
export function formatIssueBody(payload) {
  const parts = [];

  parts.push("### What happened?");
  parts.push(payload.whatHappened?.trim() || "No description provided.");
  parts.push("");

  parts.push("### What did you expect to happen?");
  parts.push(payload.whatExpected?.trim() || "No expected behavior described.");
  parts.push("");

  parts.push("### Steps to reproduce");
  parts.push(payload.stepsToReproduce?.trim() || "No steps provided.");
  parts.push("");

  if (payload.additionalNotes?.trim()) {
    parts.push("### Additional context / notes");
    parts.push(payload.additionalNotes.trim());
    parts.push("");
  }

  parts.push("---");
  parts.push("");

  if (payload.diagnostics) {
    const diag = payload.diagnostics;
    parts.push("### Diagnostics");
    parts.push(`- **App Version:** \`v${diag.appVersion || "unknown"}\``);
    parts.push(`- **OS:** \`${diag.os || "unknown"} ${diag.osVersion || ""} (${diag.arch || ""})\``);
    parts.push(`- **Locale:** \`${diag.locale || "unknown"}\``);
    parts.push(`- **Install ID:** \`${payload.installId ? payload.installId.slice(0, 8) + "..." : "none"}\``);
    parts.push(`- **App Data Dir:** \`${diag.appDataDir || "standard"}\``);
    parts.push("");

    if (diag.settings) {
      parts.push("<details>");
      parts.push("<summary>App Settings</summary>");
      parts.push("");
      parts.push("```json");
      parts.push(JSON.stringify(diag.settings, null, 2));
      parts.push("```");
      parts.push("</details>");
      parts.push("");
    }

    if (Array.isArray(diag.recentCrashes) && diag.recentCrashes.length > 0) {
      parts.push("<details>");
      parts.push(`<summary>Recent Local Crashes (${diag.recentCrashes.length})</summary>`);
      parts.push("");
      parts.push("```json");
      parts.push(JSON.stringify(diag.recentCrashes, null, 2));
      parts.push("```");
      parts.push("</details>");
      parts.push("");
    }
  } else {
    parts.push("### Diagnostics");
    parts.push("*Diagnostics omitted by user.*");
    parts.push("");
  }

  parts.push("> *Reported via OpenRize in-app bug report.*");
  return parts.join("\n");
}

/**
 * Checks and updates rate limits using KV or in-memory fallback.
 */
async function checkRateLimit(key, env) {
  const now = Math.floor(Date.now() / 1000);

  if (env?.RATE_LIMIT_KV) {
    const currentStr = await env.RATE_LIMIT_KV.get(key);
    let current = currentStr ? JSON.parse(currentStr) : { count: 0, resetAt: now + RATE_LIMIT_WINDOW_SECONDS };

    if (now > current.resetAt) {
      current = { count: 0, resetAt: now + RATE_LIMIT_WINDOW_SECONDS };
    }

    if (current.count >= MAX_REPORTS_PER_WINDOW) {
      return false;
    }

    current.count += 1;
    const ttl = Math.max(60, current.resetAt - now);
    await env.RATE_LIMIT_KV.put(key, JSON.stringify(current), { expirationTtl: ttl });
    return true;
  }

  // Fallback to in-memory limiter
  const entry = memoryRateLimits.get(key);
  if (!entry || now > entry.resetAt) {
    memoryRateLimits.set(key, { count: 1, resetAt: now + RATE_LIMIT_WINDOW_SECONDS });
    return true;
  }

  if (entry.count >= MAX_REPORTS_PER_WINDOW) {
    return false;
  }

  entry.count += 1;
  return true;
}

export default {
  async fetch(request, env, ctx) {
    // 1. CORS Preflight
    if (request.method === "OPTIONS") {
      return new Response(null, {
        status: 204,
        headers: {
          "Access-Control-Allow-Origin": "*",
          "Access-Control-Allow-Methods": "POST, OPTIONS",
          "Access-Control-Allow-Headers": "Content-Type, User-Agent",
          "Access-Control-Max-Age": "86400",
        },
      });
    }

    // 2. Reject non-POST
    if (request.method !== "POST") {
      return new Response(JSON.stringify({ error: "Method not allowed. Use POST." }), {
        status: 405,
        headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
      });
    }

    // 3. Payload size check
    const contentLength = Number.parseInt(request.headers.get("content-length") || "0", 10);
    if (contentLength > MAX_PAYLOAD_BYTES) {
      return new Response(JSON.stringify({ error: "Bug report exceeds maximum size limit (64 KB)." }), {
        status: 413,
        headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
      });
    }

    let rawBody;
    try {
      rawBody = await request.text();
    } catch {
      return new Response(JSON.stringify({ error: "Could not read request body." }), {
        status: 400,
        headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
      });
    }

    if (rawBody.length > MAX_PAYLOAD_BYTES) {
      return new Response(JSON.stringify({ error: "Bug report exceeds maximum size limit (64 KB)." }), {
        status: 413,
        headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
      });
    }

    let payload;
    try {
      payload = JSON.parse(rawBody);
    } catch {
      return new Response(JSON.stringify({ error: "Invalid JSON payload." }), {
        status: 400,
        headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
      });
    }

    // 4. Validate required fields
    if (!payload.installId || !payload.whatHappened || !payload.whatExpected || !payload.stepsToReproduce) {
      return new Response(
        JSON.stringify({
          error: "Missing required fields: installId, whatHappened, whatExpected, or stepsToReproduce.",
        }),
        {
          status: 400,
          headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
        },
      );
    }

    // 5. Rate limiting
    const clientIp = request.headers.get("cf-connecting-ip") || request.headers.get("x-forwarded-for") || "unknown";
    const ipRateKey = `ratelimit:ip:${clientIp}`;
    const installRateKey = `ratelimit:install:${payload.installId}`;

    const ipAllowed = await checkRateLimit(ipRateKey, env);
    const installAllowed = await checkRateLimit(installRateKey, env);

    if (!ipAllowed || !installAllowed) {
      return new Response(
        JSON.stringify({
          error:
            "Rate limit exceeded (maximum 5 reports per hour). Please wait before submitting another report or copy your report to the clipboard.",
        }),
        {
          status: 429,
          headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
        },
      );
    }

    // 6. Verify GITHUB_TOKEN configuration
    if (!env?.GITHUB_TOKEN) {
      return new Response(
        JSON.stringify({
          error:
            "Worker configuration error: GITHUB_TOKEN secret is not set in Cloudflare Worker settings. Please notify the maintainers.",
        }),
        {
          status: 500,
          headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
        },
      );
    }

    // 7. Format issue and post to GitHub
    const firstLine = payload.whatHappened.split("\n")[0].trim().slice(0, 70);
    const issueTitle = `[Bug Report]: ${firstLine || "Issue reported in OpenRize"}`;
    const issueBody = formatIssueBody(payload);

    try {
      const ghResponse = await fetch("https://api.github.com/repos/Kareeme246/OpenRize/issues", {
        method: "POST",
        headers: {
          Authorization: `Bearer ${env.GITHUB_TOKEN}`,
          Accept: "application/vnd.github+json",
          "User-Agent": "OpenRize-Bug-Report-Worker",
          "X-GitHub-Api-Version": "2022-11-28",
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          title: issueTitle,
          body: issueBody,
          labels: ["bug", "in-app-report"],
        }),
      });

      if (!ghResponse.ok) {
        const ghErrorText = await ghResponse.text().catch(() => "");
        console.error(`GitHub API error (${ghResponse.status}):`, ghErrorText);
        return new Response(
          JSON.stringify({
            error: `GitHub issue creation failed (${ghResponse.status}). Please copy your report using the button below.`,
          }),
          {
            status: 502,
            headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
          },
        );
      }

      const ghData = await ghResponse.json();
      return new Response(
        JSON.stringify({
          status: "created",
          issueNumber: ghData.number,
          issueUrl: ghData.html_url,
        }),
        {
          status: 201,
          headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
        },
      );
    } catch (err) {
      console.error("Failed to connect to GitHub API:", err);
      return new Response(
        JSON.stringify({
          error: "Network error reaching GitHub API. Please copy your report using the button below.",
        }),
        {
          status: 502,
          headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
        },
      );
    }
  },
};
