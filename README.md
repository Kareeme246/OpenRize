# OpenRize

**A real answer to "where did my day go?" — automatic, private, local-first time tracking for your Mac. Free, open-source, no account, no screenshots, no keystrokes.**

[![Status](https://img.shields.io/badge/status-early%20alpha-E8734A?style=for-the-badge)](#where-it-stands)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-2aea83?style=for-the-badge)](LICENSE)
[![Built with Tauri 2](https://img.shields.io/badge/Built%20with-Tauri%202-24C8DB?style=for-the-badge)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-backend-000000?style=for-the-badge&logo=rust)](https://www.rust-lang.org)

<div align="center">
<img src="app-icon.png" width="140" alt="OpenRize — the Chrono Bloom app icon" />
</div>

OpenRize is the open, local-first answer to [Rize.io](https://rize.io): time tracking that
watches what you're actually working on and sorts it into clients, projects, and tasks — without
logging a single keystroke, taking a single screenshot, or shipping a byte of your data anywhere.


---

## ⚠️ Early days — read this first

OpenRize is under active development and very early.
You can see some of the architecture underneath, and the roadmap in [`docs/rize-feature-research.md`](docs/rize-feature-research.md).

## Where it stands

**Platforms** — built and tested on macOS. Tauri itself compiles for all three, but nothing outside
macOS has been verified, and the tray glyphs are macOS template images.

| Platform | Supported |
| --- | --- |
| 🍎 macOS (Apple Silicon) | ✅ Built and tested |
| 🍎 macOS (Intel) | 🔶 Builds; untested |
| 🪟 Windows | ❌ Untested — tray glyphs are macOS-only template images |
| 🐧 Linux | ❌ Untested |

**Features** — one row per sidebar destination.

| Area | Status | What it is |
| --- | --- | --- |
| **Trackers** | ✅ Working | Manual stopwatches: name it, start it, watch the ring fill. |
| **Menu-bar tray** | ✅ Working | Idle/active glyph, live counts, pause any tracker without opening the window. |
| **Home** | 🚧 UI stub | Daily dashboard: time to review, hours tracked, today's breakdown. |
| **Sessions** | 🚧 UI stub | Day/week/month/year timeline of auto-captured activity blocks. |
| **Focus** | 🚧 UI stub | Deep-work sessions, auto-triggered by your focus rules, with a quality score. |
| **Meetings** | 🚧 UI stub | Calendar-derived meeting sessions and time-load metrics. |
| **Breaks** | 🚧 UI stub | Idle-triggered breaks, guided meditations, rest targets. |
| **Clients & Projects** | 🚧 UI stub | Client → Project → Task hierarchy, plus app/website tracking rules. |
| **Distraction Blocker** | 🚧 UI stub | Intervenes when you linger too long somewhere you've marked distracting. |
| **Dashboards & Reports** | 🚧 UI stub | Custom dashboards, profitability, utilization, timesheets. |
| **AI Agent** | 🚧 UI stub | Ask your time data questions in plain English; scheduled reports and routines. |
| **Integrations** | 🚧 UI stub | ClickUp, Linear, Asana, FreshBooks, QuickBooks, Xero, calendar, CSV, webhooks. |
| **Settings** | 🚧 UI stub | Every control rendered and labelled; none of them write anything yet. |
| **Automatic capture** | ❌ Not started | The core engine: reading the active window/app/URL in the background. |

Legend: ✅ ship it · 🚧 page exists, no engine behind it · ❌ nothing yet.

## Local-first

- **No account.** There is nothing to sign up for and nobody to sign up with.
- **No telemetry.** OpenRize makes no network calls. Not "anonymized" — none.
- **No keystroke logging, no screenshots, no screen recording.** Tracking is meant to be metadata
  about *what app you're in*, not a recording of what you typed.
- **No surveillance posture.** OpenRize is a personal/agency tool for billing your own honest hours,
  not a way to watch employees.
- Integrate local LLMS (MacOS)

## Under the hood

OpenRize is a Tauri 2 app: a Rust core with a React webview as its view.

**One rule shapes the design: Rust owns the truth.** Every IPC command returns the complete timer
list, so the frontend never has to guess at state it didn't just write, and the tray can change
state behind the window's back.

```text
┌──────────────────────────────────────────────────────────┐
│  Rust core (src-tauri)                                   │
│                                                          │
│   timers.rs ─── TimerStore: the single source of truth   │
│      │          clock policy, persistence, validation    │
│      ├── commands.rs ─── IPC surface (7 commands)        │
│      └── tray.rs ─── menu-bar glyph + menu               │
│              │                                           │
│              └─ refresh → emits "timers-changed" ─────┐  │
└───────────────────────────────────────────────────────┼──┘
                                                        │
┌───────────────────────────────────────────────────────▼──┐
│  React view (src)                                        │
│   useTimers.ts ─── adopts every Rust snapshot            │
│   pages/*.tsx ─── one file per sidebar destination       │
└──────────────────────────────────────────────────────────┘
```

Stack: [Tauri 2](https://tauri.app) · Rust · [React 19](https://react.dev) · TypeScript · [Tailwind CSS 4](https://tailwindcss.com) · [Vite](https://vite.dev)

## Building from source

There are no packaged releases yet, so building from source is the only way in. First install the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) (on macOS: Xcode command line tools
and Rust), plus Node and pnpm.

```sh
git clone git@github.com:Kareeme246/OpenRize.git
cd OpenRize
pnpm install
pnpm tauri dev          # run it
pnpm tauri build        # produce a release bundle
```

Backend tests (the timer store's clock, persistence and validation logic):

```sh
cd src-tauri && cargo test
```

## Contributing

Contributions are welcome — issues, focused PRs, or opinions about the roadmap.

Before you start, two things in this repo will save you time:

- **[`AGENTS.md`](AGENTS.md)** — project conventions. Notably: always agree on whether a feature
  lives in `src-tauri`, `src`, or both *before* writing code, and ground Tauri work in the
  [Tauri docs](https://docs.rs/tauri/2.11.5/tauri).
- **[`docs/rize-feature-research.md`](docs/rize-feature-research.md)** — every feature area scoped
  with what it does and where it likely belongs. Pick a section, and it's a task.

The build order it recommends (Sessions → Clients & Projects → Focus/Breaks/Blocker → Home →
Reports → Integrations) is a suggestion, not a commitment — say so in an issue if you'd rather
start elsewhere.

## License

**GNU Affero General Public License v3.0** — see [LICENSE](LICENSE).

AGPL-3.0 is deliberately strong copyleft: if you run a modified OpenRize as a network service, you
must offer its source to the people using it. Derivatives stay open, which is the point — the
tracking engine should never become something you can't audit.

---

Built on [Tauri](https://tauri.app), [React](https://react.dev), and [Tailwind CSS](https://tailwindcss.com).
