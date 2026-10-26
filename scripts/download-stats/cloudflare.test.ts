import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CloudflareError,
  fetchDailyInstallerGets,
  fetchInstallerGets,
  resolveToken,
} from "./cloudflare.ts";

const reply = (body: unknown, status = 200) =>
  (async () => new Response(JSON.stringify(body), { status })) as typeof fetch;

const range = [
  new Date("2026-10-20T00:00:00Z"),
  new Date("2026-10-21T00:00:00Z"),
] as const;

test("CLOUDFLARE_API_TOKEN wins over the Wrangler login", async () => {
  const token = await resolveToken(
    { CLOUDFLARE_API_TOKEN: " abc " },
    async () => "login",
  );
  assert.equal(token, "abc");
});

test("the Wrangler login is the fallback", async () => {
  assert.equal(await resolveToken({}, async () => "login"), "login");
});

test("with neither, the error names the permissions and rules out the R2 token", async () => {
  await assert.rejects(
    resolveToken({}, async () => undefined),
    (error: Error) => {
      assert.match(error.message, /Account Analytics/);
      assert.match(error.message, /Zone > Analytics > Read/);
      assert.match(error.message, /firstmate-secrets/);
      assert.match(error.message, /is not that token/);
      return true;
    },
  );
});

test("an authorization failure is not reported as zero downloads", async () => {
  const denied = reply({
    data: null,
    errors: [{ message: "not authorized", extensions: { code: "authz" } }],
  });
  await assert.rejects(
    fetchInstallerGets("t", ...range, denied),
    /Zone > Analytics > Read/,
  );
  await assert.rejects(
    fetchInstallerGets("t", ...range, reply({}, 403)),
    CloudflareError,
  );
});

test("an empty zone answer is an error, not zero", async () => {
  const nothing = reply({ data: { viewer: { zones: [] } } });
  await assert.rejects(fetchInstallerGets("t", ...range, nothing), /no data/);
});

test("groups map to rows and null dimensions become empty strings", async () => {
  const group = {
    count: 2,
    avg: { sampleInterval: 10 },
    dimensions: {
      date: "2026-10-20",
      edgeResponseContentTypeName: "dmg",
      userAgentBrowser: null,
      verifiedBotCategory: null,
      clientIP: "192.0.2.1",
    },
  };
  const result = await fetchInstallerGets(
    "t",
    ...range,
    reply({
      data: { viewer: { zones: [{ httpRequestsAdaptiveGroups: [group] }] } },
    }),
  );
  assert.equal(result.maxSampleInterval, 10);
  assert.deepEqual(result.rows, [
    {
      date: "2026-10-20",
      contentType: "dmg",
      browser: "",
      botCategory: "",
      clientIP: "192.0.2.1",
      count: 2,
    },
  ]);
});

test("each day is queried on its own, the last one ending at now", async () => {
  const windows: [string, string][] = [];
  const record = (async (_url: unknown, init?: RequestInit) => {
    const { variables } = JSON.parse(String(init?.body));
    windows.push([variables.from, variables.to]);
    return new Response(
      JSON.stringify({
        data: { viewer: { zones: [{ httpRequestsAdaptiveGroups: [] }] } },
      }),
    );
  }) as typeof fetch;
  await fetchDailyInstallerGets(
    "t",
    ["2026-10-24", "2026-10-26"],
    new Date("2026-10-26T15:30:00Z"),
    record,
  );
  assert.deepEqual(windows, [
    ["2026-10-24T00:00:00.000Z", "2026-10-25T00:00:00.000Z"],
    ["2026-10-26T00:00:00.000Z", "2026-10-26T15:30:00.000Z"],
  ]);
});
