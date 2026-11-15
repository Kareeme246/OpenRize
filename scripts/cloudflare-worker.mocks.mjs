// In-memory stand-ins for the Cloudflare bindings the Workers use, for tests.

import { RateLimiter } from "./cloudflare-worker-shared.js";

/**
 * A Durable Object namespace that runs the real limiter over a Map. Values are
 * copied in and out like real storage, and `puts` counts the writes.
 */
export function limiterNamespace() {
  const stored = new Map();
  const objects = new Map();
  const namespace = {
    stored,
    puts: 0,
    idFromName: (name) => name,
    get(id) {
      if (!objects.has(id)) {
        const storage = {
          get: async (key) => structuredClone(stored.get(`${id}/${key}`)),
          put: async (key, value) => {
            namespace.puts += 1;
            stored.set(`${id}/${key}`, structuredClone(value));
          },
          delete: async (key) => stored.delete(`${id}/${key}`),
        };
        objects.set(id, new RateLimiter({ storage }));
      }
      return {
        fetch: (url, init) => objects.get(id).fetch(new Request(url, init)),
      };
    },
  };
  return namespace;
}

/** A KV namespace or R2 bucket: `put` into a Map, never replacing silently. */
export function store() {
  const entries = new Map();
  return {
    entries,
    overwrites: 0,
    async put(key, value) {
      if (entries.has(key)) this.overwrites += 1;
      entries.set(key, value);
    },
  };
}

export function post(body, headers = {}) {
  return new Request("https://worker.example/report", {
    method: "POST",
    headers: { "Content-Type": "application/json", ...headers },
    body: typeof body === "string" ? body : JSON.stringify(body),
  });
}
