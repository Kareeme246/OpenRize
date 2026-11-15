import assert from "node:assert/strict";
import { test } from "node:test";
import worker, { parseReport } from "./cloudflare-crash-worker.js";
import { limiterNamespace, post, store } from "./cloudflare-worker.mocks.mjs";

const NOW = 1_800_000_000_000;
const DAY_MS = 24 * 60 * 60 * 1000;
const WEBHOOK = "https://hooks.example/alert";

// No test may reach the network: a call that is not stubbed fails loudly.
globalThis.fetch = async () => {
  throw new Error("unexpected network call");
};

function report(overrides = {}) {
  return {
    id: "0192e472-87a4-783a-8b1e-0123456789ab",
    timestampMs: NOW - 5000,
    appVersion: "0.9.52",
    os: "macos",
    osVersion: "15.0.1",
    arch: "aarch64",
    source: "backend_panic",
    message: "panicked at src/activity/redact.rs:12:3",
    backtrace: "   0: openrize::run\n      at src/lib.rs:1:1",
    componentStack: null,
    sent: false,
    ...overrides,
  };
}

/** Fresh stores, limiter and clock per test, so no state is shared. */
function setup(t, webhook = () => new Response("ok")) {
  t.mock.timers.enable({ apis: ["Date"], now: NOW });
  const logs = t.mock.method(console, "log", () => {});
  const errors = t.mock.method(console, "error", () => {});
  const alerts = [];
  t.mock.method(globalThis, "fetch", async (url, init) => {
    alerts.push({ url, sent: JSON.parse(init.body) });
    return webhook();
  });
  const env = {
    LIMITER: limiterNamespace(),
    CRASHES_KV: store(),
    CRASHES_BUCKET: store(),
    WEBHOOK_URL: WEBHOOK,
  };
  const pending = [];
  const ctx = { waitUntil: (promise) => pending.push(promise) };
  const send = (body = report(), headers = {}, sendEnv = env) =>
    worker.fetch(post(body, headers), sendEnv, ctx);
  const settle = () => Promise.all(pending);
  // An alert still in flight must not outlive the test's stubs.
  t.after(settle);
  return { alerts, env, errors, logs, send, settle };
}

const records = (kept) => [...kept.entries.values()].map((v) => JSON.parse(v));

test("a report is stored in both stores and acknowledged", async (t) => {
  const { env, send } = setup(t);
  const res = await send();
  assert.equal(res.status, 200);
  assert.deepEqual(await res.json(), { status: "received", id: report().id });

  const [kvKey] = env.CRASHES_KV.entries.keys();
  const [r2Key] = env.CRASHES_BUCKET.entries.keys();
  assert.match(kvKey, /^crash:[0-9a-f-]{36}$/);
  assert.match(r2Key, /^2027-01-15\/[0-9a-f-]{36}\.json$/);
  assert.deepEqual(records(env.CRASHES_KV), records(env.CRASHES_BUCKET));
  assert.deepEqual(records(env.CRASHES_KV)[0], {
    id: report().id,
    timestampMs: NOW - 5000,
    appVersion: "0.9.52",
    os: "macos",
    osVersion: "15.0.1",
    arch: "aarch64",
    source: "backend_panic",
    message: "panicked at src/activity/redact.rs:12:3",
    backtrace: "   0: openrize::run\n      at src/lib.rs:1:1",
    componentStack: null,
    receivedAtMs: NOW,
  });
});

test("parseReport keeps only the known fields, typed and capped", () => {
  const parsed = parseReport(
    report({
      message: "m".repeat(900),
      backtrace: "b".repeat(20 * 1024),
      componentStack: 7,
      ip: "203.0.113.7",
      windowTitle: "secret",
      nested: { deep: true },
    }),
    NOW,
  );
  assert.deepEqual(Object.keys(parsed), [
    "id",
    "timestampMs",
    "appVersion",
    "os",
    "osVersion",
    "arch",
    "source",
    "message",
    "backtrace",
    "componentStack",
  ]);
  assert.equal(parsed.message.length, 500);
  assert.equal(parsed.backtrace.length, 16 * 1024);
  assert.equal(parsed.componentStack, null);

  for (const bad of [
    null,
    "text",
    7,
    report({ id: "not an id" }),
    report({ id: undefined }),
    report({ source: "made_up" }),
    report({ message: "" }),
    report({ message: { text: "x" } }),
    report({ appVersion: undefined }),
  ]) {
    assert.equal(parseReport(bad, NOW), null);
  }
});

test("version and platform fields cannot carry a line break or markup", () => {
  for (const appVersion of [
    "9\nfake log line",
    "1.0 beta",
    "",
    "<b>",
    "1".repeat(33),
  ]) {
    assert.equal(parseReport(report({ appVersion }), NOW), null, appVersion);
  }
  assert.equal(
    parseReport(report({ appVersion: "1.0.0-beta.1+7" }), NOW).appVersion,
    "1.0.0-beta.1+7",
  );

  const odd = parseReport(
    report({ os: "mac\nos", osVersion: "[x](http://e.x)", arch: "@here" }),
    NOW,
  );
  assert.deepEqual([odd.os, odd.osVersion, odd.arch], ["", "", ""]);
  const windows = parseReport(
    report({ os: "windows", osVersion: "25H2 (build 26300)", arch: "x86_64" }),
    NOW,
  );
  assert.equal(windows.osVersion, "25H2 (build 26300)");
});

test("a bad version is refused before anything is stored or logged", async (t) => {
  const { env, logs, send } = setup(t);
  assert.equal(
    (await send(report({ appVersion: "9\nfake log line" }))).status,
    400,
  );
  assert.equal(env.CRASHES_KV.entries.size, 0);
  assert.equal(env.LIMITER.puts, 0);
  await send();
  assert.deepEqual(
    logs.mock.calls.map((call) => call.arguments),
    [[`crash ${report().id} backend_panic v0.9.52`]],
  );
});

test("the same id posted twice is kept twice, replacing nothing", async (t) => {
  const { env, send } = setup(t);
  await send(report({ message: "first" }));
  await send(report({ message: "second" }));
  for (const kept of [env.CRASHES_KV, env.CRASHES_BUCKET]) {
    assert.equal(kept.entries.size, 2);
    assert.equal(kept.overwrites, 0);
    assert.deepEqual(
      records(kept)
        .map((r) => r.message)
        .sort(),
      ["first", "second"],
    );
    assert.ok(
      ![...kept.entries.keys()].some((key) => key.includes(report().id)),
    );
  }
});

test("the sender's clock is only data: bad values are replaced, never thrown on", async (t) => {
  const { env, send } = setup(t);
  const bad = [
    1e20,
    -1e20,
    8640000000000001,
    1.5,
    NOW + 0.5,
    NOW + 2 * DAY_MS,
    NOW + 3650 * DAY_MS,
    NOW - 400 * DAY_MS,
    0,
    -1,
    "yesterday",
    null,
    undefined,
  ];
  for (const timestampMs of bad) {
    const res = await send(report({ timestampMs }));
    assert.equal(res.status, 200, String(timestampMs));
  }
  assert.ok(records(env.CRASHES_KV).every((r) => r.timestampMs === NOW));

  const old = NOW - 200 * DAY_MS;
  await send(report({ timestampMs: old }));
  assert.ok(records(env.CRASHES_KV).some((r) => r.timestampMs === old));
  // Every object is filed under the day it arrived, whatever the report says.
  for (const key of env.CRASHES_BUCKET.entries.keys()) {
    assert.ok(key.startsWith("2027-01-15/"), key);
  }
});

test("the alert suits Discord and Slack and carries no free text", async (t) => {
  const { alerts, send, settle } = setup(t);
  await send(
    report({
      message: "SECRET see https://evil.example/login [x](http://e.x)",
      backtrace: "SECRET frames",
      componentStack: "SECRET stack",
    }),
  );
  await settle();
  assert.equal(alerts.length, 1);
  assert.equal(alerts[0].url, WEBHOOK);
  const { content, text, ...rest } = alerts[0].sent;
  assert.deepEqual(rest, {});
  assert.equal(content, text);
  assert.equal(
    content,
    `OpenRize crash report\nID: ${report().id}\nSource: backend_panic\nVersion: v0.9.52 (macos 15.0.1 aarch64)`,
  );
});

test("a webhook that refuses the alert is logged, and the report is still kept", async (t) => {
  const { env, errors, send, settle } = setup(
    t,
    () => new Response("no_text", { status: 400 }),
  );
  assert.equal((await send()).status, 200);
  await settle();
  assert.equal(env.CRASHES_KV.entries.size, 1);
  assert.deepEqual(errors.mock.calls[0].arguments, [
    "Webhook notification failed (400)",
  ]);
});

test("alerts stop at 10 an hour while reports are still stored", async (t) => {
  const { alerts, env, send, settle } = setup(t);
  for (let i = 0; i < 25; i++) {
    assert.equal((await send()).status, 200);
  }
  await settle();
  assert.equal(alerts.length, 10);
  assert.equal(env.CRASHES_KV.entries.size, 25);

  t.mock.timers.tick(60 * 60 * 1000 + 1);
  await send();
  await settle();
  assert.equal(alerts.length, 11);
});

test("the hourly cap answers 503 and stores nothing more", async (t) => {
  const { env, send } = setup(t);
  env.WEBHOOK_URL = undefined;
  const results = await Promise.all(Array.from({ length: 320 }, () => send()));
  const statuses = results.map((res) => res.status);
  assert.equal(statuses.filter((s) => s === 200).length, 300);
  assert.equal(statuses.filter((s) => s === 503).length, 20);
  assert.equal(env.CRASHES_KV.entries.size, 300);
  assert.equal(env.CRASHES_BUCKET.entries.size, 300);

  const puts = env.LIMITER.puts;
  assert.equal((await send()).status, 503);
  assert.equal(env.LIMITER.puts, puts);

  t.mock.timers.tick(60 * 60 * 1000 + 1);
  assert.equal((await send()).status, 200);
});

test("a missing or failing limiter answers 503 and stores nothing", async (t) => {
  const { alerts, env, send, settle } = setup(t);
  const broken = {
    idFromName: () => "id",
    get: () => ({
      fetch: async () => {
        throw new Error("storage down");
      },
    }),
  };
  for (const LIMITER of [undefined, broken]) {
    const res = await send(report(), {}, { ...env, LIMITER });
    assert.equal(res.status, 503);
    assert.ok(!(await res.text()).includes("storage down"));
  }
  await settle();
  assert.equal(env.CRASHES_KV.entries.size, 0);
  assert.equal(env.CRASHES_BUCKET.entries.size, 0);
  assert.equal(alerts.length, 0);
});

test("a store that fails answers 503 so the app sends the report again", async (t) => {
  const { alerts, env, send, settle } = setup(t);
  const failing = {
    put: async () => {
      throw new Error("quota exceeded");
    },
  };
  for (const stores of [{ CRASHES_KV: failing }, { CRASHES_BUCKET: failing }]) {
    const res = await send(report(), {}, { ...env, ...stores });
    assert.equal(res.status, 503);
    assert.ok(!(await res.text()).includes("quota"));
  }
  await settle();
  assert.equal(alerts.length, 0);
});

test("stores and the webhook are optional", async (t) => {
  const { alerts, send, settle } = setup(t);
  const res = await send(report(), {}, { LIMITER: limiterNamespace() });
  assert.equal(res.status, 200);
  await settle();
  assert.equal(alerts.length, 0);
});

test("requests from a web page are refused, with no CORS header on any reply", async (t) => {
  const { env, send } = setup(t);
  const url = "https://worker.example/report";
  const replies = [
    [await send(report(), { Origin: "https://evil.example" }), 403],
    [await send(report(), { "Sec-Fetch-Site": "cross-site" }), 403],
    [await send(report(), { "Content-Type": "text/plain;charset=UTF-8" }), 415],
    [await send(report(), { "Content-Type": "text/plain" }), 415],
    [await worker.fetch(new Request(url, { method: "OPTIONS" }), env, {}), 405],
    [await worker.fetch(new Request(url), env, {}), 405],
    [await send("{not json"), 400],
    [await send({ id: "x" }), 400],
  ];
  assert.equal(env.CRASHES_KV.entries.size, 0);
  assert.equal(env.LIMITER.puts, 0);
  replies.push(
    [
      await send(report(), {
        "Content-Type": "application/json; charset=utf-8",
      }),
      200,
    ],
    [await send(report(), { "Sec-Fetch-Site": "none" }), 200],
  );
  for (const [res, status] of replies) {
    assert.equal(res.status, status);
    for (const [name] of res.headers) {
      assert.ok(!name.toLowerCase().startsWith("access-control-"), name);
    }
  }
});

test("the size cap counts bytes and cuts off a body with no declared length", async (t) => {
  const { env, send } = setup(t);
  const wide = JSON.stringify(report({ backtrace: "€".repeat(30000) }));
  assert.ok(wide.length < 64 * 1024);
  assert.equal((await send(wide)).status, 413);

  const declared = await send(report(), {
    "Content-Length": String(70 * 1024),
  });
  assert.equal(declared.status, 413);

  let pulled = 0;
  let cancelled = false;
  const endless = new Request("https://worker.example/report", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    duplex: "half",
    body: new ReadableStream({
      pull(controller) {
        pulled += 1;
        controller.enqueue(new Uint8Array(16 * 1024).fill(120));
      },
      cancel() {
        cancelled = true;
      },
    }),
  });
  assert.equal((await worker.fetch(endless, env, {})).status, 413);
  assert.ok(cancelled);
  assert.ok(pulled <= 8, `read ${pulled} chunks`);
  assert.equal(env.CRASHES_KV.entries.size, 0);
});

test("an unexpected failure is a plain 500 with no detail", async (t) => {
  const { env } = setup(t);
  const failed = await worker.fetch(post(report()), env, {
    waitUntil() {
      throw new Error("secret detail");
    },
  });
  assert.equal(failed.status, 500);
  assert.deepEqual(await failed.json(), { error: "Internal error." });
});
