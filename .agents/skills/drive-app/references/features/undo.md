---
id: undo
title: Undo and redo
summary: Reverse or reapply the last change to time entries or the catalog with Cmd/Ctrl+Z and Cmd/Ctrl+Shift+Z from any page.
surfaces: [window, cli]
routes: []
code: [src/App.tsx, src-tauri/core/src/activity/history.rs, src-tauri/core/src/activity.rs, src-tauri/src/commands.rs, src/lib/api.ts, src/hooks/useChangeToast.ts]
rize: ["rize entries list", "rize entries edit", "rize entries approve", "rize entries rm"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Undo and redo

Undo and redo are one global keyboard handler with no button or menu item. Cmd+Z (Ctrl+Z on Windows) undoes the last change to time entries or to categories and projects; adding Shift redoes it. It works from every page, including after navigating away from the page that made the change, and it leaves the browser's own text undo alone while a text field has focus. The history is kept by the running app's activity store, so it covers changes made in the window and changes made through `rize` while the app is running; it is not saved, so it is empty after a restart. Toasts that confirm an edit carry no Undo button; the keyboard is the only way.

## Sub-features

- undo: Cmd/Ctrl+Z reverses the most recent change and toasts "Undid" followed by what it was (for example "Undid accept").
- redo: Cmd/Ctrl+Shift+Z reapplies the change that was undone, toasting "Redid" and the label.
- nothing-to-do: with an empty stack the toast reads "Nothing to undo" or "Nothing to redo".
- failure: a change that later edits have overlapped is refused with "Undo failed: ..." or "Redo failed: ..." and stays on the stack.
- history-depth: the last 100 changes are kept, and any new change clears the redo stack.
- covered-changes: accept, reject and unapprove; edits of category, project, billable, description and times; adding an entry; split; merge; deleting entries; deleting a category or project; and the minutes another entry gave up to a change, which come back with it.
- history-events: undo and redo are logged on the entry and read "Undid ..." and "Redid ..." in the review panel's history tab.

## How to get to it

- Keyboard only: Cmd+Z and Cmd+Shift+Z on macOS, Ctrl+Z and Ctrl+Shift+Z on Windows, with the main window focused and no text field, select or textarea focused.
- The delete confirmation dialog in [entry review](review.md) and in [my-timesheet](my-timesheet.md) names the shortcut in its body text ("Undo with ...").
- Timers, settings and invoices are outside this history ([stopwatches](stopwatches.md), [settings](settings.md), [invoices](invoices.md)).

## Driving it

Start a session, get past first-run onboarding if it shows ([onboarding](onboarding.md)), and make changes the stack can reverse. Use full date-times and replace `2026-10-28` with today's date.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh click "Calendar"
scripts/app-drive.sh key cmd+z
scripts/app-drive.sh wait "Nothing to undo"
```

That first check needs no data. Now a real change from the window, then reverse and reapply it:

```bash
scripts/app-drive.sh rize -- entries add --from 2026-10-28T09:00 --to 2026-10-28T10:00 -d "Write the report" --category Coding
scripts/app-drive.sh rize -- entries unapprove 7f3a9c21
scripts/app-drive.sh click "Write the report"
scripts/app-drive.sh wait "Review entry"
scripts/app-drive.sh click "Accept"
scripts/app-drive.sh rize -- --json entries list --from today
scripts/app-drive.sh key escape
scripts/app-drive.sh key cmd+z
scripts/app-drive.sh wait "Undid accept"
scripts/app-drive.sh rize -- --json entries list --from today
scripts/app-drive.sh key cmd+shift+z
scripts/app-drive.sh wait "Redid accept"
scripts/app-drive.sh rize -- --json entries list --from today
```

`7f3a9c21` stands for the last eight characters of the new entry's id from `entries list`. The `key escape` closes the review panel so no field has focus; if a field does, click the sidebar's Calendar item first.

Other reversals to try, each followed by Cmd+Z, the expected toast and an `entries list` read-back:

```bash
scripts/app-drive.sh click "Delete time entry"
scripts/app-drive.sh click "Delete"
scripts/app-drive.sh key cmd+z
scripts/app-drive.sh wait "Undid delete"
scripts/app-drive.sh key s
scripts/app-drive.sh key return
scripts/app-drive.sh key return
scripts/app-drive.sh key cmd+z
scripts/app-drive.sh wait "Undid split"
```

A change made through the shell while the app runs joins the same history:

```bash
scripts/app-drive.sh rize -- entries edit 7f3a9c21 -d "Write the quarterly report"
scripts/app-drive.sh key cmd+z
scripts/app-drive.sh wait "Undid edit"
scripts/app-drive.sh rize -- --json entries list --from today
scripts/app-drive.sh stop
```

On Windows use `node scripts/app-drive-windows.mjs key ctrl+z` and `ctrl+shift+z`, and `text` to read the toast.

## What proves it works

A PASS needs the data read-back; the toast alone is not enough because it disappears after about three seconds.

- Accept undone: `rize --json entries list --from today` shows the entry's `status` back to `pending` and `approvedBy` null after Cmd+Z, and `approved` again after Cmd+Shift+Z.
- Delete undone: the deleted id is back in `entries list` with its original `description` and `categoryId`.
- Split undone: the two halves are one entry again with the original `startedAt` and `endedAt`; merge undone gives the two entries back.
- Add undone: the entry is gone from `entries list`; redo restores it.
- Edit undone: `description`, `billable`, `categoryId`, `projectId` or the times return to their previous values.
- Toast: `wait "Undid"` (or "Redid", "Nothing to undo") right after the key; run the `rize` read-back immediately after, not before.
- The entry's history: `rize --json entries show 7f3a9c21` lists `detail.events` with kinds `undone` and `redone`, and the review panel's history tab shows "Undid accept".
- Panel behaviour: an undo that removes the open entry (an added entry, a split's second half) closes the review panel on its own.

## Gotchas

- Shortcuts are ignored while an input, textarea, select or contenteditable has focus; after `fill` the key does nothing, and text undo happens inside the field instead. Move focus first.
- The keystroke is global to the app window, and `scripts/app-drive.sh key` brings the window frontmost first; it takes half a second, so allow time before reading the toast.
- Only one undo runs at a time (a second press while one is in flight is dropped). Wait for the toast between presses.
- The toast text uses the lower-cased label: "Undid accept", "Undid split", "Undid delete", "Undid edit", "Undid add", "Undid reject", "Undid unapprove", "Undid merge".
- The label "Edit" is shared by category, project, billable, description and time changes, so one Undo reverses exactly one such edit, not a whole sequence.
- A bulk change (Approve all, a bulk category change) is one stack entry and is reversed in one press.
- Undo is refused with the message "These entries have changed since; undo/redo cannot safely overwrite them." when later edits touched the same entries, and invoiced entries cannot be changed by it.
- Quitting or restarting clears the history (it lives in memory in the running app).
- A new change after an undo discards the redo stack, so redo only works straight after undo.
- Stopwatches, settings, invoices, onboarding and permissions are not undoable.
- The history is per running app: edits made with `rize` while the app is closed are not in any stack.

## Not reachable

- The keystroke path inside a native text field (the browser's own undo), which this feature deliberately leaves alone.
- The toast as a durable signal: it is short-lived, so the data read-back is the evidence.
- A visible list of the stack; there is none to inspect, only the next label via a press.
