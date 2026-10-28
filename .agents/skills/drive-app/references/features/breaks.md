---
id: breaks
title: Break reminders
summary: Reminders to take breaks after a stretch of work or at fixed times, a corner reminder panel with Start, Snooze and Skip, a running-break countdown and a welcome-back card.
surfaces: [window, reminder, background]
routes: []
code: [src/Reminder.tsx, src/pages/settings/BreakSettings.tsx, src/hooks/useBreaks.ts, src/lib/breaks.ts, src-tauri/src/breaks]
rize: ["rize settings list", "rize settings get", "rize settings set", "rize status"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Break reminders

OpenRize counts continuous work and, after the configured stretch (default from the Notifications settings), raises a reminder in a small always-on-top panel in a screen corner: "Start" a break, snooze it, or skip it. Fixed-time scheduled breaks (for example Lunch at 12:30) work the same way. A taken break becomes a break activity segment that is kept out of time entries. While a break runs, a countdown pill with "End break" and "+5 min" sits in the corner and the sidebar and Pulse show it; at the end a "Welcome back" card appears. The same panel also carries stopwatch reminders (see [stopwatches.md](stopwatches.md)). The settings live in Settings > Notifications; the rest is a separate webview window that the driver can only reach in limited ways.

## Sub-features

- interval-reminder: a reminder after "Remind me after" minutes of continuous work, with Start, Snooze and Skip.
- scheduled-breaks: fixed-time breaks with a name, time, length and weekdays; the "Add scheduled break" button in Settings.
- reminder-panel: the corner window (`reminder`) that shows the reminder card, the "Break due" nudge capsule, the break countdown tile and the welcome-back card.
- snooze-pause: "Snooze N min", the chevron "Choose snooze length", "Pause reminders for 1 hour", "Pause reminders until tomorrow" and the settings button "Resume reminders".
- break-controls: "Start", "End break", "+5 min", "Take a break now" (Pulse panel), "Skip" and "Skip today".
- settings-controls: the switches "Remind me to take breaks", "Pause stopwatches during breaks", "Chime", the pickers "Remind me after" and "Break length", the Default snooze radios, the text field "Reminder message".
- sample-reminder: in a dev build, Settings > Notifications > "Show sample" raises a reminder now.

## How to get to it

Settings (sidebar "Settings" or `cmd+comma`), then the settings nav item "Notifications" for the controls. The reminder panel itself appears on its own: when the work clock passes the threshold, at a scheduled time, or on "Show sample". There is no sidebar page for it. During a break the sidebar status strip reads "On break" style text (see Gotchas) and the Pulse panel (see [pulse.md](pulse.md)) shows "End break".

## Driving it

A fresh session may open on first-run onboarding; dismiss it first (see [onboarding.md](onboarding.md)).

```bash
scripts/app-drive.sh start
scripts/app-drive.sh tree
scripts/app-drive.sh key cmd+comma
scripts/app-drive.sh click "Notifications"
scripts/app-drive.sh wait "Break reminders"
scripts/app-drive.sh tree
```

Change settings through the window:

```bash
scripts/app-drive.sh click "Remind me to take breaks"
scripts/app-drive.sh click "Remind me to take breaks"
scripts/app-drive.sh click "Pause stopwatches during breaks"
scripts/app-drive.sh click "Chime"
scripts/app-drive.sh fill "Reminder message" "Stretch your legs"
scripts/app-drive.sh key return
scripts/app-drive.sh click "Add scheduled break"
scripts/app-drive.sh wait "Break name"
scripts/app-drive.sh fill "Break name" "Lunch"
scripts/app-drive.sh key return
scripts/app-drive.sh click "Delete Lunch"
```

"Remind me to take breaks" is a switch (`role=switch`), so each `click` flips it; click it twice to return to the starting state. The fields "Remind me after" and "Break length" are pickers; `click` opens one, `tree` lists its options.

Raise a reminder without waiting (dev and driven builds only), then read the state:

```bash
scripts/app-drive.sh click "Show sample"
scripts/app-drive.sh rize -- --json status
scripts/app-drive.sh tree
```

Seed settings and read back from the backend through `rize` (the keys follow `src-tauri/core/src/settings.rs`; hyphens and camelCase are the same key):

```bash
scripts/app-drive.sh rize -- settings get breaks.enabled
scripts/app-drive.sh rize -- settings set breaks.enabled true
scripts/app-drive.sh rize -- settings set breaks.work-minutes 15
scripts/app-drive.sh rize -- settings set breaks.break-minutes 5
scripts/app-drive.sh rize -- settings set breaks.message "Stretch your legs"
scripts/app-drive.sh rize -- settings list
scripts/app-drive.sh rize -- --json status
```

The reminder window's own buttons ("Start", "Snooze 10 min", "Skip", "End break", "+5 min", "More options") are in `src/Reminder.tsx`. On macOS the driver reads only the first window the app reports, which may or may not be the panel while it is up; run `tree` while a reminder is showing and see which one you got. If the tree shows the reminder's labels, click them as usual:

```bash
scripts/app-drive.sh tree
scripts/app-drive.sh click "Start"
scripts/app-drive.sh click "End break"
```

On Windows the panel is its own CDP target, so name it:

```bash
node scripts/app-drive-windows.mjs targets
node scripts/app-drive-windows.mjs tree -t reminder
node scripts/app-drive-windows.mjs click -t reminder "Start"
```

## What proves it works

- Settings round trip (required for a PASS of the settings controls): after toggling or picking in the window, `scripts/app-drive.sh rize -- settings get breaks.enabled` (and `breaks.work-minutes`, `breaks.break-minutes`, `breaks.message`, `breaks.chime`, `breaks.pause-stopwatches`) returns the new value; and the inverse, a `rize settings set` shows in the window's switch state in `tree`.
- Reminder raised: `scripts/app-drive.sh rize -- --json status` has `data.breaks.phase` equal to `"due"` with a `reminder` object (`source`, `label`, `plannedMs`, `canSnooze`) after "Show sample". After "Start" it is `"onBreak"` with `current` set; after "End break" it returns to `"idle"` (briefly `"over"` with `current.endedAt` set while the welcome card is up).
- Window evidence when the driver reaches the panel: the card title "Time for a N-minute break" (interval) or "<name> · N min" (scheduled), the buttons "Start" ("Start lunch" for a scheduled break named Lunch), "Snooze N min", "Choose snooze length", "Skip", "More options"; the running tile with "End break" and "+5 min"; the card "Welcome back · Nm break".
- Scheduled break saved: after "Add scheduled break" and a name edit, `settings get breaks.schedules` lists the entry with the label you typed.
- `shot` for the Notifications page (the macOS shot is of the first window only; it does not include the corner panel).

## Gotchas

- The reminder is a separate always-on-top, non-activating window. It never takes focus and appears only in the app's window list while it is shown. On macOS `scripts/ax-drive.js` always uses the first window the app reports (`AXWindows[0]`), and `shot` captures the first normal-layer window, so the panel is neither selectable nor reliably the one `tree` prints. Treat the macOS reach as partial: prove reminders with `rize --json status` (`data.breaks`) and the settings with `rize settings`.
- No break command is shipped in the CLI (it is planned for M2 in `src-tauri/cli/COMMANDS.md`); there is no CLI to start, end, snooze or skip a break. Those actions exist only as the panel's buttons and Pulse's "Take a break now" and "End break".
- "Show sample" does nothing if a break is already running (`dev_sample_reminder` returns early) and does not appear in a release build. It is gated by `import.meta.env.DEV` and `#[cfg(debug_assertions)]`, both true in a driven session.
- The Settings controls "Remind me after" and "Break length" are shown only while "Remind me to take breaks" is on; the Default snooze radios and "Reminder message" likewise. After turning the switch off, `wait` for the groups to disappear instead of clicking blindly.
- Time inputs, the weekday chips ("Monday" ... "Sunday", `aria-pressed`) and the "At" time field are in each scheduled break's row. The weekday chips are named by the full day name, not the letter. The "At" field is a native time input with no accessible name of its own; edit it through `rize settings set breaks.schedules` with a JSON array only if you must (the exact entry shape is `id`, `label`, `at` as HH:MM, `minutes`, `days`, `enabled`), or leave the default 12:30.
- An interval reminder needs `work-minutes` (at least 15, clamped) of continuous work during tracking hours. Waiting for one naturally in a driven session is impractical; use "Show sample".
- Deleting a scheduled break shows a toast `Deleted "<name>"` and the button is named `Delete <name>` with the current name.
- The chime ("Preview" button, "Chime" switch) is audible; skip it in a shared room.
- On Windows `-t reminder` only works once the window exists, which is after the first reminder is raised.
- The sidebar strip text while a break runs is `On <label in lower case> · <m:ss>`; the label and time are dynamic, so `wait` on "On break" will not match a custom label such as Lunch; `wait` on "On " plus the label.

## Not reachable

- The macOS notification sound, the chime audio and any native OS notification.
- The corner panel itself on macOS is only reachable if it happens to be the window the driver reads first; its position and non-activating behaviour are not observable.
- Natural timing: a real interval reminder after 15 or more minutes of work, snooze expiry and the end-of-break prompt are not practical to wait for.
- Break reminders over the CLI: a break command is planned (M2), so nothing drives it from a shell yet.
