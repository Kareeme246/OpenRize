/**
 * Publishes a release to the DOWNLOADS R2 bucket (--remote needs
 * CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID). Installers come from --dir
 * or the GitHub release; --promote rolls back, --purge drops the previous one.
 */
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { normalizeVersion } from "./manifest.ts";
import { PublishError, publishRelease, releaseAssetNames } from "./publish.ts";
import { cloudflareStore, directoryStore, type ObjectStore } from "./store.ts";

const { values } = parseArgs({
  options: {
    version: { type: "string" },
    dir: { type: "string" },
    bucket: { type: "string", default: "offline-studios-downloads" },
    remote: { type: "boolean", default: false },
    local: { type: "boolean", default: false },
    "persist-to": { type: "string" },
    "no-current": { type: "boolean", default: false },
    purge: { type: "boolean", default: false },
    promote: { type: "boolean", default: false },
  },
});

const scratch = mkdtempSync(join(tmpdir(), "openrize-publish-"));
process.on("exit", () => rmSync(scratch, { recursive: true, force: true }));

function remoteStore(bucket: string): ObjectStore {
  const apiToken = process.env.CLOUDFLARE_API_TOKEN;
  const accountId = process.env.CLOUDFLARE_ACCOUNT_ID;
  if (!apiToken) throw new PublishError("CLOUDFLARE_API_TOKEN is not set");
  if (!accountId) throw new PublishError("CLOUDFLARE_ACCOUNT_ID is not set");
  return cloudflareStore({ accountId, apiToken, bucket });
}

async function downloadFromGitHub(version: string): Promise<string> {
  for (const name of Object.values(releaseAssetNames(version))) {
    const response = await fetch(
      `https://github.com/Kareeme246/OpenRize/releases/download/v${version}/${name}`,
    );
    if (response.ok)
      writeFileSync(
        join(scratch, name),
        Buffer.from(await response.arrayBuffer()),
      );
    else console.log(`skip ${name}: GitHub answered ${response.status}`);
  }
  return scratch;
}

try {
  if (!values.version) throw new PublishError("--version is required");
  if (values.remote === values.local) {
    throw new PublishError("pass exactly one of --remote or --local");
  }
  const version = normalizeVersion(values.version);
  const bucket = values.bucket as string;
  const store = values.remote
    ? remoteStore(bucket)
    : directoryStore(values["persist-to"] ?? ".r2-local");
  console.log(
    `${values.remote ? "REMOTE" : "local"} bucket ${bucket}, version ${version}`,
  );
  const dir =
    values.dir ??
    (values.promote ? scratch : await downloadFromGitHub(version));
  const result = await publishRelease(store, {
    version,
    dir,
    noCurrent: values["no-current"],
    purge: values.purge,
    promote: values.promote,
    log: (line) => console.log(line),
    warn: (line) => console.error(line),
  });
  if (result.purgeRefused.length) process.exitCode = 1;
} catch (error) {
  console.error(`error: ${error instanceof Error ? error.message : error}`);
  process.exitCode = 1;
}
