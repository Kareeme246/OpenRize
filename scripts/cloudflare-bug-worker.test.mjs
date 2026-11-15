import assert from "node:assert/strict";
import { test } from "node:test";
import worker, {
  formatIssueBody,
  issueTitle,
  MAX_FIELD_CHARS,
  MAX_ISSUE_BODY_CHARS,
  parseReport,
} from "./cloudflare-bug-worker.js";
import { limiterNamespace, post } from "./cloudflare-worker.mocks.mjs";

const INSTALL_ID = "0192e472-87a4-783a-8b1e-0123456789ab";
const TOKEN = "github-token-for-tests";
const FREE_TEXT = [
  "whatHappened",
  "whatExpected",
  "stepsToReproduce",
  "additionalNotes",
];

// No test may reach the network: a call that is not stubbed fails loudly.
globalThis.fetch = async () => {
  throw new Error("unexpected network call");
};

function report(overrides = {}) {
  return {
    installId: INSTALL_ID,
    whatHappened: "Timer crashed after sleep",
    whatExpected: "Timer should continue smoothly",
    stepsToReproduce: "1. Start timer\n2. Close lid\n3. Reopen",
    ...overrides,
  };
}

const created = () =>
  Response.json(
    {
      number: 42,
      html_url: "https://github.com/Kareeme246/OpenRize/issues/42",
    },
    { status: 201 },
  );

/** A fresh limiter and a stubbed GitHub per test, so no state is shared. */
function setup(t, github = created) {
  const calls = [];
  t.mock.method(console, "error", () => {});
  t.mock.method(globalThis, "fetch", async (url, init) => {
    calls.push({ url, init, sent: JSON.parse(init.body) });
    return github(calls.length);
  });
  const limiter = limiterNamespace();
  return { calls, limiter, env: { GITHUB_TOKEN: TOKEN, LIMITER: limiter } };
}

function send(env, body = report(), headers = {}) {
  return worker.fetch(
    post(body, { "cf-connecting-ip": "203.0.113.7", ...headers }),
    env,
  );
}

async function statuses(responses) {
  return (await Promise.all(responses)).map((response) => response.status);
}

const count = (list, status) => list.filter((s) => s === status).length;

/** The lines of a Markdown body that are not inside a fenced code block. */
function outsideFences(markdown) {
  const outside = [];
  let fence = 0;
  for (const line of markdown.split("\n")) {
    const run = /^`{3,}/.exec(line)?.[0].length ?? 0;
    if (fence === 0 && run > 0) {
      fence = run;
    } else if (fence > 0 && run >= fence && /^`+\s*$/.test(line)) {
      fence = 0;
    } else if (fence === 0) {
      outside.push(line);
    }
  }
  assert.equal(fence, 0, "a code block was left open");
  return outside.join("\n");
}

test("a report becomes an issue and the reply names it", async (t) => {
  const { calls, env } = setup(t);
  const res = await send(env, report({ additionalNotes: "MacBook Pro M3" }));

  assert.equal(res.status, 201);
  assert.deepEqual(await res.json(), {
    status: "created",
    issueNumber: 42,
    issueUrl: "https://github.com/Kareeme246/OpenRize/issues/42",
  });
  assert.equal(calls.length, 1);
  const [{ url, init, sent }] = calls;
  assert.equal(url, "https://api.github.com/repos/Kareeme246/OpenRize/issues");
  assert.equal(init.headers.Authorization, `Bearer ${TOKEN}`);
  assert.deepEqual(sent.labels, ["bug", "in-app-report"]);
  assert.equal(sent.title, "[Bug Report]: Timer crashed after sleep");
  assert.ok(
    sent.body.includes(
      "### What happened?\n```\nTimer crashed after sleep\n```",
    ),
  );
  assert.ok(sent.body.includes("### Steps to reproduce\n```\n1. Start timer"));
  assert.ok(sent.body.includes("MacBook Pro M3"));
  assert.ok(sent.body.includes("*Diagnostics omitted by user.*"));
});

test("a GitHub refusal is a 502 that does not use up the caller's limit", async (t) => {
  const { calls, env, limiter } = setup(t, (call) =>
    call <= 8
      ? new Response('{"message":"secret upstream detail"}', { status: 422 })
      : created(),
  );
  for (let i = 0; i < 8; i++) {
    const res = await send(env);
    assert.equal(res.status, 502);
    assert.ok(!(await res.text()).includes("secret upstream detail"));
  }
  assert.deepEqual(Object.keys(limiter.stored.get("limiter/windows")), ["all"]);
  assert.equal((await send(env)).status, 201);
  assert.equal(calls.length, 9);
});

test("failed attempts still count toward the cap across all callers", async (t) => {
  const { calls, env } = setup(t, () => new Response("{}", { status: 422 }));
  const results = [];
  for (let i = 0; i < 35; i++) results.push((await send(env)).status);
  assert.equal(count(results, 502), 30);
  assert.equal(count(results, 429), 5);
  assert.equal(calls.length, 30);
});

test("a network error reaching GitHub is a 502 and is refunded", async (t) => {
  const { env, limiter } = setup(t, () => {
    throw new TypeError("connection reset");
  });
  for (let i = 0; i < 8; i++) {
    assert.equal((await send(env)).status, 502);
  }
  assert.deepEqual(Object.keys(limiter.stored.get("limiter/windows")), ["all"]);
});

test("without the GitHub token nothing is charged or sent", async (t) => {
  const { calls, env, limiter } = setup(t);
  env.GITHUB_TOKEN = undefined;
  const res = await send(env);
  assert.equal(res.status, 500);
  assert.ok(!(await res.text()).includes("GITHUB_TOKEN"));
  assert.equal(limiter.puts, 0);
  assert.equal(calls.length, 0);
});

test("the sixth report in an hour is refused, and the next hour starts over", async (t) => {
  t.mock.timers.enable({ apis: ["Date"], now: 1_800_000_000_000 });
  const { calls, env } = setup(t);
  for (let i = 0; i < 5; i++) {
    assert.equal((await send(env)).status, 201);
  }
  assert.equal((await send(env)).status, 429);

  t.mock.timers.tick(59 * 60 * 1000);
  assert.equal((await send(env)).status, 429);
  t.mock.timers.tick(60 * 1000 + 1);
  assert.equal((await send(env)).status, 201);
  assert.equal(calls.length, 6);
});

test("50 reports at once from one address and install create 5 issues", async (t) => {
  const { calls, env } = setup(t);
  const results = await statuses(Array.from({ length: 50 }, () => send(env)));
  assert.equal(count(results, 201), 5);
  assert.equal(count(results, 429), 45);
  assert.equal(calls.length, 5);
});

test("a new install id per report does not get one address past 5", async (t) => {
  const { calls, env } = setup(t);
  const results = await statuses(
    Array.from({ length: 20 }, () =>
      send(env, report({ installId: crypto.randomUUID() })),
    ),
  );
  assert.equal(count(results, 201), 5);
  assert.equal(calls.length, 5);
});

test("40 addresses inside one IPv6 /64 share one limit", async (t) => {
  const { calls, env } = setup(t);
  const from = (address) =>
    send(env, report({ installId: crypto.randomUUID() }), {
      "cf-connecting-ip": address,
    });
  const results = await statuses(
    Array.from({ length: 40 }, (_, i) =>
      from(`2001:db8:aa:1:${i.toString(16)}::${(i * 7).toString(16)}`),
    ),
  );
  assert.equal(count(results, 201), 5);
  // The same /64 written another way is still that /64.
  assert.equal((await from("2001:0DB8:00aa:0001::9")).status, 429);
  assert.equal((await from("2001:db8:aa:2::9")).status, 201);
  assert.equal((await from("2001:db8:aa::9")).status, 201);
  assert.equal(calls.length, 7);
});

test("the hourly cap across all callers stops rotating addresses at 30", async (t) => {
  const { calls, env } = setup(t);
  const results = await statuses(
    Array.from({ length: 45 }, (_, i) =>
      send(env, report({ installId: crypto.randomUUID() }), {
        "cf-connecting-ip": `198.51.100.${i}`,
      }),
    ),
  );
  assert.equal(count(results, 201), 30);
  assert.equal(count(results, 429), 15);
  assert.equal(calls.length, 30);
});

test("a forwarded-for header does not choose the limit bucket", async (t) => {
  const { env } = setup(t);
  const results = await statuses(
    Array.from({ length: 10 }, (_, i) =>
      worker.fetch(
        post(report({ installId: crypto.randomUUID() }), {
          "x-forwarded-for": `192.0.2.${i}`,
        }),
        env,
      ),
    ),
  );
  assert.equal(count(results, 201), 5);
});

test("refused reports write nothing to the limiter", async (t) => {
  const { env, limiter } = setup(t);
  for (let i = 0; i < 5; i++) await send(env);
  const puts = limiter.puts;
  const before = structuredClone(limiter.stored);
  for (let i = 0; i < 20; i++) {
    const res = await send(env, report({ installId: crypto.randomUUID() }));
    assert.equal(res.status, 429);
  }
  assert.equal(limiter.puts, puts);
  assert.deepEqual(limiter.stored, before);
});

test("expired windows are dropped from the limiter's storage", async (t) => {
  t.mock.timers.enable({ apis: ["Date"], now: 1_800_000_000_000 });
  const { env, limiter } = setup(t);
  for (let i = 0; i < 5; i++) {
    await send(env, report({ installId: crypto.randomUUID() }), {
      "cf-connecting-ip": `198.51.100.${i}`,
    });
  }
  assert.equal(Object.keys(limiter.stored.get("limiter/windows")).length, 11);
  t.mock.timers.tick(60 * 60 * 1000 + 1);
  await send(env);
  assert.equal(Object.keys(limiter.stored.get("limiter/windows")).length, 3);
});

test("limiter keys hold neither the address nor the install id", async (t) => {
  const { env, limiter } = setup(t);
  await send(env, report(), { "cf-connecting-ip": "203.0.113.77" });
  await send(env, report(), { "cf-connecting-ip": "2001:db8:aa:1::5" });
  const keys = Object.keys(limiter.stored.get("limiter/windows"));
  assert.equal(keys.length, 4);
  const stored = JSON.stringify([...limiter.stored]);
  for (const secret of ["203.0.113", "2001:db8", INSTALL_ID, "0192e472"]) {
    assert.ok(!stored.includes(secret), secret);
  }
  assert.ok(!stored.includes(TOKEN));
});

test("a missing limiter refuses every report with 503", async (t) => {
  const { calls, env } = setup(t);
  env.LIMITER = undefined;
  const res = await send(env);
  assert.equal(res.status, 503);
  assert.equal(typeof (await res.json()).error, "string");
  assert.equal(calls.length, 0);
});

test("a failing limiter refuses every report with 503", async (t) => {
  const { calls, env } = setup(t);
  const failures = [
    {
      idFromName: () => "id",
      get: () => ({
        fetch: async () => {
          throw new Error("storage down");
        },
      }),
    },
    {
      idFromName: () => "id",
      get: () => ({ fetch: async () => new Response("", { status: 500 }) }),
    },
    {
      idFromName: () => {
        throw new Error("no namespace");
      },
    },
  ];
  for (const limiter of failures) {
    const res = await send({ ...env, LIMITER: limiter });
    assert.equal(res.status, 503);
    assert.ok(!(await res.text()).includes("storage down"));
  }
  assert.equal(calls.length, 0);
});

test("a limiter whose storage write fails refuses, then recovers", async (t) => {
  const { calls, env, limiter } = setup(t);
  await send(env);
  const set = limiter.stored.set.bind(limiter.stored);
  limiter.stored.set = () => {
    throw new Error("write failed");
  };
  assert.equal((await send(env)).status, 503);
  limiter.stored.set = set;
  // The charge that was never stored is not counted: 4 more still fit.
  const results = await statuses(Array.from({ length: 6 }, () => send(env)));
  assert.equal(count(results, 201), 4);
  assert.equal(calls.length, 5);
});

test("user text cannot mention, link, reference or break out of its block", async (t) => {
  const attack = [
    "@octocat see torvalds/linux#1 [login](https://evil.example)",
    "<img src=https://evil.example/b.png> <!-- hidden",
    "```",
    "## Fake heading @octocat",
    "`````",
    "@octocat after the fake fence -->",
  ].join("\n");
  const live = ["@octocat", "linux#1", "](https://", "<img", "<!--", "## Fake"];

  for (const field of FREE_TEXT) {
    const { calls, env } = setup(t);
    assert.equal((await send(env, report({ [field]: attack }))).status, 201);
    const { body } = calls[0].sent;
    assert.ok(body.includes(attack), `${field} is kept as written`);
    const outside = outsideFences(body);
    for (const marker of live) {
      assert.ok(!outside.includes(marker), `${field}: ${marker} is live`);
    }
    assert.ok(outside.includes("### Diagnostics"));
  }
});

test("the title is one tidy line of the description", () => {
  const title = issueTitle(
    report({
      whatHappened: `@octocat \`x\` <b> [a](b) torvalds/linux#1 ${"y".repeat(100)}\nsecond line`,
    }),
  );
  assert.ok(!/[`@#<>[\]\n]/.test(title.slice("[Bug Report]".length)));
  assert.ok(title.length <= "[Bug Report]: ".length + 70);
  assert.ok(!title.includes("second line"));
  assert.equal(
    issueTitle(report({ whatHappened: "###\nrest" })),
    "[Bug Report]: Issue reported in OpenRize",
  );
});

test("additional notes must be text when present", async (t) => {
  const { calls, env, limiter } = setup(t);
  for (const additionalNotes of [5, [], {}, false, 0, null]) {
    const res = await send(env, report({ additionalNotes }));
    assert.equal(res.status, 400, JSON.stringify(additionalNotes));
  }
  assert.equal(
    (await send(env, report({ additionalNotes: "  " }))).status,
    201,
  );
  assert.equal(calls.length, 1);
  assert.equal(limiter.puts, 1);
});

test("the install id must be a UUID", async (t) => {
  const { calls, env, limiter } = setup(t);
  const bad = ["id", "", 7, null, `${INSTALL_ID}x`, "x".repeat(600), undefined];
  for (const installId of bad) {
    const res = await send(env, report({ installId }));
    assert.equal(res.status, 400, String(installId));
  }
  assert.equal(calls.length, 0);
  assert.equal(limiter.puts, 0);
});

test("required text must be present, text, and within the field cap", async (t) => {
  const { calls, env } = setup(t);
  const tooLong = "x".repeat(MAX_FIELD_CHARS + 1);
  for (const field of FREE_TEXT) {
    assert.equal((await send(env, report({ [field]: tooLong }))).status, 400);
  }
  for (const field of FREE_TEXT.slice(0, 3)) {
    for (const value of [undefined, "", "   ", { not: "text" }, 3]) {
      const res = await send(env, report({ [field]: value }));
      assert.equal(res.status, 400);
    }
  }
  const emoji = (n) => report({ whatHappened: "\u{1F600}".repeat(n) });
  assert.equal(parseReport(emoji(MAX_FIELD_CHARS / 2 + 1)), null);
  assert.notEqual(parseReport(emoji(MAX_FIELD_CHARS / 2)), null);
  for (const body of ["null", "[]", '"text"', "7", "{not json"]) {
    assert.equal((await send(env, body)).status, 400);
  }
  assert.equal(calls.length, 0);

  const full = Object.fromEntries(
    FREE_TEXT.map((field) => [field, "x".repeat(MAX_FIELD_CHARS)]),
  );
  assert.equal((await send(env, report(full))).status, 201);
  assert.ok(calls[0].sent.body.length <= MAX_ISSUE_BODY_CHARS);
});

test("diagnostics keep only what the app is known to send", async (t) => {
  const { calls, env } = setup(t);
  const crash = (n) => ({
    id: `0192e472-87a4-783a-8b1e-00000000000${n}`,
    timestampMs: 1729000000000,
    appVersion: "0.9.52",
    source: "backend_panic",
    message: `panic ${n} ${"m".repeat(400)}`,
    backtrace: "SECRET-backtrace",
  });
  const res = await send(
    env,
    report({
      diagnostics: {
        environment: {
          appVersion: "0.9.52",
          os: "windows",
          osVersion: "25H2 (build 26300)",
          arch: "x86_64",
          locale: "en-US",
          dataFolder: "C:\\Users\\SECRET-name",
          hostname: "SECRET-host",
        },
        preferences: {
          theme: "dark",
          accent: "SECRET-accent",
          sizeMode: "veryrelaxed",
          trayEnabled: true,
          retentionDays: 90,
          autoAcceptPercent: 300,
          weeklyTargetHours: "SECRET-hours",
          aiPrompt: "SECRET-prompt",
          crashReporting: false,
        },
        recentCrashes: [1, 2, 3, 4, 5, 6, 7].map(crash),
        windowTitles: ["SECRET-title"],
      },
      extra: "SECRET-extra",
    }),
  );
  assert.equal(res.status, 201);
  const { body } = calls[0].sent;
  assert.ok(!body.includes("SECRET"), body);
  assert.ok(body.includes("**App Version:** `v0.9.52`"));
  assert.ok(body.includes("**OS:** `windows 25H2 (build 26300) (x86_64)`"));
  assert.ok(body.includes("**Locale:** `en-US`"));
  assert.ok(body.includes("**Data folder:** `unknown`"));
  assert.ok(body.includes("<summary>App Settings</summary>"));
  assert.ok(body.includes('"sizeMode": "veryrelaxed"'));
  assert.ok(body.includes('"retentionDays": 90'));
  assert.ok(!body.includes("autoAcceptPercent"));
  assert.ok(body.includes("<summary>Recent Local Crashes (5)</summary>"));
  assert.ok(!body.includes("panic 6"));
  assert.ok(!body.includes("m".repeat(300)));
  outsideFences(body);
});

test("only the attached sections are shown", () => {
  const body = formatIssueBody(
    parseReport(report({ diagnostics: { preferences: { theme: "dark" } } })),
  );
  assert.ok(body.includes("<summary>App Settings</summary>"));
  assert.ok(!body.includes("**App Version:**"));
  assert.ok(!body.includes("Recent Local Crashes"));

  const empty = parseReport(report({ diagnostics: { preferences: 5 } }));
  assert.ok(formatIssueBody(empty).includes("*Diagnostics omitted by user.*"));
  assert.equal(parseReport(report({ diagnostics: [] })), null);
  assert.equal(parseReport(report({ diagnostics: "x" })), null);
});

test("crash text stays inside its code block", () => {
  const body = formatIssueBody(
    parseReport(
      report({
        diagnostics: {
          environment: { appVersion: "1`\n## injected", os: "windows" },
          recentCrashes: [
            {
              id: "0192e472-87a4-783a-8b1e-000000000001",
              message: "boom ```\n## Fake heading @octocat",
            },
          ],
        },
      }),
    ),
  );
  assert.ok(!body.includes("\n## injected"));
  assert.ok(body.includes("````json"));
  assert.ok(!outsideFences(body).includes("@octocat"));
});

test("deeply nested preferences are dropped, not pretty-printed", async (t) => {
  const { calls, env } = setup(t);
  let nested = "[".repeat(1000);
  nested += Array.from({ length: 5000 }, () => "1").join(",");
  nested += "]".repeat(1000);
  const body = JSON.stringify(report()).replace(
    /}$/,
    `,"diagnostics":{"preferences":{"theme":${nested},"deep":${nested}},"recentCrashes":${nested}}}`,
  );
  assert.ok(body.length < 64 * 1024);
  assert.equal((await send(env, body)).status, 201);
  assert.ok(calls[0].sent.body.length < 1000);
  assert.ok(calls[0].sent.body.includes("*Diagnostics omitted by user.*"));
});

test("the install id appears nowhere in the issue", async (t) => {
  const { calls, env } = setup(t);
  await send(
    env,
    report({
      diagnostics: { environment: { os: "macos" }, installId: INSTALL_ID },
    }),
  );
  const sent = JSON.stringify(calls[0].sent);
  assert.ok(!sent.includes(INSTALL_ID));
  assert.ok(!sent.includes(INSTALL_ID.slice(0, 8)));
  assert.ok(!sent.includes("Install ID"));
});

test("an over-long issue body is shortened with every block closed", () => {
  const long = Object.fromEntries(
    FREE_TEXT.map((field) => [field, `\`\`\`\`\n${"😀".repeat(20000)}`]),
  );
  const body = formatIssueBody(report(long));
  assert.ok(body.length <= MAX_ISSUE_BODY_CHARS);
  assert.ok(body.includes("[cut short]"));
  assert.ok(body.endsWith("> *Reported via OpenRize in-app bug report.*"));
  assert.ok(outsideFences(body).includes("### Diagnostics"));
});

test("the size cap counts bytes, not characters", async (t) => {
  const { calls, env, limiter } = setup(t);
  const body = JSON.stringify(report({ additionalNotes: "€".repeat(30000) }));
  assert.ok(body.length < 64 * 1024);
  assert.ok(new TextEncoder().encode(body).length > 64 * 1024);
  const res = await send(env, body);
  assert.equal(res.status, 413);
  assert.ok((await res.json()).error.includes("maximum size limit"));
  assert.equal(calls.length, 0);
  assert.equal(limiter.puts, 0);
});

test("a declared length over the cap is refused before the body is read", async (t) => {
  const { env } = setup(t);
  const request = post(report(), { "Content-Length": String(70 * 1024) });
  const res = await worker.fetch(request, env);
  assert.equal(res.status, 413);
  assert.equal(request.bodyUsed, false);
});

test("a body with no declared length is cut off at the cap", async (t) => {
  const { calls, env } = setup(t);
  let pulled = 0;
  let cancelled = false;
  const body = new ReadableStream({
    pull(controller) {
      pulled += 1;
      controller.enqueue(new Uint8Array(16 * 1024).fill(120));
    },
    cancel() {
      cancelled = true;
    },
  });
  const request = new Request("https://worker.example/report", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body,
    duplex: "half",
  });
  assert.equal(request.headers.get("Content-Length"), null);
  const res = await worker.fetch(request, env);
  assert.equal(res.status, 413);
  assert.ok(cancelled);
  assert.ok(pulled <= 8, `read ${pulled} chunks`);
  assert.equal(calls.length, 0);
});

test("requests from a web page are refused", async (t) => {
  const { calls, env, limiter } = setup(t);
  const refused = [
    [{ Origin: "https://evil.example" }, 403],
    [{ Origin: "null" }, 403],
    [{ "Sec-Fetch-Site": "cross-site" }, 403],
    [{ "Sec-Fetch-Site": "same-origin" }, 403],
    [{ "Content-Type": "text/plain" }, 415],
    [{ "Content-Type": "text/plain;charset=UTF-8" }, 415],
    [{ "Content-Type": "application/x-www-form-urlencoded" }, 415],
    [{ "Content-Type": "application/jsonp" }, 415],
  ];
  for (const [headers, status] of refused) {
    const res = await send(env, report(), headers);
    assert.equal(res.status, status, JSON.stringify(headers));
  }
  const untyped = new Request("https://worker.example/report", {
    method: "POST",
    body: new TextEncoder().encode(JSON.stringify(report())),
  });
  assert.equal((await worker.fetch(untyped, env)).status, 415);
  assert.equal(calls.length, 0);
  assert.equal(limiter.puts, 0);

  const accepted = [
    { "Content-Type": "application/json; charset=utf-8" },
    { "Content-Type": "Application/JSON;charset=UTF-8" },
    { "Sec-Fetch-Site": "none" },
    { "User-Agent": "OpenRize/0.9.52" },
  ];
  for (const headers of accepted) {
    assert.equal((await send(env, report(), headers)).status, 201);
  }
});

test("no response carries a CORS header, and OPTIONS is just another method", async (t) => {
  const { env } = setup(t, (call) =>
    call === 2 ? new Response("", { status: 500 }) : created(),
  );
  const url = "https://worker.example/report";
  const preflight = new Request(url, {
    method: "OPTIONS",
    headers: {
      Origin: "https://evil.example",
      "Access-Control-Request-Method": "POST",
      "Access-Control-Request-Headers": "content-type",
    },
  });
  const responses = [
    [await worker.fetch(preflight, env), 405],
    [await worker.fetch(new Request(url, { method: "OPTIONS" }), env), 405],
    [await worker.fetch(new Request(url), env), 405],
    [await send(env, report(), { Origin: "https://evil.example" }), 403],
    [await send(env, report(), { "Content-Type": "text/plain" }), 415],
    [await send(env, "{not json"), 400],
    [await send(env), 201],
    [await send(env), 502],
    [await send({ ...env, LIMITER: undefined }), 503],
    [await send({ ...env, GITHUB_TOKEN: undefined }), 500],
  ];
  for (let i = 0; i < 4; i++) await send(env);
  responses.push([await send(env), 429]);
  for (const [res, status] of responses) {
    assert.equal(res.status, status);
    assert.equal(res.headers.get("Content-Type"), "application/json");
    for (const [name] of res.headers) {
      assert.ok(!name.toLowerCase().startsWith("access-control-"), name);
    }
  }
});

test("an unexpected failure is a plain 500 with no detail", async (t) => {
  const { env } = setup(
    t,
    () => new Response("<html>not json, secret detail</html>", { status: 201 }),
  );
  const res = await send(env);
  assert.equal(res.status, 500);
  assert.deepEqual(await res.json(), { error: "Internal error." });
});
