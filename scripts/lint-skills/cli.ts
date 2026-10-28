/**
 * Lint the repo skills and the drive-app feature map.
 *
 *   node scripts/lint-skills/cli.ts [check|coverage] [--json] [--root DIR]
 *
 * All rules live in lint.ts; this file only handles arguments and output.
 */
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import {
  coverageTable,
  FEATURES_DIR,
  type Finding,
  isMapFinding,
  lintRepo,
  listSkills,
  loadFeatures,
} from "./lint.ts";

const USAGE = `usage: node scripts/lint-skills/cli.ts [command] [options]

Checks every skill under .agents/skills and the drive-app feature map against the code.

Commands:
  check       run every rule (default)
  coverage    print the per-feature coverage table template for a maintenance pass

Options:
  --json      machine-readable output
  --root DIR  lint another tree (default: this repository)
  -h, --help  show this help

Exit codes: 0 clean, 1 findings, 2 bad usage.

Example:
  node scripts/lint-skills/cli.ts coverage --json`;

function usageError(message: string): never {
  console.error(`error: ${message}\n\n${USAGE}`);
  process.exit(2);
}

function printFindings(findings: Finding[], json: boolean): void {
  if (json) {
    console.log(JSON.stringify({ ok: false, findings }));
    return;
  }
  for (const f of findings)
    console.log(`${f.file}:${f.line}: ${f.rule}: ${f.message}`);
  console.log(`skills lint: ${findings.length} problem(s)`);
}

function parseCommandLine() {
  try {
    return parseArgs({
      options: {
        json: { type: "boolean", default: false },
        root: { type: "string" },
        help: { type: "boolean", short: "h", default: false },
      },
      allowPositionals: true,
    });
  } catch (error) {
    return usageError(error instanceof Error ? error.message : String(error));
  }
}

const { values, positionals } = parseCommandLine();
if (values.help) {
  console.log(USAGE);
  process.exit(0);
}
const command = positionals[0] ?? "check";
if (command !== "check" && command !== "coverage")
  usageError(`unknown command "${command}"`);
if (positionals.length > 1)
  usageError(`unexpected argument "${positionals[1]}"`);

const root = resolve(
  values.root ?? join(dirname(fileURLToPath(import.meta.url)), "..", ".."),
);
const findings = lintRepo(root);

if (command === "check") {
  if (findings.length > 0) {
    printFindings(findings, values.json);
    process.exit(1);
  }
  const skills = listSkills(root).length;
  const features = loadFeatures(root).length;
  if (values.json) console.log(JSON.stringify({ ok: true, findings: [] }));
  else console.log(`skills lint: ok (${skills} skills, ${features} features)`);
} else {
  const broken = findings.filter(isMapFinding);
  if (broken.length > 0) {
    printFindings(broken, values.json);
    process.exit(1);
  }
  const features = loadFeatures(root);
  if (features.length === 0) {
    printFindings(
      [
        {
          file: `${FEATURES_DIR}/README.md`,
          line: 1,
          rule: "feature-index",
          message: "there is no feature map to build a coverage table from",
        },
      ],
      values.json,
    );
    process.exit(1);
  }
  if (values.json) {
    console.log(
      JSON.stringify({
        ok: true,
        features: features.map((f) => ({
          id: f.id,
          title: f.title,
          surfaces: f.surfaces,
          reachMacos: f.reachMacos,
          reachWindows: f.reachWindows,
          reachRize: f.reachRize,
          verified: f.verified,
        })),
      }),
    );
  } else {
    console.log(coverageTable(root));
  }
}
