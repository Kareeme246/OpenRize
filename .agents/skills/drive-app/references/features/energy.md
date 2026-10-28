---
id: energy
title: Battery and energy
summary: A background sampler records OpenRize's own CPU time and energy every 15 seconds, and Settings shows its energy impact, battery use, AI share and a resettable benchmark.
surfaces: [window, background]
routes: []
code: [src/components/BatteryEnergyMonitor.tsx, src/hooks/useEnergy.ts, src-tauri/src/energy.rs, src-tauri/src/energy]
rize: []
reach-macos: partial
reach-windows: none
reach-rize: none
verified: never
---

# Battery and energy

OpenRize measures its own footprint. A sampler thread reads the process CPU time and energy counters (macOS `proc_pid_rusage`, plus the on-device ML sidecar process when it runs) and the battery state every 15 seconds, classifies the power as low (under 0.5 W), medium (under 2 W) or high, tags samples taken while AI classification was busy, and stores each one in the `energy_samples` table of the session database. Settings > Advanced settings > Battery & Energy shows the current impact with its wattage, battery consumed, the share of energy spent on AI classification versus ambient tracking, baseline and peak power, total energy, CPU time and sample count over the last 24 hours, 7 days or 30 days, and a "Reset benchmark…" action that clears the history. Windows has no per-process energy counter, so the monitor is not built or shown there.

## Sub-features

- sampler: the 15 second background sampling into `energy_samples`, emitting `energy-changed` to the window after each sample.
- impact-summary: the "Current energy impact" row with a Low, Medium or High badge and the watts figure, and the "Battery consumed" row (on battery or connected to power).
- breakdown: the AI versus ambient stacked bar and the stat cards Baseline, AI Energy Share, Total Energy and Samples.
- range: the 24h, 7d and 30d choice for the window the numbers cover.
- reset: "Reset benchmark…" with a confirm dialog "Reset energy telemetry?" and the toast "Energy benchmark reset".

## How to get to it

Settings (sidebar "Settings" or `cmd+comma`), then the nav group "Advanced settings" (a disclosure button that expands to show "AI" and "Battery & Energy"), then "Battery & Energy". The section itself is headed "Advanced settings". Only macOS draws it: `HAS_ENERGY_MONITOR` is false on Windows, and Windows never starts the sampler.

## Driving it

A fresh session may open on first-run onboarding; dismiss it first (see [onboarding.md](onboarding.md)).

```bash
scripts/app-drive.sh start --json
scripts/app-drive.sh key cmd+comma
scripts/app-drive.sh click "Advanced settings"
scripts/app-drive.sh click "Battery & Energy"
scripts/app-drive.sh wait "Current energy impact"
scripts/app-drive.sh tree
```

The first sample is written about 15 seconds after launch; before it the page shows "Measuring energy impact…" and "Reset benchmark…" is disabled. Wait for samples, then change the range and reset:

```bash
scripts/app-drive.sh wait "Battery consumed" 60
scripts/app-drive.sh click -r AXRadioButton -i 0 "24h"
scripts/app-drive.sh wait "Measured over the last 24 hours"
scripts/app-drive.sh click -r AXRadioButton -i 0 "7d"
scripts/app-drive.sh wait "Measured over the last 7 days"
scripts/app-drive.sh click "Reset benchmark…"
scripts/app-drive.sh wait "Reset energy telemetry?"
scripts/app-drive.sh click "Reset telemetry"
scripts/app-drive.sh wait "Energy benchmark reset"
```

To abandon the reset instead, click "Cancel" in the dialog. The "7d" and "30d" radios also exist in the AI metrics range control on the same Settings page; run `tree`, and pick the one in the Battery & Energy block with `-i` (the energy control comes after the AI one in document order if the AI section is above it, which `tree` confirms).

Read the backend proof straight from the session database (`appData` in the `start --json` output):

```bash
sqlite3 "$HOME/Library/Application Support/com.offlinestudios.openrize.drive20481/activity.db" "select count(*), max(impact_level) from energy_samples;"
```

Replace `drive20481` with the session's identifier from `start --json`. There is no `rize` command for energy yet.

## What proves it works

- Section present: `wait "Current energy impact"` and the rows "Battery consumed", "Energy telemetry & benchmarks" and the stat card texts "Baseline", "AI Energy Share", "Total Energy", "Samples" in `tree`.
- Sampler running (required for a PASS of the sampler): the `Samples` card count is above 0 after about 30 seconds, and `select count(*) from energy_samples` is at least 1 in the session database and grows by roughly four per minute. The summary text in the first row stops saying "Measuring energy impact…".
- Range: the line "Measured over the last 24 hours" or "Measured over the last N days" follows the selected radio.
- Reset: after confirming, the toast "Energy benchmark reset" appears, `energy_samples` has fewer rows (new samples keep arriving every 15 seconds, so expect 0 or 1), the `Samples` card shows a small number and "Reset benchmark…" disables again while it is 0. Both the toast and the row count are required.
- Cancel leaves the count unchanged.
- `shot` for the stacked bar and cards.

## Gotchas

- Values are real measurements of the session's own process, not controllable. Do not assert on watts, impact level or percentages; assert on presence, counts and state changes.
- A Mac plugged in shows "Connected to power (AC)" under "Battery consumed"; a battery-only line ("Running on battery (NN%)") needs a laptop on battery, which a driven session cannot arrange.
- The dialog is destructive-toned: "Cancel" is the primary button, "Reset telemetry" the secondary one. Match by name, not position.
- "Reset benchmark…" has a trailing ellipsis character (`…`) in its name; type it exactly.
- Samples are written to the shared session database file; a `sqlite3` reader may briefly see it locked. Retry.
- The first row of the page changes from "Loading energy & battery telemetry…" to the summary as soon as the first request returns; "No energy telemetry recorded yet." appears only if the request itself failed.

## Not reachable

- On Windows there is nothing: no sampler, no Settings section.
- Real battery transitions (unplugging, charge level, Low Power Mode) and the OS energy counters themselves.
- Energy history through the CLI: a command for it is planned (M2) but not shipped, so `rize` reads nothing from here.
- The accuracy of the numbers: only their presence, counts and the reset can be checked.
