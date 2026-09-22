---
name: drive-app
description: Launch an isolated OpenRize instance, click through the real Tauri webview UI, read the result back, and tear it down. Use when verifying a UI or backend change by actually using the app, not just by reading code or running pnpm verify. Several agents can run their own instances at the same time.
---

# Driving the real OpenRize app

the app paints. Neither proves the app *works*. This skill is how you use it:
launch your own isolated instance, click real controls, type into real fields,
and read the result back out of the running app.

Everything goes through one script:

```
scripts/app-drive.sh <command> [args]
```

`pnpm dev:drive <command>` is an alias for it. Prefer the direct path in
scripts: `pnpm run` prints a banner line that will pollute anything you capture
with `$(...)`.

macOS only. There is no supported Tauri WebDriver backend for macOS, so do not
reach for Playwright or WebDriver; this is the driver.

## Prerequisites

- macOS, with the repo's dependencies installed (`pnpm install`).
- **Accessibility permission** for whatever terminal app you are running in
  (System Settings > Privacy & Security > Accessibility). Without it, every
  drive command fails with an osascript authorization error.
- **Screen Recording permission** for the same app, needed only by `shot`.
- A warm `src-tauri/target` directory. The first `start` after a clean checkout
  pays for a full `cargo build`; `start` waits up to 300s (`OPENRIZE_DRIVE_TIMEOUT`
  overrides that).

## The loop

```bash
scripts/app-drive.sh start                  # prints: session=dNNNNN port=... pid=...
scripts/app-drive.sh tree                   # what is on screen right now
scripts/app-drive.sh click "Trackers"       # click something by its visible label
scripts/app-drive.sh wait "No trackers yet" # block until the UI catches up
scripts/app-drive.sh shot                   # prints a PNG path; read it to look
scripts/app-drive.sh stop                   # kill the instance, delete its data
```

**Always `stop` when you are done.** A leaked instance keeps a Vite server, a
cargo process and a tray icon alive. `stop --all` clears every session this
checkout started.

With exactly one live session the commands need no session argument. With more
than one, pass `-s dNNNNN` (or export `OPENRIZE_SESSION=dNNNNN`).

## Finding what to click

`tree` prints the window's accessibility tree, which is the real DOM as the OS
sees it. Labels in that tree are exactly what `click` and `fill` match on:

```
$ scripts/app-drive.sh tree
[Window] OpenRize
  [WebArea] OpenRize
    [Button] Home
    [Button] Trackers
    [Button] Settings
    [TextField] New tracker name = "End to end"
    [Button] New tracker
```

- The label comes from the element's accessible name, so it is the button's
  visible text or its `aria-label`, **not** an input's placeholder. The field
  above is `"New tracker name"` even though it renders the placeholder "What are
  you working on?".
- Text fields print their current contents after `=`, so you can check what you
  typed without taking a screenshot.
- `tree --full` includes the anonymous layout groups. You rarely want it.

Matching is exact-first: an exact label beats a case-insensitive match, which
beats a substring match. If a name is still ambiguous the command refuses and
lists the candidates; disambiguate with `-i N`, or narrow with `-r AXRadioButton`
/ `-r AXTextField`.

## Driving

| Command | What it does |
| --- | --- |
| `click "<label>"` | `AXPress` on the control, which is a genuine DOM click |
| `fill "<field>" "<text>"` | clicks the field to focus it, then types |
| `key <keyspec>` | a keystroke: `return`, `tab`, `escape`, `cmd+comma`, `shift+tab`, or any literal character |
| `wait "<text>" [seconds]` | polls the tree until the text shows up (default 15s) |
| `find "<label>"` | resolves a label without acting on it |

A worked example, all the way to backend state:

```bash
SID=$(scripts/app-drive.sh start | sed -n 's/^session=\([^ ]*\).*/\1/p')
scripts/app-drive.sh click -s "$SID" "Trackers"
scripts/app-drive.sh fill  -s "$SID" "New tracker name" "End to end"
scripts/app-drive.sh click -s "$SID" "New tracker"
scripts/app-drive.sh wait  -s "$SID" "Start tracking"
scripts/app-drive.sh click -s "$SID" "Start tracking"

# read the Rust side back out of this instance's own database
sqlite3 "$HOME/Library/Application Support/com.elgohr.openrize.drive${SID#d}/activity.db" \
  "select label, started_at is not null from timers;"
# -> End to end|1

scripts/app-drive.sh stop -s "$SID"
```

## Observing

Three ways to check a change landed, cheapest first:

1. `tree` (and `wait`) for structure, labels and field contents.
2. `shot` for anything visual: spacing, colour, alignment, a broken layout. It
   prints a PNG path; read that file. Capture is by window id, so it grabs your
   window correctly even when another agent's instance sits on top of it.
3. The instance's own SQLite file and config file for backend state. `start`
   prints both paths, and the app shows its data folder under Settings > Storage
   location.

`logs [n]` tails that instance's Vite and cargo output when something will not
start or a command fails unexpectedly.

## Isolation, and running several instances at once

Each session gets:

- **its own free port**, chosen at random and checked before use, so Vite dev
  servers never collide;
- **its own Tauri `identifier`** (`com.elgohr.openrize.driveNNNNN`), which is what
  scopes `app_data_dir`, so `activity.db` and its timers are private to the session;
- **its own `XDG_CONFIG_HOME`**, because `settings.rs::config_dir()` resolves to
  `~/.config/openrize` and is *not* identifier-scoped. Without this override an
  agent clicking through Settings would rewrite the human's real preferences;
- **its own process group**, so teardown kills the whole tree and nothing else;
- **a session namespace keyed to this checkout**, so an agent in another worktree
  cannot see or stop your session.

`stop` deletes the session's `Application Support` directory along with the
process, so instances do not accumulate.

Two concurrent sessions were verified end to end: separate ports listening,
separate databases (a tracker created in one is absent from the other), and the
real app's own data and `~/.config/openrize/settings.json` untouched throughout.

### What concurrency does not cover

- `click` is coordinate-free and needs no focus, so any number of agents can
  click their own instances simultaneously.
- `fill` and `key` send **global** keystrokes and therefore have to bring their
  window frontmost first. Two agents typing at the same moment can steal each
  other's keystrokes. Keep typing steps short, and prefer `click` where a click
  will do.
- Concurrent `start`s serialize behind cargo's build lock on the shared
  `src-tauri/target`. They queue rather than fail; the app processes then run in
  parallel.

## Gotchas

- **The webview can reload under you.** Vite HMR is live, so editing `src/` while
  a session runs may remount React and drop you back on Home. Do not assume the
  page you navigated to three commands ago is still showing; `wait` for a marker
  on the page you expect, or re-click your way there.
- **Do not write `AXValue` to set a field.** WebKit accepts the write, silently
  discards it, and React would never see an `onChange` anyway. That is why `fill`
  clicks and types instead.
- **Do not run `pnpm tauri dev` directly.** Fixed port, fixed identifier, real
  user data. Use this script or `pnpm dev:screenshot`.
- `scripts/ax-drive.js` is the JXA accessibility driver behind the script. It
  walks `AXUIElement` references directly because System Events' `entire contents`
  stops at the webview boundary and reports an empty group.
