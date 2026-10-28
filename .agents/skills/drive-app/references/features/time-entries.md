---
id: time-entries
title: Time Entries
summary: Slice and total tracked time by category, project, client, app or status, and export it, as a pivot table, charts or a sortable log.
surfaces: [window, cli]
routes: [entries]
code: [src/pages/TimeEntries.tsx, src/components/Charts.tsx, src/lib/dates.ts, src/lib/palette.ts, src-tauri/core/src/reports.rs]
rize: ["rize entries list", "rize entries export", "rize entries add", "rize report"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Time Entries

Time Entries is the read-only analysis page. It totals the entries in a chosen range under a set of filters, then shows them three ways: a Table (a pivot of rows grouped by one dimension against columns of days, weeks or months), Charts (three donuts and a stacked column chart), and a Log (every entry in a sortable, virtualized list). It does not edit anything and has no review panel; approving and fixing entries happens in [my-timesheet](my-timesheet.md) and the [calendar](calendar.md). A filter set can be saved as a named view, and the filtered entries can be exported as CSV or JSON.

## Sub-features

- range: a "Date range" picker (Day, Week, Month, Last 30 days, Year) with Previous/Next step and Today; opens on the current week.
- filters: "Category", "Project" (once a project exists), "Client", "App", "Status" and "Billable" pickers, and a "Clear filters" button.
- search: a search box over descriptions and window titles.
- summary-cards: "Time", "Entries" and "Approved" cards that also switch what the Table counts, and a "Daily avg" card.
- table: a pivot with a "Group" picker (Category, Project, Client, App, Status), a "Stack" picker (Day, Week, Month), an h or % toggle, a Total row and column, and rows that expand to the latest 50 matching entries.
- charts: "By project", "By category" and "By app" donuts, and a "Time per day by category" stacked column chart whose legend rows expand.
- log: seven sortable columns (When, Description, Category, Project, App, Status, Dur.), newest first, capped at 5,000 rows.
- saved-views: a "Save view" menu that stores the current route under a name per account, lists saved views, and deletes them.
- export: an "Export" menu with CSV and JSON that writes the filtered entries to the Downloads folder.

## How to get to it

- Sidebar: Analyze > Time Entries.
- A saved view reopens its exact route (range, view, filters, grouping) from the "Views" menu.
- Group applies to the Table tab only and is hidden on Log; Stack applies to Table and Charts.
- From the shell, the same data is `rize entries list`, `rize report` and `rize entries export`.

## Driving it

Start a session, get past first-run onboarding if it shows ([onboarding](onboarding.md)), and seed a few entries across projects and categories. Use full date-times and replace `2026-10-28` with today's date.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh rize -- projects add "Client work"
scripts/app-drive.sh rize -- entries add --from 2026-10-28T09:00 --to 2026-10-28T10:00 -d "Write the report" --category Coding --project "Client work"
scripts/app-drive.sh rize -- entries add --from 2026-10-28T10:00 --to 2026-10-28T10:45 -d "Triage the inbox" --category Communication
scripts/app-drive.sh rize -- entries add --from 2026-10-27T14:00 --to 2026-10-27T15:00 -d "Design review" --category Design
```

Open the page and move through the tabs:

```bash
scripts/app-drive.sh click "Time Entries"
scripts/app-drive.sh wait "Daily avg"
scripts/app-drive.sh click "Charts"
scripts/app-drive.sh wait "By category"
scripts/app-drive.sh click "Log"
scripts/app-drive.sh wait "Sort by When"
scripts/app-drive.sh click "Sort by Dur."
scripts/app-drive.sh click "Table"
```

Range, grouping and stacking. Each picker is a combobox: click it by its label, then click the option text:

```bash
scripts/app-drive.sh click "Date range"
scripts/app-drive.sh click "Month"
scripts/app-drive.sh click "Previous month"
scripts/app-drive.sh click "Jump to today"
scripts/app-drive.sh click "Group"
scripts/app-drive.sh click "Project"
scripts/app-drive.sh click "Stack"
scripts/app-drive.sh click "Week"
```

Expand a pivot row to see its entries, filter and search:

```bash
scripts/app-drive.sh click "Coding"
scripts/app-drive.sh wait "Write the report"
scripts/app-drive.sh click "Category"
scripts/app-drive.sh click "Communication"
scripts/app-drive.sh fill "Search descriptions and window titles" "inbox"
scripts/app-drive.sh wait "Clear filters"
scripts/app-drive.sh click "Clear filters"
```

Save, reopen and delete a view:

```bash
scripts/app-drive.sh click "Save view"
scripts/app-drive.sh fill "View name" "Week by project"
scripts/app-drive.sh key return
scripts/app-drive.sh wait "Views (1)"
scripts/app-drive.sh click "Views (1)"
scripts/app-drive.sh click "Delete view Week by project"
```

Export. This writes into the real Downloads folder of the user running the driver, so for verification prefer the rize route to a scratch path and use the menu only when the export button itself is under test:

```bash
scripts/app-drive.sh rize -- entries export --format csv --out /tmp/time-entries-check.csv --period today
scripts/app-drive.sh click "Export"
scripts/app-drive.sh click "CSV"
scripts/app-drive.sh wait "Exported"
```

Read the totals back from the backend, then stop:

```bash
scripts/app-drive.sh rize -- --json report --by category --per day --period this-week
scripts/app-drive.sh rize -- --json entries list --period this-week
scripts/app-drive.sh stop
```

## What proves it works

- Page up: `wait "Daily avg"` and `tree` showing the "Time Entries" heading and a crumb for the range.
- Totals: the "Time", "Entries" and "Approved" cards in `tree` match `rize --json report --by none --per total --period this-week` (`data.cells[0].ms`, `.entries`, `.approvedMs`) for the same range and filters. This comparison is the PASS evidence for the numbers; the page alone is not.
- Table: a row button per group (named with its label, for example "Coding") and a "Total" row; expanding a row lists its entries' descriptions.
- Filters: after choosing "Communication" in Category, only matching rows remain and a "Clear filters" button appears; the same filter on the rize side is `--category Communication`.
- Charts: `tree` lists groups named "By category: Coding 1h, Communication 45m" style (title, colon, each label with its duration).
- Log: the sort buttons named "Sort by ..." and rows with the description text; clicking a header flips the arrow.
- Saved views: after saving, the menu button reads "Views (1)" and the toast "Saved view" is brief; the view survives navigating away and back.
- Export: the toast "Exported N entries to ..." names the path; check the file or the rize export output.
- Empty states: "No entries in this range" or "No entries match" with its "Clear filters" action.
- Layout and chart colours: `scripts/app-drive.sh shot`.

## Gotchas

- The page opens on the week containing today, not today; use the Date range picker's Day for a one-day view.
- Pivot cell values are hours (the "h" toggle) or percentages (the "%" toggle) as table-cell text; they are readable in `tree --full` on macOS or with the Windows driver's `text` verb, not as named controls.
- "Time", "Entries" and "Approved" are both card names and common words elsewhere (the sidebar has "Time Entries", My Timesheet has "Approved"); card buttons are named with their value appended. Use `-i N`, or `tree` first.
- Picker option labels such as "Week", "Month" and "Day" are also the names of other controls elsewhere in the app; the picker's list is portalled, so click the option right after opening its combobox.
- The Group picker is hidden (kept invisible) on the Log tab, and the h/% toggle only shows for the Table tab when the Time card is selected.
- The search box is debounced by 250 ms before it reaches the route; `wait` for "Clear filters" rather than reading `tree` immediately.
- Saved views and their names live in the webview's local storage keyed by account, not in the database, so `stop` discards them.
- The Export menu writes into the Downloads folder (or the home folder) of the real user, outside the driver's isolated data directory.
- The Log is virtualized (34px rows with overscan): rows far down a long list are absent from `tree` until scrolled to.
- A stacked day column shows `bucketLabel` text (weekday and date); labels vary with locale, so assert on totals, not column names.
- `rize entries list` returns 50 rows by default (`--limit` goes to 5000) and reports `truncated`; reports are the right comparison for totals.

## Not reachable

- Hover details on chart slices and legend popups beyond what a legend row's expand button shows.
- The exact rendered colours of the palette (`shot` shows them; nothing asserts them).
- Opening a file the Export menu wrote in an external app.
