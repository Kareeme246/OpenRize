---
id: onboarding
title: Onboarding
summary: The first-run Permissions screen (Accessibility and Automation on macOS) and the full-window Relaunch screen, and how a driven fresh session gets past them to the Calendar.
surfaces: [window]
routes: []
code: [src/components/Onboarding.tsx, src/hooks/useOnboarding.ts, src/lib/onboarding.ts, src/lib/permissions.ts, src/hooks/useAccessibilityState.ts, src/components/RelaunchModal.tsx, src/hooks/useRelaunch.ts, src/lib/relaunch.ts, src-tauri/src/onboarding.rs, src-tauri/src/permissions.rs, src-tauri/src/permissions]
rize: []
reach-macos: partial
reach-windows: none
reach-rize: none
verified: never
---

# Onboarding

On macOS the first screen of a brand-new install can be a full-window "Permissions" card that explains the two optional permissions OpenRize can use: Accessibility (recommended, it lets OpenRize read window titles) and Automation (optional, it lets OpenRize read browser tab URLs). Neither gates the app: its one primary button, "Continue", always leaves it, except after an Accessibility grant that only a restart completes, when the same button becomes "Restart required". A separate full-window "Relaunch OpenRize" dialog covers the same restart case once onboarding is over. Every other feature's verification starts from a fresh driven session, which is an empty install, so this page matters first: it says when the screen shows and how to get past it without touching a native prompt.

## Sub-features

- first-run-screen: the "Permissions" card with an Accessibility row and an Automation row, each with a role badge, a description, a status line and (while not granted) a "Grant" button.
- continue: the one button that dismisses the screen and writes the "already seen" marker.
- restart-required: after an Accessibility grant made in this running copy that still does not work, the button reads "Restart required" and relaunches the app.
- relaunch-dialog: the undismissable "Relaunch OpenRize" dialog with "Relaunch now", shown outside onboarding while the same restart is pending.
- permission-state: the backend monitor that polls the Accessibility trust flag plus a real probe every 2 seconds and reports one state (not granted, checking, working, not working, relaunch required, not applicable).
- seen-marker: the file `permissions-onboarding-done` in the settings folder, which keeps the screen from coming back.

## How to get to it

When it shows, from `onboardingStep` in `src/lib/onboarding.ts` and `onboarding_needed` in `src-tauri/src/onboarding.rs`. Both conditions must hold:

1. The marker file `permissions-onboarding-done` does not exist in the settings folder (`<XDG_CONFIG_HOME>/openrize/` for a driven session). A brand-new driven session has its own empty `XDG_CONFIG_HOME`, so the marker is always absent at the start.
2. The backend reports Accessibility as not granted (step "asking"). Any other state keeps it hidden on first load: working, checking and not working only show it if it was already up earlier in this run, and "not applicable" (Windows, Linux) never shows it. The one exception is "relaunch required", which shows it straight away as "Restart required".

So whether a fresh driven macOS session opens on onboarding or on the Calendar depends on the real macOS Accessibility trust of the dev binary `src-tauri/target/debug/openrize` (or of the terminal that launched it; the Developer tools text says a dev build launched from a terminal may be listed under the terminal). The driver itself needs Accessibility on the terminal, so many machines start straight on the Calendar, but you cannot know without looking. Run `tree` after `start`: onboarding is present when the tree has the text "Permissions" and a "Continue" button.

On Windows the screen can never appear: `HAS_PERMISSIONS` is false, the permission list is empty and the monitor never starts. A Windows session opens on the Calendar.

Re-opening it on purpose (macOS dev or driven build only): Settings > Developer > "Show onboarding" (see [dev-tools.md](dev-tools.md)).

## Driving it

Start a session and look at what it opened on:

```bash
scripts/app-drive.sh start
scripts/app-drive.sh tree
```

If the tree shows a Calendar page (sidebar items such as "Calendar" and "Settings"), onboarding is not up and there is nothing to do. If it shows the heading "Permissions", get to the Calendar like this, with no native prompt involved:

```bash
scripts/app-drive.sh wait "You can change these later in Settings."
scripts/app-drive.sh tree
scripts/app-drive.sh click "Continue"
scripts/app-drive.sh tree
```

"Continue" is enabled in every step except the short "Checking…" step (a grant being checked), where the button is disabled; `wait` a few seconds and `tree` again. Pressing it calls `finish_onboarding`, which writes the marker into the session's own config folder, so the screen does not return in this session or after a Settings visit.

A generic preamble for any other feature file's verification:

```bash
scripts/app-drive.sh start --json
scripts/app-drive.sh tree
scripts/app-drive.sh click "Continue"
```

`click "Continue"` fails with "no actionable element" when onboarding is not up; that is fine, it means the session is already on the Calendar. Never click these during a driven run: "Grant Accessibility" (it calls the native macOS Accessibility prompt and opens System Settings), "Grant Automation" (it opens System Settings), "Restart required" and "Relaunch now" (they restart the app process, which a driven dev session may not survive).

Simulating permission states instead of granting for real, with the debug-only Developer tools (all of this assumes onboarding has been dismissed first, because the full-window card sits over Settings):

```bash
scripts/app-drive.sh key cmd+comma
scripts/app-drive.sh click "Developer"
scripts/app-drive.sh wait "Simulate permissions"
scripts/app-drive.sh click -r AXRadioButton -i 0 "Off"
scripts/app-drive.sh click "Show onboarding"
scripts/app-drive.sh wait "You can change these later in Settings."
scripts/app-drive.sh shot --out /tmp/openrize-proof/onboarding-asking.png
scripts/app-drive.sh click "Continue"
scripts/app-drive.sh click "Developer"
scripts/app-drive.sh click "Reset simulations"
```

Simulating Accessibility as "On" with the radio makes the Accessibility card say "Granted" without any macOS change (see [dev-tools.md](dev-tools.md) for the radio positions and for why the "Relaunch" radio must be avoided). The marker, if you want to confirm it from a shell, is the file `permissions-onboarding-done` inside `openrize/` under the `config` folder that `start --json` printed for the session.

## What proves it works

- Screen present: the tree has the heading "Permissions", the text "You can change these later in Settings.", the rows "Accessibility" and "Automation" with role badges (Recommended, Optional), and a "Continue" button. With Accessibility not granted the Accessibility row also shows the status "Not granted" and a button named "Grant Accessibility".
- Dismissed: after `click "Continue"`, `tree` no longer contains "Permissions" as a heading and does contain the sidebar items ("Calendar", "Settings"). That, plus the marker file existing in the session config folder, is the required evidence for a PASS of "get past onboarding".
- Persistence: after navigating to Settings and back, or after `Show onboarding` then `Continue`, the screen does not come back on its own.
- Simulated state reaches the screen: with the Accessibility radio "Off" the status line reads "Not granted"; "Checking" reads "Checking…" and "Continue" is disabled; "Not working" reads "Allowed, but not working"; "On" reads "Granted"; "Relaunch" reads "Restart needed" and the primary button reads "Restart required". The status line and the button label together are the evidence.
- `shot` for layout (480 px wide card, centred on the window).

## Gotchas

- The screen is an overlay (`fixed inset-0`), not a modal dialog: the sidebar and page stay in the accessibility tree behind it, and `click` is an `AXPress` that does not care that something is on top. Do not use that to skip it; dismiss it with "Continue" so the marker is written and the next steps see a normal window.
- If Accessibility becomes trusted while the session runs (for example the machine had it off and you or someone grants it), the state goes to "Restart required" and the only button on the screen is the restart. A driven session cannot continue past that without a relaunch, and a relaunch of a dev session may end it. Stop the session and start a new one instead (`scripts/app-drive.sh stop`, then `start`).
- The "Checking…" step lasts a few seconds (three readings at a 2 second poll) after a grant; Continue is disabled during it so the answer can arrive first.
- "Grant" buttons are named "Grant Accessibility" and "Grant Automation" (their `aria-label`), not "Grant". The visible text alone would be ambiguous.
- Every status string comes from `src/lib/permissions.ts` (`stateLabel`) except the restart case, which the screen words as "Restart needed" while the Settings Overview card says "Relaunch needed".
- Settings > Overview has its own permission cards ("Allow Accessibility", "Relaunch now", "Open Automation settings") for the same states; see [settings.md](settings.md).
- Onboarding state is not stored in `settings.json` and `rize` cannot read or change it. The marker lives next to the settings file and is machine state, not account state.
- The Relaunch dialog ("Relaunch OpenRize") cannot be dismissed (Escape is refused, no close button); while it is up the Settings page behind it is unreachable, so `stop` and `start` again if a simulation of the "Relaunch" state strands you.
- "Show onboarding" is in-memory only and is lost on relaunch; it never deletes the marker.

## Not reachable

- The real macOS Accessibility and Automation prompts and the System Settings panes that "Grant Accessibility" and "Grant Automation" open are native; no driver here can click them, so a real grant cannot be tested in a driven session. Use the Developer tools simulation ([dev-tools.md](dev-tools.md)) to see every state.
- The actual app relaunch behind "Restart required" and "Relaunch now" (the process restarts; the driver loses it).
- Whether macOS really lists the dev binary as trusted: only visible by reading the screen, not controllable from here.
- Anything on Windows: the feature does not exist there.
- A way past onboarding that needs no native prompt does exist ("Continue"), so there is no harness defect for the asking, working and not working states. The one state with no exit but a restart is "relaunch required", described above; it can only arise if Accessibility is truly granted during the run.
