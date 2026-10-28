# Verification tooling: what we use, what we decided against

How a change is shown to work in the real app is described in the `drive-app` skill
(`.agents/skills/drive-app/SKILL.md`; `drive-app-windows` on Windows), with the feature map
next to it and the upkeep procedure in `.agents/skills/drive-app/references/maintenance.md`.
This file records which extra tools were built, which were not, and why, so the same
questions are not reopened from scratch. For each item: the problem it would solve here, what
it costs to own, how it fails, and the recommendation.

## 1. Session-scoped data lever (built)

`scripts/app-drive.sh rize -- ...` (and `node scripts/app-drive-windows.mjs rize -- ...`)
runs the repo's own `rize` CLI against a driven session's data.

- **Problem.** A fresh driven instance is empty, and backend proof used to mean raw `sqlite3`
  against a private schema.
- **Cost.** A few lines per driver. The first call compiles the CLI crate.
- **Failure mode.** After switching accounts inside a session it targets the session's base
  folder, not the account the app now has open. By design it cannot launch an app: `RIZE_APP`
  points at a path that does not exist, so `rize app start` and `rize track start` fail
  instead of starting a second app outside the session.
- **Recommendation.** Keep. Highest value for the least machinery: it needs nothing the repo
  does not already have, and it makes seeding and read-back the same tool.

## 2. Bug bot: automatic triage and reproduction of bug reports (not built)

- **Problem.** Reproduce a report before anyone reads it.
- **Blockers here.** There is no inbound bug-report channel on `main` yet. UI reproduction
  needs a Mac with Accessibility and Screen Recording granted, which hosted CI runners cannot
  grant. The app's privacy posture (no telemetry, activity never leaves the machine) means a
  report cannot carry the reporter's data, so a reproduction has to start from a recipe, not a
  data dump.
- **Cost.** A scheduled or triggered agent on a granted Mac, plus triage noise.
- **Failure mode.** A confident false "cannot reproduce"; automatic fix PRs on bugs nobody
  reproduced; leaked private data.
- **Recommendation.** Once a report channel exists, make its template ask for the app version,
  the OS, the output of `rize --json status` and the steps written in feature-map terms. Then
  run the verification skill on a granted dev Mac, triggered by hand, before considering any
  automation. Never auto-fix.

## 3. Scheduled maintenance pass (not built)

- **Problem.** The feature map drifts between releases; a daily pass would catch it sooner.
- **Blockers here.** It needs a granted Mac and a scheduler. Hosted runners cannot drive the UI.
- **Cost.** A machine that stays logged in with the grants, and review time for every pass.
- **Failure mode.** A pass that cannot run (locked screen, revoked grant) and reports nothing;
  a stream of unreviewed "changed" PRs.
- **Recommendation.** Follow the cadence in
  `.agents/skills/drive-app/references/maintenance.md`. Revisit a local scheduled routine on
  the maintainer's Mac once passes are routinely clean.

## 4. Video proof (not built)

- **Problem.** A motion bug (an animation, a drag, a transition) is not visible in a still.
- **Cost.** macOS `screencapture` video is whole-screen, which risks recording private windows,
  and needs Screen Recording. The Windows driver could use a CDP screencast of the webview
  only.
- **Failure mode.** Recordings that include something private; large files nobody opens.
- **Recommendation.** Window-id screenshots plus `rize --json` read-backs are enough for now.
  If a motion bug needs it, add a webview screencast on Windows first.

## 5. Performance traces (not built)

The article this practice comes from has `trace` and `profile` verbs in its CLI.

- **Problem.** Showing that a perf change helped, with numbers.
- **Cost.** Another driver verb per platform to keep working.
- **Failure mode.** Traces taken on different machines or builds compared as if comparable.
- **Recommendation.** When a perf task arrives, take a before trace with Instruments (`xctrace`,
  already present with the required Xcode) or CDP `Tracing` on Windows, fix, and re-trace with
  the same command. Add a driver verb only after it has been needed twice.

## 6. Property-based checks for core invariants (not built)

Invariants worth checking: every minute has one owner; money is integer cents.

- **Problem.** Example-based tests miss the interleavings that break an invariant.
- **Cost.** A new dev dependency, or a hand-rolled generator. That is a maintainer decision,
  so it was not made here.
- **Failure mode.** A property that passes because the generator never produces the bad case.
- **Recommendation.** The highest-value next step for correctness. Decide on a property-testing
  approach for `openrize-core`, start with entry ownership, and prove each property fails on a
  deliberately broken variant before trusting it.

## 7. PR review bots and third-party verification plugins (not adopted)

Not adopted, by decision: nothing is installed. The practice is re-derived here in repo-owned
scripts that `pnpm verify` checks, so it moves with the code and runs the same on every
machine.

## Status

Validated on this machine:

- `pnpm verify`, including the skill linter.
- `rize --dry-run`, through its cargo tests.
- The macOS driver's `doctor`, `list`, `stop --dry-run` and `stop` on fake sessions.

Written only:

- Every feature file's driving steps. Nothing was driven end to end; every feature says
  `verified: never`.
- The Windows driver changes (`doctor`, `stop --dry-run`, `shot --out`, the `rize`
  passthrough, `--json`), on Windows. They were only syntax-checked and exercised on macOS,
  where `doctor` reports `fail windows`.
