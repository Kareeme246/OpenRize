---
id: clients
title: Clients
summary: Manage the clients that own projects and invoices, with each client's projects, tracked time, ready-to-invoice time, invoiced total and default rate.
surfaces: [window, cli]
routes: [clients]
code: [src/pages/Clients.tsx, src/pages/clients, src/lib/clients.ts, src/hooks/useCatalog.ts, src-tauri/core/src/models.rs]
rize: ["rize clients list", "rize clients show", "rize clients add", "rize clients edit", "rize clients rm"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Clients

Clients are the billing parties that own projects. The Clients page lists them under Active and Archived tabs with project count, last activity, time in a chosen range, unbilled (ready to invoice) time and amount, invoiced and outstanding totals, and default hourly rate. A client opens into a detail view with its numbers, its projects (attach an existing project, detach another), its contact and billing details, its recent invoices and the Edit, Archive or Restore, and Delete actions. A client's default rate and currency are what its projects fall back to; assignment stays project to client, so a client page writes the project's client. A client that has invoices cannot be deleted, only archived. Invoice work continues in [invoices.md](invoices.md); the project side is [projects.md](projects.md).

## Sub-features

- list-and-tabs: the table per Active or Archived tab with counts and four summary cards (Active clients or Archived clients, Time tracked, Ready to invoice, Outstanding).
- search-and-range: "Search clients" (name, email, project names) and the "Time range" picker.
- create-edit: the "New client" / "Edit" sheet: name, billing email, address, default hourly rate, currency (three letters), notes.
- detail: stat cards (Projects, Time tracked, Ready to invoice, Invoiced), the Details list and the Invoices list.
- attach-detach-projects: the "Attach a project" picker ("+ Attach project") and a "Detach" button per project row.
- archive-restore-delete: Archive or Restore, and Delete (confirmed; disabled when invoices exist).
- invoice-shortcuts: the "Create invoice" strip (only when approved billable time is ready) and invoice rows that open the invoice.

## How to get to it

Sidebar > Work > Clients (button "Clients"). A client row opens its detail view; "‹ Clients" returns. A project's "Create invoice" and an invoice's client link lead from [projects.md](projects.md) and [invoices.md](invoices.md).

## Driving it

A fresh session may open on first-run onboarding (see [onboarding.md](onboarding.md)) and is empty. Seed through `rize`:

```bash
scripts/app-drive.sh start
scripts/app-drive.sh rize -- clients add "Acme Corp" --email billing@acme.example --rate 120 --currency USD --notes "Net 30"
scripts/app-drive.sh rize -- clients add "Globex" --rate 90
scripts/app-drive.sh rize -- projects add "Client work" --client "Acme Corp" --billable true
scripts/app-drive.sh rize -- projects add "Unassigned" 
scripts/app-drive.sh rize -- entries add --from 2026-10-28T09:00 --to 2026-10-28T10:30 -d "Wireframes" --project "Client work" --billable true
```

Use today's date for the entry. If the entry is still pending, approve it: `scripts/app-drive.sh rize -- entries approve --all-pending --period today`.

Open and use the page:

```bash
scripts/app-drive.sh click "Clients"
scripts/app-drive.sh wait "+ New client"
scripts/app-drive.sh tree
scripts/app-drive.sh fill "Search clients" "Acme"
scripts/app-drive.sh click "Time range"
scripts/app-drive.sh click "Archived"
scripts/app-drive.sh click "Active"
scripts/app-drive.sh click "Acme Corp"
scripts/app-drive.sh wait "Details"
scripts/app-drive.sh click "Edit"
scripts/app-drive.sh key escape
scripts/app-drive.sh click "Archive"
scripts/app-drive.sh click "Restore"
scripts/app-drive.sh click "Detach"
scripts/app-drive.sh click "Attach a project"
scripts/app-drive.sh click "Create invoice"
scripts/app-drive.sh click "Delete"
scripts/app-drive.sh click "Cancel"
scripts/app-drive.sh click "‹ Clients"
```

Create a client through the sheet:

```bash
scripts/app-drive.sh click "+ New client"
scripts/app-drive.sh wait "New client"
scripts/app-drive.sh fill "Name" "Initech"
scripts/app-drive.sh fill "Billing email" "ap@initech.example"
scripts/app-drive.sh fill -r AXTextArea "Address" "1 Main St"
scripts/app-drive.sh fill "Default hourly rate" "95"
scripts/app-drive.sh fill "Currency" "USD"
scripts/app-drive.sh click "Create client"
```

Backend side:

```bash
scripts/app-drive.sh rize -- --json clients list
scripts/app-drive.sh rize -- --json clients show "Acme Corp"
scripts/app-drive.sh rize -- clients edit "Globex" --archived true
scripts/app-drive.sh rize -- clients rm "Initech" --dry-run
scripts/app-drive.sh rize -- --yes clients rm "Initech"
```

## What proves it works

- `wait "+ New client"` and `wait "Search clients"` prove the list rendered (the heading collides with the sidebar button).
- Created client: a row button in `tree` containing the name and `rize --json clients list` returning the same `name`, `email`, `defaultRate` and `currency`. Both are required for a PASS.
- Rollups: with the seed above, `clients show "Acme Corp"` (`data.projects`) lists "Client work" and the detail view's Projects card says `1` with sub-text `1 active`; the list row's Unbilled column shows a duration only when an approved billable entry exists (the detail view then shows the "ready to invoice" strip with "Create invoice").
- Attach: after choosing a project in "Attach a project", `clients show` lists it in `data.projects` and a "Detach" button appears on its row; after Detach it is gone. Toast text (for example `Attached "Unassigned" to "Acme Corp"`) is short-lived, so use `rize` as the proof.
- Archive: the client moves to the Archived tab and `clients list` has `archivedAt` set; Restore clears it (`null`). Archived clients are left out of pickers on projects and invoices.
- Delete with no invoices: confirm dialog "Delete NAME?", then the client is absent from `clients list`. With invoices the Delete button is disabled (its `tree` entry shows disabled).
- Visual layout: `shot`.

## Gotchas

- Dynamic labels: client rows are single buttons named by the whole row text; click with the client name as a substring, run `tree` first, and add `-i N` if two match. Tabs carry counts ("Active 2").
- On the detail view "Edit", "Archive" (or "Restore"), "Delete" and "Detach" each appear once per page, except "Detach" which appears once per project row (use `-i`). While the delete dialog is open, "Delete" is ambiguous between the page button and the dialog button; use `-i` after `tree`, or `key escape`.
- "Archive" also matches the Archived tab by substring; the exact-name match "Archive" wins, but `tree` first on the list page where the tab is "Archived N".
- "Attach a project" is a disabled combobox for an archived client or when no other project exists. Its option names have the form "Project (now OtherClient)" when the project already has a client; choosing it moves the project.
- The sheet's "Name" is a text field; "Address" and "Notes" are text areas (`fill -r AXTextArea`). "Currency" takes exactly three letters or empty; the Create button stays disabled otherwise.
- Picker options (Time range, Attach a project) may not be exposed as clickable by the macOS accessibility tree; if `click` of an option fails, use `key down` and `key return` after focusing the combobox. Unverified.
- Invoiced and outstanding amounts are always USD cents formatted as dollars (see [invoices.md](invoices.md)), even for a client with another currency.

## Not reachable

- Hover tooltips (active-project count on the Projects column, "This client has invoices, so it can only be archived", "Remove NAME from NAME") are not in the accessibility tree; read the disabled state of the Delete button instead.
- There is no `rize` command for client invoices or for attaching in bulk; invoice rows can only be created through the window (see [invoices.md](invoices.md)).
