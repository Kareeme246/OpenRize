/**
 * Cloudflare Worker: OpenRize Lightweight Crash Ingestion Endpoint
 *
 * This Worker runs entirely on Cloudflare's Free Tier (100,000 requests/day).
 * It receives sanitized crash reports from OpenRize desktop clients and
 * can log them, store them in KV / R2, or forward them to a Discord/Slack webhook.
 *
 * --- QUICK SETUP (Cloudflare Dashboard) ---
 * 1. Log into your Cloudflare dashboard (https://dash.cloudflare.com/).
 * 2. Go to "Workers & Pages" -> "Create application" -> "Create Worker".
 * 3. Give it a name (e.g. `openrize-crashes`) and click "Deploy".
 * 4. Click "Edit code", replace the contents with this entire file, and click "Deploy".
 * 5. Copy the Worker URL (e.g. `https://openrize-crashes.<your-subdomain>.workers.dev`)
 *    and paste it into OpenRize Settings > Data & storage > Ingestion endpoint!
 *
 * --- OPTIONAL INTEGRATIONS ---
 * • KV Storage: Bind a KV namespace variable `CRASHES_KV` in Worker Settings > Variables.
 * • Discord/Slack: Add an environment secret `WEBHOOK_URL` in Worker Settings > Variables.
 */

export default {
  async fetch(request, env, ctx) {
    // 1. Handle CORS Preflight
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

    // 2. Reject non-POST requests
    if (request.method !== "POST") {
      return new Response(JSON.stringify({ error: "Method not allowed. Use POST." }), {
        status: 405,
        headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
      });
    }

    try {
      const payload = await request.json();

      // Validate required fields
      if (!payload.id || !payload.message || !payload.appVersion) {
        return new Response(JSON.stringify({ error: "Invalid crash report payload." }), {
          status: 400,
          headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
        });
      }

      const timestamp = new Date(payload.timestampMs || Date.now()).toISOString();
      console.log(
        `[CRASH RECEIVED] ID: ${payload.id} | Source: ${payload.source} | App: v${payload.appVersion} | OS: ${payload.os} ${payload.osVersion} (${payload.arch}) | Message: ${payload.message}`
      );

      // Optional: Store in Cloudflare KV if bound (expires after 30 days)
      if (env.CRASHES_KV) {
        ctx.waitUntil(
          env.CRASHES_KV.put(`crash:${payload.id}`, JSON.stringify(payload), {
            expirationTtl: 60 * 60 * 24 * 30,
            metadata: {
              appVersion: payload.appVersion,
              source: payload.source,
              timestamp,
            },
          })
        );
      }

      // Optional: Store in Cloudflare R2 if bound
      if (env.CRASHES_BUCKET) {
        const dateKey = timestamp.split("T")[0];
        ctx.waitUntil(
          env.CRASHES_BUCKET.put(`${dateKey}/${payload.id}.json`, JSON.stringify(payload, null, 2), {
            httpMetadata: { contentType: "application/json" },
          })
        );
      }

      // Optional: Forward alert to Discord / Slack webhook if configured
      if (env.WEBHOOK_URL) {
        const preview = (payload.message || "Unknown error").slice(0, 500);
        const webhookBody = {
          content: `🚨 **OpenRize Crash Report**\n• **ID:** \`${payload.id}\`\n• **Source:** \`${payload.source}\`\n• **Version:** \`v${payload.appVersion}\` (${payload.os} ${payload.osVersion})\n• **Time:** ${timestamp}\n\`\`\`\n${preview}\n\`\`\``,
        };

        ctx.waitUntil(
          fetch(env.WEBHOOK_URL, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(webhookBody),
          }).catch((err) => console.error("Webhook notification failed:", err))
        );
      }

      return new Response(
        JSON.stringify({
          status: "received",
          id: payload.id,
          timestamp,
        }),
        {
          status: 200,
          headers: {
            "Content-Type": "application/json",
            "Access-Control-Allow-Origin": "*",
          },
        }
      );
    } catch (err) {
      console.error("Failed to parse crash report JSON:", err);
      return new Response(JSON.stringify({ error: "Invalid JSON body." }), {
        status: 400,
        headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
      });
    }
  },
};
