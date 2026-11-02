# Windows parity audit — 2026-11-02 (v0.9.46)

Scope: UI and backend (capture, breaks, notifications, tray, Pulse, platform modules, CLI/IPC)
parity between macOS and Windows, plus Windows code quality against the platform-module pattern
in `AGENTS.md`. Status: `[ ]` open, `[x]` done, `[?]` needs a product decision, `[~]` closed without a change.

Already solid (no action): reminder panel placement/DPI/taskbar edges, Pulse flyout anchoring,
WTS lock/sleep/display-off, named-pipe DACL and same-user checks, PATH install, login item,
permissions/onboarding gating, `useTauriEvent` usage, font stacks, shortcut labels via `platform.ts`.

## Batch A1 — capture/windows.rs (tracking accuracy)
- [x] CAP-1 (high) Unreadable foreground process (UAC, LogonUI, PPL) returns `None`, so time stays
  on the previous app. Fall back to the Toolhelp exe name.
- [x] CAP-3 Never record ApplicationFrameHost; match `Windows.UI.Core.CoreWindow` child, else keep last good sample.
- [x] BRK-1 / CAP-6 `watching()` needs display-required even for fullscreen/presentation; let
  QUNS busy/D3D/presentation suppress reminders on their own.
- [x] CAP-10 / PAR-3 Opt out of EcoQoS execution-speed throttling in `AppNapAssertion`.
- [x] CAP-11 Cache `process_parents` snapshot for a few seconds.
- [x] CAP-12 Mid-file `use`, missing SAFETY comment on `GetWindowTextW`.

## Batch A2 — capture/windows/address_bar.rs (browser URLs)
- [x] CAP-4 Chromium New Tab keeps previous site's URL while omnibox is focused.
- [x] CAP-5 URL lags title ~2 s after tab switch, splitting segments.
- [x] CAP-9 Single-label intranet hosts (`http://jira/`, `server:8080`) dropped.

## Batch B — panels, tray, chime, lifecycle (breaks/surface.rs, pulse.rs, tray.rs, lib.rs, breaks/mod.rs, login_item, updater.rs)
- [x] PLT-3 / BRK-11 Move inline NSPanel code (duplicated in surface.rs and pulse.rs), Pulse
  placement, tray tint and chime into platform modules.
- [x] BRK-2 Re-assert topmost after each reminder show on Windows.
- [x] BRK-3 Re-tint tray icon when Windows light/dark theme changes.
- [x] BRK-5 Open tray menu on right-button up.
- [x] BRK-6 Double-click tray icon opens the main window.
- [x] BRK-8 `CloseRequested` on the reminder window must not exit the app.
- [x] BRK-10 Drop dead `set_visible_on_all_workspaces` stubs on Windows (kept on Linux, where it works).
- [x] BRK-12 Chime silent when the Windows sound scheme is "No sounds"; fall back.
- [x] PLT-6 `login_item::refresh` exists only on Windows; give every platform the same items.
- [x] PLT-7 Shutdown flush duplicated in updater.rs and lib.rs; extract one helper.

## Batch C — frontend (src/)
- [x] UI-1 / PLT-8 "Trash" wording → Recycle Bin on Windows (TS + Rust error in accounts.rs).
- [x] UI-2 TopBar drag region active under the native caption on Windows.
- [x] UI-3 Primary modifier accepts the other platform's key; use `hasPrimaryModifier`.
- [x] UI-4 Compact labels show ⌫ / ↵ on Windows.
- [x] UI-5 Bare `PLATFORM === "macos"` checks; move to `Record<Platform, T>`. (`HAS_DEV_TOOLS` already used a Record; left)
- [x] UI-6 Latent macOS copy (unsupportedOS reason, "Quit it from the menu bar").
- [x] UI-7 Settings overview promises permissions that Windows never shows.
- [x] UI-8 Scrollbar styles diverge (WebView2 ignores `::-webkit-scrollbar`).
- [x] UI-9 Ctrl+Y redo on Windows.
- [~] UI-10 macOS-only window keys in base `tauri.conf.json` → `tauri.macos.conf.json`. Skipped:
  `app.windows` is an array, which a platform patch replaces whole; Windows ignores the keys.

## Batch D — power, energy, CLI
- [x] PLT-2 ai/power.rs duplicates energy module inline; route through `energy::*`, neutral strings.
- [x] PLT-7b `energy::spawn_sampler` `cfg!(windows)` early return → per-platform const.
- [~] PAR-6 `rize` version guard runs only on macOS; read file version on Windows. Not applicable: Windows installers postdate the 0.8.10 floor.

## Decided 2026-11-02
- [ ] PLT-1 / PAR-1 Windows AI is rules-only. Superseded by the local-models design
  (`docs/design/local-models.md`).
- [x] PLT-4 All data and settings in Local AppData with a crash-safe one-time migration (#164, 0.9.48).
- [x] CAP-2 Per-app identity for web apps and shared runtime hosts; ordinary apps keep their ids
  (#166, 0.9.50).
- [x] CAP-8 Private windows are tracked; their titles/URLs/domains are erased once the entry is
  classified, or after a day, on both platforms (0.9.51). Edge InPrivate and Zen/Firefox private
  windows are detected from the title; Chrome-family incognito is unverified; Safari has no signal.
- [x] PLT-10 / PAR-5 rize's folder is re-ensured on the user PATH at every launch of an installed
  release build (#165, 0.9.49).
- [ ] Windows wastes work on `embed` jobs: with no embedding model each one fails its three
  attempts (`ai/worker.rs` `embed_only`). Fixed by the local-models design (phase 1 stops queueing
  them without an embedder).

## Open bugs (known, not being fixed now)
- CAP-7 Modern Standby laptops never send `PBT_APMSUSPEND`, so sleep is not seen as a suspend:
  the break engine's sleep handling (#117) and the explicit segment close on suspend do not run.
  Only the 10 s gap rule and the display-off flag protect the data. Candidate fix:
  `PowerRegisterSuspendResumeNotification` or Modern Standby's display/presence notifications.
- PLT-11 The Windows installer is not Authenticode-signed, so SmartScreen warns on first install.
  Accepted for now; the in-app updater is still protected by the minisign signature.

## Windows drive tooling (found while driving 0.9.47)
- [ ] An app restart inside a drive session (account switch, update) ends `tauri dev`, which takes
  the Vite server with it: the restarted app keeps the CDP port but loads nothing, and the driver
  reports the session dead.
- [ ] `stop` does not find the restarted app (its command line carries no identifier or port), so
  it survives, holds the session's data folder, and the delete fails with EPERM.
- [x] Skills lint fails on a Windows checkout (it now skips build output): the macOS skill references
  `src-tauri/target/debug/openrize`, which is `openrize.exe` here and absent before a build.

## Deferred (low value or unverified)
BRK-4 tray bitmap sizing, BRK-7 Pulse foreground-lock guard, BRK-9 auto-hide taskbar inset,
PLT-5 identical permissions files, PLT-9 pipe squatting, PAR-7 ARM64 updater copy,
PAR-8 Windows installer test, macOS osascript timeout (CAP-12 note).
