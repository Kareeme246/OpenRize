---
name: drive-app-windows
description: Windows only (on macOS use the drive-app skill). Launch an isolated OpenRize instance on Windows, click through the real Tauri WebView2 UI over the Chrome DevTools Protocol, read the result back, take a webview screenshot, and tear it down. Use when verifying a UI or backend change by actually using the app, not just by reading code or running pnpm verify. Several agents can run their own instances at the same time.
---

# Driving the real OpenRize app on Windows

This skill is how you use the app: launch your own isolated instance, click real
controls, type into real fields, and read the result back out of the running app.

Everything goes through one dependency-free Node script:

```
node scripts/app-drive-windows.mjs <command> [args]
```

`pnpm dev:drive:win <command>` is an alias for it. Prefer the direct path in
scripts: `pnpm run` prints a banner line that pollutes anything you capture with
`$(...)`. `pnpm dev:screenshot:win` is the alias for the one-shot screenshot.

Windows only. Do not reach for Playwright, `tauri-driver` or WebDriver, and do
not write E2E suites for the app window; this is the driver. (macOS has its own
tooling in the `drive-app` skill.)

## How it works

WebView2 is Chromium, and Chromium exposes the Chrome DevTools Protocol on a
local port when launched with `--remote-debugging-port`. `start` sets that port
through the `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` environment variable, then
every command attaches to the instance's webview over that port:

- `tree`/`find`/`click`/`fill` use the CDP Accessibility tree, the same accessible
  names a screen reader sees;
- clicks and keystrokes are real `Input.dispatchMouseEvent` / `dispatchKeyEvent`
  events, so React sees genuine user input;
- `shot` is `Page.captureScreenshot`: it captures **the webview only**, never the
  desktop, so it cannot pick up the user's other windows.

CDP only reaches the **webview**. The tray icon, native menus and the OS window
frame are not reachable.

## Prerequisites

- Windows 11 with Node 22+ (global `fetch`/`WebSocket`), pnpm, and the Rust
  toolchain. In Git Bash put cargo on PATH first: `export PATH="$HOME/.cargo/bin:$PATH"`
  (the script also prepends it for the child process).
- Dependencies installed (`pnpm install`).
- **The first `start` in a checkout compiles the whole Rust tree and takes
  minutes** (`start` waits up to 900s; `OPENRIZE_DRIVE_TIMEOUT` overrides). Run it
  in the background if your tooling has a command timeout, then use `list`/`logs`
  and `tree` once it prints its `session=` line. A warm `src-tauri/target` starts
  in seconds.

## The loop

```bash
node scripts/app-drive-windows.mjs start                  # prints: session=dNNNNN port=... cdp=... pid=...
node scripts/app-drive-windows.mjs tree                   # what is on screen right now
node scripts/app-drive-windows.mjs click "Trackers"       # click something by its accessible name
node scripts/app-drive-windows.mjs wait "No trackers yet" # block until the UI catches up
node scripts/app-drive-windows.mjs shot                   # prints a PNG path; read it to look
node scripts/app-drive-windows.mjs stop                   # kill the instance, delete its data
```

Just need a look at a change and nothing else? One shot, no session to clean up:

```bash
node scripts/app-drive-windows.mjs screenshot             # start, capture, stop; prints the PNG path
```

**Always `stop` when you are done.** A leaked instance keeps a Vite server, a
cargo process, the app and its WebView2 processes alive. `stop --all` clears
every session this checkout started, including ones whose process already died.

With exactly one live session the commands need no session argument. If you run
several (see below), pass `-s dNNNNN` (or set `OPENRIZE_SESSION=dNNNNN`).

## Finding what to click

`tree` prints the page's accessibility tree. Names in that tree are exactly what
`click` and `fill` match on:

```
$ node scripts/app-drive-windows.mjs tree
[RootWebArea] OpenRize
  [Button] Home
  [Button] Trackers
  [Button] Settings
  [Textbox] New tracker name = ""
  [Button] New tracker (disabled)
```

- The name is the element's accessible name: its visible text or `aria-label`,
  **not** an input's placeholder.
- Text fields print their current contents after `=`, so you can check what you
  typed without a screenshot. Disabled/checked/selected state is shown in
  parentheses.
- `tree --full` includes static text and unnamed structure. You rarely want it;
  use `text` to read what is rendered.

Matching is exact-first: an exact name beats a case-insensitive match, which
beats a substring match, and real controls (buttons, links, fields...) are tried
before plain text. If a name is still ambiguous the command refuses and lists the
candidates; disambiguate with `-i N`, or narrow with `-r button` / `-r textbox`.

## Driving

| Command | What it does |
| --- | --- |
| `click "<name>"` | scrolls the control into view and clicks its centre with a real mouse event; fails if something covers it |
| `fill "<field>" "<text>"` | clicks the field, selects its contents, types the text over them (`--append` keeps the old text) |
| `key <keyspec>` | a keystroke: `return`, `tab`, `escape`, `ctrl+,` (or `ctrl+comma`), `shift+tab`, `f5`, or any literal character; modifiers are `ctrl`, `shift`, `alt`, `meta` |
| `text` | the visible text of the page (`document.body.innerText`) |
| `wait "<text>" [seconds]` | polls the page until the text (or an accessible name) shows up (default 15s) |
| `find "<name>"` | resolves a name without acting on it |
| `eval "<js>"` | evaluates an expression in the page, for debugging |
| `targets` | lists the instance's webview windows |

`click`, `fill` and `key` work through CDP, not OS focus, so any number of agents
can drive their own instances at once without stealing each other's keystrokes.

Several webviews exist in one app (the main window, the Pulse and reminder
windows). Commands default to the window labelled `main`; pick another with
`-t pulse` (matches the window label, title or URL).

A worked example, all the way to backend state:

```bash
SID=$(node scripts/app-drive-windows.mjs start | sed -n 's/^session=\([^ ]*\).*/\1/p')
node scripts/app-drive-windows.mjs click -s "$SID" "Settings"
node scripts/app-drive-windows.mjs wait  -s "$SID" "Settings"
node scripts/app-drive-windows.mjs key   -s "$SID" "ctrl+,"
node scripts/app-drive-windows.mjs text  -s "$SID"
node scripts/app-drive-windows.mjs shot  -s "$SID"      # -> ...\openrize-app-drive\...\shots\<ts>.png

# read the Rust side back out of this instance's own database, if sqlite3 is installed
sqlite3 "$APPDATA/com.offlinestudios.openrize.drive${SID#d}/activity.db" "select count(*) from timers;"

node scripts/app-drive-windows.mjs stop -s "$SID"
```

## Observing

Cheapest first:

1. `tree`, `text` and `wait` for structure, labels, rendered text and field contents.
2. `shot` for anything visual: spacing, colour, alignment, a broken layout. It
   prints a PNG path; read that file. It is the webview only.
3. The instance's own SQLite file and settings file for backend state. `start`
   prints `app-data=` and `config=`; the app shows its data folder under
   Settings > Storage location.
4. `console [seconds]` for the page's console output and uncaught exceptions
   (messages logged before you connected are replayed), and `logs [n]` for the
   Vite and cargo output when something will not start or a command fails.

## Isolation, and running several agents at once

Each session gets:

- **its own free port**, chosen at random and checked before use, for Vite, and
  another for the WebView2 debugging port (bound to 127.0.0.1 only);
- **its own Tauri `identifier`** (`com.offlinestudios.openrize.driveNNNNN`), which
  scopes `%APPDATA%\<identifier>` (databases), `%LOCALAPPDATA%\<identifier>`
  (logs and the WebView2 profile) and the single-instance lock;
- **its own `XDG_CONFIG_HOME`** in the session temp dir, because the settings
  directory is not identifier-scoped. Without this an agent clicking through
  Settings would rewrite the human's real preferences;
- **its own process tree**, so teardown kills that tree and nothing else;
- **a session namespace keyed to this checkout** (under `%TEMP%\openrize-app-drive`),
  so an agent in another worktree cannot see or stop your session.

`stop` runs `taskkill /T /F` on the tree, sweeps any orphaned process whose
command line carries the session's identifier or ports, and deletes the
session's `%APPDATA%`, `%LOCALAPPDATA%` and temp directories. It refuses to
delete any directory whose name is not `com.offlinestudios.openrize.drive<digits>`.

Concurrency: agents in **different worktrees** run side by side (separate ports,
identifiers, `target` dirs and session namespaces). Within **one checkout** only
one session can be live at a time on Windows: a running `.exe` cannot be
overwritten and each session's identifier is compiled into the binary, so a
second `start` would fail to relink. `start` refuses with a message instead; `stop`
the first (or use another worktree).

**Never touch the real identifier `com.offlinestudios.openrize`**: not its
`%APPDATA%`/`%LOCALAPPDATA%` folders, not the installed app in
`%LOCALAPPDATA%\openrize`, not the user's settings file, not their running
OpenRize. Do not launch the app any other way for testing.

## Gotchas

- **Do not toggle "Launch at login" (or anything else that registers the app with
  Windows) in a driven instance.** It writes the per-user `Run` registry value
  `OpenRize`, which is shared by every build and would overwrite or remove the
  real app's entry. That setting is outside the isolation above.
- **Rust changes need `stop` then `start`.** The instance runs `tauri dev
  --no-watch`: the frontend hot-reloads, but a Rust change only shows up in a
  fresh instance. Each worktree builds into its own `src-tauri/target`, so its first `start` pays the full compile.
- **The webview can reload under you.** Vite HMR is live, so editing `src/` while
  a session runs may remount React and drop you back on Home. `wait` for a marker
  on the page you expect.
- **Do not run `pnpm tauri dev` directly.** Fixed port, fixed identifier, real
  user data. Use this script.
- A new app window opens on the real desktop while a session is live. Leave it
  alone; it is closed by `stop`.
- `fill` selects the field's text and types over it. Setting `.value` from `eval`
  would never reach React's `onChange`, which is why the script types instead.
