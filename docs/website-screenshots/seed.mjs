#!/usr/bin/env node
// Loads seed.json into an OpenRize activity database, so the app shows the same
// fictional day every time. Run it with the app closed, after the app has created
// the database once (it owns the migrations):
//
//   node docs/website-screenshots/seed.mjs "<app data dir>/activity.db"
//
// Re-running replaces the seeded rows (ids start with "seed-") and leaves everything
// else alone. Needs the `sqlite3` command line tool (preinstalled on macOS).
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dbPath = process.argv[2];
if (!dbPath) {
  console.error('usage: seed.mjs "<app data dir>/activity.db"');
  process.exit(1);
}

const seed = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), "seed.json"), "utf8"));

const sqlite = (sql) => execFileSync("sqlite3", [dbPath], { input: sql, encoding: "utf8" });
const lit = (value) => {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "number") return String(value);
  return `'${String(value).replaceAll("'", "''")}'`;
};
const insert = (table, row) =>
  `INSERT INTO ${table} (${Object.keys(row).join(", ")}) VALUES (${Object.values(row).map(lit).join(", ")});`;
const at = (time) => Date.parse(`${seed.day}T${time}${seed.utcOffset}`);

const categories = new Map(
  sqlite("SELECT name || char(9) || id FROM categories WHERE deleted_at IS NULL;")
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => line.split("\t")),
);
const categoryBillable = new Map(
  sqlite("SELECT name || char(9) || billable_default FROM categories WHERE deleted_at IS NULL;")
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const [name, flag] = line.split("\t");
      return [name, Number(flag)];
    }),
);
if (categories.size === 0) {
  console.error("No categories found: start the app once so it creates and migrates the database.");
  process.exit(1);
}
const categoryId = (name) => {
  const id = categories.get(name);
  if (!id) throw new Error(`Unknown category "${name}"`);
  return id;
};

const clientIds = new Map(seed.clients.map((client) => [client.name, client.id]));
const projectIds = new Map(seed.projects.map((project) => [project.name, project.id]));
const projectId = (name) => {
  const id = projectIds.get(name);
  if (!id) throw new Error(`Unknown project "${name}"`);
  return id;
};

/**
 * Lays an entry's activity out as consecutive segments of at most five minutes,
 * always continuing with the app that has the most time left, so the activity
 * strip reads like real app switching. The minutes must add up to the entry.
 */
function segmentsFor(entry) {
  const start = at(entry.start);
  const end = at(entry.end);
  const left = entry.activity.map(([app, minutes, title]) => ({ app, title, ms: Math.round(minutes * 60_000) }));
  const total = left.reduce((sum, item) => sum + item.ms, 0);
  if (Math.abs(total - (end - start)) > 1000) {
    throw new Error(`${entry.id}: activity adds up to ${total / 60_000} min, the entry is ${(end - start) / 60_000} min`);
  }
  const out = [];
  let cursor = start;
  let last = null;
  while (left.some((item) => item.ms > 0)) {
    const candidates = left.filter((item) => item.ms > 0 && item !== last);
    const pool = candidates.length > 0 ? candidates : left.filter((item) => item.ms > 0);
    const pick = pool.reduce((best, item) => (item.ms > best.ms ? item : best));
    const ms = Math.min(pick.ms, 5 * 60_000);
    out.push({ app: pick.app, title: pick.title, startedAt: cursor, endedAt: Math.min(cursor + ms, end) });
    cursor += ms;
    pick.ms -= ms;
    last = pick;
  }
  return out;
}

const statements = [
  "BEGIN;",
  "DELETE FROM segments WHERE entry_id LIKE 'seed-%';",
  "DELETE FROM suggestions WHERE entry_id LIKE 'seed-%';",
  "DELETE FROM classify_jobs WHERE entry_id LIKE 'seed-%';",
  "DELETE FROM time_entries WHERE id LIKE 'seed-%';",
  "DELETE FROM projects WHERE id LIKE 'seed-%';",
  "DELETE FROM clients WHERE id LIKE 'seed-%';",
];

const created = at("08:00:00");
for (const client of seed.clients) {
  statements.push(
    insert("clients", {
      id: client.id,
      name: client.name,
      email: client.email,
      default_rate: client.defaultRate,
      currency: client.currency,
      created_at: created,
      updated_at: created,
    }),
  );
}
for (const project of seed.projects) {
  statements.push(
    insert("projects", {
      id: project.id,
      client_id: clientIds.get(project.client),
      name: project.name,
      color: project.color,
      status: "active",
      billable_default: project.billable ? 1 : 0,
      created_at: created,
      updated_at: created,
    }),
  );
}

for (const entry of seed.entries) {
  const approved = entry.status === "approved";
  const ended = at(entry.end);
  statements.push(
    insert("time_entries", {
      id: entry.id,
      started_at: at(entry.start),
      ended_at: ended,
      description: entry.description,
      description_origin: "ai",
      category_id: categoryId(entry.category),
      project_id: projectId(entry.project),
      status: entry.status,
      approved_by: approved ? "user" : null,
      source: "auto",
      billable: categoryBillable.get(entry.category) ?? 0,
      created_at: ended,
      updated_at: ended,
    }),
  );
  for (const segment of segmentsFor(entry)) {
    statements.push(
      insert("segments", {
        app: segment.app,
        title: segment.title,
        kind: "activity",
        started_at: segment.startedAt,
        ended_at: segment.endedAt,
        reviewed: approved ? 1 : 0,
        entry_id: entry.id,
      }),
    );
  }
  if (!approved) {
    // A finished classification run, so the app shows its confidence instead of re-running it.
    statements.push(
      insert("classify_jobs", {
        id: `${entry.id}-job`,
        entry_id: entry.id,
        kind: "classify",
        state: "done",
        attempts: 1,
        next_at: ended,
        created_at: ended,
        updated_at: ended,
      }),
    );
    for (const [field, value] of [
      ["category", categoryId(entry.category)],
      ["project", projectId(entry.project)],
    ]) {
      const confidence = entry.confidence[field];
      statements.push(
        insert("suggestions", {
          id: `${entry.id}-${field}`,
          entry_id: entry.id,
          field,
          value_id: value,
          confidence,
          raw_confidence: confidence,
          signals: JSON.stringify([{ kind: "dominant", text: `Mostly ${entry.activity[0][0]}` }]),
          engine: "full",
          created_at: ended,
          updated_at: ended,
        }),
      );
    }
  }
}
statements.push("COMMIT;");

sqlite(statements.join("\n"));
console.log(`Seeded ${seed.entries.length} entries for ${seed.day} into ${dbPath}`);
