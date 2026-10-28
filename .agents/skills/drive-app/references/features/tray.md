---
id: tray
title: Menu-bar tray
summary: The OpenRize icon in the macOS menu bar (Windows notification area) whose glyph shows whether a stopwatch runs, whose left click opens Pulse and whose right-click menu pauses stopwatches, opens the app or quits it.
surfaces: [tray]
routes: []
code: [src-tauri/src/tray.rs]
rize: ["rize status", "rize timers list", "rize app quit"]
reach-macos: none
reach-windows: none
reach-rize: partial
verified: never
---

# Menu-bar tray

The tray is a native icon with no text. Its glyph has two forms: filled while any stopwatch is running, hollow otherwise (a template image on macOS, painted white on a dark Windows taskbar). Its hover tooltip says how many stopwatches run or are paused. A left click opens the Pulse panel ([pulse.md](pulse.md)). A right click builds a native menu with one "Pause <stopwatch name>" item per running stopwatch (or a disabled "Nothing running"), "Relaunch to finish setup" when an Accessibility grant needs a restart, "Open OpenRize" and "Quit OpenRize". It can be turned off in Settings, which removes the icon and destroys the Pulse panel, and then closing the window always quits. All of it is native UI: no driver here can see or click it. What can be checked is the state the tray reflects and the setting that mounts it.

## Sub-features

- glyph: filled when any stopwatch is running, hollow when none is.
- tooltip: "no trackers", "nothing running, N paused", "N running" or "N running, M paused", prefixed with OpenRize.
- left-click: toggles the Pulse panel.
- right-click-menu: "Pause <name>" per running stopwatch, "Nothing running", "Relaunch to finish setup", "Open OpenRize", "Quit OpenRize". Labels longer than 32 characters are cut with an ellipsis.
- tray-setting: Settings > General switch "Menu bar icon" ("Tray icon" on Windows) mounts or removes the icon; the segmented control "When I close the window" depends on it.

## How to get to it

The icon sits at the right of the macOS menu bar or in the Windows notification area (possibly inside the ^ overflow flyout). It exists whenever the setting is on, including for a background launch with no window (`rize app start`). The only in-app control is Settings > General > "Menu bar icon".

## Driving it

The icon and menu cannot be driven. Drive and read what they stand for.

Toggle the icon through the window (this changes only the session's own settings folder), and read the setting back:

```bash
scripts/app-drive.sh start
scripts/app-drive.sh key cmd+comma
scripts/app-drive.sh click "General"
scripts/app-drive.sh wait "App behavior"
scripts/app-drive.sh click "Menu bar icon"
scripts/app-drive.sh rize -- settings get tray-enabled
scripts/app-drive.sh click "Menu bar icon"
scripts/app-drive.sh rize -- settings get tray-enabled
```

The same through the backend only:

```bash
scripts/app-drive.sh rize -- settings set tray-enabled false
scripts/app-drive.sh rize -- settings get tray-enabled
scripts/app-drive.sh rize -- settings set tray-enabled true
```

Change the state the glyph and tooltip follow, then read it (the tray repaints on every stopwatch change, which `rize` triggers through the app):

```bash
scripts/app-drive.sh rize -- timers new "Client work"
scripts/app-drive.sh rize -- timers start "Client work"
scripts/app-drive.sh rize -- timers list
scripts/app-drive.sh rize -- --json status
scripts/app-drive.sh rize -- timers pause "Client work"
scripts/app-drive.sh rize -- timers list
```

`rize status` is the stand-in for looking at the menu bar: it prints "OpenRize <version> is running", the current activity and a "Timers:" line listing the running stopwatches, which is exactly the set the tray's "Pause <name>" items would list.

## What proves it works

- Setting: `scripts/app-drive.sh rize -- --json settings get tray-enabled` returns `data: false` after the switch is turned off in the window and `true` after it is turned back on, and the "Menu bar icon" switch in `tree` shows the matching checked state. Both are required for a PASS of the setting.
- Glyph and tooltip state: `scripts/app-drive.sh rize -- timers list` shows a row with STATE `running` if and only if the glyph should be filled; the tooltip's count is the number of `running` and `paused` rows.
- Menu content: the "Pause <name>" items are the running rows of `rize timers list` (the running timers in `data.timers` of `rize --json status`, those with a non-null `startedAt`).
- Clicking "Pause <name>" in the menu has the same effect as `scripts/app-drive.sh rize -- timers pause "Client work"`; verify that effect with `rize timers list`. The menu click itself cannot be observed.
- Never claim the glyph, tooltip text or menu rendering was seen. There is no PASS for those from the tools here.

## Gotchas

- A driven instance has a real tray icon in the human's menu bar, indistinguishable from the real app's. Do not click it from outside the harness, and expect to see a second icon while a session is live.
- `rize app quit` ends the session's own app process exactly as the menu's "Quit OpenRize" does, and the driven session then has nothing left to drive. Do not run it from a driven session unless you are done.
- Turning the icon off while the window is closed would leave no way to reopen the app except `rize app open`; in a driven run the window is open, so it is safe, but turn it back on before `stop`.
- The Pulse panel is destroyed when the icon is turned off and re-created on the next click.
- The tooltip text is built from fixed phrases ("no trackers", "nothing running, N paused", "N running"); the dash between OpenRize and the phrase is part of the native string and not worth matching on.
- On Windows the glyph is repainted white on a dark taskbar only the next time the icon changes state.
- A "Relaunch to finish setup" menu item appears only while the Accessibility state is "relaunch required" ([onboarding.md](onboarding.md)).

## Not reachable

- The icon, its glyph, tooltip and the native right-click menu on both platforms: they are native UI outside the webview, and the macOS driver reads only the webview window's accessibility tree and the Windows driver reads only the webview.
- The left-click that opens Pulse and the menu items "Open OpenRize" and "Quit OpenRize" (the equivalent `rize app open` and `rize app quit` exist, see [rize-cli.md](rize-cli.md), but they are not the menu).
- The menu's own placement, keyboard navigation and the Windows overflow flyout.
