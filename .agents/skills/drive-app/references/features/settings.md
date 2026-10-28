---
id: settings
title: Settings
summary: Change how OpenRize looks and behaves - theme, shape, size and accent, launch at login, menu bar icon, close behavior, tracking hours, weekly target, storage location and retention - and have every change saved at once.
surfaces: [window, cli]
routes: [settings]
code: [src/pages/Settings.tsx, src/pages/settings/SettingParts.tsx, src/hooks/useSettings.tsx, src/lib/settings.ts, src/lib/platform.ts, src/components/SegmentedControl.tsx, src-tauri/src/login_item.rs, src-tauri/src/login_item, src-tauri/core/src/settings.rs]
rize: ["rize settings list", "rize settings get", "rize settings set", "rize paths"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Settings

One page of preferences with a left navigation (Overview, Appearance, General, Tracking & work, Suggestions, Data & storage, Notifications, Advanced settings with AI and Battery & Energy, and Developer in a dev build). Preferences save automatically and repaint at once; the file is `settings.json` in the instance's config folder, and the same values are readable and writable with `rize settings`. This file covers the preference sections themselves: Appearance (theme, shape, size mode, accent colour), General (Launch at login, menu bar or tray icon, close behavior), Tracking & work (tracking hours, expected hours a week), Suggestions (what to suggest, auto-accept) and Data & storage (storage location, keep activity until). Sections owned by other features appear on the same page: Overview holds permissions and updates ([updates.md](updates.md), [onboarding.md](onboarding.md)), Data & storage holds the accounts list ([accounts.md](accounts.md)), Notifications holds break and stopwatch reminders ([breaks.md](breaks.md)), AI holds engine and learning controls ([ai.md](ai.md)), Battery & Energy is [energy.md](energy.md) and Developer is [dev-tools.md](dev-tools.md).

## Sub-features

- appearance: Theme (System, Light, Dark), Shape (Rounded, Sharper), Size mode (Compact, Normal, Relaxed, Very Relaxed) and Accent colour (Green, Royal blue, Purple, Orange), applied to the document immediately.
- launch-at-login: the "Launch at login" switch, which mirrors the operating system's login item rather than a stored flag and is re-read when the window regains focus.
- tray-and-close: the "Menu bar icon" switch ("Tray icon" on Windows) and "When I close the window" (Hide to menu bar, or Quit).
- tracking-hours: "Schedule tracking hours", a daily window or per-day schedule with "Customize per day".
- work-hours: "Expected hours" per week (10 to 60 in steps of 5) and the link to Categories.
- suggestions: "What to suggest" (Category, or Category + Project) and the auto-accept switch and threshold (see [ai.md](ai.md)).
- storage: "Storage location" with the data folder, database and preferences paths and "Open folder in Finder", plus "Keep activity until" (Forever, 7 days, 30 days).
- section-navigation: the left nav buttons and the route's `section` value that scrolls to a section.

## How to get to it

Sidebar bottom "Settings", or the keyboard shortcut Cmd+, on macOS (Ctrl+, on Windows). The sidebar's "Update available" button and the break reminder's "Reminder settings" open it on the Updates and Notifications sections. Inside the page, click a nav button to scroll to its section.

## Driving it

A fresh session is a brand-new install and may open on first-run onboarding (see [onboarding.md](onboarding.md)). Settings need no seed data.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh key cmd+comma
scripts/app-drive.sh wait "Preferences save automatically"
scripts/app-drive.sh click "Appearance"
scripts/app-drive.sh click "Light"
scripts/app-drive.sh click "Sharper"
scripts/app-drive.sh click "Relaxed"
scripts/app-drive.sh click "Royal blue"
scripts/app-drive.sh shot --out /tmp/openrize-proof/settings-appearance.png
```

On Windows use the same verbs through `node scripts/app-drive-windows.mjs` and `ctrl+,` instead of `cmd+comma`. Choices are radios, matched by their label text; labels that also occur elsewhere resolve exact-first. General, tracking and storage:

```bash
scripts/app-drive.sh click "General"
scripts/app-drive.sh click "Hide to menu bar"
scripts/app-drive.sh click "Quit"
scripts/app-drive.sh click "Hide to menu bar"
scripts/app-drive.sh click "Menu bar icon"
scripts/app-drive.sh click "Menu bar icon"
scripts/app-drive.sh click "Tracking & work"
scripts/app-drive.sh click "Schedule tracking hours"
scripts/app-drive.sh click "Customize per day"
scripts/app-drive.sh click "Collapse to single window"
scripts/app-drive.sh click "Expected hours per week"
scripts/app-drive.sh click "Data & storage"
scripts/app-drive.sh click "Keep activity until"
scripts/app-drive.sh tree
```

After clicking a picker such as "Expected hours per week" or "Keep activity until", `tree` lists its options ("35h a week", "40h a week", "Forever", "7 days", "30 days"); click one by name. On the macOS tree the switches print their state, so `tree` before and after a click shows the flip. The "Launch at login" switch must only be read, never clicked (see Gotchas):

```bash
scripts/app-drive.sh click "General"
scripts/app-drive.sh tree
```

Read and write the same preferences through the app's own endpoint, and confirm the window followed:

```bash
scripts/app-drive.sh rize -- --json settings list
scripts/app-drive.sh rize -- --json settings get theme
scripts/app-drive.sh rize -- --json settings get accent
scripts/app-drive.sh rize -- settings set accent purple
scripts/app-drive.sh rize -- settings set size-mode compact
scripts/app-drive.sh rize -- settings set weekly-target-hours 35
scripts/app-drive.sh rize -- settings set tracking-hours.enabled false
scripts/app-drive.sh rize -- --json paths
scripts/app-drive.sh wait "35h a week"
```

## What proves it works

- Page rendered: `wait "Preferences save automatically"` (the page subtitle). The heading "Settings" is also the sidebar button's name.
- A setting changed in the window is PASS only when `rize --json settings get KEY` returns the new value: for example after `click "Light"`, `data` is `"light"`; after `click "Royal blue"`, `settings get accent` is `"blue"`; after `click "Relaxed"`, `settings get size-mode` is `"relaxed"`; after `click "Quit"`, `settings get close-behavior` is `"quit"`; after the "Menu bar icon" click, `settings get tray-enabled` is `false`. The radio's checked state in `tree` must agree.
- Appearance is also visual: the document gets `data-theme`, `data-shape`, `data-size-mode` and the `--accent` colours, which no tool reads directly. Use `shot` to see the repaint (light surfaces, sharper corners, larger text, the new accent on the nav's active item) and compare with the "before" shot.
- A setting changed with `rize settings set` should repaint the open window (the app applies the change live); PASS needs the changed radio or picker value in `tree`/`shot` as well as the `settings get` read-back. The Windows and macOS windows are expected to follow within a moment; if they do not, that is a finding.
- Expected hours: `settings get weekly-target-hours` returns `35`; the Calendar's and My Timesheet's daily target is a fifth of it (see [calendar.md](calendar.md)).
- Storage: the Storage location block lists the data folder, "Database" and "Preferences" paths; they must equal `data.dataDir`, `data.databaseFile` and `data.configFile` in `rize --json paths`, and the config path must be under the session's config folder, not under the real `~/.config/openrize`.
- Tracking hours: `settings get tracking-hours.enabled` and the note "Currently within tracking window" or "Currently outside tracking window" in `tree`.
- Retention: `settings get retention-days` returns 0, 7 or 30.

## Gotchas

- Do not toggle "Launch at login" in a driven instance on Windows. It writes the per-user `Run` registry value `OpenRize`, which every build shares, so a driven dev instance would overwrite or remove the real app's entry; it is outside the session's isolation. The row is live on Windows (it is not greyed out), so the only safe way to verify it is to read it. Do the same on macOS as a precaution: it asks the OS to register the running app as a login item, which is not session-scoped; whether a dev binary registers is unverified, so only read the row. The row's state is the OS's, not `settings.json`, so `rize settings` has no key for it.
- Launch-at-login states come from the OS: "Start OpenRize in the background when you log in" (normal), "Allow OpenRize under Login Items in System Settings to finish" (with an "Open Login Items" button, native), "Needs macOS 13 or later" or "Not available on Linux yet".
- Labels that repeat: "Off"/"On"/"Real" are Developer tool radios, not here. "Normal" and "Relaxed" are exact matches ahead of "Very Relaxed" by the matching rules. "Hide to menu bar" is "Hide to system tray" on Windows, and "Menu bar icon" is "Tray icon" there. The nav button "Appearance" and the section both exist, but only the button is actionable.
- "Advanced settings" is a disclosure button; its sub-buttons "AI" and "Battery & Energy" only appear after it is expanded ("Battery & Energy" is absent on Windows). "Developer" appears only in a macOS dev build.
- "Customize per day" and "Collapse to single window" only appear when "Schedule tracking hours" is on; day rows are checkboxes named by weekday ("Monday") and time fields named like "Monday tracking start time". `fill` into a time field types text; use it only if the field accepts it, and read back with `settings get tracking-hours.monday.start`.
- The "Daily window" Start and End time fields are labelled "Start" and "End".
- With the menu bar icon off, "When I close the window" is forced to quit and its description says so.
- Settings changed with `rize settings set` while the app is closed apply at next launch; with the driver the app is running, so they apply live.
- Scroll position: clicking a nav button scrolls the content; the active nav item follows scrolling, so `tree` may show a different `aria-current` than the one you clicked after a slow scroll. `wait` for a marker from the section.

## Not reachable

- The OS login-item registration itself: Launch at login can be observed but must not be toggled in a driven instance, and the OS side (System Settings > Login Items, Task Manager > Startup apps, the `Run` key) is not reachable by either driver.
- "Open folder in Finder" (or File Explorer) hands a path to the OS; the opened window is native and not reachable. "Open Login Items" and "Apple intelligence settings" likewise open System Settings.
- The window frame's caption colour on Windows follows the theme (native); only the webview is reachable.
