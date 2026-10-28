---
id: timesheets
title: Timesheets
summary: Read a day or week of tracked hours as a project-by-day or category-by-day grid, narrow it by client, project and category, and approve entries in place.
surfaces: [window, cli]
routes: [timesheets]
code: [src/pages/Timesheets.tsx, src/pages/timesheets, src/lib/timesheetGrid.ts, src/lib/settings.ts, src/lib/dates.ts]
rize: ["rize entries list", "rize entries add", "rize entries approve", "rize report"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Timesheets

Timesheets is the weekly hours grid in the Analyze group. Rows are projects (or categories), columns are the days of the chosen week, and each cell is that row's hours for the day, shaded and counted against the expected hours from Settings. A Day scale shows one column with a summary card above it; rows are grouped under their client whenever a filter is on or the scale is Day. Part of each total can still be waiting on review, which shows as a "+hours" marker and a Review badge. This page reads entries; to fix or bulk-edit them use [my-timesheet](my-timesheet.md).

## Sub-features

- scale: a Day or Week control (opens on Week), with Previous/Next step and Today.
- filters: a "Filters" button that opens Clients, Projects and Categories checklists at once, one chip per dimension to open just that list, and a "Clear" button; "No client", "No project" and "Uncategorized" are options.
- rows-menu: the "Rows" header menu switches the row dimension between Project and Category.
- grid: a row per project or category with a progress line "Share of expected hours" and an "N of Mh" label, day cells in decimal hours, a Total (hours) column and a footer total row; weekends are hatched and today is tinted.
- expand-row: a row expands to the other dimension per day in Week, or to its individual entries in Day.
- open-day: in Week, a day cell opens that day in Day scale.
- review-badge: a "+hours Review N" badge on a row total, and a "Review" or "Accept" action on each entry in an expanded Day row.
- day-summary: in Day, a card (labelled "Day summary") with Pending review, Approved and Time entry hours, a "Review progress" line and an "Approved entries" progress bar.
- empty-states: "No time tracked this week" or "No time tracked this day", and "Nothing matches these filters" with a "Clear filters" button.

## How to get to it

- Sidebar: Analyze > Timesheets.
- The Review badge and an entry's "Review" link navigate away to My Timesheet's To review tab ([my-timesheet](my-timesheet.md)); an entry's "Accept" approves it here.

## Driving it

Start a session, get past first-run onboarding if it shows ([onboarding](onboarding.md)), and seed entries on two days under one client's project. Use full date-times, and replace `2026-10-28` and `2026-10-27` with today and yesterday.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh rize -- clients add "Acme"
scripts/app-drive.sh rize -- projects add "Client work" --client Acme
scripts/app-drive.sh rize -- entries add --from 2026-10-28T09:00 --to 2026-10-28T11:00 -d "Write the report" --category Coding --project "Client work"
scripts/app-drive.sh rize -- entries add --from 2026-10-27T13:00 --to 2026-10-27T14:30 -d "Design review" --category Design --project "Client work"
scripts/app-drive.sh rize -- entries add --from 2026-10-28T12:00 --to 2026-10-28T12:30 -d "Triage the inbox" --category Communication
scripts/app-drive.sh rize -- --json entries list --from today
scripts/app-drive.sh rize -- entries unapprove 7f3a9c21
```

`7f3a9c21` stands for the last eight characters of the "Write the report" id from the list; unapproving it leaves part of that cell waiting on review. Open the grid and navigate:

```bash
scripts/app-drive.sh click "Timesheets"
scripts/app-drive.sh wait "Total (hours)"
scripts/app-drive.sh tree
scripts/app-drive.sh click "Previous week"
scripts/app-drive.sh click "Jump to today"
scripts/app-drive.sh click -r AXRadioButton "Day"
scripts/app-drive.sh wait "Day summary"
scripts/app-drive.sh click -r AXRadioButton "Week"
```

Rows menu and expanding rows:

```bash
scripts/app-drive.sh click "Rows"
scripts/app-drive.sh click "Category"
scripts/app-drive.sh click "Rows"
scripts/app-drive.sh click "Project"
scripts/app-drive.sh click "Client work, 3.5 of"
scripts/app-drive.sh click "Client work, Wednesday, October 28"
```

In Week the last click opens that day in Day scale. In Day, expanding the row lists its entries with Accept or Review:

```bash
scripts/app-drive.sh click "Client work, 2 of"
scripts/app-drive.sh click "Accept"
```

Filters:

```bash
scripts/app-drive.sh click "Filters"
scripts/app-drive.sh click "Acme"
scripts/app-drive.sh key escape
scripts/app-drive.sh wait "Clear"
scripts/app-drive.sh click "Clear"
scripts/app-drive.sh click "Projects"
scripts/app-drive.sh click "No project"
scripts/app-drive.sh click "Clear"
```

Read the numbers back and stop:

```bash
scripts/app-drive.sh rize -- --json report --by project --per day --period this-week
scripts/app-drive.sh stop
```

## What proves it works

- Grid up: `wait "Total (hours)"`, and `tree` showing a row button named "Client work, 3.5 of 40h" style (name, decimal hours, "of", the target hours), one cell button per non-empty day named "Client work, Wednesday, October 28: 2h", and a footer "Total (hours)".
- Numbers match: a cell's decimal hours equal the rize report cell for the same project and day (`rize --json report --by project --per day --period this-week`, `data.cells[].ms` divided by 3,600,000, rounded to one decimal). A PASS compares at least one cell and the grand total.
- Review marker: with one entry unapproved, `tree --full` (or the Windows `text` verb) shows a "+2" style delta and a "Review" badge button; after "Accept" on that entry the badge is gone and `rize --json entries list` shows `status: approved`.
- Filters: after choosing a client, only its projects remain and `tree` shows a "Clear" button and a count beside "Filters"; clearing restores the unfiltered rows.
- Day: `wait "Day summary"`, with figures for "Pending review", "Approved" and "Time entry hours", and `tree` listing the progress bar "Approved entries".
- Empty state: `wait "No time tracked this week"` on a week with no entries.
- Layout (hatching, today tint, progress lines): `scripts/app-drive.sh shot`.

## Gotchas

- The page opens on the Week scale, not Day, and on the current week (Monday start).
- Dynamic labels: row and cell names include numbers and dates ("Client work, 3.5 of 40h", "Client work, Wednesday, October 28: 2h"). Match the stable prefix: the project name followed by the hours and " of" for a row, or the project name plus the weekday and date for a cell.
- A cell button exists only where the row has time on that day; empty days are unnamed boxes.
- "Project" and "Category" appear several times: the Rows menu options (menu radio items), the Filters chip "Projects"/"Categories", and the options in a checklist. Open one menu at a time and `tree` before clicking when a name repeats.
- Checklist options are checkboxes named by the client, project or category name; the closed dropdown is dismissed by Escape or an outside click.
- Today's column is tinted by the local date, weekends are hatched, and the first/last days follow Monday-to-Sunday weeks, so seeded dates near a week edge may land on the previous or next week.
- This page's day is local midnight to midnight, not the Calendar's 5 AM boundary.
- Entries still being categorized count towards totals but show as "Processing" in Day instead of Accept or Review.
- The expected-hours denominator comes from Settings working hours ([settings](settings.md)); a changed target changes every "of Mh" label and progress line.
- A Day-scale row only expands in place; clicking its day cell toggles the same row instead of changing scale.
- The Review badge and a no-category entry's "Review" link navigate away to My Timesheet; use `tree` to confirm you changed page.

## Not reachable

- Hover tooltips on cells ("2h 15m · 45m to review") and the review badge.
- The exact hatch and tint rendering (visible in `shot` only).
- Per-cell review deltas as named controls: they are plain text, readable with `tree --full` or the Windows `text` verb.
