This is a TAURI desktop app project. Because of this there are a few considerations to ensure that code quality remains high in this codebase.

Never directly edit `CLAUDE.md` - it only imports this file; make all agent-instruction changes here in `AGENTS.md`.

- This project uses typescript on the frontend. Always properly type variables and never use the "any" type to get around linter warnings.
- Subscribe to Tauri events with `useTauriEvent` (`src/hooks/useTauriEvent.ts`), not a raw `listen` in an effect; its docs explain the stale-listener and unlisten races it handles.
- It is imperative to always ask the user whether or not a feature will live in the src-tauri (rust backend) or src (react frontend) or a combination of both.
- It is very important that when writing code to ALWAYS ground decisions in the
[tauri documentation](https://docs.rs/tauri/2.11.5/tauri) and Rust language documentation.
When it comes to React frontend code, you can generally trust your instincts and don't need to go to documentation unless the problem is difficult or user requests you to do so.

## Verifying changes

Before calling a task done, run:

```
pnpm fix
pnpm verify
```

`pnpm fix` applies every safe auto-fix available (Biome `--write`, `cargo
fmt`, `cargo clippy --fix` for machine-applicable lints) and `pnpm verify` is
the single canonical check — it runs Biome (lint + format check) and `tsc
--noEmit` on the frontend, then `cargo fmt --check` and `cargo clippy -- -D
warnings` on the backend. Running `fix` first means the easy, mechanical
stuff (formatting, trivial lints) never shows up as a `verify` failure to
puzzle over — `verify` failing should mean something actually needs a
judgment call. Do not invent ad hoc `biome`/`cargo fmt`/`cargo clippy`
invocations of your own; use these scripts so checks stay consistent across
agents and humans.

Do not run `cargo build`, `cargo check`, or `pnpm build` speculatively "just
to check" — `pnpm verify` already covers correctness. Reserve full builds for
when release/bundle behavior specifically needs testing.

Do not add test frameworks, CI workflows, or git hooks on your own
initiative. Verification for this project is deliberately just these scripts
(`pnpm verify`, `pnpm dev:screenshot`, and `pnpm dev:drive` below) — that
is a decision, not an oversight.

## On-device AI sidecar

AI categorization lives in `src-tauri/src/ai/` (module docs in `ai/mod.rs`
explain the tiers). The ML calls go to a Swift sidecar in `src-tauri/swift/`
that `src-tauri/build.rs` compiles with `swift build` on macOS and ships as a
Tauri `externalBin` (`tauri.macos.conf.json`), so every Rust build needs
Xcode 26+ command line tools. Bump `protocolVersion` in `Entry.swift` and
`PROTOCOL_VERSION` in `ai/sidecar.rs` together when the wire format changes.

## Visually verifying UI changes

Tauri has no supported WebDriver backend for macOS (only Linux/webkit2gtk and
Windows/Edge are supported), so there is no Playwright-style automated driver
for the real app on this machine. To actually see a change running, use:

```
pnpm dev:screenshot
```

This launches an isolated `tauri dev` instance on a randomly chosen port with
its own Tauri app `identifier` (so its app-data storage never collides with
the real app data or with another agent's concurrent run), waits for
the window, repositions it on-screen, screenshots it, tears the whole process
tree down, and prints the PNG path — read that file to look at the result.

- Do not run `pnpm tauri dev` directly to check a UI change — it uses a fixed
  port and identifier and will collide with another agent's instance or with
  a real running copy of the app. Always go through `pnpm dev:screenshot`.
- Both scripts run `tauri dev --no-watch`: the frontend hot-reloads, but a
  Rust change only shows up in a fresh instance (`pnpm dev:drive stop`, then
  `start`).
- Do not write Playwright/WebDriver/E2E test suites to drive the app window —
  see the WebDriver limitation above; such a test would not exercise the real
  Tauri backend on this platform.
- macOS only. If you are not on macOS, state that visual verification isn't
  available in this environment rather than attempting a workaround.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
