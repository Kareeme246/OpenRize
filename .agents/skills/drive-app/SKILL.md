---
name: drive-app
description: macOS only (on Windows use the drive-app-windows skill). Verify a change in the real OpenRize app - run doctor, launch an isolated instance, drive the feature through the feature map with the UI driver and the rize data lever, prove it with backend read-backs and screenshots, and clean up. Use whenever a UI or backend change must be shown to work by actually using the app, not just by reading code or running pnpm verify. Several agents can run their own instances at the same time.
---

# Verifying a change in the real OpenRize app

Verification means closing the loop yourself: use the changed feature in the real
app and show evidence that it works. Reading code, `pnpm verify` and unit tests do
not do that. Two tools give you the lever, and both beat a throwaway script:

- **the UI driver**, `scripts/app-drive.sh`: launches your own isolated instance, clicks
  real controls, types into real fields and reads the screen back;
- **the data lever**, `rize`, reached per session through
  `scripts/app-drive.sh rize -- ...`: seeds data into the instance and reads backend
  state back as JSON, so proof never needs raw SQL against a private schema.

`pnpm dev:drive <command>` is an alias for the driver. Prefer the direct path in
scripts: `pnpm run` prints a banner line that will pollute anything you capture
with `$(...)`.

macOS only; on Windows use the `drive-app-windows` skill. There is no supported
Tauri WebDriver backend for macOS (only Linux/webkit2gtk and Windows/Edge), so do not
reach for Playwright or WebDriver, and do not write Playwright/WebDriver/E2E suites
for the app window: such a test would not exercise the real Tauri backend here. This
is the driver.

| Group | Command | What it does |
| --- | --- | --- |
| lifecycle | `doctor [--json]` | checks this machine can launch and drive an instance; launches nothing |
| lifecycle | `start [--json]` | launches an isolated instance and prints its session id |
| lifecycle | `list [--json]` | lists the live sessions started from this checkout |
| lifecycle | `stop [-s ID \| --all] [--dry-run] [--json]` | kills the instance and deletes its data; `--dry-run` only reports |
| inspection | `tree [--full]` | prints the window's accessibility tree |
| inspection | `find "<label>"` | resolves a label without acting on it |
| inspection | `wait "<text>" [seconds]` | polls the tree until the text shows up (default 15s) |
| inspection | `shot [--out PATH]` | screenshots the window, prints the PNG path |
| inspection | `logs [n]` | tails the instance's Vite and cargo output |
| interaction | `click "<label>"` | `AXPress` on the control, which is a genuine DOM click |
| interaction | `fill "<field>" "<text>"` | clicks the field to focus it, then types |
| interaction | `key <keyspec>` | a keystroke: `return`, `tab`, `escape`, `backspace`, `delete`, `space`, arrows, `cmd+comma`, `shift+tab`, or any literal character |
| data | `rize [-s ID] -- <args>` | runs the repo's `rize` CLI against this session's data |

Every verb except `doctor` and `start` takes `-s dNNNNN` to pick a session. Exit
codes: 0 success, 1 failure, 2 usage error (unknown command, missing arguments).
`scripts/app-drive.sh --help` prints the full usage.

## 1. Launch

```bash
scripts/app-drive.sh start --json
# stdout is one JSON object: {"session":"d24987","port":24987,"pid":...,"identifier":"...","config":"...","appData":"...","log":"..."}
```

Capture the session id from the JSON (progress goes to stderr, so stdout is clean):

```bash
SID=$(scripts/app-drive.sh start --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["session"])')
# or, without python3:
SID=$(scripts/app-drive.sh start --json | sed -n 's/.*"session":"\([^"]*\)".*/\1/p')
echo "$SID"
```

Plain `start` prints `session=dNNNNN port=... pid=... identifier=...` and a second
line with `config=` and `app-data=`.

- **Cold build.** A warm `src-tauri/target` starts in seconds. The first `start` after a
  clean checkout pays for a full `cargo build`; `start` waits up to 300s
  (`OPENRIZE_DRIVE_TIMEOUT` overrides that). If your command runner times out sooner,
  run `start` in the background with its output redirected to a file, then poll
  `scripts/app-drive.sh list` until the session shows.
- **`--no-watch`.** The instance runs `tauri dev --no-watch`: the frontend hot-reloads,
  but a Rust change only shows up in a fresh instance (`stop`, then `start`).
- **Never `pnpm tauri dev`.** It uses a fixed port and identifier and collides with
  another agent's instance or a real running copy of the app. When you only need one
  look at one screen and no interaction, `pnpm dev:screenshot` launches, captures by
  window id, tears down and prints the PNG path; read that file. A look is not proof of
  behaviour.

With exactly one live session the commands need no session argument. With more than
one, pass `-s dNNNNN` (or export `OPENRIZE_SESSION=dNNNNN`).

## 2. Doctor

Run `scripts/app-drive.sh doctor` before `start`, and again whenever a command fails
unexpectedly. It launches nothing. Output from a ready machine (paths shortened, one
warning kept to show its shape):

```
ok   macos  Darwin
ok   node-modules  .../node_modules/.bin/tauri
ok   pnpm  /opt/homebrew/bin/pnpm
ok   cargo  /Users/me/.cargo/bin/cargo
ok   accessibility  granted
ok   screen-recording  granted
ok   sqlite3  /usr/bin/sqlite3
ok   warm-build  .../src-tauri/target/debug/openrize
warn sessions  0 live, 2 stale (directory left by a dead process): d24987 d32305
     fix: scripts/app-drive.sh stop --all
doctor: ready
```

Every non-`ok` line is followed by a `fix:` line. `fail` blocks (exit 1, and the last
line says how many problems); `warn` only limits what you can do. `doctor --json`
prints `{"ok":true|false,"checks":[{"id","status","detail","fix"?}]}`.

| Check | Bad status | What it means |
| --- | --- | --- |
| `macos` | fail | the driver only works on macOS |
| `node-modules` | fail | `node_modules/.bin/tauri` is missing: run `pnpm install` |
| `pnpm`, `cargo` | fail | the toolchain `start` needs is not on PATH |
| `accessibility` | fail | the terminal app has no Accessibility grant: every drive verb (`tree`, `find`, `click`, `fill`, `key`, `wait`) fails and `start` cannot confirm the window |
| `screen-recording` | warn | the terminal app has no Screen Recording grant: `shot` fails, everything else works |
| `sqlite3` | warn | only matters if you read the database file directly |
| `warm-build` | warn | no `src-tauri/target/debug/openrize` yet: the first `start` compiles the whole Rust tree |
| `sessions` | warn | stale session directories left by dead processes: `scripts/app-drive.sh stop --all` |

Grant permissions to the terminal app running the command (System Settings > Privacy
& Security), then rerun `doctor`.

**Doctor after start.** The instance is ready when `start` has returned (it waits until
the tree shows a `[Button]` node) and `tree` shows `[Button]` nodes. A fresh session is an
empty install with its own empty settings folder, so it may open on the first-run
Permissions screen instead of the Calendar: the tree shows the heading "Permissions" and a
"Continue" button. `click "Continue"` gets past it with no native prompt (it fails with
"no actionable element" when the screen is not up, which is fine). Never click "Grant
Accessibility", "Grant Automation", "Restart required" or "Relaunch now" in a driven
session: they open native prompts or restart the app. Details in
[onboarding](references/features/onboarding.md).

## 3. Drive

1. Find the feature in [the feature map](references/features/README.md). Open only the
   files for the features your change touches.
2. Follow each file's "Driving it". Labels in those commands come from the code, not from
   memory; if one no longer matches the screen, that is a defect in the map (see "Keeping
   this skill true").
3. Drive the success path, and also the cancel, empty, error and persistence paths that
   apply. Persistence: `stop` deletes the instance's data, so there is no stop-and-start
   persistence check. Instead navigate away and back, or read the state back through `rize`.

### Finding what to click

`tree` prints the window's accessibility tree, which is the real DOM as the OS sees it.
Labels in that tree are exactly what `click`, `fill`, `find` and `wait` match on
(illustrative output):

```
$ scripts/app-drive.sh tree
[Window] OpenRize
  [WebArea] OpenRize
    [Button] Calendar
    [Button] Stopwatches
    [Button] Settings
    [TextField] New tracker name = "End to end"
    [Button] New tracker
```

- The label comes from the element's accessible name, so it is the button's visible text or
  its `aria-label`, **not** an input's placeholder. The field above is `"New tracker name"`
  even though it renders the placeholder "What are you working on?".
- Text fields print their current contents after `=`, so you can check what you typed
  without taking a screenshot.
- `tree --full` includes the anonymous layout groups. You rarely want it.

Matching is exact-first: an exact label beats a case-insensitive match, which beats a
substring match. If a name is still ambiguous the command refuses and lists the
candidates; disambiguate with `-i N`, or narrow with `-r AXRadioButton` / `-r AXTextField`.

### Seeding data and reading it back

A fresh instance is empty. Seed it through the data lever instead of clicking through
setup, and read the result back as JSON. The first `rize` call compiles the CLI crate
(debug profile); later ones are fast. `--json`, `--full`, `--yes` and `--dry-run` after the
`--` reach `rize`.

```bash
TODAY=$(date +%F)
scripts/app-drive.sh rize -s "$SID" -- projects add "Client work"
scripts/app-drive.sh rize -s "$SID" -- entries add --from "${TODAY}T09:00" --to "${TODAY}T10:00" -d "Write the report" --category Coding --project "Client work"
scripts/app-drive.sh rize -s "$SID" -- --json projects list
scripts/app-drive.sh rize -s "$SID" -- --json entries list --from today
```

The instance holds its stores, so `rize` reaches it over the app's local endpoint and the
change shows in the window at once. Times must be full date-times, and the Calendar day
starts at 5 AM. Every `rize` command is listed in `src-tauri/cli/COMMANDS.md`; only the
Shipped ones work.

A worked example, all the way to backend state:

```bash
SID=$(scripts/app-drive.sh start --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["session"])')
scripts/app-drive.sh click -s "$SID" "Stopwatches"
scripts/app-drive.sh fill  -s "$SID" "New tracker name" "End to end"
scripts/app-drive.sh click -s "$SID" "New tracker"
scripts/app-drive.sh wait  -s "$SID" "Stopwatch End to end"
scripts/app-drive.sh rize  -s "$SID" -- --json timers list
scripts/app-drive.sh shot  -s "$SID" --out /tmp/openrize-proof/stopwatch-created.png
scripts/app-drive.sh stop  -s "$SID"
```

### Concurrency

- `click` is coordinate-free and needs no focus, so any number of agents can click their
  own instances simultaneously.
- `fill` and `key` send **global** keystrokes and therefore have to bring their window
  frontmost first. Two agents typing at the same moment can steal each other's keystrokes.
  Keep typing steps short, and prefer `click` where a click will do.
- Concurrent `start`s serialize behind cargo's build lock on the shared `src-tauri/target`.
  They queue rather than fail; the app processes then run in parallel.

## 4. Prove

Evidence counts in this order, strongest first:

1. **Backend state read back** through `scripts/app-drive.sh rize -- --json ...` (or the
   instance's own SQLite file; `start --json` prints its `appData` folder). This shows the
   Rust side did the thing, not just that the UI looks right.
2. **`tree` and `wait` labels**: structure, text, field contents, counts.
3. **`shot --out PATH` screenshots** for anything visual: spacing, colour, alignment, a
   broken layout. Look at the PNG you captured. Any visual claim needs a screenshot you
   actually looked at.

Rules:

- "Tests pass" and "`pnpm verify` passes" are not proof that a feature works.
- A claim with no artifact is inconclusive, and you report it as inconclusive.
- Evidence must survive cleanup. Write shots with `--out` to a path outside the session
  (it survives `stop`). Shots taken without `--out` land in the session folder; `stop`
  moves them to `${TMPDIR:-/tmp}/openrize-app-drive-evidence/<session id>/` and prints where.
- Report the exact commands you ran and the outputs you saw, not a summary of them.
- Each feature file's "What proves it works" says which evidence a PASS needs. Use it.

## 5. Clean up

```bash
scripts/app-drive.sh stop            # the one live session
scripts/app-drive.sh stop -s dNNNNN  # one of several
scripts/app-drive.sh stop --all      # every session from this checkout, stale ones included
scripts/app-drive.sh stop --all --dry-run   # report only: kills and deletes nothing
```

**Always `stop` when you are done.** A leaked instance keeps a Vite server, a cargo process
and a tray icon alive. Use `--dry-run` first when unsure what is there; `--json` prints
the same plan as one object. A session whose process died (a window closed by hand) is
stale: its directories remain until `stop --all`.

`stop` kills the session's process group, deletes its `Application Support/<identifier>`
folder and its session folder. Before deleting anything it refuses (and deletes nothing for
that session) if the session's meta file looks corrupted, i.e. the data folder is not a
`com.offlinestudios.openrize.drive<digits>` directory directly under Application Support, or the
session folder is not directly under this checkout's session root. Screenshots made
without `--out` are moved to the evidence folder first; screenshots made with `--out` are
never touched.

## Isolation, and running several instances at once

Each session gets:

- **its own free port**, chosen at random and checked before use, so Vite dev servers never
  collide;
- **its own Tauri `identifier`** (`com.offlinestudios.openrize.driveNNNNN`), which is what
  scopes `app_data_dir`, so `activity.db` and its timers are private to the session;
- **its own `XDG_CONFIG_HOME`**, because `settings.rs::config_dir()` resolves to
  `~/.config/openrize` and is *not* identifier-scoped. Without this override an agent
  clicking through Settings would rewrite the human's real preferences;
- **its own process group**, so teardown kills the whole tree and nothing else;
- **a session namespace keyed to this checkout**, so an agent in another worktree cannot see
  or stop your session.

Two concurrent sessions were verified end to end: separate ports listening, separate
databases (a tracker created in one is absent from the other), and the real app's own data
and `~/.config/openrize/settings.json` untouched throughout.

## What this machine may not be able to exercise

State exactly what you could not exercise; never skip it silently.

- **No Accessibility grant:** every drive verb fails. `doctor` says so.
- **No Screen Recording grant:** `shot` fails; everything else works. Visual claims are then
  unverified.
- **Headless or CI runners** cannot grant either permission. UI verification there is
  impossible: label it unverified, do not skip it silently, and do not substitute
  `pnpm verify`.
- **Native surfaces are unreachable:** the tray icon and its menu, OS permission prompts,
  OS notifications and the window frame. Pulse opens only from a native tray click, so it
  cannot be reached.
- **The driver reads only the app's first window.** The Pulse panel and the break reminder
  corner panel are other windows and are not in the tree it prints.
- **Dragging is unsupported** (for example dragging on the Calendar timeline to add a
  session). Seed with `rize entries add` instead.
- **The on-device AI tier** needs macOS 26 with Apple Intelligence; without it only the rule
  and personal-model tiers run.

A run that could not exercise something reports it as exactly that, naming the feature file
and the missing capability.

## Gotchas

- **The webview can reload under you.** Vite HMR is live, so editing `src/` while a session
  runs may remount React and drop you back on the Calendar. Do not assume the page you navigated to
  three commands ago is still showing; `wait` for a marker on the page you expect, or
  re-click your way there.
- **Do not write `AXValue` to set a field.** WebKit accepts the write, silently discards it,
  and React would never see an `onChange` anyway. That is why `fill` clicks and types
  instead.
- **Do not run `pnpm tauri dev` directly.** Fixed port, fixed identifier, real user data.
  Use this script or `pnpm dev:screenshot`.
- **`rize` targets the session's base data folder.** After switching accounts inside the
  session the app reads another folder, so `rize` reads the wrong one. Switch accounts only
  when you are verifying that, and read state from the UI then.
- **`rize app start` and `rize track start` cannot launch a second app** from the
  passthrough, by design: it points `RIZE_APP` at a path that does not exist.
- `scripts/ax-drive.js` is the JXA accessibility driver behind the script. It walks
  `AXUIElement` references directly because System Events' `entire contents` stops at the
  webview boundary and reports an empty group.

## Keeping this skill true

The feature map and this skill rot silently. A stale label wastes a run, or worse, yields a
false pass.

- A change to a user-visible feature updates its feature file in the same PR. Set
  `verified:` only if you actually drove it end to end.
- A new page, route or window gets a new feature file; the skill linter fails `pnpm verify`
  until it exists.
- A harness defect found while verifying (a driver command fails, a label is stale, a step
  the file calls reachable is not) is reported as a defect and fixed or filed, never a
  footnote under a PASS.
- `pnpm verify` runs the skill linter (`node scripts/lint-skills/cli.ts`). It guarantees
  structure, real paths, real commands and complete coverage; only a live pass can tell
  that a label is still on screen.
- Cadence, procedure and result vocabulary: [maintenance](references/maintenance.md).
