---
id: calendar
title: Calendar
summary: See a day, week or month of tracked time as blocks you can open, review and add to.
surfaces: [window, cli]
routes: [calendar]
code: [src/pages/Calendar.tsx, src/pages/calendar, src/components/AiEngineBanner.tsx, src/components/Page.tsx, src/lib/dates.ts, src/lib/entries.ts, src/lib/settings.ts, src-tauri/core/src/entry_builder.rs]
rize: ["rize entries list", "rize entries add", "rize entries unapprove", "rize review", "rize app open"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Calendar

The Calendar is the app's home page (the first route, and what the sidebar's Calendar item returns to). It draws the tracked day as a timeline of entry blocks, a week as seven day columns, or a month as a grid of day totals, with a right-hand panel that shows either the range summary or the docked review panel for the open entry. Opening a day rebuilds its entries from recorded activity first, so a past day with no entries yet fills in. The docked panel is described in [entry review](review.md); undoing a change is described in [undo and redo](undo.md).

## Sub-features

- day-view: the timeline for one calendar day (5 AM to 5 AM), with the Time Entries, Category and Projects lanes, an activity strip, break bands and a now line that carries "Recording" while a session is live.
- week-view: seven day columns of compact entry blocks; a day header opens that day.
- month-view: a grid of day cells with total time, a category bar and a "to review" count; a cell opens that day.
- range-summary: the right panel when no entry is open: Work hours against the daily or weekly target, Time by category, and Top apps (Day only).
- date-navigation: Previous/Next step, a Today button, and the Day, Week, Month scale control.
- review-mode: the "Review N pending" button or the R key starts a queue of pending entries and steps through them; "All caught up" when the queue is empty.
- select-and-step: click a block, or press J and K, to open an entry in the docked panel.
- quick-add: dragging on the empty timeline (or pressing Enter on the "Drag to add a session" control) creates an "Untitled session" left pending for review.
- delete-shortcut: Backspace or Delete deletes the selected entry (approved entries refuse).
- empty-and-error-states: "No entries recorded for this day", "Nothing here yet" on a future day, and an inline error with Retry.

## How to get to it

- Sidebar: Track > Calendar. The pending count for today shows as a badge on the item.
- It is the route the app opens on, and the one Today returns to; the Calendar remembers the last day and scale you viewed.
- From My Timesheet: the "Review N" button opens the Calendar in review mode ([my-timesheet](my-timesheet.md)). From the Timesheets grid: a "Review" badge opens My Timesheet, not the Calendar ([timesheets](timesheets.md)).
- From the menu-bar panel's "N to review" button, which opens the window in review mode ([pulse](pulse.md)).
- From the shell: `rize app open --review` shows the window on today's review queue.
- Keyboard: R starts review mode, J and K move between blocks, Escape leaves review mode, Backspace or Delete removes the selected entry. These are ignored while a text field, select or textarea has focus.

## Driving it

A fresh driver session is a brand-new install and may open on first-run onboarding instead of the Calendar; get past it with the steps in [onboarding](onboarding.md) first. The database starts with eleven default categories (Coding, Research, Communication, Design, Writing, Planning and others) and no projects, entries or timers.

Start a session and seed entries through `rize`. Times must be full date-times (a bare `09:00` is rejected). Replace `2026-10-28` with today's date, and remember the Calendar day runs from 5 AM, so times before 05:00 belong to the previous calendar day.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh rize -- projects add "Client work"
scripts/app-drive.sh rize -- entries add --from 2026-10-28T09:00 --to 2026-10-28T10:00 -d "Write the report" --category Coding --project "Client work"
scripts/app-drive.sh rize -- entries add --from 2026-10-28T10:00 --to 2026-10-28T10:45 -d "Triage the inbox" --category Communication
scripts/app-drive.sh rize -- --json entries list --from today
```

`entries add` with a category creates an approved entry. To get a pending entry for review, send one back with the id suffix the list printed (the last eight characters; `7f3a9c21` here):

```bash
scripts/app-drive.sh rize -- entries unapprove 7f3a9c21
scripts/app-drive.sh rize -- review
```

Look at the page and move around it:

```bash
scripts/app-drive.sh tree
scripts/app-drive.sh wait "Day summary"
scripts/app-drive.sh click "Previous day"
scripts/app-drive.sh click "Jump to today"
scripts/app-drive.sh click "Next day"
scripts/app-drive.sh click -r AXRadioButton "Week"
scripts/app-drive.sh wait "Week summary"
scripts/app-drive.sh click -r AXRadioButton "Month"
scripts/app-drive.sh wait "Month summary"
scripts/app-drive.sh click -r AXRadioButton "Day"
```

Open an entry, step through entries and start review mode:

```bash
scripts/app-drive.sh click "Write the report"
scripts/app-drive.sh wait "Review entry"
scripts/app-drive.sh key escape
scripts/app-drive.sh key r
scripts/app-drive.sh wait "Review entry"
scripts/app-drive.sh key j
scripts/app-drive.sh key k
```

Open a day from the month grid by its date label (the cell label is "Wednesday, October 28: 1h 45m", so the weekday and date are the stable part):

```bash
scripts/app-drive.sh click -r AXRadioButton "Month"
scripts/app-drive.sh click "Wednesday, October 28"
scripts/app-drive.sh wait "Day summary"
```

Ask the app itself to open the review queue, as the menu-bar button does:

```bash
scripts/app-drive.sh rize -- app open --review
scripts/app-drive.sh wait "Review entry"
```

Shut the session down when done:

```bash
scripts/app-drive.sh stop
```

On Windows use the same verbs through `node scripts/app-drive-windows.mjs`, with `-r button` or `-r radio` in place of the macOS AX roles; `text` prints the page's visible text, which is the cheapest way to read the summary panel.

## What proves it works

Cheapest first; a PASS needs the first item for the behaviour under test, plus the rize read-back for anything that changes data.

- Navigation: `wait "Day summary"` (or "Week summary", "Month summary") after the scale click, and `tree` showing the heading for the date you stepped to.
- A seeded entry is drawn: `tree` lists a button named like "Write the report, 9:00 AM" followed by the end time and duration, and the Category lane lists a button named "Coding".
- Range summary: `tree` shows "Work hours", "Time by category" and, for a day, "Top apps" only when segments of at least a minute exist (a seeded-only session has none, so its absence is expected).
- Review mode: `wait "Review entry"` plus the "1 of 1" position text; after the last accept `wait "All caught up"`.
- Data effects: `scripts/app-drive.sh rize -- --json entries list --from today` and check `data.entries[].status` (`pending` or `approved`) and `approvedBy`.
- Visual layout (block density, hatching for pending, the now line, the activity strip): `scripts/app-drive.sh shot`.

## Gotchas

- Entry block labels are dynamic: the accessible name is the description, a comma, then the time range and duration (for example "Write the report, 9:00 AM" ... "1h"). Click on the description alone; it matches by substring. A category named like a description makes the click ambiguous, so use `-i N`.
- The Category and Projects lanes repeat each entry as a second and third button named after the category or project; clicking those selects the same entry.
- The "Review N pending" button label includes a count and the R hint, so click it by `key r` or use `wait` on "Review entry" after the key.
- The week view day headers have no aria-label (their text is the weekday, date, total and pending count), so find them with `tree` and click by the weekday text; there is no stable label.
- The sidebar's Calendar item has the accessible name "Calendar" only; its pending badge is separate text.
- Shortcuts (R, J, K, Escape, Backspace) stand down while a text field, select or textarea has focus. After a `fill` anywhere, move focus before sending keys, for example by clicking the Calendar sidebar item.
- `scripts/app-drive.sh key` has no keyspec for Backspace or Delete, so the delete shortcut cannot be sent on macOS; use the entry's Delete button ([entry review](review.md)). The Windows driver does accept `backspace` and `delete`.
- Past days are rebuilt from recorded activity on open, and the rebuild emits `entries-changed`; a seeded manual entry survives it.
- A day with no entries shows "No entries recorded for this day" only after loading finishes; `wait` for it rather than reading `tree` straight away.
- The window can remount when the dev server reloads, which drops you back on the Calendar's default day.
- A live session in a driven instance needs real app activity; a fresh session records nothing without Accessibility, so the "Recording" now line and the Top apps list are only reachable with real capture (see [capture](capture.md)).

## Not reachable

- Dragging on the timeline to add a session: the driver clicks and types but cannot drag. The keyboard path (Enter or Space on the "Drag to add a session" button) needs that control focused, which `click` does not guarantee, so treat quick-add as unverified; seed with `rize entries add` instead.
- Pinch and Cmd/Ctrl-wheel zoom of the day timeline, and hover tooltips.
- Picking a split cut by pointer on the timeline (the keyboard route is in [entry review](review.md)).
- The menu-bar "N to review" button itself (native panel); `rize app open --review` is the equivalent.
- Colour, hatching and animation details beyond what `shot` shows.
