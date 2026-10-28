---
id: my-timesheet
title: My Timesheet
summary: Work through a day, week or month of entries as a grouped table, approving, editing and deleting them singly or in bulk.
surfaces: [window, cli]
routes: [timesheet]
code: [src/pages/MyTimesheet.tsx, src/components/Picker.tsx, src/components/ConfirmDialog.tsx, src/lib/entries.ts, src/lib/dates.ts, src/lib/confidence.ts]
rize: ["rize entries list", "rize entries add", "rize entries edit", "rize entries approve", "rize entries unapprove", "rize entries rm", "rize review", "rize report"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# My Timesheet

My Timesheet is the review table: one row per time entry for the chosen day, week or month, grouped by project, category, app or not at all, with status tabs (To review, Processing, Approved, All), three summary cards, and an "Approve all" action. Rows can be edited in place (description, project, category, billable), accepted or unapproved one at a time, or selected and changed in bulk. Clicking a row's empty space opens the docked [entry review](review.md) panel on the right. It opens on today, on the To review tab, grouped by project, and unlike the Calendar its day is midnight to midnight.

## Sub-features

- summary-cards: "Pending review", "Approved" and "Total" cards, plus a "Reviewed share of tracked time" bar and an "N% reviewed" figure.
- status-tabs: To review, Processing, Approved and All, each with a count.
- grouping: a "Group" picker with Project, Category, App and None; group header rows collapse and expand.
- filters: "Category" and "Project" filter pickers (the Project filter appears once a project exists) and a Columns menu that shows or hides Project, Category and Billable.
- inline-edit: description button, per-row Project and Category pickers, and a billable toggle on each row.
- row-actions: "Accept" or "Unapprove" per row, and a "More actions" menu with Open in Calendar, Split, and Delete (or "Unapprove to delete" for an approved row).
- bulk-actions: row checkboxes and a "Select all shown" checkbox open a bar with Approve N, Unapprove N, Set category, Billable, Not billable, Delete N and Clear.
- approve-all: the "Approve all" button or Cmd/Ctrl+Shift+Enter, behind a confirmation that names the count and time.
- review-handoff: the "Review N" button opens the Calendar in review mode ([calendar](calendar.md)).
- add-time: "+ Add time" opens the Add time sheet ([entry review](review.md)).
- date-navigation: Previous/Next step, Today, and a Day, Week, Month scale control.

## How to get to it

- Sidebar: Track > My Timesheet.
- From the Timesheets grid: a "Review" badge or a row's "Review" link opens this page on the To review tab for the same day or week ([timesheets](timesheets.md)).
- Keyboard: Cmd/Ctrl+Shift+Enter (no field focused) starts Approve all.

## Driving it

Start a session, get past first-run onboarding if it shows ([onboarding](onboarding.md)), and seed entries. Use full date-times and replace `2026-10-28` with today's date.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh rize -- projects add "Client work"
scripts/app-drive.sh rize -- entries add --from 2026-10-28T09:00 --to 2026-10-28T10:00 -d "Write the report" --category Coding --project "Client work"
scripts/app-drive.sh rize -- entries add --from 2026-10-28T10:00 --to 2026-10-28T10:45 -d "Triage the inbox" --category Communication
scripts/app-drive.sh rize -- entries add --from 2026-10-28T11:00 --to 2026-10-28T12:00 -d "Planning call" --category Planning
scripts/app-drive.sh rize -- --json entries list --from today
```

Those entries are approved. Send two back to review using the last eight characters of their ids from the list (`7f3a9c21` and `b04e6d58` here):

```bash
scripts/app-drive.sh rize -- entries unapprove 7f3a9c21 b04e6d58
scripts/app-drive.sh rize -- review
```

Open the page and look at it:

```bash
scripts/app-drive.sh click "My Timesheet"
scripts/app-drive.sh wait "Pending review"
scripts/app-drive.sh tree
```

Tabs, grouping, scale and date:

```bash
scripts/app-drive.sh click "All"
scripts/app-drive.sh click "Approved"
scripts/app-drive.sh click "To review"
scripts/app-drive.sh click "Group"
scripts/app-drive.sh click "Category"
scripts/app-drive.sh click -r AXRadioButton "Week"
scripts/app-drive.sh click "Previous week"
scripts/app-drive.sh click "Jump to today"
scripts/app-drive.sh click -r AXRadioButton "Day"
```

Row actions and bulk actions:

```bash
scripts/app-drive.sh click "✓ Accept"
scripts/app-drive.sh click "Select all shown"
scripts/app-drive.sh wait "selected"
scripts/app-drive.sh click "Billable"
scripts/app-drive.sh click "Clear"
```

Approve all asks first. The dialog moves focus to itself, not to a button, so click its confirm button:

```bash
scripts/app-drive.sh click "Approve all"
scripts/app-drive.sh wait "Each entry keeps the category and project it has now."
scripts/app-drive.sh click -i 2 "Approve"
```

Delete a row through its menu and confirm, then add time:

```bash
scripts/app-drive.sh click "More actions"
scripts/app-drive.sh click "Delete"
scripts/app-drive.sh wait "Delete 1 entry?"
scripts/app-drive.sh click "Delete"
scripts/app-drive.sh click "+ Add time"
scripts/app-drive.sh wait "Add time"
scripts/app-drive.sh click "Cancel"
```

Read the backend back and stop:

```bash
scripts/app-drive.sh rize -- --json entries list --from today
scripts/app-drive.sh rize -- --json report --by status --per total --period today
scripts/app-drive.sh stop
```

## What proves it works

- Page loaded: `wait "Pending review"` (the first summary card) and `tree` showing the "My Timesheet" heading; in the To review tab, a row per pending entry whose description button carries the entry's description.
- Counts: the tab names carry live counts (for example "To review" followed by 2); after an accept the count drops by one and the row leaves the To review tab. A PASS reads both the tab count and `rize --json entries list --from today` (`data.entries[].status`).
- Accept: `status` flips to `approved`, `approvedBy` is `user`, and the Approved card's figure increases.
- Bulk and Approve all: the bar text "N selected" appears, then every selected entry shows `approved`/`billable` as asked in the rize list; entries without a category are skipped, and the Approve all dialog says so when some stay behind.
- Delete: the id is gone from `entries list` and the row disappears; a short-lived toast "Entry deleted" may be missed.
- Empty state: `wait "All caught up for"` once every pending entry is accepted.
- Layout: `scripts/app-drive.sh shot`.

## Gotchas

- Many controls repeat: every row has pickers named "Category" and "Project" and an "Unapprove" or "✓ Accept" button, and the filter bar has "Category" and "Project" too. Use `-i N` after reading `tree`, or narrow with the row's checkbox label first.
- Row checkboxes are named "Select" followed by the description (for example "Select Write the report"); the header checkbox is "Select all shown".
- The "Approve all" button's name also contains its shortcut hint; the confirmation dialog's title is dynamic ("Approve 2 entries, 1h 45m?") and its confirm button is plain "Approve", which collides with the page's "Approved" card and tab by substring. Prefer the dialog title for `wait` and `-i` for the click.
- The tabs, the "Approved" card and the sidebar are all candidates for the same text; read the role in `tree` (WebKit may report ARIA tabs as radio buttons) and add `-r`.
- "Group" is a combobox; its current value is not part of its name, so read it from `tree` or `shot`. Choosing an option means clicking the combobox, then clicking the option by its text.
- Description edit: clicking a row's description button opens an input named "Description"; Enter or blur saves it, Escape reverts. Approved rows disable it.
- Column visibility and the sidebar state live in the webview's local storage, not the database, so they persist across steps of a session but not across sessions.
- The page's day is local midnight to midnight, while the Calendar's day starts at 5 AM; an entry at 02:00 appears under the earlier Calendar day but under this date here.
- Approve all and Approve N skip entries with no category and entries still categorizing.
- Cmd/Ctrl+Shift+Enter is ignored while a field has focus; `scripts/app-drive.sh key cmd+shift+return` after moving focus.
- The Processing tab counts entries the AI is still classifying (or recording); a seeded session rarely has any.
- Invoiced entries lose their Unapprove and Delete actions (the menu item is disabled with a hint) ([invoices](invoices.md)).

## Not reachable

- AI confidence figures beside Project and Category chips, and the Processing state, need real classification ([ai](ai.md)).
- Hover tooltips (the reasons given on disabled controls).
- Clicking a row's empty space (the pointer shortcut that opens the docked panel) has no accessible name. The panel is reachable here through a row's "More actions" menu, then Split, which selects the entry and opens it; otherwise open the entry from the Calendar.
