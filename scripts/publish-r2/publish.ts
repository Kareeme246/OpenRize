import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { basename, join } from "node:path";
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
import { verifyMinisign } from "./minisign.ts";
import type { ObjectStore } from "./store.ts";

export class PublishError extends Error {}

export interface PublishOptions {
  version: string;
  dir: string;
  noCurrent?: boolean;
  purge?: boolean;
  promote?: boolean;
  /** Updater public key the installers must be signed by; tauri.conf.json's by default. */
  publicKey?: string;
  log?: (line: string) => void;
  warn?: (line: string) => void;
  now?: () => Date;
}

export interface PublishResult {
  purgeRefused: string[];
}

const MIN_INSTALLER_BYTES = 1_000_000;
const IMMUTABLE = "public, max-age=31536000, immutable";
// Releases up to 0.10.2 were published without a DMG signature; from this version on it is required.
const DMG_SIGNED_SINCE = "0.10.3";

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

const isSigned = (target: OpenRizeTarget, version: string) =>
  target === "windows" || compare(version, DMG_SIGNED_SINCE) >= 0;

/** Every release asset the publisher reads: the installers and the signatures it requires. */
export function releaseFileNames(version: string): string[] {
  const names = releaseAssetNames(version);
  return OPENRIZE_TARGETS.flatMap((target) =>
    isSigned(target, version)
      ? [names[target], `${names[target]}.sig`]
      : [names[target]],
  );
}

function updaterPublicKey(): string {
  const config: { plugins: { updater: { pubkey: string } } } = JSON.parse(
    readFileSync(
      new URL("../../src-tauri/tauri.conf.json", import.meta.url),
      "utf8",
    ),
  );
  return config.plugins.updater.pubkey;
}

/**
 * The release itself is not trusted: an installer must carry the updater key's signature, made for
 * this version's file. Only DMGs older than `DMG_SIGNED_SINCE` pass on a format check alone.
 */
function checkInstaller(
  target: OpenRizeTarget,
  version: string,
  path: string,
  bytes: Uint8Array,
  publicKey: string,
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
  if (!isSigned(target, version)) return;
  let comment: string;
  try {
    comment = verifyMinisign(
      bytes,
      readFileSync(`${path}.sig`, "utf8"),
      publicKey,
    );
  } catch (error) {
    throw new PublishError(
      `${path} is not signed by the updater key: ${error instanceof Error ? error.message : error}`,
    );
  }
  // Tauri signs "file:<name>" into the trusted comment; the name carries the version, so another
  // release's genuine installer cannot stand in for this one. Only its letter case differs.
  const signedFile = `file:${basename(path)}`.toLowerCase();
  if (!comment.toLowerCase().split("\t").includes(signedFile)) {
    throw new PublishError(`${path}.sig was made for another file: ${comment}`);
  }
}

/** Order matters: installers first, then the manifest, `current.json` last, so no visitor sees a missing build. */
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

  const names = releaseAssetNames(version);
  const builds: ReleaseManifest["builds"] = {};
  const files: Partial<Record<OpenRizeTarget, Uint8Array>> = {};
  const publicKey = options.publicKey ?? updaterPublicKey();
  for (const target of OPENRIZE_TARGETS) {
    const path = join(options.dir, names[target]);
    if (!existsSync(path)) continue;
    const bytes = new Uint8Array(readFileSync(path));
    checkInstaller(target, version, path, bytes, publicKey);
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
