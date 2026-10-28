---
id: categories
title: Categories
summary: Create, edit, reorder, archive and delete the categories that classify time, and choose which of them count toward work hours.
surfaces: [window, cli]
routes: [categories]
code: [src/pages/Categories.tsx, src/pages/categories, src/components/SegmentedControl.tsx, src/lib/palette.ts, src-tauri/core/src/categories.rs]
rize: ["rize categories list", "rize categories add", "rize categories edit", "rize categories rm"]
reach-macos: partial
reach-windows: partial
reach-rize: partial
verified: never
---

# Categories

Categories describe the kind of work an entry is (Coding, Meetings, Admin). The Categories page lists them under Active and Archived tabs with the time tracked in a chosen range, a one-line "what counts" description and a "Counts as work" switch that feeds the daily and weekly work totals. The first three active categories are the review panel's number-key shortcuts 1 to 3, and the order is changed by dragging rows. A category's sheet edits its name, colour, description (used by Apple Intelligence when it is available), optional "Apps & sites" rules, and offers Archive or Delete. A category with entries is archived and can be restored; an unused one is deleted for good. See [review.md](review.md) for how categories are assigned to entries and [ai.md](ai.md) for how the description and rules feed suggestions.

## Sub-features

- list-and-tabs: the table of categories per Active or Archived tab, with counts, three stat cards (Active categories or Archived categories, Time tracked, Count as work) and the "Time range" picker.
- search: the "Search categories" field (matches name, description and hints).
- create-edit: the "New category" / "Edit" sheet with name, colour swatch, "What counts as ..." description, "+ Add apps & sites rules" and the "Count toward work hours" switch.
- counts-as-work: the per-row switch named "Count NAME toward work hours".
- reorder: drag a row to change the order; the top three get shortcut keys 1 to 3.
- archive-delete-restore: "Archive category..." (when it has entries) or "Delete category..." (when unused), confirmed, and "Restore" on the Archived tab.

## How to get to it

Sidebar > Work > Categories (button "Categories"). Settings > Tracking & work > "Count toward Work Hours" has a "Manage categories" link that opens this page (see [settings.md](settings.md)).

## Driving it

A fresh session is a brand-new install that may open on first-run onboarding (see [onboarding.md](onboarding.md)). Seed through `rize`:

```bash
scripts/app-drive.sh start
scripts/app-drive.sh rize -- categories add "Deep work" --color "#75a4e5" --description "Focused building" --work true
scripts/app-drive.sh rize -- categories add "Admin" --work false
scripts/app-drive.sh rize -- --json categories list
```

Open the page and use it:

```bash
scripts/app-drive.sh click "Categories"
scripts/app-drive.sh wait "+ New category"
scripts/app-drive.sh tree
scripts/app-drive.sh fill "Search categories" "Deep"
scripts/app-drive.sh click "Time range"
scripts/app-drive.sh click "Archived"
scripts/app-drive.sh click "Active"
scripts/app-drive.sh click "Count Admin toward work hours"
```

Create and edit through the sheet:

```bash
scripts/app-drive.sh click "+ New category"
scripts/app-drive.sh wait "New category"
scripts/app-drive.sh fill "Name" "Research"
scripts/app-drive.sh fill -r AXTextArea "What counts as Research?" "Reading and note taking"
scripts/app-drive.sh click "+ Add apps & sites rules"
scripts/app-drive.sh fill -r AXTextArea "Apps & sites" "arxiv.org, notion.so"
scripts/app-drive.sh click "Create category"
scripts/app-drive.sh click "Edit"
scripts/app-drive.sh click "Archive category..."
scripts/app-drive.sh click "Delete category..."
```

After `click "Edit"` or a category's name the sheet is titled "Edit NAME". The destructive button of the confirm dialog is "Archive" (used category) or "Delete" (unused one); `tree` shows which, and "Cancel" backs out. Restore from the Archived tab:

```bash
scripts/app-drive.sh click "Archived"
scripts/app-drive.sh click "Restore"
```

Backend side:

```bash
scripts/app-drive.sh rize -- --json categories list
scripts/app-drive.sh rize -- categories edit "Research" --work false
scripts/app-drive.sh rize -- categories edit "Research" --archived true
scripts/app-drive.sh rize -- categories rm "Research" --dry-run
scripts/app-drive.sh rize -- --yes categories rm "Research"
```

## What proves it works

- `wait "+ New category"` plus `wait "Search categories"` prove the page rendered (the heading "Categories" collides with the sidebar button name).
- A created category appears as a button named after it in `tree`, and `rize --json categories list` returns it with the same `name`, `color`, `description` and `countsAsWork`. Both are required for a PASS.
- Toggle: after clicking the "Count NAME toward work hours" switch, `categories list` shows `countsAsWork` flipped (the switch's checked state also appears in `tree`). The "Count as work" stat card text `N of M` changes with it.
- Archive of a category with entries: `categories list` shows `archived: true` and it appears on the Archived tab; the Archived tab count changes. Delete of an unused one: it is absent from `categories list`. `rize categories rm` reports which of the two happened (`removal` in `--json`).
- Restore: back on the Active tab, `archived: false`.
- Order: `sort` values in `categories list` change after a reorder. Reordering needs a drag, so verify it with `rize` read-back only if you can perform one (see Not reachable).
- Visual: `shot` for colours, the shortcut-key badges 1 to 3 and layout.

## Gotchas

- Row names are dynamic. A category row has two buttons with the category name's text: the name button (named exactly the category name) and the "Edit" button; "Edit" appears once per row, so with several categories `click "Edit"` is ambiguous; use `-i N` or click the name.
- The switch labels contain the category name ("Count Admin toward work hours"); the sheet's switch is named "Count toward work hours". With the sheet open both may be in the tree; the page's per-row ones start with "Count " and end with the category name.
- Tabs are named with a count suffix ("Active 2"); substring clicks work.
- The description field's name changes with the typed name: "What counts as this category?" while empty, then "What counts as NAME?". Read it from `tree` after typing the name.
- The "Apps & sites" box only exists after clicking "+ Add apps & sites rules" (or when the category already has hints).
- The "Built-in prompt" block with its "Clear" button is shown only when the category has an AI prompt and the on-device model is available (see [ai.md](ai.md)).
- Colour swatches are radios named "Colour #hex"; take the hex from `tree`.
- Deleting a used category is not possible from this page; it archives. Undo of a delete is available with the keyboard undo (see [undo.md](undo.md)).
- Reordering is not exposed by `rize` yet (listed as Planned in COMMANDS.md).

## Not reachable

- Drag-and-drop reorder: rows are HTML5 draggable `li` elements, and neither driver here issues a drag (macOS uses AXPress; the Windows driver sends clicks and keys). The reorder itself cannot be driven or observed.
- Hover tooltips ("Keyboard shortcut N in review panel") are not in the accessibility tree.
