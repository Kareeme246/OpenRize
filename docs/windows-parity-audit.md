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

## Needs a product decision
- [?] PLT-1 / PAR-1 Windows AI is rules-only (no learning/kNN). Pure-Rust lexical tier?
- [?] PLT-4 Databases live in Roaming AppData; move to Local with a migration?
- [?] CAP-2 `bundle_id` is just the exe name (java, python, PWAs merge). Changing it splits history.
- [?] CAP-7 Modern Standby never sends PBT_APMSUSPEND; break engine misses sleep.
- [?] CAP-8 Incognito/private windows recorded on both platforms.
- [?] PLT-10 / PAR-5 Re-run CLI PATH install at every launch on Windows (macOS relinks each launch).
- [?] PLT-11 NSIS installer is not Authenticode-signed (SmartScreen warning).

## Deferred (low value or unverified)
BRK-4 tray bitmap sizing, BRK-7 Pulse foreground-lock guard, BRK-9 auto-hide taskbar inset,
PLT-5 identical permissions files, PLT-9 pipe squatting, PAR-7 ARM64 updater copy,
PAR-8 Windows installer test, macOS osascript timeout (CAP-12 note).
