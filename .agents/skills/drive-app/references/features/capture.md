---
id: capture
title: Automatic tracking
summary: Once a second OpenRize records the frontmost app, window title and browser URL as activity segments, with pause and resume, an idle threshold, tracking hours and manual focus sessions.
surfaces: [background, cli, window]
routes: []
code: [src-tauri/src/capture.rs, src-tauri/src/capture, src-tauri/core/src/capture.rs, src-tauri/src/ax.rs, src-tauri/src/activity.rs, src-tauri/core/src/activity.rs, src/components/Sidebar.tsx]
rize: ["rize status", "rize track start", "rize track stop", "rize track idle", "rize focus start", "rize focus stop"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Automatic tracking

A background sampler wakes every second, reads the frontmost window (app name, window title through the macOS Accessibility API, browser URL through AppleScript, or the Windows equivalents), the user's idle time and whether the screen is locked, and folds consecutive samples into segments. Time entries are built from those segments (see [time-entries.md](time-entries.md) and [review.md](review.md)). Tracking can be paused and resumed from the sidebar strip, the Pulse panel ([pulse.md](pulse.md)) and `rize track`; it also follows configured tracking hours; an idle threshold (default 5 minutes) turns time without input into idle; and a manual focus session replaces automatic capture with a labelled segment until stopped. The sampler runs whether or not a window is open. Accessibility permission only improves window titles; tracking of apps works without it ([onboarding.md](onboarding.md)).

## Sub-features

- sampler: the 1 Hz background capture of app, title and URL into segments, with a 20 second grace before an app switch is recorded and a gap rule that closes the segment after sleep.
- pause-resume: the sidebar strip's icon button (named "Pause" while tracking, "Resume" while paused) and `rize track stop` / `rize track start`.
- tracking-hours: the window configured in Settings > Tracking & work; outside it the strip reads "Outside tracking hours" and the button starts tracking manually.
- idle-threshold: minutes without input before time counts as idle (`rize track idle`); Pulse shows "Idle <time> · not counting".
- focus-session: a manual labelled session (`rize focus start`, `rize focus stop`) that closes the automatic segment and records a "focus" segment.
- away-detection: a locked screen, switched-out session or asleep displays close the activity segment and count as idle.
- status-strip: the sidebar's live line: "Tracking · <app>", "Tracking active", "Tracking paused", "Outside tracking hours", or the break countdown.

## How to get to it

The sampler has no page; it runs from app launch, including a background launch with no window. The visible controls are the status strip at the bottom of the sidebar (a pulsing dot, the status text and the Pause or Resume icon), the same control in the Pulse panel, and the `rize track` and `rize focus` commands. The tracking hours and the saved entries it produces are on the Settings > "Tracking & work" page and the Calendar.

## Driving it

A fresh session may open on first-run onboarding; dismiss it first (see [onboarding.md](onboarding.md)).

Read the state, pause and resume through the window:

```bash
scripts/app-drive.sh start
scripts/app-drive.sh rize -- --json status
scripts/app-drive.sh tree
scripts/app-drive.sh click "Pause"
scripts/app-drive.sh wait "Tracking paused"
scripts/app-drive.sh rize -- --json status
scripts/app-drive.sh click "Resume"
scripts/app-drive.sh wait "Tracking"
```

The strip's button is an icon whose only name is the icon's `aria-label` ("Pause" or "Resume"); if `click "Pause"` reports no actionable element, run `tree` and use the button next to the status text. The tooltip text ("Pause tracking", "Resume tracking") is not a label.

The same through the CLI:

```bash
scripts/app-drive.sh rize -- track stop
scripts/app-drive.sh rize -- track start
scripts/app-drive.sh rize -- track idle 10
scripts/app-drive.sh rize -- --json status
scripts/app-drive.sh rize -- focus start "Deep work"
scripts/app-drive.sh rize -- --json status
scripts/app-drive.sh rize -- focus stop
```

`rize track idle` takes whole minutes from 1 to 1440; anything else exits 2 with `INVALID_ARGUMENT`.

Look at the proof of what was captured:

```bash
scripts/app-drive.sh rize -- --json status --full
scripts/app-drive.sh rize -- entries list --from today
scripts/app-drive.sh rize -- review
```

## What proves it works

- Tracking state: `scripts/app-drive.sh rize -- --json status` has `data.activity.captureEnabled` true or false, `data.activity.trackingActive` true while tracking, and `data.activity.inTrackingHours`. After "Pause" or `track stop`, `captureEnabled` is false and `trackingActive` false, and the sidebar strip reads "Tracking paused" (`wait "Tracking paused"`); both are required for a PASS. After "Resume" or `track start` they return to true and the strip reads "Tracking" followed by the app name, or "Tracking active" when no app is known.
- Output of `rize track start` and `rize track stop`: JSON `data.captureEnabled` true or false.
- Idle threshold: `rize track idle 10` returns `data.idleThresholdMinutes` equal to 10, and `data.activity.idleThresholdMs` in the next `status` equals 600000.
- Focus session: after `focus start "Deep work"`, `status` has `data.activity.current.kind` equal to `"focus"` and `current.label` equal to `"Deep work"`, `data.activity.current.app` equal to `"Focus"`, and the strip reads "Tracking · Focus". After `focus stop`, `data.activity.current.kind` is no longer `"focus"`.
- Capture itself: `status` `data.activity.current.app` is the frontmost application and `trackedMs` grows second by second while it is frontmost; `--full` adds the window title and URL. With the harness in front this is usually "OpenRize" or the terminal, so treat it as proof that sampling works, not of which app a human used.
- `shot` for the strip's visual state.

## Gotchas

- `rize focus start` and `focus stop` need the running app (exit 3, `APP_NOT_RUNNING`, otherwise); a driven session has it running. `rize track start` and `rize app start` inside the session's `rize` passthrough can never launch a second app (the driver points `RIZE_APP` at a path that does not exist).
- `rize track stop` stores the setting, so it survives restarts; `scripts/app-drive.sh stop` deletes it with the session.
- The strip's text is dynamic: `Tracking · <app>` contains the live app name, so `wait` on "Tracking" (a substring), not on the full line. While a break runs it reads `On <label> · <m:ss>` instead.
- Tracking hours are on by default; outside them (`inTrackingHours` false) a session that has `captureEnabled` true can still show "Outside tracking hours" and a button whose tooltip is "Start tracking" while nothing is recorded. Check `inTrackingHours` before concluding the sampler is broken.
- A tick after a long gap (the laptop slept, or the debugger stopped the process) closes the segment at the last sample, so a gap larger than 10 seconds is not counted.
- App switches shorter than the 20 second grace are folded into the previous segment, so a quick tab flip does not create a new segment; use a longer dwell in any capture test.
- A driven session reads the real frontmost window, so what it records is whatever is in front of the machine; do not assert on specific app names.
- Window titles need Accessibility permission; without it `title` is empty and the app is still recorded.
- Windows capture is experimental; `src-tauri/src/capture/windows.rs` has its own title and URL logic. The reach is partial for the same reasons as macOS.
- Output of `rize status` omits titles and URLs unless `--full` is passed.

## Not reachable

- Real foreground changes: no driver can bring another app to the front in a controlled way, so segment boundaries per app, URL capture and browser automation prompts are not testable here except by a human using the machine.
- Sleep, lid close, screen lock and display sleep behaviour.
- The OS permission dialogs for Accessibility and Automation ([onboarding.md](onboarding.md)).
- Idle time driven by real keyboard and mouse input (the driver's synthetic keystrokes reset it).
- The Pause control in the tray and Pulse panel ([tray.md](tray.md), [pulse.md](pulse.md)).
