/**
 * R2 layout: openrize/releases/<version>/<installer>, .../manifest.json,
 * openrize/current.json. Import-free so Node can run it; the website has its
 * own strict `parseManifest` copy, so keep the format byte-identical.
 */
export type OpenRizeTarget = "macos-arm64" | "windows";

export interface ManifestBuild {
  key: string;
  filename: string;
  contentType: string;
  size: number;
  sha256: string;
}

export interface ReleaseManifest {
  schema: 1;
  version: string;
  publishedAt: string;
  builds: Partial<Record<OpenRizeTarget, ManifestBuild>>;
}

export const OPENRIZE_TARGETS: readonly OpenRizeTarget[] = [
  "macos-arm64",
  "windows",
];
export const CURRENT_MANIFEST_KEY = "openrize/current.json";

/** At most 64 characters, so a version can never build an over-long object key. */
const VERSION = /^(?=.{1,64}$)\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?$/;

export function installerFor(
  target: OpenRizeTarget,
  version: string,
): { filename: string; contentType: string } {
  return target === "macos-arm64"
    ? {
        filename: `OpenRize-${version}-arm64.dmg`,
        contentType: "application/x-apple-diskimage",
      }
    : {
        filename: `OpenRize-${version}-windows.exe`,
        contentType: "application/octet-stream",
      };
}

export function normalizeVersion(input: string): string {
  const version = input.replace(/^v/, "");
  if (!VERSION.test(version))
    throw new Error(`Invalid OpenRize version: ${input}`);
  return version;
}

export function releasePrefix(version: string): string {
  return `openrize/releases/${normalizeVersion(version)}/`;
}

export function versionManifestKey(version: string): string {
  return `${releasePrefix(version)}manifest.json`;
}

export function parseManifest(input: unknown): ReleaseManifest {
  const fail = (why: string): never => {
    throw new Error(`Invalid OpenRize manifest: ${why}`);
  };
  if (typeof input !== "object" || input === null) return fail("not an object");
  const raw = input as Record<string, unknown>;
  if (raw.schema !== 1) fail("unsupported schema");
  if (typeof raw.version !== "string" || !VERSION.test(raw.version))
    fail("bad version");
  const version = raw.version as string;
  if (
    typeof raw.publishedAt !== "string" ||
    Number.isNaN(Date.parse(raw.publishedAt))
  ) {
    fail("bad publishedAt");
  }
  if (typeof raw.builds !== "object" || raw.builds === null)
    fail("missing builds");
  const builds: ReleaseManifest["builds"] = {};
  for (const [target, value] of Object.entries(
    raw.builds as Record<string, unknown>,
  )) {
    if (!(OPENRIZE_TARGETS as readonly string[]).includes(target)) {
      fail(`unknown target ${target}`);
    }
    const build = value as Record<string, unknown>;
    const expected = installerFor(target as OpenRizeTarget, version);
    if (
      build.filename !== expected.filename ||
      build.contentType !== expected.contentType
    ) {
      fail(`${target} name or type`);
    }
    if (build.key !== releasePrefix(version) + expected.filename)
      fail(`${target} key`);
    if (!Number.isSafeInteger(build.size) || (build.size as number) <= 0)
      fail(`${target} size`);
    if (
      typeof build.sha256 !== "string" ||
      !/^[0-9a-f]{64}$/.test(build.sha256)
    ) {
      fail(`${target} sha256`);
    }
    builds[target as OpenRizeTarget] = build as unknown as ManifestBuild;
  }
  if (!Object.keys(builds).length) fail("no builds");
  return { schema: 1, version, publishedAt: raw.publishedAt as string, builds };
}
