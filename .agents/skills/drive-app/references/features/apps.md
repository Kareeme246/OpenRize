---
id: apps
title: Apps
summary: See which apps and websites were used on a day, set a default category and project for each, and exclude sensitive ones from tracking.
surfaces: [window, background]
routes: [apps]
code: [src/pages/Apps.tsx, src/pages/AppsTimeline.tsx, src/lib/activity.ts, src/lib/palette.ts, src/pages/calendar/timeline.ts, src/components/Charts.tsx, src-tauri/core/src/activity.rs]
rize: []
reach-macos: partial
reach-windows: partial
reach-rize: none
verified: never
---

# Apps

Apps is the page titled "Apps & Websites". It lists every application and website the tracker has ever seen and lets you do three things per item: give it a default category, give it a default project, and exclude it from tracking altogether (an excluded app is dropped from the activity the moment it is switched on). It has two tabs: a Timeline of one day's activity with a draggable playhead, a donut of time per app and a table of that day's apps with their defaults, and a Table of every app ever recorded with search and filters. The page ignores the route's scale, date, tab and appId fields; it always opens on the Timeline tab and today.

## Sub-features

- timeline-day: a strip of the day's activity coloured by app, an hour axis, and a "Activity playhead" slider that reports the time and the apps active at it.
- day-navigation: Previous day, Next day and Today for the Timeline (no week or month).
- day-apps-table: "Total tracked time" donut and an "Apps & Websites" table of the day's share, duration and per-app "Default category for ..." and "Default project for ..." pickers.
- all-apps-table: the Table tab lists every app and website with its identifier, Default Category, Default Project and a Tracked or Excluded toggle.
- search-and-filters: a search box, All Types / Apps / Websites, and All Status / Tracked / Excluded.
- exclusion: the Tracked/Excluded toggle excludes an app; its pickers are then disabled.
- counters: "Total", "Tracking" and (when any are excluded) "Excluded" counts in the header.
- empty-states: "No activity tracked this day", "No applications recorded yet.", "No applications matching query" and "Loading applications...".

## How to get to it

- Sidebar: Track > Apps.
- The route carries scale, date, tab and appId in its type, but nothing reads them; there is no deep link to a particular day or app.

## Driving it

Apps are created by the capture engine when it sees a frontmost app or site, and activity only exists for days the tracker ran. A fresh driver session has neither: it starts with onboarding and no recorded apps ([onboarding](onboarding.md)), and `rize` cannot create apps (its apps commands are not shipped). Two ways to get rows:

1. Let the driven instance capture real activity ([capture](capture.md)): grant Accessibility to the dev build as the onboarding feature describes, then switch between a couple of apps for a minute.
2. For the Table tab only, insert an app row directly into the session's database (the path `start` prints, in the macOS Application Support folder under the session's identifier). This is a test-only seed and bypasses the app, so reload the page afterwards by leaving and re-entering it:

```bash
scripts/app-drive.sh start
sqlite3 "$HOME/Library/Application Support/com.offlinestudios.openrize.drive51234/activity.db" "INSERT INTO apps (id, kind, identifier, display_name, first_seen, last_seen, created_at, updated_at) VALUES ('seed-notes', 'app', 'com.apple.Notes', 'Notes', 1, 1, 1, 1);"
scripts/app-drive.sh rize -- projects add "Client work"
```

Replace `51234` with the digits of the session id that `start` printed (the id `d51234` gives `51234`).

Open the page and use the Timeline:

```bash
scripts/app-drive.sh click "Apps"
scripts/app-drive.sh wait "Apps & Websites"
scripts/app-drive.sh click "Previous day"
scripts/app-drive.sh click "Jump to today"
scripts/app-drive.sh wait "Total tracked time"
scripts/app-drive.sh tree
```

Move the playhead from the keyboard (Left and Right step five minutes; Home and End jump to the ends of the day's range) after focusing it, then read its value from `tree`:

```bash
scripts/app-drive.sh click "Activity playhead"
scripts/app-drive.sh key left
scripts/app-drive.sh key home
scripts/app-drive.sh tree
```

Set a default on a day's app, then switch to the Table tab:

```bash
scripts/app-drive.sh click "Default category for Notes"
scripts/app-drive.sh click "Writing"
scripts/app-drive.sh click "Table"
scripts/app-drive.sh wait "Application / Website"
```

Table filters, per-app defaults and the exclusion toggle:

```bash
scripts/app-drive.sh fill "Search apps" "Notes"
scripts/app-drive.sh click "Websites"
scripts/app-drive.sh click "All Types"
scripts/app-drive.sh click "Excluded"
scripts/app-drive.sh click "All Status"
scripts/app-drive.sh click "Default category"
scripts/app-drive.sh click "Coding"
scripts/app-drive.sh click -i 3 "Tracked"
scripts/app-drive.sh tree
```

Confirm the stored result and stop:

```bash
sqlite3 "$HOME/Library/Application Support/com.offlinestudios.openrize.drive51234/activity.db" "SELECT display_name, default_category_id IS NOT NULL, excluded FROM apps;"
scripts/app-drive.sh stop
```

## What proves it works

- Page up: `wait "Apps & Websites"`, then `tree` shows the tabs "Timeline" and "Table" and the header counters "Total", "Tracking".
- Timeline with data: `wait "Total tracked time"`, an "Activity playhead" slider whose value text reads like a time and the active apps, and one row per app in the day table. Without recorded activity the correct result is `wait "No activity tracked this day"`.
- Default set: a toast such as `Default category for "Notes" set` (short-lived) and, durably, `SELECT default_category_id IS NOT NULL FROM apps` returning 1 for that app.
- Exclusion: the toggle reads "Excluded", the row's two pickers are disabled in `tree`, the header gains an "Excluded" counter with the value 1 (two text nodes in `tree`), and `SELECT excluded FROM apps` returns 1.
- Filters: after "Excluded" in the status group only excluded rows remain; "Websites" hides rows of kind app.
- Visual (strip colours, donut): `scripts/app-drive.sh shot`.

## Gotchas

- `rize` has no apps command (the apps list and edit commands in the rize reference are still planned), so there is no rize read-back; use the session database as the evidence and say so in the result.
- "Tracked" and "Excluded" are both filter buttons and per-row toggles, and "Apps" is also the sidebar item and the filter button. Run `tree` and use `-i N`, or `-r` to narrow; the filter buttons come before the rows.
- The Table tab's search box is named "Search apps" (its placeholder is "Search by app name or bundle ID..."); fill it with `scripts/app-drive.sh fill "Search apps" "Notes"`. It exists only on the Table tab.
- Pickers are comboboxes labelled "Default category", "Default project" (Table) and "Default category for NAME", "Default project for NAME" (Timeline); the current value is not part of the name. The Default project picker is replaced by the text "No projects yet" until a project exists.
- A picker offers "(None)" first; choosing it clears the default.
- The Timeline's "Defaults unavailable" text shows for a day app that has no matching app record, typically after history was purged.
- Excluding takes effect for the very next captured sample; samples already recorded stay.
- The Timeline's day always begins on today, and its stepping is by calendar day (midnight to midnight), unlike the Calendar's 5 AM boundary.

## Not reachable

- Real capture of apps and sites without Accessibility permission, and the OS permission prompt itself ([capture](capture.md)).
- Dragging the playhead with the pointer (the keyboard path above covers the same value).
- Hover tooltips such as the full identifier on a truncated Identifier cell.
- App icons (the page shows an initial letter, so nothing to verify).
