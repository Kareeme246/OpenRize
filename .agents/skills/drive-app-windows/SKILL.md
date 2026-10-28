---
name: drive-app-windows
description: Windows only (on macOS use the drive-app skill). Verify a change in the real OpenRize app on Windows - run doctor, launch an isolated instance, click through the real Tauri WebView2 UI over the Chrome DevTools Protocol, seed and read back data with the rize passthrough, take webview screenshots as evidence, and tear it down. Use when verifying a UI or backend change by actually using the app, not just by reading code or running pnpm verify. Several agents can run their own instances at the same time. The shared feature map is in the drive-app skill.
---

# Driving the real OpenRize app on Windows

This skill is how you use the app: launch your own isolated instance, click real
controls, type into real fields, and read the result back out of the running app. To
verify a change, follow `## Verifying a change` below: it points at the shared feature
map and maintenance procedure in the `drive-app` skill.

**Status of this file's newer commands.** `doctor`, `stop --dry-run` / `--json`,
`shot --out` and the `rize` passthrough were added together with their macOS
counterparts. They were syntax-checked on macOS only (`doctor` there reports
`fail windows`, as designed) and have never run on Windows: treat them as unverified on
Windows until a Windows maintenance pass drives them.

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
node scripts/app-drive-windows.mjs doctor                 # check this machine first; launches nothing
node scripts/app-drive-windows.mjs start                  # prints: session=dNNNNN port=... cdp=... pid=...
node scripts/app-drive-windows.mjs tree                   # what is on screen right now
node scripts/app-drive-windows.mjs click "Stopwatches"    # click something by its accessible name
node scripts/app-drive-windows.mjs wait "No stopwatches yet" # block until the UI catches up
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
`stop --dry-run` (add `--all` for every session) kills and deletes nothing and prints
what would happen; `stop --json` and `doctor --json` print one JSON object.

## Doctor

`node scripts/app-drive-windows.mjs doctor` launches nothing. It checks `windows`
(the script only runs on Windows), `node` (22 or newer, for the global `fetch` and
`WebSocket`), `node-modules` (`pnpm install` ran), `cargo` (on PATH or in
`%USERPROFILE%\.cargo\bin`), `warm-build` (warns when `openrize.exe` is missing: the
first `start` compiles the whole Rust tree) and `sessions` (warns about stale session
directories: run `stop --all`). `fail` blocks and exits 1; `warn` only limits you. Run it
before `start` and whenever a command fails unexpectedly. There is no permission check:
CDP needs no OS grant.

With exactly one live session the commands need no session argument. If you run
several (see below), pass `-s dNNNNN` (or set `OPENRIZE_SESSION=dNNNNN`).

## Finding what to click

`tree` prints the page's accessibility tree. Names in that tree are exactly what
`click` and `fill` match on:

```
$ node scripts/app-drive-windows.mjs tree
[RootWebArea] OpenRize
  [Button] Calendar
  [Button] Stopwatches
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

# read the Rust side back out through the rize passthrough (see below)
node scripts/app-drive-windows.mjs rize -s "$SID" -- --json status

node scripts/app-drive-windows.mjs stop -s "$SID"
```

## Observing

Cheapest first:

1. `tree`, `text` and `wait` for structure, labels, rendered text and field contents.
2. `shot` for anything visual: spacing, colour, alignment, a broken layout. It
   prints a PNG path; read that file. It is the webview only.
3. Backend state through `rize` (strongest for data claims), or the instance's own
   SQLite file and settings file. `start` prints `app-data=` and `config=`; the app shows
   its data folder under Settings > Storage location.
4. `console [seconds]` for the page's console output and uncaught exceptions
   (messages logged before you connected are replayed), and `logs [n]` for the
   Vite and cargo output when something will not start or a command fails.

## Seeding and reading data: the `rize` passthrough

A fresh instance is empty. `rize` runs the repo's own CLI against this session's data, so
you seed data and read backend state with the same tool:

```bash
node scripts/app-drive-windows.mjs rize -s "$SID" -- projects add "Client work"
node scripts/app-drive-windows.mjs rize -s "$SID" -- entries add --from 2026-10-28T09:00 --to 2026-10-28T10:00 -d "Write the report" --category Coding --project "Client work"
node scripts/app-drive-windows.mjs rize -s "$SID" -- --json entries list --from today
```

Everything after the first `--` goes to `rize` verbatim (`--json`, `--full`, `--yes` and
`--dry-run` reach it). The first call compiles the CLI crate (debug profile). The instance
holds its stores, so `rize` reaches it over the app's local endpoint (a named pipe) and the
change shows in the window at once. `RIZE_APP` points at a path that does not exist, so
`rize app start` or `rize track start` can never launch a second app. After switching
accounts inside the session it still targets the session's base data folder. Only Shipped
commands in `src-tauri/cli/COMMANDS.md` work.

## Screenshots as evidence

`shot` prints the PNG path (webview only). `shot --out PATH` writes it at `PATH` (must end
in `.png`; parent folders are created), and `PATH` survives `stop`. A shot taken without
`--out` lands in the session folder; `stop` moves it to
`<temp dir>\openrize-app-drive-evidence\<session id>\` and prints where, then deletes
the session.

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

## Verifying a change

Verification means closing the loop yourself: use the changed feature in the real app and
show evidence. Reading code, `pnpm verify` and unit tests are not proof. The same five
phases as on macOS apply, and the feature map, evidence rules and maintenance procedure are
shared:

- Feature map: [the feature map](../drive-app/references/features/README.md).
- Maintenance: [maintenance](../drive-app/references/maintenance.md).

1. **Launch.** `doctor`, then `start`. Windows allows one live session per checkout.
2. **Doctor.** `fail` blocks, `warn` limits. A fresh session is an empty install; Windows
   never shows the first-run Permissions screen, so it opens on the Calendar.
3. **Drive.** Find the feature in the map, open only the files your change touches and
   follow their "Driving it". Drive the success path and the cancel, empty, error and
   persistence paths that apply (`stop` deletes the data, so persistence means navigating
   away and back, or reading through `rize`).
4. **Prove.** Strongest first: backend state read back through `rize --json`; then `tree`,
   `text` and `wait`; then `shot --out PATH` screenshots you actually looked at. "Tests pass"
   is not proof. A claim with no artifact is inconclusive and is reported as inconclusive.
   Report the exact commands and outputs.
5. **Clean up.** `stop` (or `stop --all`; `stop --dry-run` first when unsure).

The feature files are written for the macOS driver. Translate their commands:

| In the feature file | On Windows |
| --- | --- |
| `scripts/app-drive.sh` | `node scripts/app-drive-windows.mjs` |
| `cmd+comma`, `cmd+...` | `ctrl+comma`, `ctrl+...` |
| `-r AXRadioButton`, `-r AXTextField` | `-r radio`, `-r textbox` (CDP role names) |
| a Pulse or reminder window | add `-t pulse` (or the window label, title or URL) to pick it |
| reading the page text | `text` prints `document.body.innerText` (macOS has no such verb) |
| `shot --out /tmp/proof/x.png` | `shot --out C:\temp\proof\x.png` |

Each file's `reach-windows` says how much of it this driver can exercise. The tray, native
menus, OS notifications and the window frame are unreachable here, and the Pulse window
opens only from a native tray click. Report anything you could not exercise as exactly
that, never silently.

## Gotchas

- **Do not toggle "Launch at login" (or anything else that registers the app with
  Windows) in a driven instance.** It writes the per-user `Run` registry value
  `OpenRize`, which is shared by every build and would overwrite or remove the
  real app's entry. That setting is outside the isolation above.
- **Rust changes need `stop` then `start`.** The instance runs `tauri dev
  --no-watch`: the frontend hot-reloads, but a Rust change only shows up in a
  fresh instance. Each worktree builds into its own `src-tauri/target`, so its first `start` pays the full compile.
- **The webview can reload under you.** Vite HMR is live, so editing `src/` while
  a session runs may remount React and drop you back on the Calendar. `wait` for a marker
  on the page you expect.
- **Do not run `pnpm tauri dev` directly.** Fixed port, fixed identifier, real
  user data. Use this script.
- A new app window opens on the real desktop while a session is live. Leave it
  alone; it is closed by `stop`.
- `fill` selects the field's text and types over it. Setting `.value` from `eval`
  would never reach React's `onChange`, which is why the script types instead.
- **Never send a lone Alt key to the window** (for example to win
  `SetForegroundWindow` for a native screenshot). It puts the window in Windows'
  keyboard menu mode, and the webview's debugging port stops answering until an
  Escape reaches the window. The page itself keeps running, so users are not
  affected; only the driver is. If every command says the port "is not
  answering", send Escape.
