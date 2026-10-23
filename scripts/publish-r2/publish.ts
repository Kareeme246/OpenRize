import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  CURRENT_MANIFEST_KEY,
  installerFor,
  type ManifestBuild,
  normalizeVersion,
  OPENRIZE_TARGETS,
  type OpenRizeTarget,
  parseManifest,
  type ReleaseManifest,
  releasePrefix,
  versionManifestKey,
} from "./manifest.ts";
import type { ObjectStore } from "./store.ts";

/** A refusal the operator can act on; the CLI prints it without a stack trace. */
export class PublishError extends Error {}

export interface PublishOptions {
  version: string;
  /** Directory of installers named like the GitHub release assets. */
  dir: string;
  /** Upload and verify the version but leave `current.json` alone. */
  noCurrent?: boolean;
  /** Delete the previous release's objects once the new one is current. */
  purge?: boolean;
  /** Make an already published version current; uploads nothing. */
  promote?: boolean;
  log?: (line: string) => void;
  warn?: (line: string) => void;
  now?: () => Date;
}

export interface PublishResult {
  /** Previous-release objects --purge declined or failed to delete. */
  purgeRefused: string[];
}

const MIN_INSTALLER_BYTES = 1_000_000;
const IMMUTABLE = "public, max-age=31536000, immutable";

/** The installer file names GitHub release assets carry, per target. */
export function releaseAssetNames(
  version: string,
): Record<OpenRizeTarget, string> {
  return {
    "macos-arm64": `openrize_${version}_aarch64.dmg`,
    windows: `openrize_${version}_x64-setup.exe`,
  };
}

const sha256 = (bytes: Uint8Array) =>
  createHash("sha256").update(bytes).digest("hex");
const compare = (a: string, b: string) =>
  a.localeCompare(b, undefined, { numeric: true });

/** Installer sanity checks that catch a wrong or truncated file before it goes public. */
function checkInstaller(
  target: OpenRizeTarget,
  path: string,
  bytes: Uint8Array,
) {
  if (bytes.length < MIN_INSTALLER_BYTES) {
    throw new PublishError(`${path} is only ${bytes.length} bytes`);
  }
  const buffer = Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (
    target === "windows" &&
    buffer.subarray(0, 2).toString("latin1") !== "MZ"
  ) {
    throw new PublishError(`${path} is not a Windows executable`);
  }
  if (
    target === "macos-arm64" &&
    buffer.subarray(-512, -508).toString("latin1") !== "koly"
  ) {
    throw new PublishError(`${path} is not a DMG`);
  }
}

/**
 * Publish a release. Order matters: installers are uploaded and read back first, then the version
 * manifest, and `current.json` last, so a visitor never sees a build that is not there.
 */
export async function publishRelease(
  store: ObjectStore,
  options: PublishOptions,
): Promise<PublishResult> {
  const log = options.log ?? (() => {});
  const warn = options.warn ?? (() => {});
  const version = normalizeVersion(options.version);
  if (options.purge && (options.promote || options.noCurrent)) {
    throw new PublishError(
      "--purge only applies when publishing a version as current",
    );
  }

  const readManifest = async (key: string): Promise<ReleaseManifest | null> => {
    const bytes = await store.get(key);
    return bytes
      ? parseManifest(JSON.parse(Buffer.from(bytes).toString("utf8")))
      : null;
  };
  const putManifest = (key: string, manifest: ReleaseManifest) =>
    store.put(key, Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`), {
      contentType: "application/json",
      cacheControl: "no-store",
    });
  const setCurrent = async (manifest: ReleaseManifest) => {
    await putManifest(CURRENT_MANIFEST_KEY, manifest);
    log(`current -> ${version}`);
  };

  const existing = await readManifest(versionManifestKey(version));

  if (options.promote) {
    if (!existing) throw new PublishError(`${version} is not published`);
    await setCurrent(existing);
    if ((await readManifest(CURRENT_MANIFEST_KEY))?.version !== version) {
      throw new PublishError(
        "current.json did not read back as the promoted version",
      );
    }
    return { purgeRefused: [] };
  }
  if (existing) {
    throw new PublishError(
      `${version} is already published; versions are immutable (use --promote to make it current)`,
    );
  }
  const current = await readManifest(CURRENT_MANIFEST_KEY);
  if (current && !options.noCurrent && compare(version, current.version) < 0) {
    throw new PublishError(
      `current is ${current.version}, newer than ${version}; use --no-current to stage it, then --promote to roll back deliberately`,
    );
  }

  // 1. Locate and validate the installers.
  const names = releaseAssetNames(version);
  const builds: ReleaseManifest["builds"] = {};
  const files: Partial<Record<OpenRizeTarget, Uint8Array>> = {};
  for (const target of OPENRIZE_TARGETS) {
    const path = join(options.dir, names[target]);
    if (!existsSync(path)) continue;
    const bytes = new Uint8Array(readFileSync(path));
    checkInstaller(target, path, bytes);
    const { filename, contentType } = installerFor(target, version);
    builds[target] = {
      key: releasePrefix(version) + filename,
      filename,
      contentType,
      size: bytes.length,
      sha256: sha256(bytes),
    } satisfies ManifestBuild;
    files[target] = bytes;
  }
  if (!Object.keys(builds).length) {
    throw new PublishError(
      `no installers named ${Object.values(names).join(" or ")} found`,
    );
  }

  // 2. Upload each installer and read it back before anything advertises it.
  for (const target of OPENRIZE_TARGETS) {
    const build = builds[target];
    if (!build) continue;
    await store.put(build.key, files[target] as Uint8Array, {
      contentType: build.contentType,
      cacheControl: IMMUTABLE,
    });
    const copy = await store.get(build.key);
    if (!copy || copy.length !== build.size || sha256(copy) !== build.sha256) {
      throw new PublishError(
        `read-back of ${build.key} does not match the local file`,
      );
    }
    log(
      `uploaded ${build.key} (${build.size} bytes, sha256 ${build.sha256.slice(0, 12)}...)`,
    );
  }

  // 3. Version manifest, then 4. current pointer.
  const manifest = parseManifest({
    schema: 1,
    version,
    publishedAt: (options.now?.() ?? new Date()).toISOString(),
    builds,
  });
  await putManifest(versionManifestKey(version), manifest);
  if (!(await readManifest(versionManifestKey(version)))) {
    throw new PublishError("version manifest did not read back");
  }
  log(`manifest ${versionManifestKey(version)}`);
  if (options.noCurrent) {
    log("left current.json alone (--no-current)");
    return { purgeRefused: [] };
  }
  await setCurrent(manifest);

  // 5. Releases are kept by default. Only with --purge, and only once the new release is current and
  // read back, drop the previous one.
  if ((await readManifest(CURRENT_MANIFEST_KEY))?.version !== version) {
    throw new PublishError(
      "current.json did not read back as the new version; keeping everything",
    );
  }
  const purgeRefused: string[] = [];
  if (current && current.version !== version && options.purge) {
    const newKeys = new Set([
      ...Object.values(builds).map((build) => build.key),
      versionManifestKey(version),
    ]);
    const previousKeys = [
      ...Object.values(current.builds).map((build) => build.key),
      versionManifestKey(current.version),
    ];
    let deleted = 0;
    for (const key of previousKeys) {
      // Never delete anything outside the previous release's folder or anything the new release uses.
      if (
        !key.startsWith(releasePrefix(current.version)) ||
        key.startsWith(releasePrefix(version)) ||
        newKeys.has(key)
      ) {
        purgeRefused.push(`${key} (not exclusively the previous release's)`);
        continue;
      }
      try {
        await store.delete(key);
        if (await store.get(key)) throw new Error("still present after delete");
        deleted++;
        log(`purged ${key}`);
      } catch (error) {
        purgeRefused.push(
          `${key} (${error instanceof Error ? error.message.split("\n")[0] : error})`,
        );
      }
    }
    log(`purge: ${deleted} deleted, ${purgeRefused.length} refused`);
    for (const line of purgeRefused) warn(`refused to delete ${line}`);
    if (purgeRefused.length) {
      warn(
        "published and current, but the previous release was not fully purged",
      );
    }
  }
  return { purgeRefused };
}
