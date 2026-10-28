---
id: dev-tools
title: Developer tools
summary: In a macOS dev build, simulate Accessibility and Automation permission states and re-run the first-run Permissions screen without touching macOS privacy settings or any data.
surfaces: [window]
routes: []
code: [src/pages/settings/DevToolsSetting.tsx, src/lib/devTools.ts, src-tauri/src/dev_tools.rs, src-tauri/src/onboarding.rs]
rize: []
reach-macos: partial
reach-windows: none
reach-rize: none
verified: never
---

# Developer tools

Settings > Developer is a panel for testing permission and onboarding behaviour by hand. "Simulate permissions" makes the running app report an Accessibility state (Real, Off, Checking, Not working, Relaunch, On) and an Automation answer (Real, Off, On) instead of the real ones; macOS is never asked or changed, and a simulation ends on relaunch, on "Allow Accessibility", or with "Reset simulations". "Re-run onboarding" opens the first-run Permissions screen now (simulating Accessibility as off when it would not otherwise show). "Reset" clears every simulation and any re-run. A last block names this build's identifier and offers the `tccutil` command that would revoke its real Accessibility grant, a "Copy" button and "Open Accessibility settings". These tools are the way to see every permission state without changing the machine, which makes them valuable for verifying the Overview permission cards and the first-run screen (see [onboarding.md](onboarding.md) and [settings.md](settings.md)).

## Sub-features

- simulate-accessibility: radios Real, Off, Checking, Not working, Relaunch, On for the Accessibility state.
- simulate-automation: radios Real, Off, On for the Automation answer.
- rerun-onboarding: "Show onboarding" opens the Permissions screen again.
- reset: "Reset simulations" clears simulations and the re-run.
- real-grant-help: the build's identifier, the copyable revoke command, "Copy" and "Open Accessibility settings".

## How to get to it

Which builds and identifiers allow it, from `src-tauri/src/dev_tools.rs` `allowed(identifier, debug_build)` and the page's own gate:

- The Rust side must be a debug build (`cfg!(debug_assertions)`); a release build never qualifies, whatever the identifier.
- The app identifier must be the release identifier `com.offlinestudios.openrize` followed by a dot and at least one more character: `.dev` (`pnpm tauri dev`), `.devshotNNNN` (the screenshot script) or `.driveNNNN` (the driver). The release identifier itself, `com.offlinestudios.openrize`, never qualifies, and neither do `com.offlinestudios.openrize.` (nothing after the dot) or `com.offlinestudios.openrizex`.
- The frontend only draws the section when Vite's dev server is serving it (`import.meta.env.DEV`) and the platform has permissions to test (`HAS_PERMISSIONS`, which is macOS only). So the "Developer" nav button exists in a macOS dev or driven session and nowhere else. A Windows driven session has a qualifying identifier (`.driveNNNN`) but no section, so every command here is macOS only.
- If the backend refuses (not a debug build, or a non-qualifying identifier), the panel shows "Developer tools need the dev identifier. Start the app with pnpm tauri dev." and every control is absent.

Path: Settings (Cmd+,), then the "Developer" nav button (after "Advanced settings").

## Driving it

A fresh macOS session is an empty install and may open on first-run onboarding (see [onboarding.md](onboarding.md)); the Developer tools can be used from either screen's underlying Settings page only once that screen is dismissed.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh key cmd+comma
scripts/app-drive.sh wait "Preferences save automatically"
scripts/app-drive.sh click "Developer"
scripts/app-drive.sh wait "Simulate permissions"
scripts/app-drive.sh tree
```

Pick states. The Accessibility and Automation groups reuse the names "Real", "Off" and "On", so match by position: the Accessibility group comes first (index 0), Automation second (index 1).

```bash
scripts/app-drive.sh click -r AXRadioButton -i 0 "Off"
scripts/app-drive.sh click "Checking"
scripts/app-drive.sh click "Not working"
scripts/app-drive.sh click -r AXRadioButton -i 0 "On"
scripts/app-drive.sh click -r AXRadioButton -i 1 "Off"
scripts/app-drive.sh click -r AXRadioButton -i 1 "On"
scripts/app-drive.sh click -r AXRadioButton -i 0 "Real"
scripts/app-drive.sh click -r AXRadioButton -i 1 "Real"
```

Read the effect on the Overview permission cards, then re-run onboarding and reset:

```bash
scripts/app-drive.sh click "Overview"
scripts/app-drive.sh wait "Not granted"
scripts/app-drive.sh click "Developer"
scripts/app-drive.sh click "Show onboarding"
scripts/app-drive.sh wait "You can change these later in Settings."
scripts/app-drive.sh shot --out /tmp/openrize-proof/dev-onboarding.png
scripts/app-drive.sh click "Continue"
scripts/app-drive.sh click "Developer"
scripts/app-drive.sh click "Reset simulations"
```

Do not pick the Accessibility radio "Relaunch" (see Gotchas). Keep no other state: `Reset simulations` returns every report to the real state. There is no `rize` command for any of this.

## What proves it works

- Section present: `wait "Simulate permissions"` and the badge text "Dev build only" in `tree`. Absent: the nav button "Developer" does not exist, or the panel says "Developer tools need the dev identifier." That second text is the correct PASS for a release-like identifier and cannot be produced by a driven session (identifier `.driveNNNN` always qualifies in a debug build).
- Identifier: the Reset block's text contains "This dev build runs as com.offlinestudios.openrize.driveNNNN, not the installed app's com.offlinestudios.openrize." The session's `identifier` (from `start --json`) must equal that value.
- Simulation reaches the app, not just the panel: after Accessibility "Off", the Overview Accessibility card shows "Not granted" with the primary button "Allow Accessibility"; "Checking" shows "Checking…"; "Not working" shows "Allowed, but not working" and the amber note about removing OpenRize from the Accessibility list; "On" shows "Granted". Both the radio's checked state in `tree` and the card text are required for a PASS. Back to "Real" shows what macOS truly reports for the dev binary.
- Automation: the Automation card follows its group; "Off" and "On" change its state label.
- Re-run onboarding: after "Show onboarding" the full-screen Permissions screen appears (heading "Permissions", text "You can change these later in Settings.", rows Accessibility and Automation, button "Continue"). With Accessibility "Off" it also offers "Grant" buttons.
- Reset: the Permissions screen is gone and the radios return to Real. The dev panel always re-reads `dev_tools_info`, so its radios show what the backend holds.
- `shot` for the panel and the re-run screen.

## Gotchas

- Choosing the Accessibility radio "Relaunch" raises the full-window "Relaunch OpenRize" screen ("Accessibility is allowed. Relaunch to finish." with "Relaunch now"). It cannot be dismissed (Escape is refused, no close button), so the panel and "Reset simulations" are no longer reachable, and "Relaunch now" restarts the app process, which a driven dev session may not survive. If you get stuck, `scripts/app-drive.sh stop` and `start` again; do not use this option unless that is what you are testing. While the first-run Permissions screen is up, the relaunch state is shown on that screen instead (its button becomes "Restart required").
- The first-run screen is a full-window cover: while it is up the Settings page behind it is not reachable. Dismiss it with "Continue", then click "Developer" again.
- "Show onboarding" does not delete the saved "already seen" marker; it is kept in memory only and lost on relaunch. In a driven session the marker file lives in the session's own config folder (not the real one), but the tool is built so it works either way.
- "Open Accessibility settings" appears twice: in the Reset block and, when the card falls back to its default action, on the Overview Accessibility card. Run `tree` and pick with `-i`.
- "Copy" writes the revoke command (`tccutil reset Accessibility` plus the identifier) to the clipboard and shows the toast "Copied"; it never runs it. Do not run `tccutil` for the release identifier. The command is hidden when the identifier equals the release one.
- Radios "Real", "Off", "On": positional `-i` follows DOM order (Accessibility first). Confirm the order in `tree` before relying on the index.
- A simulation ends on relaunch, on pressing "Allow Accessibility", or on "Reset simulations". Always reset at the end of a verification step so later checks see real states.
- The panel loads asynchronously; before it loads it says "Loading…".

## Not reachable

- macOS itself: the real Accessibility and Automation grants, the System Settings pane that "Open Accessibility settings" opens, and the permission dialogs are native; no driver reaches them, and these tools exist precisely so they need not be touched.
- The clipboard contents after "Copy" (only the toast text "Copied", which is short-lived).
- Any of this on Windows: the section is not rendered there, and no `rize` command exposes it.
