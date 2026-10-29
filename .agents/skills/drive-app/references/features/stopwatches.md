---
id: stopwatches
title: Stopwatches
summary: Keep any number of named stopwatches, start and pause them, edit their segments, reorder them, and turn their time into a calendar entry.
surfaces: [window, pulse, tray, reminder, cli]
routes: [timers]
code: [src/pages/Timers.tsx, src/components/StopwatchCard.tsx, src/components/TimerInfoSheet.tsx, src/components/HourRing.tsx, src/hooks/useTimers.ts, src/hooks/useTimerReorder.ts, src/lib/timers.ts, src/lib/timerInsertion.ts, src-tauri/core/src/timers.rs, src-tauri/src/tray.rs]
rize: ["rize timers list", "rize timers new", "rize timers start", "rize timers pause", "rize timers reset", "rize timers rename", "rize timers rm", "rize timers segments"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Stopwatches

Stopwatches is the manual-timing page, the one item in the sidebar's Manual group. Each stopwatch is a named card with a ring that starts and pauses it and a running total. A stopwatch's time is the sum of its segments, each a start and an end as a time entry is (pausing records one from the start of the run to the pause; segments can also be added, changed or removed by hand), so it survives restarts, and the menu-bar menu lists the running ones. Rust owns the state and the page re-renders from the full list after every command and from a broadcast when the tray or `rize` changes something. A stopwatch's time can be logged as a calendar entry through "Add to calendar", which opens the Add time sheet described in [entry review](review.md).

## Sub-features

- create: a "New tracker name" field and a "New tracker" button; the new stopwatch starts paused at 00:00.
- start-pause: the ring button on a card ("Start tracking" while paused, "Pause tracking" while running).
- header-counters: the page header shows the running count and the total across all stopwatches.
- rename: double-click the name, or edit "Tracker name" in the Info sheet; a blank name reverts.
- info-sheet: "Info" opens a sheet with Name, Details (Elapsed, Status, Created), the Segments list (change a segment's start or end, remove a segment, "Add a segment" with a start and an end), and Saved states; its button is "Done".
- save-state: "Save state" records the time as it reads now (a toast "Saved ... at ...").
- add-to-calendar: "Add to calendar" opens Add time prefilled with the name and the elapsed range, and resets the stopwatch once the entry is added (needs at least a minute).
- reorder: the "Reorder NAME" handle (drag, or arrow keys while it has focus) and the menu's "Move up" and "Move down".
- reset-delete: "Reset to zero" and "Delete", each behind a confirmation.
- reveal: a timer row in the Pulse panel opens this page and focuses the card.
- reminder: a stopwatch that has run a long time raises a reminder card with Pause and Keep going buttons, outside the main window.
- empty-state: "No stopwatches yet".

## How to get to it

- Sidebar: Manual > Stopwatches.
- From the Pulse panel: a timer row ("Show NAME in Stopwatches") opens the page and focuses that card ([pulse](pulse.md)).
- From the menu-bar menu, which lists running stopwatches ([tray](tray.md)).
- From the shell: `rize timers list` and its siblings; the window updates at once.

## Driving it

Start a session, get past first-run onboarding if it shows ([onboarding](onboarding.md)), then create and control stopwatches from the page or from `rize`:

```bash
scripts/app-drive.sh start
scripts/app-drive.sh click "Stopwatches"
scripts/app-drive.sh wait "No stopwatches yet"
scripts/app-drive.sh fill "New tracker name" "Deep work"
scripts/app-drive.sh click "New tracker"
scripts/app-drive.sh wait "Stopwatch Deep work"
scripts/app-drive.sh rize -- timers new "Email"
scripts/app-drive.sh rize -- --json timers list
```

Start, read the tick, pause. With several cards every ring button has the same name, so pick by position or by session id from the rize list:

```bash
scripts/app-drive.sh click -i 1 "Start tracking"
scripts/app-drive.sh wait "Pause tracking"
scripts/app-drive.sh rize -- --json timers list
scripts/app-drive.sh click -i 1 "Pause tracking"
scripts/app-drive.sh rize -- timers start "Email"
scripts/app-drive.sh rize -- timers pause "Email"
```

The Options menu, the Info sheet and its segments:

```bash
scripts/app-drive.sh click "Options for Deep work"
scripts/app-drive.sh click "Info"
scripts/app-drive.sh wait "Info - Deep work"
scripts/app-drive.sh fill "New segment start" "2026-09-01T09:00:00"
scripts/app-drive.sh fill "New segment end" "2026-09-01T09:05:00"
scripts/app-drive.sh click "Add"
scripts/app-drive.sh wait "Segment start"
scripts/app-drive.sh fill "Tracker name" "Deep work (client)"
scripts/app-drive.sh key return
scripts/app-drive.sh click "Done"
```

Save state, move, add to calendar:

```bash
scripts/app-drive.sh click "Options for Deep work (client)"
scripts/app-drive.sh click "Save state"
scripts/app-drive.sh click "Options for Email"
scripts/app-drive.sh click "Move up"
scripts/app-drive.sh click "Options for Deep work (client)"
scripts/app-drive.sh click "Add to calendar"
scripts/app-drive.sh wait "Add time"
scripts/app-drive.sh click "Category"
scripts/app-drive.sh click "Coding"
scripts/app-drive.sh click "Add entry"
```

Reset and delete with their confirmations, then clean up. The confirm title is dynamic, so wait on its stable body text:

```bash
scripts/app-drive.sh click "Options for Email"
scripts/app-drive.sh click "Reset to zero"
scripts/app-drive.sh wait "Set its time back to 0:00 and stop it."
scripts/app-drive.sh click "Reset"
scripts/app-drive.sh click "Options for Email"
scripts/app-drive.sh click "Delete"
scripts/app-drive.sh click -i 2 "Delete"
scripts/app-drive.sh stop
```

Segments through the shell are a start and an end, listed by id and added or edited the same way (`--for` derives the end as the start plus a length):

```bash
scripts/app-drive.sh rize -- timers segments list "Deep work"
scripts/app-drive.sh rize -- timers segments add "Deep work" --from 2026-09-01T09:00 --to 2026-09-01T10:30
scripts/app-drive.sh rize -- timers segments add "Deep work" --from 2026-09-01T11:00 --for 45m
scripts/app-drive.sh rize -- --json timers segments edit "Deep work" 3 --to 2026-09-01T10:45
```

Preview the destructive shell commands without changing anything, or run one for real with `--yes`:

```bash
scripts/app-drive.sh rize -- --dry-run timers reset "Deep work"
scripts/app-drive.sh rize -- --dry-run timers rm "Deep work"
scripts/app-drive.sh rize -- --yes timers reset "Deep work"
```

## What proves it works

- Created: `wait "Stopwatch Deep work"` (the card is a section named "Stopwatch" plus the label) and `rize --json timers list` returning an array element with `label`, `accumulatedMs: 0`, `startedAt: null` and an `ordinal`.
- Started: the ring button reads "Pause tracking", the page's "running" counter is 1 in `tree`, and `startedAt` is a number in the rize list. Two `timers list` calls a few seconds apart show the ring's elapsed text growing (for example 00:03 then 00:07 in `tree`), while `accumulatedMs` stays 0 until the pause.
- Started reads from zero: right after Start the ring reads 00:00 and counts 00:01, 00:02, 00:03; it never jumps up and falls back on Pause. `rize --json timers list` shows `startedAt` within a second of the machine's clock (`date +%s` times 1000), and the card's reading equals now minus `startedAt`.
- Paused: `startedAt` is `null`, `accumulatedMs` is greater than 0 and `segments` has one more element whose `endedAt - startedAt` equals its `durationMs` and the time the run lasted.
- Info edits: after "Add" with a start and an end 5 minutes apart, `segments[]` has an element with `durationMs` 300000 and the card's total grows by 05:00; editing a segment's end changes its `endedAt` and the total by the same amount; an end before the start is refused with "A segment cannot end before it starts."; a renamed stopwatch shows its new "Stopwatch ..." section name and `label`.
- Segments from `rize`: `timers segments add ... --to` and `--for` give the same segment; `segments edit` with only `--to` leaves `startedAt` alone; `segments add` with an end before the start exits 2 with `INVALID_ARGUMENT`; the window shows the change at once.
- Move: the order of `ordinal` in `timers list` swaps with the card order in `tree`.
- Add to calendar: a new entry appears in `rize --json entries list --from today` whose description is the stopwatch's name, and the stopwatch returns to 00:00 with `accumulatedMs: 0`.
- Reset and delete: the toast is brief, so rely on `timers list` (zeroed, or the element gone) and on `wait "No stopwatches yet"` after the last delete.
- Visual (ring progress, running accent edge): `scripts/app-drive.sh shot`.

## Gotchas

- Dynamic labels: every card exposes "Stopwatch LABEL", "Reorder LABEL" and "Options for LABEL"; a rename changes all three. Use a label unique enough not to be a substring of another (for example "Email" vs "Email follow-up" makes substring clicks ambiguous).
- "Start tracking" and "Pause tracking" are the ring buttons' names on every card, and the sidebar's tracking strip also offers a "Pause"/"Resume" control for capture; do not confuse the two. Disambiguate with `-i N`, in card order.
- Double-click rename cannot be sent (the driver only presses); rename through the Info sheet's "Tracker name" field or `rize timers rename`.
- The segment inputs in the Info sheet are date-time fields named "Segment start" and "Segment end" (one pair per segment) and "New segment start" and "New segment end" for the Add a segment row, which opens on the last hour. A segment may be zero length but not end before it starts, and a change commits on Enter or when focus leaves the row. Date-time fields may not be fillable by the driver as plain text; check `rize timers segments list` after trying, and say so if the field cannot be driven.
- The Info sheet's submit is "Done" and there is no Cancel; Escape and the "Close" button also close it. Opening it is a modal dialog, so card controls behind it are inert.
- "Add to calendar" is disabled until at least a minute has elapsed (the menu item is greyed with a hint).
- The ring display switches to hours (1:02:03) past an hour; `wait` on a short prefix like "00:0" is brittle, so prefer `rize` for exact elapsed values.
- Timers are shared through a module cache and a single 1 Hz clock; a started stopwatch keeps running across page navigation and appears in the tray menu.
- Reordering by drag is not drivable; the menu moves and the handle's arrow keys (when it has focus) persist the same order. `rize` has no reorder command yet.
- Timers are not part of the undo history ([undo and redo](undo.md)); only entry and catalog changes are.
- Reset and delete through `rize` need `--yes` outside a terminal, or `--dry-run` to preview.

## Not reachable

- Dragging a card to reorder, and the drop-position indicator.
- The menu-bar tray menu and glyph that reflect running stopwatches ([tray](tray.md)).
- The stopwatch reminder card (Pause and Keep going), which lives in the separate reminder window; the macOS driver reads only the first window of the app and the Windows driver reaches it only with `-t` and its window label ([breaks](breaks.md)).
- The Pulse panel's timer rows on macOS, where the driver reads only the first window; on Windows `-t pulse` targets that webview ([pulse](pulse.md)).
- Removing a segment and saved states through `rize`: `segments rm` and the save commands are planned, not shipped, so the Info sheet is the only way to remove a segment or inspect saved states.
