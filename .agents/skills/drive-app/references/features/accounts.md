---
id: accounts
title: Accounts
summary: Keep separate sets of time, projects, clients, invoices and stopwatches under local named accounts, and switch between them or back to No account.
surfaces: [window]
routes: []
code: [src/components/AccountSwitcher.tsx, src/components/AddAccountSheet.tsx, src/components/MissingAccountDialog.tsx, src/components/SwitchingOverlay.tsx, src/components/PausedOnSwitchBanner.tsx, src/pages/settings/AccountsSetting.tsx, src/hooks/useAccounts.tsx, src/lib/accounts.ts, src-tauri/src/accounts.rs, src-tauri/core/src/accounts.rs]
rize: []
reach-macos: partial
reach-windows: partial
reach-rize: none
verified: never
---

# Accounts

An account is a name and its own data folder: no password, no encryption, no sync. The app's data directory itself is "No account"; accounts sit beside it under an accounts folder, and nothing exists until the first one is added. An account switcher in the top bar lists accounts, adds one ("Add account..."), links to "Manage accounts..." (Settings > Data & storage) and signs out back to No account. Choosing another account, adding one or signing out restarts the app into it after at least three seconds behind a "Switching to NAME..." screen; running stopwatches and a live break are paused first and stay with the account they belong to. When the user returns, a banner offers to resume them. Adding an account from No account that already has data asks whether to move that data into it. If the remembered account's folder has gone missing, the app opens with no account and shows a one-time dialog. Settings > Data & storage lists the accounts with rename, delete (to the Trash) and add. Settings itself is covered in [settings.md](settings.md); stopwatches in [stopwatches.md](stopwatches.md); breaks in [breaks.md](breaks.md).

## Sub-features

- switcher: the top-bar "Account: NAME" button and its menu (account rows, "Add account...", "Manage accounts...", "Sign out").
- add-account: the "Add an account" sheet, with the "Your existing data" choice (move it in, or leave it with no account).
- switch-and-restart: the pause notice ("Switch to NAME?" with "Pause and switch" or "Pause and sign out"), the full-window "Switching to NAME..." / "Signing out..." screen and the restart.
- paused-banner: "Paused when you switched accounts at TIME:" with "Resume" or "Resume both" and "Keep paused".
- manage: Settings > Data & storage > Accounts, with "Open now", "Rename...", "Delete...", "Remove" and "Add account...".
- missing-account: the dialog "NAME's data is missing" with "Remove NAME from the list".
- read-only: accounts written by a newer release are shown but their controls are disabled.

## How to get to it

Top bar, the button right of the sidebar toggle ("Account: No account" on a fresh install). Settings > Data & storage (Cmd+, then the "Data & storage" nav button) for the management list. There is no route of its own.

## Driving it

A fresh session is an empty install and may open on first-run onboarding (see [onboarding.md](onboarding.md)). Start with data worth moving, then add an account through the window:

```bash
scripts/app-drive.sh start --json
scripts/app-drive.sh rize -- projects add "Client work"
scripts/app-drive.sh rize -- timers new "Reading"
scripts/app-drive.sh rize -- timers start "Reading"
scripts/app-drive.sh click "Account: No account"
scripts/app-drive.sh wait "Keep separate data with accounts"
scripts/app-drive.sh click "Add account"
scripts/app-drive.sh wait "Add an account"
scripts/app-drive.sh fill "Name" "Personal"
scripts/app-drive.sh tree
scripts/app-drive.sh click "Add and move data"
scripts/app-drive.sh wait "Switching to Personal..."
```

The sheet shows "Your existing data" only when No account has data; its two radios start with "Move it into Personal" and "Leave it with no account" (their names continue with a description, so click by that start). Without existing data the submit button is "Add account". The restart takes about three seconds; afterwards re-read the window:

```bash
scripts/app-drive.sh list
scripts/app-drive.sh wait "Account: Personal"
scripts/app-drive.sh tree
scripts/app-drive.sh shot --out /tmp/openrize-proof/accounts-switched.png
```

Switch, sign out and manage:

```bash
scripts/app-drive.sh click "Account: Personal"
scripts/app-drive.sh click "Add account..."
scripts/app-drive.sh key escape
scripts/app-drive.sh click "Account: Personal"
scripts/app-drive.sh click "Sign out"
scripts/app-drive.sh wait "Account: No account"
scripts/app-drive.sh click "Account: No account"
scripts/app-drive.sh click "Personal"
scripts/app-drive.sh click "Pause and switch"
scripts/app-drive.sh wait "Paused when you switched accounts"
scripts/app-drive.sh click "Resume"
scripts/app-drive.sh key cmd+comma
scripts/app-drive.sh click "Data & storage"
scripts/app-drive.sh click "Rename..."
scripts/app-drive.sh fill "Name" "Personal 2"
scripts/app-drive.sh click "Rename"
scripts/app-drive.sh click "Delete..."
scripts/app-drive.sh click "Cancel"
```

"Pause and switch" appears only when a stopwatch is running or a break is on; otherwise the switch starts at once. "Delete..." is disabled for the open account ("Open now") and moves another account's folder to the Trash after a dialog titled "Delete NAME?" whose confirm button is "Delete NAME". Read the stored state:

```bash
cat "$HOME/Library/Application Support/com.offlinestudios.openrize.drive20481/accounts.json"
ls "$HOME/Library/Application Support/com.offlinestudios.openrize.drive20481/accounts"
sqlite3 "$HOME/Library/Application Support/com.offlinestudios.openrize.drive20481/activity.db" "select count(*) from projects;"
scripts/app-drive.sh rize -- --json paths
```

Replace the digits with your session's (from `start --json`, `identifier` and `appData`).

## What proves it works

- Switcher state: the top-bar button's name `Account: NAME` in `tree` is the open account; the Settings list marks it "Open now". Both are required for a PASS of a switch.
- Switching screen: `wait "Switching to Personal..."` (or "Signing out...") proves the request was accepted; it stays up at least three seconds.
- After the restart, the data is the account's: the projects page of the new account is empty unless data was moved, or shows "Client work" when the move choice was used. A one-time toast "Your data is now in Personal. No account starts empty." appears after a move (short-lived, so confirm with data). Back in No account (Sign out), the old data is there when it was left behind, or empty when it was moved.
- On disk: `accounts.json` lists the account (`name`, `id`) and `current` is its id while it is open and null after Sign out; the account's data lives in `accounts/ID/activity.db` beside the base `activity.db`. A moved account has the original rows in `accounts/ID/activity.db` (`select count(*) from projects` is 1) and the base database is no longer the live one.
- Paused work: after a switch with a running stopwatch, the destination account shows no running stopwatch; returning to the original account shows the banner "Paused when you switched accounts at TIME: Reading." with "Resume". "Resume" starts it again and the banner goes away; "Keep paused" dismisses the banner and leaves it paused.
- Rename/delete: the list row shows the new name; the deleted account's folder is gone from `accounts/` (it moved to the Trash) and `accounts.json` no longer lists it. A duplicate name shows an inline message "There is already an account called NAME.".
- Visual: `shot` for the switcher menu, tiles and the overlay.

## Gotchas

- The driver's `rize` passthrough runs with `--data-dir` set to the session's base data folder. That folder is the open data only while No account is selected. With an account open, `scripts/app-drive.sh rize -- ...` still reads and writes the base (No account) folder, not the open account's data, so seeding or reading through it does not touch what the window shows. Seed before switching, or read the account's own database with `sqlite3` on `accounts/ID/activity.db`. Do not add a second `--data-dir`.
- A switch relaunches the app process. Whether `tauri dev`, the dev server and the driver's process lookup survive that restart is unverified; run `scripts/app-drive.sh list` and `wait` after the three seconds, and treat a lost session as a finding to report, not a pass. The overlay's own 20 second patience timeout shows the toast "OpenRize did not restart. Try switching again." when no restart arrives.
- Account names are dynamic. The switcher button name is `Account: NAME`; account rows in the menu are named by the account name; use `tree` after opening the menu.
- "Add account" (the empty-state button and the submit button) and "Add account..." (the menu item and Settings button) differ by the trailing dots; the exact-first matching keeps them apart.
- "Name" is used by the Add and Rename sheets only while one is open.
- Switch pauses all running stopwatches, including ones started during the three second wait.
- Menu rows are `menuitem` and `menuitemradio` elements. The macOS tree may print them under roles other than Button; if `click` reports no actionable element named "Sign out", run `tree` and use `-r` with the role shown.
- Isolation: an account lives inside the session's identifier-scoped data folder, so `stop` deletes it; Trash for a deleted account goes to the real user Trash. Prefer not to delete accounts with `Delete...` in a driven run; use `Remove` for a missing one.
- Names are at most 40 characters and unique case-insensitively.

## Not reachable

- The restart itself and the Trash move are OS-level; the driver sees only the window before and after.
- The one-time "just adopted" toast and the missing-account dialog (`NAME's data is missing`) need a data folder deleted behind the app's back and a relaunch; that can be staged on disk but the relaunch path is untested through the driver.
- No `rize` command switches or lists accounts yet (an account command is Planned in COMMANDS.md); `rize paths` reports the open account only when pointed at an account folder, which the driver passthrough does not do.
