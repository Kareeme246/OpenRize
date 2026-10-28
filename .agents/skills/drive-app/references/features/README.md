# Feature map

One file per user-visible feature of OpenRize: how to reach it, the exact commands that drive it with `scripts/app-drive.sh` and `rize`, and the evidence that proves it works. Every label in a "Driving it" section is the control's real accessible name taken from the code, never a placeholder. `verified: never` means a file is code-derived and has not yet been driven end to end by a maintenance pass. The skill linter (`node scripts/lint-skills/cli.ts`, run by `pnpm verify`) checks the structure, paths, commands and coverage of this map; only a live pass can check that a label is still on screen (see [maintenance](../maintenance.md)).

## Sweep order

Walk the table top to bottom for a broad regression pass, starting with Onboarding, because every other feature starts from a fresh session that may open on it.

| Feature | What it is | Surfaces | Reach (macOS / Windows / rize) | Verified |
| --- | --- | --- | --- | --- |
| [Onboarding](onboarding.md) | The first-run Permissions screen (Accessibility and Automation on macOS) and the full-window Relaunch screen, and how a driven fresh session gets past them to the Calendar. | window | partial / none / none | never |
| [Calendar](calendar.md) | See a day, week or month of tracked time as blocks you can open, review and add to. | window, cli | partial / partial / partial | never |
| [Entry review](review.md) | Approve, reject, edit, split, merge, delete and add time entries from the panel that opens beside a timeline or list. | window, cli | partial / partial / partial | never |
| [My Timesheet](my-timesheet.md) | Work through a day, week or month of entries as a grouped table, approving, editing and deleting them singly or in bulk. | window, cli | partial / partial / partial | never |
| [Apps](apps.md) | See which apps and websites were used on a day, set a default category and project for each, and exclude sensitive ones from tracking. | window, background | partial / partial / none | never |
| [Time Entries](time-entries.md) | Slice and total tracked time by category, project, client, app or status, and export it, as a pivot table, charts or a sortable log. | window, cli | partial / partial / partial | never |
| [Timesheets](timesheets.md) | Read a day or week of tracked hours as a project-by-day or category-by-day grid, narrow it by client, project and category, and approve entries in place. | window, cli | partial / partial / partial | never |
| [Projects](projects.md) | Create, edit, complete, archive and delete the projects that time belongs to, with per-project time, budget, billable amount and ready-to-invoice time. | window, cli | partial / partial / partial | never |
| [Categories](categories.md) | Create, edit, reorder, archive and delete the categories that classify time, and choose which of them count toward work hours. | window, cli | partial / partial / partial | never |
| [Clients](clients.md) | Manage the clients that own projects and invoices, with each client's projects, tracked time, ready-to-invoice time, invoiced total and default rate. | window, cli | partial / partial / partial | never |
| [Invoices](invoices.md) | Build invoices from approved billable time, retainers and manual items, preview the real PDF while editing, finalize it into a numbered frozen document, and mark it paid or void. | window | partial / partial / none | never |
| [Stopwatches](stopwatches.md) | Keep any number of named stopwatches, start and pause them, edit their segments, reorder them, and turn their time into a calendar entry. | window, pulse, tray, reminder, cli | partial / partial / partial | never |
| [Settings](settings.md) | Change how OpenRize looks and behaves - theme, shape, size and accent, launch at login, menu bar icon, close behavior, tracking hours, weekly target, storage location and retention - and have every change saved at once. | window, cli | partial / partial / partial | never |
| [Accounts](accounts.md) | Keep separate sets of time, projects, clients, invoices and stopwatches under local named accounts, and switch between them or back to No account. | window | partial / partial / none | never |
| [AI categorization](ai.md) | Suggest a category and project for each closed time entry from your rules, a personal model and Apple's on-device model, with calibrated confidence, auto-approval and learning from your reviews. | window, background | partial / partial / partial | never |
| [Developer tools](dev-tools.md) | In a macOS dev build, simulate Accessibility and Automation permission states and re-run the first-run Permissions screen without touching macOS privacy settings or any data. | window | partial / none / none | never |
| [Break reminders](breaks.md) | Reminders to take breaks after a stretch of work or at fixed times, a corner reminder panel with Start, Snooze and Skip, a running-break countdown and a welcome-back card. | window, reminder, background | partial / partial / partial | never |
| [Pulse](pulse.md) | The small panel under the menu-bar or tray icon that shows whether tracking is on, how today is going, the next break and running stopwatches, with a door into the main window. | pulse, tray | none / none / partial | never |
| [Automatic tracking](capture.md) | Once a second OpenRize records the frontmost app, window title and browser URL as activity segments, with pause and resume, an idle threshold, tracking hours and manual focus sessions. | background, cli, window | partial / partial / partial | never |
| [Undo and redo](undo.md) | Reverse or reapply the last change to time entries or the catalog with Cmd/Ctrl+Z and Cmd/Ctrl+Shift+Z from any page. | window, cli | partial / partial / partial | never |
| [Battery and energy](energy.md) | A background sampler records OpenRize's own CPU time and energy every 15 seconds, and Settings shows its energy impact, battery use, AI share and a resettable benchmark. | window, background | partial / none / none | never |
| [Updates](updates.md) | The in-app updater checks GitHub Releases on a schedule, shows an "Update available" button and what is new in Settings, and installs and restarts only when asked. | window, background | partial / partial / none | never |
| [Menu-bar tray](tray.md) | The OpenRize icon in the macOS menu bar (Windows notification area) whose glyph shows whether a stopwatch runs, whose left click opens Pulse and whose right-click menu pauses stopwatches, opens the app or quits it. | tray | none / none / partial | never |
| [rize CLI](rize-cli.md) | The rize command line tool that reads and edits OpenRize data with the app open or closed, controls the app and tracking, and prints plain tables or a stable JSON envelope. | cli | partial / partial / partial | never |

## Not reachable by any tool here

Native surfaces sit outside the webview that the two drivers read, so no feature file can claim them as driven:

- The tray icon, its glyph, tooltip and right-click menu, and the left click that opens Pulse. The Pulse panel is therefore reachable only from a native tray click and never appears in either driver's tree ([Pulse](pulse.md), [Menu-bar tray](tray.md)).
- Operating-system permission prompts and the System Settings panes behind "Grant" buttons ([Onboarding](onboarding.md), [Developer tools](dev-tools.md)); use the Developer tools simulation to see every permission state.
- OS notifications, sounds and the window frame ([Break reminders](breaks.md)).
- The macOS driver reads only the app's first window, so the break reminder corner panel and the Pulse window are not in its tree; the Windows driver reads each webview but not the native frame.
- Dragging (for example on the Calendar timeline): neither driver can drag, so seed with `rize entries add` ([Calendar](calendar.md)).
- Real foreground changes, sleep, lock and idle time driven by real input ([Automatic tracking](capture.md)), and the real relaunch behind "Relaunch now" ([Onboarding](onboarding.md)).
- The on-device AI tier, which needs macOS 26 with Apple Intelligence on the host ([AI categorization](ai.md)).

Each file's own "Not reachable" section lists the exact pieces. A run that cannot exercise something reports it as exactly that.
