# Website screenshots

How the OpenRize screenshots on offlinestudios.net are made, so they can be remade identically.
The screenshots show one fictional working day (Monday, September 28, 2026). The day lives in
`seed.json`; nothing about the images is hand-edited.

| File | What it is |
|---|---|
| `seed.json` | The day: clients, projects, 11 time entries with their app activity, and the AI confidence on the three entries still waiting for review. |
| `seed.mjs` | Loads `seed.json` into an OpenRize `activity.db`. Idempotent: it replaces only rows whose id starts with `seed-`. |
| `freeze-time.js` | Pins the webview's clock to 4:41 PM on the seeded day, so the Calendar opens on it with the "now" line under the last entry. |
| `capture.sh` | Does everything: runs the app, seeds it, and shoots the window in dark and light. |

## Which version to shoot

The seed was designed against OpenRize **v0.4.11, commit `5abb172`** (Sep 28, 2026). Check that commit
out (a worktree is simplest, since `capture.sh` patches `index.html` while it runs) and point the script at it:

```sh
git worktree add --detach ../openrize-website-shots 5abb172
(cd ../openrize-website-shots && pnpm install)
docs/website-screenshots/capture.sh ../openrize-website-shots ./website-screenshots-out
```

Later versions look different (for example, from `1586b98` the day summary folds everything past the top
three categories into "Other"), and the database schema keeps moving, so `seed.mjs` may need updating
before it works on a newer checkout. Shooting a newer version is fine; the seed is the part that stays
identical. If you do, update the commit above.

## What the script does

1. Runs `pnpm tauri dev` from the checkout under its own app identifier
   (`com.offlinestudios.openrize.websiteshot`) and its own `XDG_CONFIG_HOME`. Settings (including the
   theme) live in `$XDG_CONFIG_HOME/openrize/settings.json`, so without that the run would use, and
   change, your real settings. Your real data is never touched.
2. Launches once to create and migrate the database, then runs `seed.mjs` on it.
3. Injects `freeze-time.js` into the checkout's `index.html` (restored on exit).
4. Launches the app per theme with a 1492 x 937 pt window, captures it by window id with the system
   shadow, and composes it at 2x onto a 1604 x 1050 canvas with the window's corner at (56, 38).

It needs macOS (System Events and `screencapture`), a display at least that large, `magick`
(ImageMagick) and `sqlite3`. Accessibility permission for your terminal is required for window placement.

## Using the images

The script writes lossless `openrize-dark.png` and `openrize-light.png`. For the website, shrink them
with a 1024-color palette (about 600 KB each; 256 colors visibly dithers the text) and drop them in as
`public/apps/openrize/screenshot-dark.png` and `screenshot-light.png`:

```sh
magick openrize-dark.png -colors 1024 -define png:compression-level=9 -strip screenshot-dark.png
```

## Changing the day

Edit `seed.json`, not the app. Each entry's `activity` is `[app, minutes, window title]` and must add up
to the entry's length (`seed.mjs` refuses otherwise). The "Top apps" list, the activity strip and the
category totals are all derived from it. Entries with `"status": "pending"` and a `confidence` appear in
review: 0.85 and up reads as high, 0.6 to 0.85 as medium, below 0.6 as "Needs you".
