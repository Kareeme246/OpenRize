import assert from "node:assert/strict";
import { test } from "node:test";
import worker, { formatIssueBody } from "./cloudflare-bug-worker.js";

test("formatIssueBody formats full markdown correctly", () => {
  const payload = {
    installId: "0192e472-87a4-783a-8b1e-0123456789ab",
    whatHappened: "Timer crashed after sleep",
    whatExpected: "Timer should continue smoothly",
    stepsToReproduce: "1. Start timer\n2. Close lid\n3. Reopen",
    additionalNotes: "Using MacBook Pro M3",
    diagnostics: {
      appVersion: "0.9.35",
      os: "macos",
      osVersion: "15.0.0",
      arch: "aarch64",
      locale: "en-US",
      appDataDir: "~/Library/Application Support/com.offlinestudios.openrize",
      settings: {
        theme: "dark",
        trayEnabled: true,
      },
      recentCrashes: [
        {
          id: "crash-1",
          source: "backend_panic",
          message: "something panicked",
          timestampMs: 1729000000000,
        },
      ],
    },
  };

  const md = formatIssueBody(payload);
  assert.ok(md.includes("### What happened?\nTimer crashed after sleep"));
  assert.ok(md.includes("### What did you expect to happen?\nTimer should continue smoothly"));
  assert.ok(md.includes("### Steps to reproduce\n1. Start timer\n2. Close lid\n3. Reopen"));
  assert.ok(md.includes("### Additional context / notes\nUsing MacBook Pro M3"));
  assert.ok(md.includes("**App Version:** `v0.9.35`"));
  assert.ok(md.includes("**OS:** `macos 15.0.0 (aarch64)`"));
  assert.ok(md.includes("<summary>App Settings</summary>"));
  assert.ok(md.includes("<summary>Recent Local Crashes (1)</summary>"));
});

test("formatIssueBody handles omitted diagnostics", () => {
  const payload = {
    installId: "0192e472-87a4-783a-8b1e-0123456789ab",
    whatHappened: "A glitch",
    whatExpected: "No glitch",
    stepsToReproduce: "Click button",
  };

  const md = formatIssueBody(payload);
  assert.ok(md.includes("*Diagnostics omitted by user.*"));
});

test("worker handles CORS preflight OPTIONS request", async () => {
  const req = new Request("https://example.com/report", {
    method: "OPTIONS",
  });
  const res = await worker.fetch(req, {}, {});
  assert.equal(res.status, 204);
  assert.equal(res.headers.get("Access-Control-Allow-Origin"), "*");
});

test("worker rejects non-POST requests with 405", async () => {
  const req = new Request("https://example.com/report", {
    method: "GET",
  });
  const res = await worker.fetch(req, {}, {});
  assert.equal(res.status, 405);
  const data = await res.json();
  assert.ok(data.error.includes("Method not allowed"));
});

test("worker rejects payloads exceeding size limit with 413", async () => {
  const bigNotes = "x".repeat(70 * 1024);
  const req = new Request("https://example.com/report", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      installId: "test-install",
      whatHappened: "test",
      whatExpected: "test",
      stepsToReproduce: "test",
      additionalNotes: bigNotes,
    }),
  });
  const res = await worker.fetch(req, {}, {});
  assert.equal(res.status, 413);
  const data = await res.json();
  assert.ok(data.error.includes("maximum size limit"));
});

test("worker validates required fields with 400", async () => {
  const req = new Request("https://example.com/report", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      installId: "test-install",
      whatHappened: "something",
      // missing whatExpected and stepsToReproduce
    }),
  });
  const res = await worker.fetch(req, {}, {});
  assert.equal(res.status, 400);
  const data = await res.json();
  assert.ok(data.error.includes("Missing required fields"));
});

test("worker returns 500 when GITHUB_TOKEN is not configured", async () => {
  const req = new Request("https://example.com/report", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "cf-connecting-ip": "1.2.3.4",
    },
    body: JSON.stringify({
      installId: "unique-install-id-1",
      whatHappened: "Bug description",
      whatExpected: "Expected result",
      stepsToReproduce: "Steps to repro",
    }),
  });
  const res = await worker.fetch(req, {}, {});
  assert.equal(res.status, 500);
  const data = await res.json();
  assert.ok(data.error.includes("GITHUB_TOKEN"));
});
