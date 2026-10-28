import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  coverageTable,
  FEATURES_DIR,
  type Finding,
  lintRepo,
  parseRoutes,
  parseUsageVerbs,
} from "./lint.ts";

const made: string[] = [];
after(() => {
  for (const dir of made) rmSync(dir, { recursive: true, force: true });
});

const COMMANDS = `# rize command reference

| Command | What it does | App command | App closed | Status |
| --- | --- | --- | --- | --- |
| \`rize\` / \`rize status [--full]\` | Status. | \`a\` | Yes | Shipped |
| \`rize entries list\` | List. | \`b\` | Yes | Shipped |
| \`rize entries show <id>\` | Show. | \`c\` | Yes | Shipped |
| \`rize timers start\` / \`pause\` / \`rm <timer>\` | Timers. | \`d\` | Yes | Shipped |
| \`rize invoices list\` / \`show <invoice>\` | Invoices. | \`e\` | Yes | M2 |
`;

const TYPES = `export type Route =
  | {
      name: "calendar";
      /** Local date; today when absent. */
      date?: string;
      filters?: { name: "nested" };
    }
  | {
      name: "timers";
    };

export type Other = { name: "ignored" };
`;

const DRIVE_SH = `#!/usr/bin/env bash
usage() {
  cat <<'USAGE'
usage: scripts/app-drive.sh <command> [args]

  start                       launch an isolated instance
  click  [-s ID] <name>       click a control
  fill   [-s ID] <name> <txt> type text
  rize   [-s ID] -- <args>    run the repo's rize CLI

Options common to the driving commands:
  -s, --session ID   target a specific session
USAGE
}
`;

const DRIVE_WIN = `const USAGE = \`usage: node scripts/app-drive-windows.mjs <command> [args]

  start                       launch an isolated instance
  text   [-s ID]              print the window's visible text

Options:
  -s, --session ID   target a specific session\`;
`;

const SKILL = `---
name: demo
description: Drives the demo. Use when checking the demo.
---

# Demo

See [the reference](references/notes.md) and \`scripts/app-drive.sh\`.

\`\`\`bash
scripts/app-drive.sh start
scripts/app-drive.sh click "Save"
node scripts/app-drive-windows.mjs text
scripts/app-drive.sh rize -- entries list --json
pnpm verify
rize timers start
\`\`\`
`;

interface FeatureOptions {
  id: string;
  title: string;
  routes: string;
  code: string;
  rize: string;
}

function feature(options: Partial<FeatureOptions> & { id: string }): string {
  const o: FeatureOptions = {
    title: "Calendar",
    routes: "[calendar]",
    code: "[src/pages/Calendar.tsx, src/Pulse.tsx, src/Reminder.tsx]",
    rize: '["rize entries list"]',
    ...options,
  };
  return `---
id: ${o.id}
title: ${o.title}
summary: See the calendar.
surfaces: [window, pulse]
routes: ${o.routes}
code: ${o.code}
rize: ${o.rize}
reach-macos: full
reach-windows: partial
reach-rize: none
verified: never
---

# ${o.title}

What it is.

## Sub-features

- day: one day

## How to get to it

Sidebar.

## Driving it

\`\`\`bash
scripts/app-drive.sh click "Save"
\`\`\`

## What proves it works

A label.

## Gotchas

- none

## Not reachable

Nothing.
`;
}

const INDEX = `# Feature map

Labels come from the code.

| Feature | What it is | Surfaces | Reach (macOS / Windows / rize) | Verified |
| --- | --- | --- | --- | --- |
| [Calendar](calendar.md) | Days. | window, pulse | full / partial / none | never |
| [Timers](timers.md) | Timers. | window | full / full / full | never |

## Not reachable by any tool here

The tray menu.
`;

function base(): Record<string, string> {
  return {
    "package.json": JSON.stringify({ scripts: { verify: "x", fix: "y" } }),
    "src-tauri/cli/COMMANDS.md": COMMANDS,
    "src/lib/types.ts": TYPES,
    "scripts/app-drive.sh": DRIVE_SH,
    "scripts/app-drive-windows.mjs": DRIVE_WIN,
    "src/pages/Calendar.tsx": "export {};\n",
    "src/pages/Timers.tsx": "export {};\n",
    "src/Pulse.tsx": "export {};\n",
    "src/Reminder.tsx": "export {};\n",
    ".agents/skills/demo/SKILL.md": SKILL,
    ".agents/skills/demo/references/notes.md": "# Notes\n\nPlain.\n",
    [`${FEATURES_DIR}/README.md`]: INDEX,
    [`${FEATURES_DIR}/calendar.md`]: feature({ id: "calendar" }),
    [`${FEATURES_DIR}/timers.md`]: feature({
      id: "timers",
      title: "Timers",
      routes: "[timers]",
      code: "[src/pages/Timers.tsx]",
    }),
  };
}

/** Builds a fixture repo; a null override deletes that file. */
function repo(overrides: Record<string, string | null> = {}): string {
  const root = mkdtempSync(join(tmpdir(), "lint-skills-"));
  made.push(root);
  const files = { ...base(), ...overrides };
  for (const [rel, text] of Object.entries(files)) {
    if (text === null) continue;
    const path = join(root, rel);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, text);
  }
  return root;
}

function only(findings: Finding[], rule: string): Finding[] {
  return findings.filter((f) => f.rule === rule);
}

/** Asserts the fixture produced exactly one finding of `rule`, at `file:line`. */
function expectOne(
  overrides: Record<string, string | null>,
  rule: string,
  file: string,
  line: number,
): Finding {
  const findings = lintRepo(repo(overrides));
  const hits = only(findings, rule);
  assert.equal(hits.length, 1, JSON.stringify(findings, null, 2));
  assert.equal(hits[0].file, file);
  assert.equal(hits[0].line, line);
  return hits[0];
}

const SKILL_FILE = ".agents/skills/demo/SKILL.md";
const CAL = `${FEATURES_DIR}/calendar.md`;
const TIM = `${FEATURES_DIR}/timers.md`;
const README = `${FEATURES_DIR}/README.md`;

test("the passing fixture has no findings", () => {
  assert.deepEqual(lintRepo(repo()), []);
});

test("catalogs parse from the real source formats", () => {
  assert.deepEqual(
    [...parseUsageVerbs(DRIVE_SH, /^usage\(\)\s*\{/m)],
    ["start", "click", "fill", "rize"],
  );
  assert.deepEqual(
    [...parseUsageVerbs(DRIVE_WIN, /^const USAGE = `/m)],
    ["start", "text"],
  );
  assert.deepEqual(
    parseRoutes(TYPES)?.map((r) => r.name),
    ["calendar", "timers"],
  );
  assert.equal(parseRoutes("export type Nope = {};"), null);
});

// ---------------------------------------------------------------- skill rules

test("frontmatter: missing block, wrong name, long description", () => {
  expectOne({ [SKILL_FILE]: "# no block\n" }, "frontmatter", SKILL_FILE, 1);
  const wrong = SKILL.replace("name: demo", "name: other");
  const hit = expectOne({ [SKILL_FILE]: wrong }, "frontmatter", SKILL_FILE, 2);
  assert.match(hit.message, /directory name "demo"/);
  const long = SKILL.replace(
    "description: Drives the demo. Use when checking the demo.",
    `description: ${"a".repeat(1025)}`,
  );
  expectOne({ [SKILL_FILE]: long }, "frontmatter", SKILL_FILE, 3);
  const noDescription = SKILL.replace(
    "description: Drives the demo. Use when checking the demo.\n",
    "",
  );
  expectOne({ [SKILL_FILE]: noDescription }, "frontmatter", SKILL_FILE, 1);
});

test("frontmatter: name must be kebab-case", () => {
  const dir = ".agents/skills/Bad_Name/SKILL.md";
  const text = SKILL.replace("name: demo", "name: Bad_Name");
  expectOne({ [dir]: text }, "frontmatter", dir, 2);
});

test("link: a relative link must resolve", () => {
  const broken = SKILL.replace("references/notes.md", "references/gone.md");
  expectOne({ [SKILL_FILE]: broken }, "link", SKILL_FILE, 8);
  const external = `${SKILL}\n[web](https://example.com) [anchor](#top)\n`;
  assert.deepEqual(
    only(lintRepo(repo({ [SKILL_FILE]: external })), "link"),
    [],
  );
});

test("path: an inline-code repo path must exist in the verification skills", () => {
  const drive = ".agents/skills/drive-app/SKILL.md";
  const skill = SKILL.replace("name: demo", "name: drive-app");
  const text = `${skill}\nSee \`src/pages/Missing.tsx\` and \`src/pages/\`.\n`;
  const lines = text.split("\n").length - 1;
  const hit = expectOne({ [drive]: text }, "path", drive, lines);
  assert.match(hit.message, /Missing\.tsx/);
  const windows = ".agents/skills/drive-app-windows/SKILL.md";
  expectOne(
    { [windows]: text.replace("name: drive-app", "name: drive-app-windows") },
    "path",
    windows,
    lines,
  );
});

test("path: other skills may name paths this repository does not have", () => {
  const text = `${SKILL}\nSee \`src/pages/Missing.tsx\`.\n`;
  assert.deepEqual(only(lintRepo(repo({ [SKILL_FILE]: text })), "path"), []);
});

test("placeholder: markers are rejected", () => {
  for (const marker of ["TODO", "tbd is fine in lower case", "{{ name }}"]) {
    const text = `${SKILL}\n${marker}\n`;
    const findings = only(
      lintRepo(repo({ [SKILL_FILE]: text })),
      "placeholder",
    );
    assert.equal(
      findings.length,
      marker === "TODO" || marker.startsWith("{{") ? 1 : 0,
    );
  }
  const lorem = `${SKILL}\nLorem Ipsum\n`;
  assert.equal(
    only(lintRepo(repo({ [SKILL_FILE]: lorem })), "placeholder").length,
    1,
  );
});

test("command: driver verbs", () => {
  const text = SKILL.replace(
    'scripts/app-drive.sh click "Save"',
    'scripts/app-drive.sh bogus "Save"',
  );
  const hit = expectOne({ [SKILL_FILE]: text }, "command", SKILL_FILE, 12);
  assert.match(hit.message, /"bogus" is not a verb/);
  const win = SKILL.replace("windows.mjs text", "windows.mjs nope");
  expectOne({ [SKILL_FILE]: win }, "command", SKILL_FILE, 13);
});

test("command: the rize passthrough is checked, quoted text is not", () => {
  const ok = `${SKILL}\n\`\`\`bash\nscripts/app-drive.sh rize -s 12 -- --json entries list\nscripts/app-drive.sh fill "Note" "run rize nonsense"\n\`\`\`\n`;
  assert.deepEqual(only(lintRepo(repo({ [SKILL_FILE]: ok })), "command"), []);
  const bad = SKILL.replace(
    "rize -- entries list --json",
    "rize -- entries frobnicate --json",
  );
  const hit = expectOne({ [SKILL_FILE]: bad }, "command", SKILL_FILE, 14);
  assert.match(hit.message, /"frobnicate" is not a shipped subcommand/);
  const unknown = SKILL.replace("rize timers start", "rize bogus run");
  expectOne({ [SKILL_FILE]: unknown }, "command", SKILL_FILE, 16);
});

test("command: a planned rize command says so", () => {
  const text = SKILL.replace("rize timers start", "rize invoices list");
  const hit = expectOne({ [SKILL_FILE]: text }, "command", SKILL_FILE, 16);
  assert.match(hit.message, /planned \(status M2\), not shipped/);
});

test("command: inline code and pnpm scripts", () => {
  const good = `${SKILL}\nRun \`pnpm verify\` or \`pnpm exec tsc\`, not the \`rize\` binary.\n`;
  assert.deepEqual(only(lintRepo(repo({ [SKILL_FILE]: good })), "command"), []);
  const bad = `${SKILL}\nRun \`pnpm nonsense\`.\n`;
  const lines = bad.split("\n").length - 1;
  expectOne({ [SKILL_FILE]: bad }, "command", SKILL_FILE, lines);
});

test("the generic rules also cover other markdown under the skills", () => {
  const notes = ".agents/skills/demo/references/notes.md";
  expectOne({ [notes]: "# Notes\n\nFIXME later\n" }, "placeholder", notes, 3);
});

// ---------------------------------------------------------------- feature rules

test("feature-frontmatter: order, domains and id", () => {
  const swapped = feature({ id: "calendar" }).replace(
    "title: Calendar\nsummary: See the calendar.\n",
    "summary: See the calendar.\ntitle: Calendar\n",
  );
  expectOne({ [CAL]: swapped }, "feature-frontmatter", CAL, 4);
  const surface = feature({ id: "calendar" }).replace(
    "[window, pulse]",
    "[window, desk]",
  );
  expectOne({ [CAL]: surface }, "feature-frontmatter", CAL, 5);
  const reach = feature({ id: "calendar" }).replace(
    "reach-macos: full",
    "reach-macos: most",
  );
  expectOne({ [CAL]: reach }, "feature-frontmatter", CAL, 9);
  const verified = feature({ id: "calendar" }).replace(
    "verified: never",
    "verified: 2026-10-01 by-hand",
  );
  expectOne({ [CAL]: verified }, "feature-frontmatter", CAL, 12);
  const id = feature({ id: "calendar" }).replace("id: calendar", "id: other");
  expectOne({ [CAL]: id }, "feature-frontmatter", CAL, 2);
  const unquoted = feature({ id: "calendar", rize: "[rize entries list]" });
  expectOne({ [CAL]: unquoted }, "feature-frontmatter", CAL, 8);
  const missing = feature({ id: "calendar" }).replace("verified: never\n", "");
  expectOne({ [CAL]: missing }, "feature-frontmatter", CAL, 12);
  const dated = feature({ id: "calendar" }).replace(
    "verified: never",
    "verified: 2026-10-01 macos-drive",
  );
  assert.deepEqual(lintRepo(repo({ [CAL]: dated })), []);
});

test("feature-body: H1, sections, order and emptiness", () => {
  const h1 = feature({ id: "calendar" }).replace("# Calendar\n", "# Cal\n");
  expectOne({ [CAL]: h1 }, "feature-body", CAL, 15);
  const missing = feature({ id: "calendar" }).replace(
    "## Gotchas\n\n- none\n\n",
    "",
  );
  const hit = expectOne({ [CAL]: missing }, "feature-body", CAL, 1);
  assert.match(hit.message, /missing section "## Gotchas"/);
  const empty = feature({ id: "calendar" }).replace("- none\n", "");
  expectOne({ [CAL]: empty }, "feature-body", CAL, 37);
  const swapped = feature({ id: "calendar" })
    .replace("## Gotchas", "## TMP")
    .replace("## Not reachable", "## Gotchas")
    .replace("## TMP", "## Not reachable");
  assert.ok(
    only(lintRepo(repo({ [CAL]: swapped })), "feature-body").length > 0,
  );
});

test("feature-code: every path must exist", () => {
  const text = feature({
    id: "calendar",
    code: "[src/pages/Calendar.tsx, src/Pulse.tsx, src/Reminder.tsx, src/Gone.tsx]",
  });
  expectOne({ [CAL]: text }, "feature-code", CAL, 7);
});

test("feature-route: unknown, duplicated and unowned routes", () => {
  const unknown = feature({ id: "calendar", routes: "[calendar, nowhere]" });
  expectOne({ [CAL]: unknown }, "feature-route", CAL, 6);
  const duplicated = feature({
    id: "timers",
    title: "Timers",
    routes: "[timers, calendar]",
    code: "[src/pages/Timers.tsx]",
  });
  const findings = only(lintRepo(repo({ [TIM]: duplicated })), "feature-route");
  assert.deepEqual(findings.map((f) => f.file).sort(), [CAL, TIM]);
  assert.match(findings[0].message, /owned by 2 features/);
  const unowned = feature({
    id: "timers",
    title: "Timers",
    routes: "[]",
    code: "[src/pages/Timers.tsx]",
  });
  const hit = expectOne(
    { [TIM]: unowned },
    "feature-route",
    "src/lib/types.ts",
    9,
  );
  assert.match(hit.message, /owned by no feature/);
});

test("feature-route: a missing Route union is a finding, not a crash", () => {
  const hit = expectOne(
    { "src/lib/types.ts": "export type Nothing = 1;\n" },
    "feature-route",
    "src/lib/types.ts",
    1,
  );
  assert.equal(
    hit.message,
    "could not find the Route union; update the linter",
  );
});

test("feature-coverage: an uncovered page is reported", () => {
  const hit = expectOne(
    { "src/pages/Extra.tsx": "export {};\n" },
    "feature-coverage",
    "src/pages/Extra.tsx",
    1,
  );
  assert.match(hit.message, /not covered/);
  const viaDirectory = feature({
    id: "calendar",
    code: "[src/pages, src/Pulse.tsx, src/Reminder.tsx]",
  });
  assert.deepEqual(
    lintRepo(
      repo({ "src/pages/Extra.tsx": "export {};\n", [CAL]: viaDirectory }),
    ),
    [],
  );
  expectOne(
    {
      "src/Pulse.tsx": "export {};\n",
      [CAL]: feature({
        id: "calendar",
        code: "[src/pages/Calendar.tsx, src/Reminder.tsx]",
      }),
    },
    "feature-coverage",
    "src/Pulse.tsx",
    1,
  );
});

test("feature-rize: only shipped command words", () => {
  const planned = feature({ id: "calendar", rize: '["rize invoices list"]' });
  const hit = expectOne({ [CAL]: planned }, "feature-rize", CAL, 8);
  assert.match(hit.message, /planned \(status M2\)/);
  const args = feature({
    id: "calendar",
    rize: '["rize entries list --json"]',
  });
  expectOne({ [CAL]: args }, "feature-rize", CAL, 8);
  const unknown = feature({ id: "calendar", rize: '["rize entries frob"]' });
  expectOne({ [CAL]: unknown }, "feature-rize", CAL, 8);
  const bare = feature({ id: "calendar", rize: '["rize entries"]' });
  expectOne({ [CAL]: bare }, "feature-rize", CAL, 8);
  const ok = feature({
    id: "calendar",
    rize: '["rize status", "rize timers pause"]',
  });
  assert.deepEqual(lintRepo(repo({ [CAL]: ok })), []);
});

test("feature-index: missing, unlinked, duplicated and dangling entries", () => {
  expectOne({ [README]: null }, "feature-index", README, 1);
  const missing = INDEX.replace(
    "| [Timers](timers.md) | Timers. | window | full / full / full | never |\n",
    "",
  );
  const hit = expectOne({ [README]: missing }, "feature-index", README, 5);
  assert.match(hit.message, /timers\.md is not linked/);
  const twice = INDEX.replace(
    "\n## Not",
    "| [Timers](timers.md) | Timers. | window | full / full / full | never |\n\n## Not",
  ).replace("never |\n| [Timers]", "never |\n| [Timers]");
  assert.ok(
    only(lintRepo(repo({ [README]: twice })), "feature-index").length >= 1,
  );
  const dangling = INDEX.replace("(timers.md)", "(gone.md)");
  const findings = only(
    lintRepo(repo({ [README]: dangling })),
    "feature-index",
  );
  assert.equal(findings.length, 2);
  assert.ok(
    findings.some((f) => f.line === 8 && /does not exist/.test(f.message)),
  );
  const header = INDEX.replace("What it is", "Summary");
  expectOne({ [README]: header }, "feature-index", README, 1);
  const h1 = INDEX.replace("# Feature map", "# Features");
  expectOne({ [README]: h1 }, "feature-index", README, 1);
});

test("feature-index: a linked duplicate is reported on the second row", () => {
  const row =
    "| [Calendar](calendar.md) | Days. | window, pulse | full / partial / none | never |\n";
  const twice = INDEX.replace(row, row + row);
  const hit = expectOne({ [README]: twice }, "feature-index", README, 8);
  assert.match(hit.message, /linked more than once/);
});

test("rules are skipped when there is no feature map", () => {
  const overrides: Record<string, string | null> = {};
  for (const key of Object.keys(base()))
    if (key.startsWith(FEATURES_DIR)) overrides[key] = null;
  assert.deepEqual(lintRepo(repo(overrides)), []);
});

// ---------------------------------------------------------------- coverage

test("coverage prints the template in index order", () => {
  const table = coverageTable(repo());
  assert.equal(
    table,
    [
      "| Feature | Surfaces | Reach (macOS / Windows / rize) | Result | Evidence | Notes |",
      "| --- | --- | --- | --- | --- | --- |",
      "| Calendar | window, pulse | full / partial / none | | | |",
      "| Timers | window, pulse | full / partial / none | | | |",
    ].join("\n"),
  );
  const reordered = INDEX.replace(
    /\| \[Calendar\].*\n\| \[Timers\].*\n/,
    "| [Timers](timers.md) | T. | window | full / full / full | never |\n| [Calendar](calendar.md) | C. | window | full / full / full | never |\n",
  );
  assert.match(
    coverageTable(repo({ [README]: reordered })),
    /\| Timers[\s\S]*\| Calendar/,
  );
});

// ---------------------------------------------------------------- the real repository

test("the real repository lints clean", () => {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
  assert.deepEqual(lintRepo(root), []);
});
