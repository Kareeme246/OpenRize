---
id: projects
title: Projects
summary: Create, edit, complete, archive and delete the projects that time belongs to, with per-project time, budget, billable amount and ready-to-invoice time.
surfaces: [window, cli]
routes: [projects]
code: [src/pages/Projects.tsx, src/pages/projects, src/lib/clients.ts, src/hooks/useCatalog.ts, src-tauri/core/src/projects.rs, src-tauri/core/src/models.rs]
rize: ["rize projects list", "rize projects show", "rize projects add", "rize projects edit", "rize projects rm"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Projects

Projects say which work time belongs to. The Projects page lists them under Active, Completed and Archived tabs with last activity, due date, time in a chosen range and budget use. A project opens into a detail view with totals, time by category, time per week, recent entries, matched apps, the rules that feed it and the Complete, Archive and Delete actions. A project can carry AI hints (folders, repos, domains, keywords) that become rules, a client, an hourly rate, a budget in hours or amount, and a due date. Discovery suggests new projects from the last 14 days of titles. Entry assignment itself happens in the review panels (see [review.md](review.md)); invoicing a project's ready time continues in [invoices.md](invoices.md).

## Sub-features

- list-and-tabs: the table of projects per status tab (Active, Completed, Archived), each with a count, sorted by last activity.
- search-and-filters: the "Search projects" field, the Client filter (including "No client") and the "Time range" picker (This week, This month, Last 30 days, All time) that sets the Time column.
- create-edit: the "New project" / "Edit" sheet with name, colour, client (pick or create inline), description, AI hints with a live "Would have matched" preview, hourly rate, billable default, budget (None, Hours, Amount; Total or Month) and due date.
- detail: per-project stat cards (Total, This week, Budget used, Billable), "Time by category", weekly columns, "Recent entries", "Matched apps" and "Rules".
- status-actions: Complete, Reopen, Archive and Delete (confirmed) on the detail view.
- ready-to-invoice: the "ready to invoice" strip and its "Create invoice" button, which opens a new invoice for the project's client.
- discovery: "Suggested ..." strips with Create and Dismiss, shown on the Active tab.
- import-csv: the "Import CSV" button (columns name, client, budget, due date).

## How to get to it

Sidebar > Work > Projects (the sidebar button is named "Projects"). Clicking a row opens its detail view; "‹ Projects" returns to the list. From a client's detail view a project row also opens here (see [clients.md](clients.md)).

## Driving it

A fresh session is a brand-new install: if it opens on the first-run Permissions screen, get past it first (see [onboarding.md](onboarding.md)). The database is empty, so seed through `rize`.

```bash
scripts/app-drive.sh start
scripts/app-drive.sh rize -- clients add "Acme Corp" --rate 120 --email billing@acme.example
scripts/app-drive.sh rize -- projects add "Client work" --client "Acme Corp" --billable true --hints "acme.example, ~/Code/acme" --budget-kind hours --budget 40 --budget-period total --due 2026-12-15
scripts/app-drive.sh rize -- entries add --from 2026-10-28T09:00 --to 2026-10-28T10:30 -d "Wireframes" --project "Client work" --billable true
scripts/app-drive.sh rize -- entries approve --all-pending --period today
```

Use today's date in the `--from` and `--to` values. The last command only matters if the new entry is still pending; check with `scripts/app-drive.sh rize -- --json entries list --period today --status pending`.

Open the page and read it:

```bash
scripts/app-drive.sh click "Projects"
scripts/app-drive.sh wait "+ New project"
scripts/app-drive.sh tree
scripts/app-drive.sh click "Completed"
scripts/app-drive.sh click "Active"
scripts/app-drive.sh fill "Search projects" "Client"
scripts/app-drive.sh click "Time range"
scripts/app-drive.sh tree
```

After clicking "Time range" the options are listed in the tree (This week, This month, Last 30 days, All time); click one by its name. Open a project and act on it:

```bash
scripts/app-drive.sh click "Client work"
scripts/app-drive.sh wait "Recent entries"
scripts/app-drive.sh click "Edit"
scripts/app-drive.sh wait "Edit Client work"
scripts/app-drive.sh key escape
scripts/app-drive.sh click "Complete"
scripts/app-drive.sh click "Reopen"
scripts/app-drive.sh click "Archive"
scripts/app-drive.sh click "Delete"
scripts/app-drive.sh click "Cancel"
scripts/app-drive.sh click "‹ Projects"
```

Create a project through the sheet:

```bash
scripts/app-drive.sh click "+ New project"
scripts/app-drive.sh wait "New project"
scripts/app-drive.sh fill "Name" "Website redesign"
scripts/app-drive.sh fill "Description" "Marketing site refresh"
scripts/app-drive.sh fill -r AXTextArea "AI hints" "~/Code/website, example.com"
scripts/app-drive.sh click "Create project"
```

Read the backend side with `rize`:

```bash
scripts/app-drive.sh rize -- --json projects list
scripts/app-drive.sh rize -- --json projects show "Client work"
scripts/app-drive.sh rize -- projects edit "Website redesign" --status archived
scripts/app-drive.sh rize -- projects rm "Website redesign" --dry-run
scripts/app-drive.sh rize -- --yes projects rm "Website redesign"
```

## What proves it works

Cheapest first; each needs the stated evidence for a PASS.

- `wait "+ New project"` and `wait "Search projects"` prove the page rendered. The heading "Projects" is not a usable marker, because the sidebar button has the same name.
- A seeded or created project appears as a row button in `tree` whose name starts with the project name. PASS needs the row plus `rize --json projects list` returning the same name in `data.projects`.
- Budget: with the seed above and 90 minutes tracked, the row shows `1.5 / 40h` (`wait "1.5 / 40h"`), and `rize --json projects show "Client work"` has `data.stats.totalMs` of 5400000.
- Ready to invoice: the detail view shows "ready to invoice" only when the project has approved billable entries (`data.stats.unbilledEntries` above 0 in `projects show`); the "Create invoice" button appears too when the project has a client.
- Status changes: after Complete, the project leaves the Active tab; `projects show` returns `data.project.status` of `completed`. After Archive, `archived`. After Reopen, `active`.
- Delete: confirmation dialog titled "Delete Client work?", then the row is gone and `projects list` no longer has it. Entries keep their time.
- Create/edit through the sheet: PASS needs `rize --json projects show` with the typed name, description and hints (`data.project.aiHints`). The "Created ..." toast is short-lived, so do not require it.
- Visual layout (columns, budget bar colour at 80% and over): `shot`.

## Gotchas

- Dynamic labels: project rows are one button whose name is the whole row text (name, client, relative times). Click with the project name as a substring and run `tree` first; if two rows match, add `-i N`. Tabs are named with their count (for example "Active 1"); the substring `click "Active"` works.
- "Client" is a combobox name on the page filter and again inside the New project sheet. If a click reports it is ambiguous, pass `-i 0` or `-i 1` after `tree`. The sheet's field named "Name" is a text field; the "AI hints" box is a text area, so `fill` needs `-r AXTextArea`.
- Picker options (the "Time range" list, the sheet's Client, Budget and Per pickers) are `role=option` buttons. Whether the macOS accessibility tree exposes them as clickable is unverified here; if `click` of an option name fails, focus the combobox with `click`, then use `key down` and `key return`.
- Colour swatches are radios named "Colour #hex"; the hex values come from `src/lib/palette.ts`, so read them from `tree`.
- The Delete button on the detail view and the confirm button of the dialog are both named "Delete"; with the dialog open use `-i` after `tree`, or `key escape` to cancel.
- Entries trimmed or merged by other features change totals; read totals from `rize` when exact numbers matter.
- Discovery strips need recent activity with repeated titles; a fresh session has none, so Create and Dismiss are not exercised by seeding alone.
- `rize projects rm` refuses when it would change approved entries; the UI delete shows the same error in an inline alert.

## Not reachable

- The "Import CSV" file chooser is a native dialog; no tool here can pick a file in it. Import cannot be driven (the import command is not a Shipped `rize` command either).
- Discovery suggestions need real captured activity (see [capture.md](capture.md)); only the empty state is reachable from seeding.
- The hover tooltips ("Columns: name, client, budget, due date", the Time column's range) are not in the accessibility tree.
