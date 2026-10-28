---
id: pulse
title: Pulse
summary: The small panel under the menu-bar or tray icon that shows whether tracking is on, how today is going, the next break and running stopwatches, with a door into the main window.
surfaces: [pulse, tray]
routes: []
code: [src/Pulse.tsx, src-tauri/src/pulse.rs, src-tauri/src/pulse]
rize: ["rize status", "rize timers list"]
reach-macos: none
reach-windows: none
reach-rize: partial
verified: never
---

# Pulse

Pulse is a 320 pt wide panel that opens when you left-click the OpenRize icon in the menu bar (macOS) or system tray (Windows). It shows the live capture state with a pause or resume button, today's work against the daily target as a ring and a stacked category bar, the next break with "Pause 1h" and "Take a break now", up to three stopwatches that can be paused or resumed, and a footer with "N to review" and "Open OpenRize". It is a separate webview window (label `pulse`), created on the first icon click and hidden between uses. The panel only reads live state; anything historical or editable stays in the main window. The tray icon and its click are native, so no driver here can open the panel (see [tray.md](tray.md)).

## Sub-features

- capture-status: a dot and line "Tracking · <app>", "Tracking active", "Tracking paused", "Outside tracking hours" or "Idle <time> · not counting", with a detail line and a pause or resume icon button.
- today-summary: the ring labelled "<n>% of today's target", the work total, "of <target> target · <time> to go", a stacked category bar and up to four legend entries, or "Nothing tracked yet today".
- break-row: "Break" with the next break summary, "Pause 1h" or "Resume" for reminders, "Take a break now", and during a break the countdown with "End break".
- timer-rows: up to three stopwatches with a name button ("Show <name> in Stopwatches"), the elapsed time and a pause or resume icon button; "+N more" for the rest.
- review-door: "<n> to review →" or "All caught up ✓" or "Nothing to review".
- open-app: "Open OpenRize" with the shortcut shown beside it (Cmd+O on macOS, Ctrl+O elsewhere); Escape closes the panel.

## How to get to it

Left-click the OpenRize icon in the menu bar (macOS) or the notification area (Windows). Right-click opens the native tray menu instead (see [tray.md](tray.md)). The icon exists only while Settings > General "Menu bar icon" ("Tray icon" on Windows) is on. There is no keyboard shortcut and no menu item that opens the panel.

## Driving it

The panel cannot be opened by any driver here, because the only opener is the native icon click. What can be driven is everything the panel reads and writes, through the main window and `rize`, and the proof of what the panel would show comes from `rize`.

Read the same live state the panel shows:

```bash
scripts/app-drive.sh rize -- --json status
scripts/app-drive.sh rize -- timers list
```

Change what the panel would display and read it back (a dismissed first-run screen is required first, see [onboarding.md](onboarding.md)):

```bash
scripts/app-drive.sh rize -- timers new "Client work"
scripts/app-drive.sh rize -- timers start "Client work"
scripts/app-drive.sh rize -- timers list
scripts/app-drive.sh rize -- track stop
scripts/app-drive.sh rize -- --json status
scripts/app-drive.sh rize -- track start
scripts/app-drive.sh rize -- timers pause "Client work"
```

The main window mirrors the same state in the sidebar strip:

```bash
scripts/app-drive.sh wait "Tracking"
scripts/app-drive.sh tree
```

Trying the panel itself means a human clicking the icon; there is no `scripts/app-drive.sh` verb and no `rize` command for it. On Windows `node scripts/app-drive-windows.mjs targets` would list a `pulse` webview only after a human has clicked the tray icon once; `-t pulse` then reads it. Do not rely on that in an automated run.

## What proves it works

- Everything the panel renders comes from the same four sources as `rize --json status`: `data.activity` (`captureEnabled`, `trackingActive`, `inTrackingHours`, `idleMs`, `idleThresholdMs`, `current.app`, `trackedMs`), `data.pendingEntries` (the review count), `data.timers` (each with `startedAt`, null when paused) and `data.breaks` (`phase`, `next`, `pausedUntil`, `current`). A PASS for a Pulse-related backend change is the correct value in those fields after the action.
- The pause button in the panel calls the same command as the sidebar toggle and `rize track stop`; the evidence is `data.activity.captureEnabled` flipping to `false` and the main window's strip reading "Tracking paused".
- A timer started in the panel is `startedAt` non-null in `rize timers list` (STATE running) and shows in the Stopwatches page.
- The panel's own layout (spacing, ring, bar colours) has no tool here; it needs a human look. Never claim a visual PASS for it from `shot`, which captures the main window only.

## Gotchas

- `src/Pulse.tsx` renders its icon-only buttons without an accessible name: the pause or resume button and the timer pause or resume buttons carry only a tooltip ("Pause tracking", "Resume tracking", "Pause <name>", "Resume <name>") and their icons are `aria-hidden`. Even if a driver reached the window, there would be no stable label for them; the timer name button is the exception, `aria-label` "Show <name> in Stopwatches".
- The panel stays loaded between opens. When it regains focus it reloads the catalog and entries, so a stale panel catches up on the next open.
- It sizes itself: it measures its card and calls `resize_pulse_panel`; the first open waits for the first measurement. A bug that stops the measurement shows as a panel that never opens.
- The day it shows is the Calendar's day, which turns at 5 AM, not midnight.
- On macOS the panel is a non-main NSPanel so tiling window managers do not adopt it; on Windows it opens in the work-area corner. Neither placement can be observed here.
- A second left-click shortly after the panel hid on blur is treated as "close" (a 300 ms guard), so a quick click pair does not reopen it.

## Not reachable

- The panel itself: opening it needs a native tray icon click, which neither the macOS accessibility driver nor the Windows DevTools driver can make. Its window is therefore never in the accessibility tree they read (the macOS driver reads only the app's first window anyway).
- The tray icon, its glyph and the native right-click menu ([tray.md](tray.md)).
- Placement under the icon, the panel's blur-to-hide behaviour and the corner placement on Windows.
- Visual correctness of the panel (no screenshot of it can be taken by the shipped scripts).
