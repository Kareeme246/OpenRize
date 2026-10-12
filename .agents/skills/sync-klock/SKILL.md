---
name: sync-klock
description: Carry OpenRize changes into Klock, the enterprise fork at ../klock, by merging OpenRize main into it and opening a Klock PR. Use after a change to OpenRize's shared foundation (time entries, capture, AI, invoices, calendar, timesheet, the CLI, settings) has merged to main, when the user asks to sync, upstream or port a change to Klock, or to check whether Klock is behind.
---

# Syncing OpenRize into Klock

Klock (`../klock`, github.com/Kareeme246/klock) is a downstream fork: it shares
OpenRize's history and adds an organization layer. Shared-foundation changes are
made here first and reach Klock by **merging OpenRize's `main` into Klock**.
Klock's `UPSTREAM.md` explains the model; its `scripts/sync-openrize.sh` does the
git work. Run everything below from the Klock checkout.

## When a change needs to go to Klock

Almost every OpenRize change does: Klock runs the same tracker. The exceptions
(OpenRize's changelog, licence, release and updater plumbing) are handled by
Klock's `.openrize-sync` rules, so do not decide per change. Sync after the
OpenRize PR has merged to `main`; the script only ever merges `main`.

When an OpenRize PR changes behaviour Klock builds on, say so in its description
("Klock: ..."), so whoever syncs knows what to check.

## Steps

1. **See what is pending.** `scripts/sync-openrize.sh status` lists the OpenRize
   changes Klock lacks, dry-runs the merge (conflicting files) and lists the
   Klock-specific checks the changes trigger. Nothing is written.
2. **Start the sync.** Confirm the Klock checkout is clean and on `main`, then
   `scripts/sync-openrize.sh pr`. It branches `sync/openrize-<rev>` from
   `origin/main`, merges, keeps Klock's side of Klock-owned files, and replays
   earlier conflict resolutions (rerere). Exit 3 means conflicts remain; it
   lists them.
   - The very first sync needs `--baseline 7ab9544` (see `UPSTREAM.md`).
3. **Resolve what is left, by hand.** Keep both intents: OpenRize's change
   applied to Klock's version of the code. Klock's names, version, licence,
   enterprise commands and migrations stay. Use OpenRize's PR description and
   tests as the spec for its side. Work through every Klock-specific check from
   `status`: these are behaviours Klock depends on that the incoming change may
   break (for example, a published entry being split or removed). Fix them on
   the sync branch, with tests, as follow-up commits after the merge commit.
4. **Finish.** Ask the user before this step: it pushes and opens a PR.
   `scripts/sync-openrize.sh finish` runs `pnpm fix` and `pnpm verify`, commits
   the merge, pushes, and opens the PR, whose body lists the incoming changes
   and the Klock-specific checks. Verify the result in the running Klock app
   (`pnpm dev:drive`) as you would any change.

## Rules

- Never cherry-pick or copy code from OpenRize into Klock: only a merge records
  ancestry, and without it the change shows up as pending in every later sync.
- Never resolve a conflict by dropping one side wholesale (`checkout --ours` /
  `--theirs`) unless the file is Klock-owned. Add it to `.openrize-sync` instead
  if it always should be.
- When Klock starts depending on a shared-code behaviour, add a `watch` line
  for it to `.openrize-sync`, so future syncs flag changes to it.
