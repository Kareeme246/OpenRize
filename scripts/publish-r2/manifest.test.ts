import assert from "node:assert/strict";
import { test } from "node:test";
import {
  installerFor,
  normalizeVersion,
  parseManifest,
  releasePrefix,
} from "./manifest.ts";

const sha256 = "a".repeat(64);
const build = (target: "macos-arm64" | "windows", version = "0.9.12") => ({
  key: releasePrefix(version) + installerFor(target, version).filename,
  ...installerFor(target, version),
  size: 10,
  sha256,
});
const manifest = (overrides: Record<string, unknown> = {}) => ({
  schema: 1,
  version: "0.9.12",
  publishedAt: "2026-10-15T00:00:00Z",
  builds: { "macos-arm64": build("macos-arm64"), windows: build("windows") },
  ...overrides,
});

test("a complete manifest parses", () => {
  assert.equal(parseManifest(manifest()).version, "0.9.12");
  assert.deepEqual(
    Object.keys(
      parseManifest(manifest({ builds: { windows: build("windows") } })).builds,
    ),
    ["windows"],
  );
});

test("versions normalise and reject path tricks", () => {
  assert.equal(normalizeVersion("v1.2.3"), "1.2.3");
  assert.throws(() => normalizeVersion("../1.2.3"));
  assert.throws(() => normalizeVersion("1.2"));
  assert.equal(normalizeVersion(`1.2.3-${"a".repeat(58)}`).length, 64);
  assert.throws(() => normalizeVersion(`1.2.3-${"a".repeat(59)}`));
  assert.throws(
    () => parseManifest(manifest({ version: `0.9.12-${"a".repeat(100)}` })),
    /bad version/,
  );
});

test("malformed manifests are rejected", () => {
  const withWindows = (fields: Record<string, unknown>) =>
    manifest({ builds: { windows: { ...build("windows"), ...fields } } });
  assert.throws(() => parseManifest(null));
  assert.throws(() => parseManifest(manifest({ schema: 2 })));
  assert.throws(() => parseManifest(manifest({ builds: {} })), /no builds/);
  assert.throws(
    () => parseManifest(manifest({ builds: { linux: build("windows") } })),
    /unknown target/,
  );
  assert.throws(
    () =>
      parseManifest(
        withWindows({
          key: "openrize/releases/0.9.11/OpenRize-0.9.12-windows.exe",
        }),
      ),
    /key/,
  );
  assert.throws(
    () => parseManifest(withWindows({ key: "other/x.exe" })),
    /key/,
  );
  assert.throws(() => parseManifest(withWindows({ size: 0 })), /size/);
  assert.throws(() => parseManifest(withWindows({ sha256: "xyz" })), /sha256/);
  assert.throws(
    () => parseManifest(withWindows({ filename: "../evil.exe" })),
    /name/,
  );
});
