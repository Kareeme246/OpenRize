---
id: rize-cli
title: rize CLI
summary: The rize command line tool that reads and edits OpenRize data with the app open or closed, controls the app and tracking, and prints plain tables or a stable JSON envelope.
surfaces: [cli]
routes: []
code: [src-tauri/cli, src-tauri/cli/COMMANDS.md, src-tauri/core/src/rpc.rs, src-tauri/core/src/protocol.rs, src-tauri/core/src/ipc.rs, src-tauri/src/rpc.rs, install.sh]
rize: ["rize status", "rize app start", "rize app status", "rize app open", "rize app quit", "rize paths", "rize install-path", "rize uninstall-path", "rize completions", "rize track start", "rize track stop", "rize track idle", "rize focus start", "rize focus stop"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# rize CLI

`rize` is a separate binary that ships inside the app (and can be installed alone by `install.sh`). With the app running it sends each request over a local socket (a Unix socket on macOS, a named pipe on Windows) to the app, which stays the only writer, so open windows and the tray update at once. With the app closed it opens the stored data itself through `openrize-core`, the same code the app answers with, behind a lock file that keeps the two from holding the stores together. It never creates the data: only the app makes and migrates the database. Every command, the app command behind it and whether it works with the app closed are listed in `src-tauri/cli/COMMANDS.md`, which is the authoritative table; this entry covers the part of the tool that is not one feature's data: the app and path commands, output and exit conventions, `--dry-run` and the installers. The data commands belong to the features they edit (for example [time-entries.md](time-entries.md), [projects.md](projects.md), [stopwatches.md](stopwatches.md), [settings.md](settings.md)).

## Sub-features

- status: `rize` or `rize status` - whether the app runs, tracking state, current activity, today's totals, entries to review and running timers.
- app-control: `rize app start [--track] [--show]`, `rize app status`, `rize app open [--review]`, `rize app quit`.
- tracking: `rize track start`, `rize track stop`, `rize track idle <minutes>` and `rize focus start [label]` / `rize focus stop` (see [capture.md](capture.md)).
- paths: `rize paths` - the open account, settings file, data directory and database path.
- path-install: `rize install-path [--dir DIR]` puts rize on PATH (a symlink in `~/.local/bin` by default on macOS, the user PATH on Windows); `rize uninstall-path` removes it.
- completions: `rize completions bash|zsh|fish|powershell|elvish` prints a shell completion script.
- json-envelope: `--json` prints one envelope for every command, errors included.
- exit-codes: 0 success, 1 failed, 2 invalid arguments, 3 the app must be running, 4 incompatible versions.
- dry-run: `--dry-run` previews `timers reset`, `timers rm`, `entries rm`, `entries rebuild`, `projects rm`, `clients rm` and `categories rm` and changes nothing.
- standalone-installer: `install.sh` downloads the notarized rize from a GitHub release and installs it (network; macOS Apple Silicon only).

## How to get to it

From a terminal: `rize ...` once it is on PATH (`rize install-path`, or the bundled binary at `openrize.app/Contents/MacOS/rize`, or the standalone install). In a driven session there is no PATH step: `scripts/app-drive.sh rize -- status` (or any other rize command after the `--`) runs the repo's own build of rize against that session's data folder. In the app there is no UI for the CLI itself; its effects show in the open window.

## Driving it

A fresh session may open on first-run onboarding; dismiss it first (see [onboarding.md](onboarding.md)). The `rize` passthrough needs a live session and passes `--data-dir` for you; do not pass your own. The first call compiles the CLI crate (debug), later calls are fast.

```bash
scripts/app-drive.sh start --json
scripts/app-drive.sh rize -- status
scripts/app-drive.sh rize -- --json status
scripts/app-drive.sh rize -- app status
scripts/app-drive.sh rize -- --json app status
scripts/app-drive.sh rize -- paths
scripts/app-drive.sh rize -- --json paths
```

App and tracking:

```bash
scripts/app-drive.sh rize -- app open
scripts/app-drive.sh rize -- app open --review
scripts/app-drive.sh rize -- track stop
scripts/app-drive.sh rize -- track start
scripts/app-drive.sh rize -- track idle 10
scripts/app-drive.sh rize -- focus start "Deep work"
scripts/app-drive.sh rize -- focus stop
```

`rize app open` shows the app's window and `--review` lands on today's review queue; both change the live window, which `scripts/app-drive.sh tree` reads.

Output formats and exit codes:

```bash
scripts/app-drive.sh rize -- --json timers list
scripts/app-drive.sh rize -- --json timers start "no such timer"; echo "exit=$?"
scripts/app-drive.sh rize -- --json track idle 0; echo "exit=$?"
scripts/app-drive.sh rize -- --bogus; echo "exit=$?"
scripts/app-drive.sh rize -- --json --bogus; echo "exit=$?"
```

Expected: the first is `ok: true` with `data` a list; the second `ok: false`, `error.code` `NOT_FOUND`, exit 1; the third `INVALID_ARGUMENT`, exit 2; the fourth an argument error printed to stderr, exit 2; the fifth the same as a JSON envelope on stdout, exit 2.

`--dry-run` (it needs a seeded timer or entry to preview):

```bash
scripts/app-drive.sh rize -- timers new "Client work"
scripts/app-drive.sh rize -- --dry-run timers reset "Client work"
scripts/app-drive.sh rize -- --json --dry-run timers rm "Client work"
scripts/app-drive.sh rize -- timers list
scripts/app-drive.sh rize -- --json --dry-run status; echo "exit=$?"
```

The dry runs print "Dry run: nothing was changed." and the stopwatch is still listed afterwards; the last command is rejected with `INVALID_ARGUMENT` and exit 2 because `status` is not previewable.

Local-only commands that touch no session data:

```bash
scripts/app-drive.sh rize -- completions zsh
scripts/app-drive.sh rize -- install-path --dir /tmp/openrize-proof/bin
```

`install-path --dir` creates `rize` as a symlink in that scratch folder and prints it; a second run reports it unchanged.

## What proves it works

- Plain status (required for a PASS of the app commands): `rize status` prints "OpenRize <version> is running, tracking." (or "tracking is off", or "outside tracking hours"), a "Now:" line, a "Today:" line and "Timers:" when any run. `rize --json app status` has `ok: true`, `data.running: true`, `data.appVersion` and `data.dataDir` equal to the session's `appData` from `start --json`.
- JSON envelope (required for a PASS of any `--json` change): the stdout of every command is exactly one JSON object with `schemaVersion`, `ok`, `data` on success and `error { code, message }` on failure (plus `candidates` for `AMBIGUOUS`); `schemaVersion` is a number. Window titles and URLs are absent unless `--full` is passed.
- Exit codes: read `$?` right after the command. 0 success, 1 `NOT_FOUND` / `FAILED` and other operation failures, 2 `INVALID_ARGUMENT` (bad flags, an unconfirmed deletion, `--dry-run` on a non-previewable command), 3 `APP_NOT_RUNNING`, 4 `INCOMPATIBLE`.
- Dry run: `data.dryRun` is `true`, `data.operation` names the command (for example `timerReset`), and `data.refused` is `null` or `{ code, message }`; nothing changes in `rize timers list`, the window or the database. Required: the dry run's output plus an unchanged read-back afterwards.
- `rize paths`: `data.dataDir` and `data.databaseFile` point inside the session's app-data folder, and `data.configFile` is the session's settings file.
- Completions: output starts with the shell's completion preamble and names subcommands such as `track` and `timers`.
- Window evidence of a CLI write: a change made through the CLI shows in `scripts/app-drive.sh tree` without a restart (for example a stopwatch created by `timers new` appearing on the Stopwatches page).

## Gotchas

- The passthrough points `RIZE_APP` at a path that does not exist, so inside a session `rize app start` and `rize track start` can never launch a second app: when the session's app is running they report it was already running, and when it is not they fail. They are still wrong in a real terminal against real data: `rize app start` launches the installed app in the background, and `rize track start` launches it and turns tracking on. Do not run either against anything but a driven session.
- `rize app quit` ends the session's app (the same shutdown as the tray's Quit) and returns once the stores are released; the driven window is gone afterwards and every later `rize` call falls back to the closed-app path or exits 3. Use it only as the last step, then `scripts/app-drive.sh stop`.
- `rize install-path` without `--dir` writes the real `~/.local/bin/rize` symlink in the human's home, and `rize uninstall-path` (it has no `--dir`) removes that real symlink. Neither belongs in a driven session: it is not isolated by the session. Use `install-path --dir <scratch folder>` only, and do not run `uninstall-path` at all. Windows edits the real user PATH in the registry the same way.
- `install.sh` downloads from GitHub and installs into `~/.local/bin` by default. Never run it from a driven session; with `--uninstall --dir <scratch folder>` it only removes a file you name.
- The idle threshold is `1` to `1440` minutes; anything else exits 2 with `INVALID_ARGUMENT`.
- Deletions and resets prompt on a terminal and need `--yes` otherwise (exit 2 with a hint); `--dry-run` never prompts. A driven shell has no terminal, so a real `rm` without `--yes` fails with exit 2, which is a safe default.
- A day word such as `today` is local; a bare `--to 2026-09-30` covers that day. Ranges default to today.
- `RIZE_APP` and `--data-dir` are global; the passthrough sets both for you. A plain `rize` on PATH uses the account the installed app remembers, which is the human's data.
- With the app closed rize answers from the stores; commands that need the live app (`app open`, `app quit`, `focus start`, `focus stop`) exit 3 or start the app.
- The standalone rize finds the installed app only to launch it.
- Output is plain aligned tables without colour; durations read `3h 25m` and times are local.
- `src-tauri/cli/rize.1` is the man page; keep `COMMANDS.md`, the man page and the clap help in step when a command changes.

## Not reachable

- A real PATH setup and a login shell using it: `install-path` without `--dir` and `uninstall-path` change the human's machine and must not be run here.
- `install.sh` against GitHub: it needs the network and writes outside the session; only its `--uninstall --dir <scratch folder>` and `--help` paths are local.
- Exit 3 (`APP_NOT_RUNNING`) and exit 4 (`INCOMPATIBLE`) from a live session: reaching them means quitting the app or mismatching versions.
- Planned commands (marked M2 or Later in `src-tauri/cli/COMMANDS.md`) for breaks, apps, invoices, AI, energy, updates, login item, accounts and a live status watch: they do not exist yet, so those features are driven only through the window.
- The app's menu-bar behaviour that `rize app quit` shares ([tray.md](tray.md)).
