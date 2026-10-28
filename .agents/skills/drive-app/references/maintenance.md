# Maintaining the verification skill

The feature map and the skills rot silently. A label gets renamed, a page moves, a driver
verb changes, and nothing fails until an agent trusts the stale line: it wastes a run or,
worse, reports a false pass. A maintenance pass is how the map stays true. This file is the
procedure.

## Cadence

1. **Every PR that changes a user-visible feature** updates that feature's file in
   `.agents/skills/drive-app/references/features/`, and its `verified:` line only if the
   feature was actually driven.
2. **A full maintenance pass before every release tag** (see `RELEASING.md`).
3. **At least weekly while `main` is changing**, a pass that may be partial but always
   produces the full table.

A pass is also due whenever a verification run hits a label or command that no longer
matches.

## Procedure

1. `pnpm verify`. The skill linter is part of it and must be clean.
2. `scripts/app-drive.sh doctor`. Anything marked `fail` blocks the pass (record the rows
   as `BLOCKED`); `warn` limits it.
3. Generate the table:

   ```bash
   node scripts/lint-skills/cli.ts coverage
   ```

   It prints one row per feature in index order, with the Result, Evidence and Notes
   columns empty. Fill them in as you go.
4. For every row:
   1. Read the feature's `code` paths and compare them with the file. Look for drift:
      renamed labels, moved controls, new or removed sub-features, changed commands.
   2. Drive the file's "Driving it" steps on a fresh session (`scripts/app-drive.sh start --json`;
      `stop` it when the row is done). Start with the [onboarding](features/onboarding.md)
      steps when the session opens on the Permissions screen.
   3. Collect the evidence its "What proves it works" asks for. Save screenshots with
      `shot --out` to a folder that outlives the session.
   4. Fill the row.
5. Fix drift in the feature files and the skills in the same pass. Never edit product code in
   a maintenance pass: a product regression is reported (an issue or a handoff), not
   documented away.
6. After driving a feature end to end with proof, set its `verified:` to
   `YYYY-MM-DD macos-drive` (or `windows-drive`, `rize`) in its frontmatter. Do not set it for
   a feature you did not drive.

## Result vocabulary

The Result column holds exactly one of:

| Result | Meaning |
| --- | --- |
| `PASS` | driven end to end and the proof was captured |
| `FAIL` | the product does not do what the file says: a regression, report it |
| `DEFECT` | the harness or the map is wrong: a driver command fails, a label in the file is stale, or a step is unreachable that the file says is reachable |
| `BLOCKED: <reason>` | could not run, and the reason says what (for example no Accessibility grant) |
| `NOT REACHABLE` | the file's own "Not reachable" section covers it |

Evidence is a path or the command output. A `PASS` with empty Evidence is not a `PASS`.

## Two rules that exist because they failed in practice

- A pass never ends with "all features exercised". It ends with the table: one row per
  feature, every Result filled.
- A harness defect is a `DEFECT` row plus a fix or a filed follow-up, never a footnote under
  a `PASS`.

## Outcome of a pass

Exactly one of:

- `clean` - no changes were needed.
- `changed` - one PR with the corrected map and skills, confined to `.agents/skills/` plus
  the drivers when a harness defect needed a fix.
- `blocked` - names the blocker (for example, no machine with the Accessibility and Screen
  Recording grants).

State the outcome, then the table.

## What the linter guarantees, and what it cannot

`node scripts/lint-skills/cli.ts` (run by `pnpm verify`) guarantees structure, real repo
paths, real driver, `rize` and `pnpm` commands, route and page coverage, and an index that
links every feature file. It cannot know that a label is still on screen or that a flow
still works. Only a live pass can.
