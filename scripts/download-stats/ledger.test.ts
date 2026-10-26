import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { classify } from "./classify.ts";
import {
  daysBetween,
  daysToFetch,
  emptyLedger,
  readLedger,
  recordSnapshot,
  withEmptyDays,
  writeLedger,
} from "./ledger.ts";

const days = classify([
  {
    date: "2026-10-20",
    contentType: "dmg",
    browser: "Chrome",
    botCategory: "",
    clientIP: "192.0.2.1",
    count: 1,
  },
]);

test("recording the same days twice leaves the ledger unchanged, and no IP is stored", () => {
  const dir = mkdtempSync(join(tmpdir(), "download-stats-"));
  try {
    const path = join(dir, "nested", "ledger.json");
    for (let run = 0; run < 2; run++) {
      writeLedger(
        path,
        recordSnapshot(readLedger(path), days, {}, [], "2026-10-26"),
      );
    }
    const once = readFileSync(path, "utf8");
    writeLedger(
      path,
      recordSnapshot(readLedger(path), days, {}, [], "2026-10-26"),
    );
    assert.equal(readFileSync(path, "utf8"), once);
    assert.equal(readLedger(path).days["2026-10-20"]?.macos.likelyHuman, 1);
    assert.doesNotMatch(once, /192\.0\.2\.1/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("older days stay when a later run no longer sees them", () => {
  const first = recordSnapshot(
    emptyLedger(),
    days,
    {},
    undefined,
    "2026-10-26",
  );
  const later = recordSnapshot(
    first,
    classify([]),
    {},
    undefined,
    "2026-11-30",
  );
  assert.ok(later.days["2026-10-20"]);
});

test("a corrupt ledger is an error, not a fresh start", () => {
  const dir = mkdtempSync(join(tmpdir(), "download-stats-"));
  try {
    const path = join(dir, "ledger.json");
    writeFileSync(path, "{ not json");
    assert.throws(() => readLedger(path));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("only unseen days, yesterday and today are asked about", () => {
  const window = daysBetween("2026-10-22", "2026-10-26");
  assert.deepEqual(window.length, 5);
  const ledger = recordSnapshot(
    emptyLedger(),
    withEmptyDays({}, ["2026-10-22", "2026-10-24", "2026-10-25"]),
    {},
    undefined,
    "2026-10-25",
  );
  assert.deepEqual(daysToFetch(window, ledger, "2026-10-26", false), [
    "2026-10-23",
    "2026-10-25",
    "2026-10-26",
  ]);
  assert.deepEqual(daysToFetch(window, ledger, "2026-10-26", true), window);
});

test("a day that is no longer sampled drops its estimate flag", () => {
  const sampled = recordSnapshot(
    emptyLedger(),
    days,
    { "2026-10-20": 10 },
    undefined,
    "2026-10-26",
  );
  const exact = recordSnapshot(sampled, days, {}, undefined, "2026-10-27");
  assert.deepEqual(sampled.sampled, { "2026-10-20": 10 });
  assert.deepEqual(exact.sampled, {});
});
