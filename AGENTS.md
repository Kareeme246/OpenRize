This is a TAURI desktop app project. Because of this there are a few considerations to ensure that code quality remains high in this codebase.

- This project uses typescript on the frontend. Always properly type variables and never use the "any" type to get around linter warnings.
- It is imperative to always ask the user whether or not a feature will live in the src-tauri (rust backend) or src (react frontend) or a combination of both.
- It is very important that when writing code to ALWAYS ground decisions in the 
[tauri documentation](https://docs.rs/tauri/2.11.5/tauri) and Rust language documentation.
When it comes to React frontend code, you can generally trust your instincts and don't need to go to documentation unless the problem is difficult or user requests you to do so.

## Verifying changes

Before calling a task done, run:

```
pnpm verify
```

This is the single canonical check — it runs Biome (lint + format check) and
`tsc --noEmit` on the frontend, then `cargo fmt --check` and
`cargo clippy -- -D warnings` on the backend. Do not invent ad hoc
`biome`/`cargo fmt`/`cargo clippy` invocations of your own; use this script so
checks stay consistent across agents and humans. `pnpm fix` runs the same
checks but applies safe auto-fixes (Biome `--write`, `cargo fmt`) instead of
just reporting them.

Do not run `cargo build`, `cargo check`, or `pnpm build` speculatively "just
to check" — `pnpm verify` already covers correctness. Reserve full builds for
when release/bundle behavior specifically needs testing.

Do not add test frameworks, CI workflows, or git hooks on your own
initiative. Verification for this project is deliberately just these two
scripts (`pnpm verify` and `pnpm dev:screenshot` below) — that is a decision,
not an oversight.

## Visually verifying UI changes

Tauri has no supported WebDriver backend for macOS (only Linux/webkit2gtk and
Windows/Edge are supported), so there is no Playwright-style automated driver
for the real app on this machine. To actually see a change running, use:

```
pnpm dev:screenshot
```

This launches an isolated `tauri dev` instance on a randomly chosen port with
its own Tauri app `identifier` (so its app-data storage never collides with
the real `trackers.json` or with another agent's concurrent run), waits for
the window, repositions it on-screen, screenshots it, tears the whole process
tree down, and prints the PNG path — read that file to look at the result.

- Do not run `pnpm tauri dev` directly to check a UI change — it uses a fixed
  port and identifier and will collide with another agent's instance or with
  a real running copy of the app. Always go through `pnpm dev:screenshot`.
- Do not write Playwright/WebDriver/E2E test suites to drive the app window —
  see the WebDriver limitation above; such a test would not exercise the real
  Tauri backend on this platform.
- macOS only. If you are not on macOS, state that visual verification isn't
  available in this environment rather than attempting a workaround.
