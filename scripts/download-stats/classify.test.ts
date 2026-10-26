import assert from "node:assert/strict";
import { test } from "node:test";
import {
  classify,
  type DownloadRow,
  isKnownBrowser,
  platformOf,
  totalByPlatform,
} from "./classify.ts";

const row = (overrides: Partial<DownloadRow> = {}): DownloadRow => ({
  date: "2026-10-20",
  contentType: "dmg",
  browser: "Chrome",
  botCategory: "",
  clientIP: "192.0.2.1",
  count: 1,
  ...overrides,
});

const today = (rows: DownloadRow[], platform: "macos" | "windows" = "macos") =>
  (
    classify(rows)["2026-10-20"] as NonNullable<
      ReturnType<typeof classify>[string]
    >
  )[platform];

test("a browser download with no bot category is a likely human", () => {
  assert.deepEqual(today([row()]), {
    raw: 1,
    likelyHuman: 1,
    verifiedBot: 0,
    nonBrowser: 0,
    repeat: 0,
  });
});

test("curl and unidentified clients are not people", () => {
  const tally = today([
    row({ browser: "Curl", clientIP: "192.0.2.2" }),
    row({ browser: "Unknown", clientIP: "192.0.2.3" }),
    row({ browser: "python-requests", clientIP: "192.0.2.4" }),
  ]);
  assert.equal(tally.likelyHuman, 0);
  assert.equal(tally.nonBrowser, 3);
});

test("a verified bot is excluded even with a browser user agent", () => {
  const tally = today([
    row({ botCategory: "Search Engine Crawler" }),
    row({ browser: "GoogleBot", botCategory: "Search Engine Crawler" }),
    row({ browser: "Unknown", botCategory: "AI Crawler" }),
  ]);
  assert.equal(tally.likelyHuman, 0);
  assert.equal(tally.verifiedBot, 3);
});

test("repeat clicks from one IP, platform and day count once", () => {
  const tally = today([
    row({ count: 3 }),
    row({ browser: "Firefox" }),
    row({ clientIP: "192.0.2.9" }),
  ]);
  assert.equal(tally.raw, 5);
  assert.equal(tally.likelyHuman, 2);
  assert.equal(tally.repeat, 3);
});

test("the same IP counts again on another platform or another day", () => {
  const days = classify([
    row(),
    row({ contentType: "bin" }),
    row({ date: "2026-10-21" }),
  ]);
  const total = totalByPlatform(days);
  assert.equal(total.macos.likelyHuman, 2);
  assert.equal(total.windows.likelyHuman, 1);
  assert.equal(total.macos.repeat + total.windows.repeat, 0);
});

test("every request lands in exactly one bucket", () => {
  const days = classify([
    row({ count: 4 }),
    row({ browser: "Curl", clientIP: "192.0.2.2", count: 2 }),
    row({ botCategory: "AI Crawler", clientIP: "192.0.2.3" }),
    row({ contentType: "bin", clientIP: "192.0.2.4", count: 2 }),
  ]);
  for (const tallies of Object.values(days)) {
    for (const t of Object.values(tallies)) {
      assert.equal(
        t.raw,
        t.likelyHuman + t.verifiedBot + t.nonBrowser + t.repeat,
      );
    }
  }
});

test("only named browsers pass; names match without case or separators", () => {
  for (const name of ["Chrome", "ChromeMobile", "MobileSafari", "Edge"]) {
    assert.ok(isKnownBrowser(name), name);
  }
  assert.ok(isKnownBrowser("Samsung Browser"));
  for (const name of ["Curl", "Unknown", "", "GoogleBot", "AppleBot", "wget"]) {
    assert.ok(!isKnownBrowser(name), name);
  }
});

test("content types map to platforms and anything else is an error", () => {
  assert.equal(platformOf("dmg"), "macos");
  assert.equal(platformOf("bin"), "windows");
  assert.throws(() => platformOf("json"));
});
