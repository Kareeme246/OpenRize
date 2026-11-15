/**
 * Shared by the OpenRize bug report and crash report Workers: the rate limiter
 * (a Durable Object), the request gate and the bounded body reader.
 */

export const MAX_BODY_BYTES = 64 * 1024;
export const HOUR_SECONDS = 60 * 60;

const STORAGE_KEY = "windows";
const JSON_CONTENT_TYPE = /^application\/json\s*(;\s*charset=[\w-]+\s*)?$/i;

export function reply(status, body) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

/**
 * Counts events per key in fixed windows. One named instance serves a whole
 * Worker, and it runs single-threaded, so a check and its charge cannot be
 * split by another request the way a KV read-then-write can.
 *
 * Every window lives in one stored value. The counters are decided on the
 * in-memory copy without awaiting in between, and expired windows are dropped
 * on each charge, so the value stays as small as the limits of the keys in use.
 */
export class RateLimiter {
  constructor(state) {
    this.storage = state.storage;
  }

  async fetch(request) {
    const { op, keys, windowSeconds } = await request.json();
    this.loaded ??= this.storage.get(STORAGE_KEY).then((value) => value ?? {});
    const windows = await this.loaded;
    const now = Date.now();
    const live = (key) => windows[key]?.resetAt > now;

    let allowed = true;
    if (op === "charge") {
      allowed = keys.every(
        ({ key, limit }) => !live(key) || windows[key].count < limit,
      );
      // A refused request changes nothing, so it costs no storage write.
      if (!allowed) return reply(200, { allowed });
      for (const key of Object.keys(windows)) {
        if (!live(key)) delete windows[key];
      }
      for (const { key } of keys) {
        windows[key] ??= { count: 0, resetAt: now + windowSeconds * 1000 };
        windows[key].count += 1;
      }
    } else if (op === "refund") {
      for (const { key } of keys) {
        if (!live(key)) continue;
        windows[key].count -= 1;
        if (windows[key].count <= 0) delete windows[key];
      }
    } else {
      return reply(400, { error: "Unknown operation." });
    }

    try {
      await this.storage.put(STORAGE_KEY, windows);
    } catch (error) {
      // Memory is now ahead of storage: read it again on the next call.
      this.loaded = undefined;
      throw error;
    }
    return reply(200, { allowed });
  }
}

async function callLimiter(env, body) {
  const limiter = env.LIMITER.get(env.LIMITER.idFromName("limiter"));
  const response = await limiter.fetch("https://limiter/", {
    method: "POST",
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`limiter replied ${response.status}`);
  return response.json();
}

/**
 * Charges every `{ key, limit }` if all of them are under their limit, and
 * none of them otherwise. Throws when the limiter is not bound or fails, which
 * the caller must treat as a refusal.
 */
export async function charge(env, keys, windowSeconds = HOUR_SECONDS) {
  const { allowed } = await callLimiter(env, {
    op: "charge",
    keys,
    windowSeconds,
  });
  return allowed === true;
}

/** Gives back one earlier charge of each key. */
export async function refund(env, keys) {
  await callLimiter(env, { op: "refund", keys });
}

/**
 * The reply for a request that must not be read at all, or null. The desktop
 * app posts JSON and sends neither `Origin` nor `Sec-Fetch-Site`; a browser
 * sends `Origin` on every cross-origin POST, and cannot send JSON to another
 * origin without a preflight, which gets the 405 below.
 */
export function refusal(request) {
  if (request.method !== "POST") {
    return reply(405, { error: "Method not allowed. Use POST." });
  }
  const site = request.headers.get("Sec-Fetch-Site") ?? "none";
  if (request.headers.has("Origin") || site !== "none") {
    return reply(403, { error: "Requests from web pages are not accepted." });
  }
  if (!JSON_CONTENT_TYPE.test(request.headers.get("Content-Type") ?? "")) {
    return reply(415, { error: "Content-Type must be application/json." });
  }
  if (Number(request.headers.get("Content-Length") ?? 0) > MAX_BODY_BYTES) {
    return reply(413, { error: "Request exceeds the 64 KB size limit." });
  }
  return null;
}

/**
 * The request body as text, or null once it passes `maxBytes`. It counts bytes
 * as they arrive and stops reading there, so a body sent without a
 * Content-Length is never buffered whole.
 */
export async function readBody(request, maxBytes = MAX_BODY_BYTES) {
  if (!request.body) return "";
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > maxBytes) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  return new Blob(chunks).text();
}
