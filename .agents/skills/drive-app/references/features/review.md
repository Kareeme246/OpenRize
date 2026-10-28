---
id: review
title: Entry review
summary: Approve, reject, edit, split, merge, delete and add time entries from the panel that opens beside a timeline or list.
surfaces: [window, cli]
routes: []
code: [src/components/EntryReviewPanel.tsx, src/components/EntryReviewSheet.tsx, src/hooks/useEntryReview.ts, src/lib/confidence.ts, src/lib/entryShortcuts.ts, src/lib/merge.ts, src/components/SplitDialog.tsx, src/components/AddTimeSheet.tsx, src/components/ConfirmDialog.tsx, src/components/Sheet.tsx, src/pages/calendar/SplitPicker.tsx, src/hooks/useChangeToast.ts, src-tauri/core/src/activity.rs]
rize: ["rize entries add", "rize entries show", "rize entries edit", "rize entries approve", "rize entries unapprove", "rize entries reject", "rize entries split", "rize entries merge", "rize entries rm", "rize review"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Entry review

Entry review is the panel titled "Review entry" that opens when you select a time entry. It is one component bound to one hook, shared by the Calendar (docked on the right, with a "3 of 7" position while stepping through a queue), My Timesheet (a 320px aside), and a project's recent entries ([projects](projects.md)). It is where a pending entry is accepted or rejected, where its description, times, category, project and billable flag are edited, and where it is split, merged with a neighbour or deleted. Adding time by hand uses a separate sheet, "Add time", reached from My Timesheet and from a stopwatch. The Time Entries page and the Timesheets grid do not open this panel.

## Sub-features

- accept: the Accept button or Cmd/Ctrl+Enter approves the entry; it needs a category first.
- reject: the Reject button or Cmd/Ctrl+Backspace rejects the AI suggestion; the entry stays pending.
- unapprove: an approved entry shows "Approved" and an Unapprove button that returns it to pending (not for invoiced entries).
- edit-description: E or a click on the description opens an editor; Enter or Save commits, Escape or Cancel reverts.
- edit-times: the "Start time" and "End time" fields commit on Enter or blur and refuse an end at or before the start.
- pick-category-project: the Category section (C) and Project section (P, only when AI suggests projects too) list ranked options; 1 to 3 pick the top three; "More categories" holds the rest and "Manage categories..." jumps to the Categories page.
- billable: the Billable checkbox.
- split: S or the Split button; on a Calendar timeline the cut is picked on the block first ("Split here, at ..."), elsewhere the "Split entry" dialog opens directly.
- merge: "Merge up" and "Merge down" buttons join the entry to an earlier or later neighbour on the same day within five minutes, claiming any gap as tracked time.
- delete: the "Delete time entry" button asks "Delete time entry?" first; approved and invoiced entries refuse.
- detail-tabs: apps, titles, log and history tabs under the actions (history lists Created, Suggested, Accepted, Undid ... events).
- ai-extras: confidence meter and "Rule" marker, "Couldn't categorize this entry" with Retry, and the "Always categorize ... as ...?" rule prompt with Create rule and Dismiss.
- add-time: the "Add time" sheet (Date, Start, End, Description, Category, Project, Billable, "Add entry") creates an approved manual entry.

## How to get to it

- Calendar: click an entry block (or its Category or Projects lane block), press J or K, press R for review mode, or use the "Review N pending" button ([calendar](calendar.md)).
- My Timesheet: click a row's empty space, or use a row's "More actions" menu then Split ([my-timesheet](my-timesheet.md)).
- Project detail: a recent entry opens the same panel ([projects](projects.md)).
- Add time: My Timesheet's "+ Add time" button; a stopwatch's options menu, "Add to calendar" ([stopwatches](stopwatches.md)).
- Keys that are active while an entry is open and no field has focus: Cmd/Ctrl+Enter accept, Cmd/Ctrl+Backspace reject, S split, Escape close (or leave split mode), 1 to 3, C, P, E, J and K. Merge has no key.

## Driving it

Start a session, get past first-run onboarding if it shows ([onboarding](onboarding.md)), and seed one pending entry plus a neighbour to merge with. Times must be full date-times; replace `2026-10-28` with today's date.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh rize -- entries add --from 2026-10-28T09:00 --to 2026-10-28T10:00 -d "Write the report" --category Coding
scripts/app-drive.sh rize -- entries add --from 2026-10-28T10:00 --to 2026-10-28T10:30 -d "Triage the inbox" --category Communication
scripts/app-drive.sh rize -- --json entries list --from today
scripts/app-drive.sh rize -- entries unapprove 7f3a9c21
scripts/app-drive.sh rize -- review
```

`7f3a9c21` stands for the last eight characters of the first entry's id from the list. Open the panel from the Calendar and act on it:

```bash
scripts/app-drive.sh click "Calendar"
scripts/app-drive.sh click "Write the report"
scripts/app-drive.sh wait "Review entry"
scripts/app-drive.sh click "Accept"
scripts/app-drive.sh wait "Approved"
scripts/app-drive.sh click "Unapprove"
```

Keyboard equivalents (focus must not be in a field):

```bash
scripts/app-drive.sh key cmd+return
scripts/app-drive.sh key cmd+backspace
scripts/app-drive.sh key s
scripts/app-drive.sh key e
scripts/app-drive.sh key c
scripts/app-drive.sh key 1
scripts/app-drive.sh key escape
```

Edit the description. Press E to open the editor, fill the textarea named "Entry description", then press Enter to save (focus is in the field, so `key return` is read by the field, not as a shortcut):

```bash
scripts/app-drive.sh key e
scripts/app-drive.sh fill -r AXTextArea "Entry description" "Write the quarterly report"
scripts/app-drive.sh key return
```

Close the panel by name:

```bash
scripts/app-drive.sh click "Close review panel"
```

Edit times and billable. The time fields are named, the checkbox is named by its label:

```bash
scripts/app-drive.sh fill "End time" "1030AM"
scripts/app-drive.sh key return
scripts/app-drive.sh click "Billable"
```

Split on the Calendar day view: S starts the cut picker (a control named "Split here, at ..."), Enter picks the suggested cut, the "Split entry" dialog opens with its "Split at" field focused, and Enter submits it:

```bash
scripts/app-drive.sh click "Write the report"
scripts/app-drive.sh key s
scripts/app-drive.sh wait "Split here"
scripts/app-drive.sh key return
scripts/app-drive.sh wait "Split entry"
scripts/app-drive.sh key return
```

Merge, delete and the confirm dialog:

```bash
scripts/app-drive.sh click "Merge ⭣"
scripts/app-drive.sh click "Delete time entry"
scripts/app-drive.sh wait "Delete time entry?"
scripts/app-drive.sh click "Delete"
```

Add time from My Timesheet:

```bash
scripts/app-drive.sh click "My Timesheet"
scripts/app-drive.sh click "+ Add time"
scripts/app-drive.sh wait "Add time"
scripts/app-drive.sh fill "Description" "Planning call"
scripts/app-drive.sh click "Category"
scripts/app-drive.sh click "Meetings"
scripts/app-drive.sh click "Add entry"
```

Read the effect back through the backend, and stop:

```bash
scripts/app-drive.sh rize -- --json entries show 7f3a9c21
scripts/app-drive.sh rize -- --json entries list --from today
scripts/app-drive.sh stop
```

Everything the panel does also exists as `rize entries approve`, `reject`, `unapprove`, `edit`, `split --at`, `merge`, `rm --yes` and `add`; use those to arrange state and to check the result, not to replace the panel when the panel is under test.

## What proves it works

A PASS needs the visible panel state and the matching backend row.

- Panel opened: `wait "Review entry"`; in review mode also the "N of M" position text.
- Accept: `tree` shows "Approved" in the action row and an "Unapprove" button; `rize --json entries list` shows that entry with `status: approved` and `approvedBy: user`.
- Unapprove: the Accept button is back and `status` is `pending`.
- Edit: `rize --json entries show` returns `detail.entry.description`, `startedAt`/`endedAt`, `categoryId`, `projectId` and `billable` matching what was typed; `detail.events` gains an `edited` event, which the history tab shows as "Edited by you".
- Split: two entries exist where there was one, the first ending where the second starts (`endedAt` equals the other's `startedAt`), and the history tab of either shows "Split".
- Merge: one entry spans the earliest start to the latest end, and the neighbour is gone from `entries list`.
- Delete: the id no longer appears in `entries list`; the toast "Entry deleted" is short-lived (about three seconds), so do not rely on it alone.
- Add time: a new `source` manual entry with `status: approved` appears after "Add entry"; the sheet closes.
- Undo of any of the above: see [undo and redo](undo.md).
- Layout and the confidence meter: `scripts/app-drive.sh shot`.

## Gotchas

- Button names carry extra text: the accessible name of Accept is "Accept" followed by the shortcut hint, Reject likewise, and the merge buttons are "Merge ⭡" and "Merge ⭣" (arrow glyphs are part of the name). Match on the word; if the hint hides at narrow widths the name is just the word.
- The panel's close control is named "Close review panel" (the Delete button is the other "✕", labelled "Delete time entry"). `key escape` closes it too, but only while no field has focus.
- The four tabs are plain buttons whose text is lowercase: "apps", "titles", "log", "history". "Apps" also matches the sidebar item exactly, so for the apps tab use `tree` to see the order and `-i N`, or use "history", "titles" and "log", which are unique.
- The dialog's submit "Split" and the panel's "Split" button share a name; `key return` in the dialog avoids the clash, otherwise use `-i`.
- Approved entries are locked: Accept turns into "Approved", Split, Delete, the time fields, Billable, description and category/project edits are disabled. Unapprove first. Invoiced entries cannot be unapproved or deleted ([invoices](invoices.md)).
- Accept is disabled until the entry has a category; pending seeding through `rize entries unapprove` keeps the category, so Accept works at once.
- A manual `rize entries add` that has a description and category is approved, not pending. Only an add with neither description, category nor project (or one made on the Calendar by dragging) is pending.
- Time fields are native time inputs; `fill` types characters into them, and whether a value like "1030AM" lands depends on the webview's locale. Confirm with `tree`, which prints the field's value, and fall back to `rize entries edit --to` to arrange times.
- Shortcuts ignore keypresses while any input, select or textarea has focus, so after `fill` the next `key s` does nothing until focus moves.
- `scripts/app-drive.sh key backspace` and `key delete` exist, so Cmd+Backspace (Reject) is `key cmd+backspace` on macOS; the Windows driver spells it `ctrl+backspace`. Like every shortcut it is ignored while a field has focus.
- The category and project pickers' option names are the category and project names, so "Coding" or "Client work" is the label to click after opening a section; the 1 to 3 keys only act on the section that is open (press C first).
- The project section exists only when the AI setting suggests projects as well as categories ([settings](settings.md)); with no active projects it reads "No projects yet".
- Toasts are information only and carry no Undo button; reverse a change with the keyboard ([undo and redo](undo.md)).
- Merge refuses when the neighbour is approved and says why in the button's tooltip; the tooltip text is not in the accessibility tree, so check `rize entries merge` output for the refusal reason.

## Not reachable

- AI-driven parts: the confidence meter and "Rule" marker, "Couldn't categorize this entry" with Retry, and the "Always categorize ... as ...?" prompt with Create rule and Dismiss need real classification results or repeated corrections; `rize` cannot seed suggestions ([ai](ai.md)).
- Picking a split cut with the pointer on the timeline (only the keyboard route is described above).
- Hover tooltips, including the reasons shown on disabled buttons.
- Tab-order and focus-ring checks beyond what `tree` and `shot` show.
