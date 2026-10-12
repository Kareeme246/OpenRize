---
name: sync-klock
description: Keep OpenRize and Klock, its enterprise fork at ../klock, in step by porting changes by hand in both directions. Send - after an OpenRize fix or small product change merges to main, record it in Klock's port inbox (FROM_OPENRIZE.md). Receive - work through OpenRize's own inbox (FROM_KLOCK.md) and port the Klock changes that fit OpenRize. Use after merging a change to the shared foundation (time entries, capture, AI, invoices, calendar, timesheet, the CLI, settings), or when the user asks to sync, port or upstream a change between OpenRize and Klock.
---

# Syncing OpenRize and Klock

Klock (`../klock`, github.com/Kareeme246/klock) started as a fork of OpenRize and
adds an organization layer. The two products diverge over time, so changes move
between them **by hand, in both directions**, never by merging one repo into the
other. Each repo keeps a git-ignored port inbox at its root listing the other
repo's changes it still has to consider:

| Inbox | Lives in | Written by | Worked through by |
|---|---|---|---|
| `FROM_OPENRIZE.md` | Klock | OpenRize's `sync-klock` (send) | Klock's `sync-openrize` (receive) |
| `FROM_KLOCK.md` | OpenRize | Klock's `sync-openrize` (send) | OpenRize's `sync-klock` (receive) |

Klock has the mirror of this skill, `sync-openrize`; keep the two in step when
changing either. The inboxes are local files, never committed. Both checkouts
sit side by side; from any OpenRize worktree, Klock's checkout is:

```sh
KLOCK="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")/../klock"
```

## Send: record an OpenRize change for Klock

Do this after the OpenRize PR has merged to `main`, once per PR.

Record bug fixes and product changes to the shared foundation. Skip what is
OpenRize's alone: version bumps, the changelog, release, updater and CI plumbing,
licence and branding, and docs that only describe OpenRize. When unsure, record
it; the receiving side decides.

Append one entry to `$KLOCK/FROM_OPENRIZE.md` (create it with the heading
`# Changes from OpenRize to port into Klock` if missing). Skip a PR already in
the file, under any section:

```md
## OpenRize #101: fix: keep the live entry's second half after a split
Merged 2026-10-10 as ea46b52 - https://github.com/Kareeme246/OpenRize/pull/101

- **Problem:** what a user saw, or what was missing, in a sentence or two.
- **Fix:** how it was solved: the key idea, the files that carry it, and the
  tests that pin it.
- **Port notes:** anything that may play out differently in Klock, judged
  against "Where OpenRize and Klock differ" below. Omit when nothing does.
```

Write it for an agent who has not seen the PR: specific enough to re-implement
the fix in a codebase that may have drifted, short enough to scan.

## Receive: port Klock changes into OpenRize

1. **Read the inbox.** `FROM_KLOCK.md` at the OpenRize root. Work only on
   entries under neither `## Porting` nor `## Skipped`.
2. **Triage each entry** against "Where OpenRize and Klock differ" and the
   current OpenRize code:
   - Already in OpenRize, or no longer relevant: move it to `## Skipped` with a
     one-line reason.
   - Contradicts an OpenRize product decision, or only makes sense with the
     organization layer: move it to `## Skipped` with the reason. If it is
     partly useful (a generic fix inside an enterprise change), port that part.
   - A larger feature or a product choice the code cannot settle: ask the user
     before porting it.
   - Otherwise, port it.
3. **Port on a branch** (`chore/port-from-klock-<date>` or a `fix/` name for a
   single fix), one commit per entry with a Conventional Commit subject that
   names the source, e.g. `fix: keep the live entry's second half after a split
   (Klock #42)`. Re-implement the change against OpenRize's code with the source
   PR (`gh-axi pr view 42 --repo Kareeme246/klock`, and its diff) as the spec;
   copy code only where both sides are still identical. Carry its tests over.
   Bump the version and follow `RELEASING.md` as for any user-facing change.
4. **Verify** with `pnpm fix`, `pnpm verify` and, for anything visible, the
   running app (`drive-app` skill).
5. **Open the PR** listing every ported entry with its source PR link, and every
   skipped one with its reason. Move the ported entries under `## Porting` with
   the PR number; once it merges, delete them from the inbox.
6. **Do not echo it back.** A port is not a new change: never record it into
   Klock's inbox.

## Where OpenRize and Klock differ

Klock-side behaviour a ported or recorded change must not break, and OpenRize
choices a Klock change must not override. Add a line whenever a new difference
appears; keep this list identical in both skills.

- **Organizations.** Klock publishes time entries to an organization
  (`enterprise_source_bindings`, `server/`, `src-tauri/sync-protocol`, the
  `enterprise` modules, the `organizations` pages). Splitting, merging,
  deleting, undoing or redoing a published entry must not orphan its shared
  record, and rebuilt entries (`entry_builder.rs`) must keep published entries
  frozen. OpenRize has none of this.
- **Migrations.** Each app has its own migration sequence
  (`src-tauri/core/src/migrations.rs`); a ported schema change gets a new
  migration in the receiving repo, never the source's number.
- **Commands and state.** Klock registers enterprise commands and state in
  `src-tauri/src/lib.rs` alongside the shared ones.
- **Identity.** Each app keeps its own name, identifier, version, CLI name
  (`rize` / `klock`), config dir and data dirs; nothing in Klock is keyed on
  OpenRize's names.
- **Licence and release.** OpenRize is AGPL and released publicly with the
  in-app updater; Klock is proprietary and ships without OpenRize's updater
  artifacts. Release, CI, changelog and licence changes never cross over.
