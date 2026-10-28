---
id: ai
title: AI categorization
summary: Suggest a category and project for each closed time entry from your rules, a personal model and Apple's on-device model, with calibrated confidence, auto-approval and learning from your reviews.
surfaces: [window, background]
routes: []
code: [src/components/AiEngineBanner.tsx, src/components/AiLearning.tsx, src/hooks/useAiStatus.ts, src/hooks/useAiMetrics.ts, src-tauri/src/ai, src-tauri/core/src/ai, src-tauri/swift]
rize: ["rize entries show", "rize settings get", "rize settings set"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# AI categorization

Closed time entries are queued and drained one at a time by a background worker. Each runs through three tiers and an arbiter blends them into a calibrated confidence per field (category and project): T0 your rules (a hit is certain), T1 your personal model (nearest approved entries plus a text classifier trained on them), and T2 Apple's on-device foundation model through a Swift sidecar. Corrections feed back at once; three consistent corrections for an app or site offer a rule; the personal models retrain in the background and the confidence curve is refit from accept and reject history. Nothing makes a network call. The engine is `full` (all three tiers), `fallback` (rules and personal model, because Apple Intelligence is off, still downloading, unsupported or the OS is older than macOS 26) or `rules` (the sidecar is missing or failed, as on Windows). The window shows it in Settings (Suggestions and AI sections), as a dismissible banner on the Calendar when Apple Intelligence could be turned on, and as suggestions in the review panel (see [review.md](review.md), [calendar.md](calendar.md)). Rules come from project and category hints (see [projects.md](projects.md), [categories.md](categories.md)).

## Sub-features

- engine-status: the "Suggestion engine" row ("Rules, your personal model, and Apple's on-device model", "Using rules and your personal model only. REASON.", or "Using your rules only: on-device ML isn't available in this build"), the "Apple Intelligence" row with Enabled, Not enabled or Downloading and the "Apple intelligence settings" button.
- fallback-banner: the Calendar banner "Apple Intelligence is off." / "The on-device model is still downloading." with "Turn on in System Settings" and "Dismiss".
- suggestion-settings: "What to suggest" (Category, Category + Project), "Auto-accept confident suggestions", the "Auto-accept threshold" slider with its exact-value field and live preview, and "Custom instructions".
- effectiveness: AI effectiveness (7d, 30d, 90d), Acceptance, Auto-approved and Changed cards, "Who decided" tier split and the calibration chart.
- personal-models: "Personal model" lines and "Retrain now" (queued, running, with a note).
- reset-learned: "Reset learned data" and its confirmation.
- classification: the worker itself - suggestions stored per entry with engine, confidence, signals and outcome, visible in the review panel and `rize entries show`.

## How to get to it

Settings (Cmd+, on macOS), then "Suggestions" for the preference controls, and "Advanced settings" > "AI" for engine, effectiveness, models and reset. The banner is on the Calendar page (see [calendar.md](calendar.md)). Suggestions on an entry appear in the review panel when an entry is selected.

## Driving it

A fresh session is an empty install and may open on first-run onboarding (see [onboarding.md](onboarding.md)). The engine status loads on its own; the first probe can show "Starting the on-device models..." for a moment.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh key cmd+comma
scripts/app-drive.sh wait "Preferences save automatically"
scripts/app-drive.sh click "Suggestions"
scripts/app-drive.sh tree
scripts/app-drive.sh click "Category"
scripts/app-drive.sh click "Category + Project"
scripts/app-drive.sh click "Auto-accept confident suggestions"
scripts/app-drive.sh click "Auto-accept confident suggestions"
scripts/app-drive.sh click "Advanced settings"
scripts/app-drive.sh click "AI"
scripts/app-drive.sh wait "AI effectiveness"
scripts/app-drive.sh click "30d"
scripts/app-drive.sh click "90d"
scripts/app-drive.sh tree
scripts/app-drive.sh shot --out /tmp/openrize-proof/ai-settings.png
```

"AI effectiveness" is the group heading inside the AI section; on a machine without the on-device model the section heading is "AI & suggestions". Threshold, retrain and reset:

```bash
scripts/app-drive.sh fill "Auto-accept threshold exact value" "90"
scripts/app-drive.sh key return
scripts/app-drive.sh click "Retrain now"
scripts/app-drive.sh click "Reset…"
scripts/app-drive.sh wait "Reset learned data?"
scripts/app-drive.sh click "Reset learned data"
```

The reset button is named with a single ellipsis character (U+2026), as in the command above. "Retrain now" is disabled when the sidecar is unavailable.

Read and set the same preferences, and read an entry's classification, through `rize`:

```bash
scripts/app-drive.sh rize -- --json settings get ai-suggest
scripts/app-drive.sh rize -- --json settings get auto-accept
scripts/app-drive.sh rize -- settings set auto-accept-percent 90
scripts/app-drive.sh rize -- --json entries list --period today --status pending
scripts/app-drive.sh rize -- --json entries show 3f9a1c2e
```

Replace `3f9a1c2e` with the last characters of an entry id from the list. The `entries show` JSON has `data.detail.suggestions` (one per field, with `engine`, `confidence`, `valueId`, `signals`, `alternatives`, `outcome`) and `data.detail.job` (`state`).

## What proves it works

- Engine status in the window is read from the live status; the summary text must match what the machine can do:
  - Apple Intelligence on macOS 26 or later: "Rules, your personal model, and Apple's on-device model" and the Apple Intelligence row says Enabled.
  - macOS with the sidecar but no Apple Intelligence (off, downloading, ineligible or older than macOS 26): "Using rules and your personal model only." followed by the reason, and the row shows the reason (or "Not enabled" / "Downloading..."). The Calendar shows the banner only for the fixable reasons (off or downloading), not for an ineligible Mac or an old OS.
  - Windows or a build without the sidecar: heading "AI & suggestions", the row "Suggestions use your rules", no "Suggestion engine", no "Custom instructions", no "Personal model", and "Reset learned data" still present.
- A dev instance on a machine without Apple Intelligence can still show and prove: the engine summary and reason, the fallback banner, every preference control (`rize settings get` read-back after each click), the empty effectiveness state, the cold-start text and the reset flow. This is the normal case for a driven run.
- Empty effectiveness: with no reviews, the cards show `-`, "Who decided" says "No decisions in this period yet." and the calibration area says "No reviewed model suggestions in the last 30 days yet. Rule matches are certain, so they aren't charted.". The calibration line reads "Learning: 0 of 50 reviews before calibration starts. Until then, confidence is capped at 90%." The preview line under the threshold says "No suggested entries in the last 30 days to preview against yet."
- Preference round trip: after each click, `rize --json settings get KEY` (`ai-suggest` `"category"` or `"categoryProject"`, `auto-accept` true or false, `auto-accept-percent`) must equal what was chosen, and the radio or switch state in `tree` must agree. Both are required for a PASS.
- Threshold slider: the exact-value field commits on blur or Enter; the slider range is 50 to 100 and saves about 350 ms after the last change. Read back with `settings get auto-accept-percent`.
- Reset: the confirmation titled "Reset learned data?" and afterwards the toast "Learned data reset" (short-lived); the proof is the counters returning to the cold-start text.
- Real classification: a pending entry with a suggestion shows in `rize entries show` with `suggestions[].engine` of `rules`, `fallback` or `full` and a `job.state` of `done`. This needs a closed entry that went through the worker, which a fresh session produces only from real captured activity (see [capture.md](capture.md)); seeded manual entries may not be queued, so treat their absence of a suggestion as expected.
- `shot` for the chart and layout.

## Gotchas

- Two controls named "Category" can exist: the "What to suggest" radio and nav or table entries elsewhere; on the Settings page only the radio is actionable with that exact name. "30d" and "90d" are radios in the effectiveness card.
- "AI" is a nav sub-button that exists only after "Advanced settings" is expanded.
- The "Suggestion engine" row, "Custom instructions" textarea (`fill -r AXTextArea "Custom instructions" ...`, saved on blur) and "Personal model" block are hidden when the on-device model is unavailable.
- The status refreshes when the window regains focus and on every `ai-status-changed` event, so right after launch the text may still say "Starting the on-device models..."; `wait` for the final text.
- Dismissing the banner stores the dismissed reason in browser storage for that window profile only; a fresh session shows it again. `Dismiss` is also the name of other controls; `tree` first.
- A text inconsistency to know about: the Apple Intelligence row's button on an unsupported OS says "Apple Intelligence requires macOS 15.1 or later." while the engine text says "needs macOS 26 or later". The code paths differ; report rather than assume one is right.
- The auto-accept switch reads "Starts after 50 reviewed suggestions; until then only rule matches auto-approve" until calibration starts; only rule matches (certain) clear a threshold above 90.
- The worker retrains only while the machine is idle, on power and outside Low Power Mode, so "Retrain now" may sit in "Retrain queued..." in a driven run.
- Rules (hint matches) are created by project and category sheets; a rule hit has confidence 100 percent and engine `rules`.

## Not reachable

- Apple's on-device model and the Swift sidecar cannot be forced on or off from here; their availability depends on the host Mac (macOS 26 or later with Apple Intelligence enabled). The `full` tier and the Enabled state cannot be proven on a machine without it, and the "Turn on in System Settings" and "Apple intelligence settings" buttons open System Settings (native).
- Retraining and calibration refits happen on a schedule under idle and power conditions that a driven run does not control, and training artifacts live in the session's `ml` folder (read it from disk if needed); no tool here triggers them deterministically.
- Real suggestions need real captured activity; no `rize` command classifies or retrains yet (those are M2 in COMMANDS.md).
