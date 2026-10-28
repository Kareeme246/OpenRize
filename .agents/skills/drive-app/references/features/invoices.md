---
id: invoices
title: Invoices
summary: Build invoices from approved billable time, retainers and manual items, preview the real PDF while editing, finalize it into a numbered frozen document, and mark it paid or void.
surfaces: [window]
routes: [invoices]
code: [src/pages/Invoices.tsx, src/pages/invoices, src/components/PdfViewer.tsx, src/hooks/useInvoicePreview.ts, src/lib/invoices.ts, src-tauri/src/invoices]
rize: []
reach-macos: partial
reach-windows: partial
reach-rize: none
verified: never
---

# Invoices

Invoices are drafts (editable, with their time reserved) that Finalize turns into numbered, frozen PDFs. The list shows Open, Overdue and Paid totals with a table of invoices. A draft opens an editor: collapsible sections for Client, Bill to, From, Invoice details, Line items and Payment and notes on the left, and the real invoice PDF on the right, re-rendered by Rust a moment after each edit. A finalized invoice opens as the archived document with Export PDF, Mark paid or Mark unpaid, and Void; a voided one can be deleted. "Invoice settings" holds the sender profile, logo, default terms, notes and the next invoice number. OpenRize does not send invoices or take payment.

The design rule that shapes verification (from AGENTS.md): Rust owns all money (integer USD cents) and the only paper renderer (`src-tauri/src/invoices/pdf.rs`); the editor shows those same bytes through pdf.js on a canvas. There is no HTML imitation of the paper and no total computed in TypeScript. Clients (see [clients.md](clients.md)) and projects (see [projects.md](projects.md)) start invoices; entries become billable time through [review.md](review.md).

## Sub-features

- list: the invoice table (Invoice, Client, Issue date, Due date, Amount, Status) with Open, Overdue and Paid sums, "Search invoices" and the "All statuses" filter.
- editor: the draft editor with its six sections, the Rust-priced "Amount due", "Save draft", "Export draft PDF", "Delete draft" and discard confirmation.
- line-items: tracked-time lines (one per approved billable entry, via "+ Tracked time" or "Add all"), "+ Retainer / fixed fee" and "+ Manual item", each with description, quantity, unit and rate.
- live-preview: the PDF pane ("Invoice preview" with "Zoom in", "Zoom out", a page count and "Updating..." while a render is pending).
- finalize: "Finalize invoice" with its confirmation; assigns the next number and locks the invoice.
- document-view: a finalized invoice with facts, the archived PDF, Export PDF, Mark paid, Mark unpaid, Void and Delete invoice (void only).
- invoice-settings: the "Invoice settings" sheet (business name, address, email, phone, logo, default payment instructions and notes, default payment terms, next invoice number).

## How to get to it

Sidebar > Work > Invoices (button "Invoices"). "+ New invoice" starts an empty draft. "Create invoice" on a project's detail (see [projects.md](projects.md)) or a client's detail (see [clients.md](clients.md)) opens a draft for that client prefilled with its ready time. A row in the list, or an invoice row on a client's detail, opens the editor (draft) or the document (any other status).

## Driving it

A fresh session is empty and may open on first-run onboarding (see [onboarding.md](onboarding.md)). No invoice `rize` command is Shipped yet (they are M2 in COMMANDS.md), so seed only the inputs through `rize` and create the invoice in the window.

```bash
scripts/app-drive.sh start --json
scripts/app-drive.sh rize -- clients add "Acme Corp" --email billing@acme.example --rate 120
scripts/app-drive.sh rize -- projects add "Client work" --client "Acme Corp" --billable true
scripts/app-drive.sh rize -- entries add --from 2026-10-28T09:00 --to 2026-10-28T10:30 -d "Wireframes" --project "Client work" --billable true
scripts/app-drive.sh rize -- entries approve --all-pending --period today
scripts/app-drive.sh rize -- --json entries list --period today --status approved
```

Use today's date for the entry. The approve command only matters if the entry was left pending; the final list must show it approved and billable, otherwise no time is ready to invoice.

Set the sender profile once, so Finalize is allowed:

```bash
scripts/app-drive.sh click "Invoices"
scripts/app-drive.sh wait "+ New invoice"
scripts/app-drive.sh click "Invoice settings"
scripts/app-drive.sh wait "Your business details appear under From"
scripts/app-drive.sh fill "Business name" "Studio North"
scripts/app-drive.sh fill -r AXTextArea "Address" "12 Quay Street, Leith"
scripts/app-drive.sh fill "Next invoice number (2026)" "7"
scripts/app-drive.sh click "Save"
```

The year in the last field's name is the current year, so read it from `tree`. Create a draft from the client's ready time, then edit and preview:

```bash
scripts/app-drive.sh click "Clients"
scripts/app-drive.sh click "Acme Corp"
scripts/app-drive.sh click "Create invoice"
scripts/app-drive.sh wait "Create draft invoice"
scripts/app-drive.sh wait "Invoice preview"
scripts/app-drive.sh wait "Page 1 of 1"
scripts/app-drive.sh click "+ Manual item"
scripts/app-drive.sh fill "Item line description" "Hosting, October"
scripts/app-drive.sh fill "Item line rate" "25"
scripts/app-drive.sh wait "Amount due"
scripts/app-drive.sh shot --out /tmp/openrize-proof/invoice-draft.png
scripts/app-drive.sh click "Save draft"
```

A draft can also start from the list: `click "+ New invoice"`, then the "Client" combobox, then add lines with "+ Tracked time" (sheet "Add tracked time": "Select all", "None", then the "Add" submit button named like "Add 1 entry"), "+ Retainer / fixed fee" or "+ Manual item". The "Add all" button on the ready-time banner adds every ready entry. Remove a line with "Remove item line" (or "Remove tracked time line", "Remove retainer line").

Finalize, then act on the document:

```bash
scripts/app-drive.sh click "Finalize invoice"
scripts/app-drive.sh wait "Finalize this invoice?"
scripts/app-drive.sh click "Finalize"
scripts/app-drive.sh wait "Amount due"
scripts/app-drive.sh click "Mark paid"
scripts/app-drive.sh click "Mark unpaid"
scripts/app-drive.sh click "Void"
scripts/app-drive.sh click "Void invoice"
scripts/app-drive.sh click "Delete invoice"
scripts/app-drive.sh click "Back to invoices"
```

Delete an unfinished draft from the list with its "Delete draft invoice for Acme Corp" button and then "Delete draft" in the dialog, or from the editor with "Delete draft". Read the stored result:

```bash
sqlite3 "$HOME/Library/Application Support/com.offlinestudios.openrize.drive20481/activity.db" "select number, status, total_cents, length(pdf) from invoices;"
sqlite3 "$HOME/Library/Application Support/com.offlinestudios.openrize.drive20481/activity.db" "select kind, description, quantity_hundredths, rate_cents, amount_cents from invoice_lines;"
```

Replace the digits in the identifier with those from your session (`start --json` prints it as `identifier` and the folder as `appData`).

## What proves it works

Be explicit about what each tool can see, because the paper itself is a canvas:

- The accessibility tree shows that a PDF was produced and displayed, not what is on the paper: the preview region named "Invoice preview" (editor) or "Invoice INV-2026-0007" (document, the number follows "Invoice "), an image per page named "Page 1 of 1", and the page count text "1 page". These prove the Rust render succeeded and pdf.js loaded the bytes. A failure shows an alert "The preview could not be displayed", or a placeholder such as "Add a line item to see the invoice." / "Select a client and add line items to see the invoice.".
- The money is Rust's: the editor's per-line amounts, "Amount due" and the finalize dialog text ("locked at $...") come from the Rust quote, so they are valid numeric proof. With the seed above, 90 minutes at 120 per hour shows `$180.00` on the tracked-time line; adding a 25 manual item makes "Amount due" `$205.00`. Required for a PASS of a pricing change: the amounts in `tree` and the stored `total_cents` (20500) agree.
- The paper's layout and wording (sender block, logo, line table, page breaks, fonts) can only be judged by looking: `shot` of the window (the canvas is in the screenshot), and for specimens the Rust test described in AGENTS.md, `OPENRIZE_INVOICE_SAMPLES=/tmp/openrize-proof/specimens cargo test --manifest-path src-tauri/Cargo.toml --lib invoices::tests::renders`, which writes specimen PDFs you can open and read. A visual change to the PDF is PASS only with a `shot` or specimen that was actually looked at.
- Lifecycle: after Finalize the page shows the document view ("Amount due", "Export PDF", "Mark paid"), the list status pill reads Open and the row has a number like `INV-2026-0007`; `sqlite3` shows `status` of `open`, a non-null `number` and a non-zero `length(pdf)`. Mark paid gives `paid` and a pill Paid; Void gives `void` and the time is released (the approved entry is again ready to invoice on the client).
- Draft save: toast "Draft saved" is short-lived; the proof is the row in the list with status Draft and `select status from invoices` returning `draft`.
- Export PDF opens a native save dialog; the toast "PDF saved to ..." cannot be relied on (see Not reachable).

## Gotchas

- Section headers are buttons whose names include a summary, for example "Client Acme Corp" once collapsed. The Client section button and the "Client" combobox share the word; run `tree` and use `-i N` when a click is ambiguous. "Name", "Address" and "Email" exist in both Bill to and From (and "Business name"/"Address" in Invoice settings); open only the section you need, or use `-i`.
- Line fields are named by line kind: "Tracked time line description", "Retainer line rate", "Item line quantity" (a manual item is called "Item"). With several lines, names repeat; use `-i N` in tree order.
- Tracked-time lines have no quantity or unit field; their hours come from the entry. Their rate field is empty and an amount shows "-" until Rust prices it; an entry whose project and client have no rate shows an inline alert and blocks saving.
- The preview is debounced (400 ms) and the paper dims with "Updating..." while a render is pending; `wait` for "Amount due" or the page count after typing, never assert immediately.
- Below 1000 px of window width the editor shows an "Edit" / "Preview" toggle (`aria-pressed`) instead of both panes; click "Preview" to see the paper.
- "Finalize invoice" refuses with "Add your business name and address in the From section before finalizing." until the profile (or the From fields) are filled.
- A client with invoices cannot be deleted; a finalized invoice cannot be edited, only paid, unpaid, voided, exported.
- Dates: "Overdue" is a display status computed from the due date, not a stored status.
- The sidebar "Invoices" label and the page heading share the name; use "+ New invoice" or "Search invoices" as the page-ready marker.
- Isolation: the Next invoice number and the logo live in the session's data, so they do not touch the real app. Do not use real business details.

## Not reachable

- The content of the PDF: pdf.js draws to a canvas, so no text on the paper is in the accessibility tree. Only `shot` (visual), specimen PDFs from the Rust test, and the stored bytes in `sqlite3` reach it. No `rize` command can render or write a PDF yet (the PDF export command is M2 in COMMANDS.md).
- Export PDF and Export draft PDF open the OS save dialog (native); no tool here can fill or confirm it. The file write is untested through the driver.
- The logo chooser ("Choose logo" / "Replace logo") opens a native file picker. "Remove" can be driven only after a logo exists.
- Every other state transition is reachable through the window only; no invoice `rize` command is Shipped (list, show, draft, quote, finalize, paid, void, pdf and profile commands are all M2).
