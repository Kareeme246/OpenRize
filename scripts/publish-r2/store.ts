import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, resolve, sep } from "node:path";

export interface PutOptions {
  contentType: string;
  cacheControl: string;
}

/** The three bucket operations the publisher needs. `get` is null only when the object is definitely absent. */
export interface ObjectStore {
  get(key: string): Promise<Uint8Array | null>;
  put(key: string, body: Uint8Array, options: PutOptions): Promise<void>;
  delete(key: string): Promise<void>;
}

/** Cloudflare rejects a single REST upload above this size (the limit wrangler enforces too). */
export const MAX_UPLOAD_BYTES = 300 * 1024 * 1024;

/** R2's "The specified key does not exist" error code; any other 404 (such as a missing bucket) is a real failure. */
const NO_SUCH_KEY = 10007;

interface CloudflareStoreConfig {
  accountId: string;
  bucket: string;
  apiToken: string;
  fetch?: typeof fetch;
}

async function failure(
  action: string,
  key: string,
  response: Response,
): Promise<Error> {
  const body = await response.text().catch(() => "");
  return new Error(
    `R2 ${action} ${key}: HTTP ${response.status} ${body.slice(0, 500)}`.trim(),
  );
}

/**
 * The bucket through Cloudflare's REST API, authenticated with an account API token that has
 * Workers R2 Storage Edit. These are the same object endpoints `wrangler r2 object` calls; no S3 keys.
 */
export function cloudflareStore(config: CloudflareStoreConfig): ObjectStore {
  const send = config.fetch ?? fetch;
  const url = (key: string) =>
    `https://api.cloudflare.com/client/v4/accounts/${encodeURIComponent(config.accountId)}` +
    `/r2/buckets/${encodeURIComponent(config.bucket)}/objects/${key.split("/").map(encodeURIComponent).join("/")}`;
  const request = (key: string, init: RequestInit) =>
    send(url(key), {
      ...init,
      headers: { ...init.headers, Authorization: `Bearer ${config.apiToken}` },
    });
  return {
    async get(key) {
      const response = await request(key, { method: "GET" });
      if (response.ok) return new Uint8Array(await response.arrayBuffer());
      if (response.status === 404) {
        const body = await response.text().catch(() => "");
        try {
          const code = (JSON.parse(body) as { errors?: { code?: number }[] })
            .errors?.[0]?.code;
          if (code === NO_SUCH_KEY) return null;
        } catch {
          // Not a Cloudflare error body, so it is not a confirmed missing key.
        }
        throw new Error(`R2 get ${key}: HTTP 404 ${body.slice(0, 500)}`.trim());
      }
      throw await failure("get", key, response);
    },
    async put(key, body, options) {
      if (body.byteLength > MAX_UPLOAD_BYTES) {
        throw new Error(
          `${key} is ${body.byteLength} bytes, over the ${MAX_UPLOAD_BYTES} byte REST upload limit`,
        );
      }
      const response = await request(key, {
        method: "PUT",
        headers: {
          "Content-Type": options.contentType,
          "Cache-Control": options.cacheControl,
        },
        body,
      });
      if (!response.ok) throw await failure("put", key, response);
    },
    async delete(key) {
      const response = await request(key, { method: "DELETE" });
      if (!response.ok && response.status !== 404)
        throw await failure("delete", key, response);
    },
  };
}

/** A plain directory standing in for the bucket, for `--local` rehearsals. Content types are not kept. */
export function directoryStore(root: string): ObjectStore {
  const base = resolve(root);
  const pathFor = (key: string) => {
    const path = resolve(base, key);
    if (!path.startsWith(base + sep))
      throw new Error(`Key escapes the local bucket: ${key}`);
    return path;
  };
  return {
    async get(key) {
      try {
        return new Uint8Array(readFileSync(pathFor(key)));
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
        throw error;
      }
    },
    async put(key, body) {
      const path = pathFor(key);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, body);
    },
    async delete(key) {
      rmSync(pathFor(key), { force: true });
    },
  };
}
