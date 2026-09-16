# Rize.io Feature Research — for OpenRize Implementation

Research compiled 2026-09-16 from rize.io marketing pages, `rize.io/for-ai`, `docs.rize.io`, and the Rize changelog, for the purpose of planning OpenRize (this repo's Tauri clone) feature work.

**How to use this doc:** each `##` header below is a feature area that corresponds to a distinct sidebar / navigation destination in the real Rize app. Pick a header, and it should contain enough detail to scope an implementation task. Sources are cited inline where useful; anything not confirmed by a source is marked **(inferred)**.

**Before implementing anything from this doc:** per `CLAUDE.md`, confirm with the user whether the feature belongs in `src-tauri` (Rust backend), `src` (React frontend), or both — this doc intentionally does not make that call. Where a feature clearly implies local background processes (e.g. window/activity capture, idle detection), that leans backend; where it's purely display/interaction over data the backend already exposes, that leans frontend. Flag these leanings per-section but treat them as a starting hypothesis, not a decision.

---

## What Rize Is (product framing)

Automatic, passive time-tracking desktop app for Mac/Windows aimed at agencies, professional-services firms, studios, consultants, and freelancers who bill time. Core pitch: no manual timers — the app watches the active window/app/URL in the background and auto-categorizes work into clients/projects/tasks. Privacy stance: no keystroke logging, no screen recording, no screenshots. Explicitly *not* an employee-surveillance tool (contrasted with Hubstaff/Time Doctor) and *not* an invoicing tool (exports/integrates instead).

Pricing (context only, not for implementation): Basic $12.99/mo, Pro $28.99/mo, Max $49.99/mo, Team $39.99/seat/mo (2-seat min), Enterprise custom.

OpenRize already has the skeleton of this: `src-tauri/src/timers.rs` (backend timer/session logic), `src/pages/Trackers.tsx`, `src/components/StopwatchCard.tsx`, `src/components/HourRing.tsx`, `src/hooks/useTimers.ts` — i.e. the "Sessions/Timer" area below is already partially started. Current sidebar (`src/components/Sidebar.tsx`) only has **Trackers** and **Settings**; everything else below is unbuilt.

---

## Home / Daily Dashboard

Landing view when opening the app.

- **Time to review** — card listing pending/uncategorized time entries that need a client/project/task assigned.
- **Daily summary** — snapshot of the day's activity (likely: hours tracked, focus time, category breakdown).
- **Workspace hours chart** — visualizes tracked time across the team for the week (team-context only).
- **Quick links** — shortcuts to frequently used pages.
- Sidebar itself is **customizable**: users can choose which nav items are visible, hide unused ones, and reorder them, per-workspace. Workspaces can set a custom logo shown in the sidebar. There's a **collapsible sidebar** mode and a **workspace switcher** built into the side nav for moving between Individual and Team workspaces without losing current view context.

**Leaning:** frontend (aggregation view over data owned by other backend modules), except the "which items are visible/reordered" preference which needs small persisted state (likely a local settings store — could be Tauri backend or a frontend-only local store).

---

## Sessions (Timeline & Timer)

The core tracking primitive — arguably the most important header to get right first, since OpenRize's existing timer/tracker code maps here.

- A **session** is a block of work activity. Rize logs "every app switch, document change, and meeting" as granular entries — sub-5-minute tasks are captured, unlike manual trackers.
- Three session types: **Focus**, **Meeting**, **Break** (see their own sections below — each is really a sub-mode of Sessions).
- **Automatic capture**: background process reads active-window metadata (app name, window title, URL) continuously; no manual start/stop required.
- **Timeline UI**: a day-calendar-style timeline where sessions are laid out chronologically. Users can select a time range on the timeline and manually add a focus session, meeting, or break via drag-and-drop.
- **Session Timer**: a radial/countdown timer widget (visual countdown + controls) for the currently active session, with a "Start Session" control in the bottom-left of the app and a Timer tab offering "Start Focus" etc.
- **Calendar view scales**: Day / Week / Month / Year views, each showing time entries and sessions at that granularity.
- **Review panel**: clicking a pending/uncategorized entry opens a context panel docked to the day calendar, showing contributing apps/websites, AI category suggestions with confidence scores, and the ability to create tasks in connected tools (ClickUp, Linear, etc.) directly from the panel.
- Idle-detection specifics aren't documented publicly beyond: idle detection can stop tracking after a configurable period of inactivity (5 minutes was cited as a per-category default) and can trigger a Break session automatically.

**Leaning:** heavily backend for capture/idle-detection/session-generation (this is exactly what `src-tauri/src/timers.rs` should own — active window polling would need a Tauri plugin or OS-level API); frontend for the timeline, timer widget, and review panel UI.

---

## Focus

A specialized session type and its own nav concept (deep-work tracking).

- **Automatic trigger**: a Focus Session is created when ~75% of a user's time within any 15+ minute window is spent in apps/websites whose rule is set to "Focus mode."
- **Calendar-keyword trigger**: calendar events containing configurable keywords (default `#rize-focus`, `focus`, `flow`) auto-start a Focus session.
- **Manual start**: drag-select on the Sessions timeline → "Focus"; or the Timer tab's "Start Focus" button; or the global "Start Session" button.
- **Focus quality scoring**: monitored via 20+ attributes, detecting context-switching and app-based distractions during the session.
- Optional **music** selection during focus sessions **(inferred detail, unconfirmed specifics)**.
- Users configure which apps/websites count as "Focus" via tracking rules.

**Leaning:** backend for the detection/scoring engine and rule evaluation; frontend for configuration UI and the focus-score visualization.

---

## Meetings

Second session type.

- Detected automatically from **calendar integration** (event start/end become session bounds) or created manually.
- Keyword-based detection can tag which project/client a meeting belongs to by scanning event titles/descriptions.
- Provides meeting-specific metrics and time analysis **(inferred: e.g. meeting hours as % of total, meeting load per day/week)**.

**Leaning:** backend for calendar sync + keyword tagging; frontend for meeting list/metrics display.

---

## Breaks

Third session type, framed around work-life balance rather than raw tracking.

- Triggered **automatically** by idle-time detection, or started manually.
- Includes **guided meditations** as an in-app feature during a break.
- Break metrics aim to ensure users are taking adequate rest (e.g. break frequency/length vs. targets) **(inferred specifics)**.

**Leaning:** backend for idle-triggered break creation; frontend for the meditation content/UI and break metrics.

---

## Clients, Projects & Tasks (Categorization)

The billing hierarchy and the rules engine that assigns time to it. Docs note Rize is mid-migration from a legacy flat "Categories" model to a newer **Labels + App & Website Rules** model — worth deciding which model OpenRize should build directly rather than replicating the migration.

- **Hierarchy**: Client → Project → Task. Time entries/sessions get tagged at this granularity for billing.
- **Tracking rules**: map a given app / website / window-title pattern to a Client/Project/Task (or a Label). "Most specific matching rule" wins when multiple rules could apply.
- **AI auto-tagging**: when no rule matches, activity defaults to "Miscellaneous"; after ~2 minutes in Miscellaneous, an LLM (Rize cites GPT-5) auto-assigns the most likely category using signals from keywords, past entries, calendar events, and linked external tasks (ClickUp/Linear/etc.), and shows a **plain-English explanation** and **confidence score** for each suggestion.
- If AI categorization is disabled, activity becomes "Uncategorized" after ~10 minutes and needs manual resolution.
- **Per-category settings** (4 toggles cited): counts toward Focus scoring; counts toward Work Hours; has its own idle-detection timeout; triggers the Distraction Blocker.
- **Team-level clients/projects/tasks**: admins can create Team Clients/Projects/Tasks (vs. personal ones) and view time any team member has tagged to them — used for team workload visibility and simplifying client invoicing.

**Leaning:** backend for rule evaluation, AI categorization calls, and the Miscellaneous/Uncategorized timeout state machine; frontend for rule authoring UI, the client/project/task tree, and the suggestion-review UI (confidence scores, accept/reject).

---

## Distraction Blocker

Sits under Settings/Tracking Rules conceptually, but is significant enough to be its own doc category.

- Intervenes when a user opens an app/website categorized as a **distraction**, has spent longer than a configured **threshold** there, and blocking is enabled for the current session type.
- On trigger, user can: dismiss until the threshold is hit again, mark that category as non-distracting *for this session*, disable the blocker for the rest of the session, or jump to blocker settings.
- Configured per app/website from **Settings → Tracking Rules**; only fires during sessions where blocking is enabled (e.g. typically Focus sessions).

**Leaning:** backend for enforcement (needs to intercept/observe foreground-app changes, same primitive as session capture) and threshold timers; frontend for the intervention prompt/modal and settings UI.

---

## Work Hours

A configuration + reporting concept: the user's expected working hours, used as a baseline for metrics.

- Categories can be marked as counting toward "Work Hours."
- Likely used to compute utilization %, capacity remaining, and to distinguish "work" time from personal/break time in reports **(inferred)**.

**Leaning:** mostly configuration (frontend) reading from settings; backend only needs to expose the toggle already defined under Clients/Projects/Tasks categorization.

---

## Dashboards & Reports

Analytics layer, built on top of everything above. Rize ships a **Custom Dashboards** system plus purpose-built report types.

### Custom Dashboards
- Widget library includes: member breakdowns, task tracking, project charts, client analytics, focus scores, team timetables.
- Three starter templates: **Team Overview** (time by member/task/project/client), **Clients Overview** (time per client for workload/billing), **Personal Productivity** (focus scores, category breakdown, time distribution).
- Build flow: create from template or blank → "Customize" → "Edit Widgets" → drag-and-drop layout → "Add Widget" to browse/add more → filter any dashboard by day/week/month/custom range → unlimited dashboards, switchable.
- Currently admin-only on team workspaces; member-level access was "coming soon" as of the source changelog.

### Profitability
- Real-time P&L per project: connects tracked time to cost rates and contract values to compute margin.
- Utilization: billable vs. non-billable hours across the team.
- Budget tracking: actual hours vs. estimate/budget per client, including retainer-style monthly-cap consumption.
- Aggregatable by client, project, team member, and time period; shows effective hourly rates, budget burn rate, and time-to-completion trend.
- Framed as *proactive* (catch margin problems before the invoice goes out), not just historical reporting.

### Resourcing (capacity/utilization)
- Breaks down hours by team member, project, and client to spot who's overloaded vs. who has spare capacity.
- Used for staffing/assignment decisions instead of guesswork.

### Timesheets
- Personal and team hour review, built from the same auto-captured/AI-tagged entries rather than manual timesheet entry.

### Team Analytics
- Aggregate utilization reporting across the org (distinct from the client/project-scoped Profitability view).

**Leaning:** backend for aggregation queries (these need to run over potentially large session/entry history — better done server/rust-side than in the React layer); frontend for the widget system, drag-and-drop editor, and charts.

---

## Teams

Multi-user / org-management area. **Note:** relevant mainly once OpenRize has a multi-user or sync backend — currently the repo looks single-user/local, so this entire header may be out of scope until that's decided; flag to the user before starting.

- **Create a team**: "+Add Team" in the Teams nav → set a name → lands on Teams → [Team] → Settings.
- **Members**: "+ Add Member" in Team Settings → Team Members; role is set at invite time (role-based permissions).
- **Team Clients/Projects/Tasks**: see Categorization section — admins can view time any member tagged to team-level clients/projects/tasks, improving workload visibility and simplifying client invoicing.
- **Workspace switcher**: side nav lets a user switch between their Individual workspace and any Team workspace without losing view context.
- **Team Billing settings**: distinct settings tab for the team's own subscription/billing.

**Leaning:** requires backend (multi-user data model, permissions, likely a sync/server component) far beyond what a local Tauri app does alone — this is the header most likely to need an explicit architecture conversation with the user before scoping.

---

## AI Agent, Reports & Routines

The "for-ai"-flavored feature set; five sub-concepts bundled under one nav area in the marketing site's feature list, likely one destination in-app.

- **Rize Agent**: conversational interface for querying time/team data in plain English — e.g. "Show me this week's time entries" (returns day/project breakdown with billable %), "What's my profitability on the Acme project?" (margin, revenue, trend comparison), "Approve all entries from Monday" (manager action + summary by member). Read-write: can also mutate data (approvals) not just answer questions.
- **AI Reports**: template-based recurring analysis (scheduled/repeatable report generation, as opposed to one-off dashboard views).
- **Routines**: scheduled automated prompts — recurring agent invocations rather than user-initiated chat.
- **Agent Context**: custom team-level instructions that steer how the agent interprets/answers queries for that org.
- **Shared AI Skills**: a reusable prompt library, shareable across a team.

**Leaning:** backend-heavy (LLM integration, data access layer, scheduling for Routines/AI Reports) with a thin chat-style frontend. This is a natural candidate to build *after* the core tracking/categorization data model exists, since the agent is fundamentally a query layer over that data.

---

## Integrations

- Task/PM tools: ClickUp, Linear, Asana, and more — used both for AI-tagging signal (linked tasks) and for creating tasks directly from the entry-review panel.
- Accounting/invoicing: FreshBooks, QuickBooks, Xero (via Zapier), plus generic CSV export — Rize itself does not generate invoices.
- Calendar integration (Google Calendar etc., implied throughout Meetings/Focus) for meeting detection and focus-keyword scanning.
- **Webhooks** and a **GraphQL API** are documented as their own top-level doc categories — i.e. Rize exposes a public API surface, not just built-in integrations.

**Leaning:** backend (OAuth flows, API clients, webhook delivery); frontend only for connection management UI.

---

## MCP Server

- Exposes "structured, read-write access to your Rize time data, projects, clients, and reporting context" to MCP-compatible AI clients (Claude, ChatGPT, etc.) via OAuth.
- Functionally a specialized API surface over the same data as the in-app AI Agent — same use cases (query time entries, profitability, team workload) but reachable from external AI tools instead of only Rize's own chat UI.
- Requires explicit user OAuth approval before any external tool can access data.

**Leaning:** backend — this is literally a server component; likely lowest priority relative to core tracking.

---

## Desktop Widget

- A lightweight, persistent OS-level widget (menu bar / system tray, per the "for-ai" page's mention of a native background app) — likely showing current session/timer status without opening the full app window.
- OpenRize already has tray icon assets (`src-tauri/icons/tray-active.png`, `tray-idle.png`) and `src-tauri/src/tray.rs`, suggesting this is partially scaffolded already.

**Leaning:** backend/Tauri-native (tray icon, small native window), minimal frontend if the widget is its own lightweight webview.

---

## Settings

Bottom-of-sidebar, always-present nav item (already exists in OpenRize). Per the 2026 Settings redesign changelog, sections are grouped into:

- **Account** — personal profile/auth.
- **Billing** — individual subscription.
- **Organization** — org-level settings (team-context).
- **Workspace tabs** — one per workspace, each with its own settings (this is where Calendars, Tracking Rules / Labels & App-Website Rules, Distraction Blocker config, Focus keywords, and Theme presumably live, based on cross-references found elsewhere in the docs).
- **Theme** — includes ability to revert to a legacy dashboard layout during a transition period (probably not relevant to replicate).
- **Command Palette** — quick-access command search, was called out as living near Settings/Support in the nav.

**Leaning:** mixed — mostly frontend forms over backend-persisted config, consistent with what's already in `src/pages/Settings.tsx`.

---

## Suggested build order (not a commitment, just a starting recommendation)

Given what's already scaffolded in this repo (timers, tray icons, a Trackers page, Settings page):

1. **Sessions / Timeline** — formalize the existing timer/tracker work into the session model (Focus/Meeting/Break types, timeline UI, day/week/month/year calendar views).
2. **Clients, Projects & Tasks** — the categorization hierarchy and manual tagging UI; this unblocks almost everything else (Focus, reports, profitability all depend on it).
3. **Focus / Breaks / Distraction Blocker** — the automatic-detection layer on top of Sessions.
4. **Home / Daily Dashboard** — once Sessions + Categorization exist, this is mostly aggregation.
5. **Dashboards & Reports / Profitability** — deeper analytics once enough entry history/data model exists to report on.
6. **Teams** — only after confirming with the user whether OpenRize is meant to support multi-user/sync at all.
7. **AI Agent / MCP / Integrations** — highest-value "for-ai" differentiators, but also the most dependent on everything above already existing and being reliable.

Confirm this ordering (and the frontend/backend split for each header) with the user before starting implementation, per project convention.
