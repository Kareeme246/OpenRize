---
id: updates
title: Updates
summary: The in-app updater checks GitHub Releases on a schedule, shows an "Update available" button and what is new in Settings, and installs and restarts only when asked.
surfaces: [window, background]
routes: []
code: [src/hooks/useUpdates.ts, src-tauri/src/updater.rs]
rize: []
reach-macos: partial
reach-windows: partial
reach-rize: none
verified: never
---

# Updates

OpenRize updates itself through `tauri-plugin-updater`. In a release build a background scheduler checks the newest GitHub Release 10 seconds after launch, then hourly, and again when a window regains focus if the last check is at least 5 minutes old. A found update shows as a green "Update available" button in the sidebar and as a card in Settings > Overview with the notes of every release between the installed and the offered version; "Install & restart" downloads it, verifies its signature and restarts into it. Nothing installs without that click. Publishing a release ships it to every user, and the check is a real network call, so this feature is the one where a driven instance must stay away from the real thing. A driven instance is always a debug build, which never checks the network; the UI can still be exercised with a simulated update.

## Sub-features

- scheduled-check: the launch, hourly and focus checks (release builds only).
- manual-check: the Settings button "Check for updates".
- update-card: Settings > Overview > Updates, with the row "OpenRize updates", its summary line and the "What's new" block listing the release notes.
- install: the button "Install & restart" that downloads, verifies and relaunches into the new version.
- sidebar-banner: the sidebar button "Update available", which opens Settings on the Updates card.
- simulated-update: a debug-only fake update (version 9.9.9) switched on by an environment variable.

## How to get to it

Settings (sidebar "Settings" or `cmd+comma`) opens on Overview, whose second group is "Updates". When an update exists, the sidebar bottom shows "Update available", which jumps to the same card. No keyboard shortcut.

## Driving it

A fresh session may open on first-run onboarding; dismiss it first (see [onboarding.md](onboarding.md)).

What can be done safely in a driven (debug) instance: look at the card and press "Check for updates". In a debug build the check does no network call; it sets the card's error to "Development builds don't check for updates."

```bash
scripts/app-drive.sh start
scripts/app-drive.sh key cmd+comma
scripts/app-drive.sh wait "OpenRize updates"
scripts/app-drive.sh tree
scripts/app-drive.sh click "Check for updates"
scripts/app-drive.sh wait "Development builds don't check for updates."
scripts/app-drive.sh shot --out /tmp/openrize-proof/updates-card.png
```

To see the available-update state, start the session with the simulation variable. The shell exports it into the app's environment:

```bash
OPENRIZE_SIMULATE_UPDATE=1 scripts/app-drive.sh start
scripts/app-drive.sh wait "Update available"
scripts/app-drive.sh click "Update available"
scripts/app-drive.sh wait "What's new"
scripts/app-drive.sh tree
```

The card then shows "Version 9.9.9 is available. You have <current version>." and a "What's new" list with 9.9.9 and 9.9.8 (the simulated notes "Simulated release notes for the in-app updater", "A second simulated line..." and "An older simulated release"). The button is "Install & restart". The simulation sets no downloadable payload, so pressing it fails with "There's no update to install." in the card; still, do not press it as a habit, because in a release build the same button downloads from the internet and restarts the app.

## What proves it works

- Idle debug build (required for a PASS of the plain card): the row "OpenRize updates" shows "Up to date · <version>" where the version equals the app's version (`scripts/app-drive.sh rize -- --json app status` has `data.appVersion` with the same number), and the button is "Check for updates", enabled.
- After pressing "Check for updates": the red error line "Development builds don't check for updates." appears, no network request was made, and the summary line is unchanged.
- Simulated update (required for a PASS of the available state): the sidebar button "Update available" exists, the summary reads "Version 9.9.9 is available. You have <version>.", the button changes to "Install & restart", and the block "What's new" lists two release versions with notes, with markdown headings and bold markers stripped and list items shown as bullets.
- `shot` for the card and the sidebar banner.
- Not provable here: that a real release downloads, verifies and installs. Only a human with a release build and a published newer release can do that.

## Gotchas

- Never press "Install & restart" outside the simulation, and never run a driven session from a release build: it would download a real release and restart the app, replacing the executable the session depends on. A driven session is a debug build, so this cannot happen by accident, but the label is the same in the simulation, so keep the rule.
- The simulation exists only in a debug build (`cfg!(debug_assertions)`) and only when `OPENRIZE_SIMULATE_UPDATE` is set when the app starts; it cannot be switched on later.
- Automatic checks only log their failures; only a manual check or install shows an error in the card, so an offline laptop does not show a stale error.
- The sidebar banner text is "Update available" (its `aria-label`); the card button changes between "Install & restart" and "Check for updates" depending on whether an update exists, so wait for the one you expect.
- While a check or download runs the button is disabled and the summary reads "Checking for updates…" or "Downloading N%…", then "Installing - OpenRize will restart". These are short; `wait` on a stable string instead.
- Update commands in the CLI are planned (M2); none is shipped, so nothing in `rize` touches the updater. Use `rize app status` only to read the app version.
- Platforms the release feed does not carry (anything other than Apple Silicon macOS and 64-bit Windows) report "Builds from source don't update in place. Pull and rebuild to update." instead.
- Settings > Overview also holds the Permissions cards on macOS, above this card; see [settings.md](settings.md).

## Not reachable

- The real network check against GitHub Releases, signature verification, download progress, installation and the restart into a new version: debug builds never check, and no tool here should trigger a real install.
- The GitHub release notes fetch for versions between the installed and the offered one.
- The macOS and Windows installer behaviour after the app quits.
- Any CLI path: update commands are planned but not shipped.
