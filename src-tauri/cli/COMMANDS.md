# rize command reference

Every command `rize` has or is planned to have, the app command it maps to, and whether it
works while the app is closed. Keep this file current: a PR that adds, changes or removes a
command, or changes the app command behind one, updates its row here.

## How rize reaches the data

- **App running:** rize sends the request to the app's local endpoint (a Unix socket on macOS,
  a named pipe on Windows). The app stays the only writer while it runs, and open windows and the
  tray update at once.
- **App closed, or not installed at all:** rize opens the stored data itself and answers with
  the same code the app uses (`openrize_core::rpc`). Everything that reads or edits stored data
  works; nothing new is tracked until the app runs again, and it picks the edits up at launch. A
  lock file keeps the app and rize from ever holding the stores at once.
- **rize never creates the data.** Only the app makes and migrates the database. Without one,
  rize exits with "This laptop doesn't have any rize data. Are you sure you've installed the app
  before?"; with one from another app version, it fails with `INCOMPATIBLE` and leaves it alone.

There are two ways to get rize: it comes bundled with the app (`rize install-path` puts it on
PATH), or the curl installer (`install.sh`) installs it on its own. A standalone rize finds the
installed app only to launch it (`rize app start`, `rize track start`).

`rize app start` only launches the app (in the background, no window). Tracking is its own
command: `rize track start`, or `rize app start --track` to do both.

## Status

- **Shipped:** in the current binary.
- **M2:** next.
- **Later:** planned, not scheduled.

## Commands

In the "App closed" column, "Yes" means the command works with the app closed or not installed
at all, as long as the app has run once on this machine. "Needs app" means the command depends on
live app state, so rize says the app is not running and suggests `rize app start`. "Starts app"
means the command launches the app itself, because that is what it asks for, so it needs the app
installed.

| Command | What it does | App command | App closed | Status |
| --- | --- | --- | --- | --- |
| `rize` / `rize status [--full]` | Whether the app is running, tracking state, current activity, today's totals, entries to review and running timers (break state in `--json`). Titles and URLs only with `--full`. | `activity_snapshot`, `list_timers`, `break_state` | Stored totals and timers; no live activity | Shipped |
| `rize app start [--track] [--show]` | Launch the app in the background (menu bar only, no window or Dock icon). `--track` also turns tracking on; `--show` opens the window. Waits until the app answers. | launches the app with `--background` | Starts app | Shipped |
| `rize app status` | Running or not, app version and data directory. | `Hello` handshake | Yes | Shipped |
| `rize app open [--review]` | Show the window, on today's review queue with `--review`. | `open_main_window` | Starts app | Shipped |
| `rize app quit` | Close the open segment and exit, as the tray's Quit does. Returns once the app has released the data. | `AppHandle::exit` | Nothing to do | Shipped |
| `rize track start` | Turn tracking on, launching the app if needed. | `set_capture_enabled` | Starts app | Shipped |
| `rize track stop` | Turn tracking off. | `set_capture_enabled` | Yes (saved for next launch) | Shipped |
| `rize track idle <minutes>` | Set the idle threshold. | `set_idle_threshold` | Yes | Shipped |
| `rize focus start [label]` / `rize focus stop` | Start or end a manual focus session. | `start_session`, `stop_session` | Needs app | Shipped |
| `rize timers list` | All timers with elapsed time and state. | `list_timers` | Yes | Shipped |
| `rize timers new <label>` | Create a timer. | `create_timer` | Yes | Shipped |
| `rize timers start` / `pause` / `reset` / `rm <timer>` | Control or delete a timer by id or label; reset and rm are confirmed. | `start_timer`, `pause_timer`, `reset_timer`, `delete_timer` | Yes | Shipped |
| `rize timers rename <timer> <label>` | Rename a timer. | `rename_timer` | Yes | Shipped |
| `rize entries list` | Filter by range, project, client, category, app, status, billable, search and scope. Newest first, `--limit` 1 to 5000. | `query_time_entries` | Yes | Shipped |
| `rize entries show <id>` | One entry with its apps, events and AI suggestion; window titles with `--full`. | `get_entry_detail` | Yes | Shipped |
| `rize entries add --from --to [fields]` | Create a manual entry. | `create_time_entry` | Yes | Shipped |
| `rize entries edit <id...> [fields]` | Change description, project, category, billable, start or end on one or many entries; `none` clears a project or category. | `update_time_entry`, `update_time_entries` | Yes | Shipped |
| `rize entries approve <id...>` / `--all-pending [range]` | Approve entries, or every pending entry in a range. | `approve_time_entries` | Yes | Shipped |
| `rize entries unapprove <id...>` | Send entries back to review. | `unapprove_time_entries` | Yes | Shipped |
| `rize entries reject <id>` | Reject an AI suggestion. | `reject_time_entry` | Yes | Shipped |
| `rize entries split <id> --at <time>` | Split one entry in two. | `split_time_entry` | Yes | Shipped |
| `rize entries rm <id...>` | Delete entries (confirmed). | `delete_time_entries` | Yes | Shipped |
| `rize entries rebuild [range]` | Rebuild entries from recorded activity (confirmed). | `rebuild_time_entries` | Yes | Shipped |
| `rize entries export --format csv\|json [--out file]` | Export filtered entries to stdout or a file. | `export_time_entries` (`reports::export_body`) | Yes | Shipped |
| `rize review [range]` | Pending entries, today by default. | `query_time_entries` (pending) | Yes | Shipped |
| `rize report --by project\|client\|category\|app\|status\|none --per day\|week\|month\|total` | Totals for a range, with the filters of `entries list`. | `entry_rollup` | Yes | Shipped |
| `rize projects list` / `show <project>` | Projects; one with this month's time, budget and rules. | `list_projects`, `project_stats`, `project_rules` | Yes | Shipped |
| `rize projects add` / `edit` / `rm` | Manage projects; rm is confirmed. | `create_project`, `update_project`, `delete_project` | Yes | Shipped |
| `rize clients list` / `show` / `add` / `edit` / `rm` | Manage clients; show lists their projects; rm is confirmed. | `list_clients`, `create_client`, `update_client`, `delete_client` | Yes | Shipped |
| `rize categories list` / `add` / `edit` / `rm` | Manage categories; rm is confirmed. | `list_categories`, `create_category`, `update_category`, `delete_category` | Yes | Shipped |
| `rize settings list` / `get <key>` / `set <key> <value>` | Every preference by dotted key, such as `tracking-hours.start` or `breaks.enabled`. Writes one field at a time. | `get_settings`, `SettingsStore::patch` | Yes (applies on next launch) | Shipped |
| `rize paths` | Settings file, data directory and database path. | `storage_paths` | Yes | Shipped |
| `rize install-path` | Put `rize` on PATH: a symlink on macOS, the user PATH on Windows. | client only | Yes | Shipped |
| `rize completions bash\|zsh\|fish\|powershell\|elvish` | Print a shell completion script. | client only | Yes | Shipped |
| `rize projects discover` / `dismiss <key>` | Suggested projects from recent activity. | `discover_projects`, `dismiss_project_suggestion` | Yes | M2 |
| `rize projects import <file.csv>` | Bulk import projects. | `import_projects_csv` | Yes | M2 |
| `rize projects hints <project> --preview` | Preview which activity a hint list would match. | `preview_project_hints` | Yes | M2 |
| `rize apps list` / `edit <app>` | Default category or project per app; exclude or include an app. | `list_apps`, `update_app` | Yes | M2 |
| `rize invoices list` / `show <invoice>` | Invoices with status and totals. | `list_invoices`, `get_invoice` | Yes | M2 |
| `rize invoices billable --client --from --to` | Uninvoiced billable time for a client. | `list_billable_entries` | Yes | M2 |
| `rize invoices draft` / `quote` | Create or update a draft; `quote` prints totals without saving. | `save_invoice_draft`, `quote_invoice` | Yes | M2 |
| `rize invoices finalize` / `paid` / `unpaid` / `void` / `rm` | Invoice lifecycle; finalize and void are confirmed. | `finalize_invoice`, `set_invoice_paid`, `void_invoice`, `delete_draft_invoice` | Yes | M2 |
| `rize invoices pdf <invoice> --out file` | Write the PDF, the same bytes the app renders. | `get_invoice_pdf` | Yes | M2 |
| `rize invoices profile show` / `edit`, `logo set <file>` / `clear` | Sender profile and logo. | `get_invoice_profile`, `update_invoice_profile`, `set_invoice_logo`, `clear_invoice_logo` | Yes | M2 |
| `rize breaks status` / `start` / `end` / `snooze <min>` / `skip` / `extend` / `pause [--until]` | Control break reminders. | `break_state`, `start_break`, `end_break`, `snooze_break`, `skip_break`, `extend_break`, `pause_break_reminders` | Needs app | M2 |
| `rize breaks list --from --to` | Past breaks. | `list_breaks` | Yes | M2 |
| `rize ai status` / `rules` / `rules accept` / `reject <rule>` | Classifier state, queue and suggested rules. | `ai_status`, `resolve_rule_suggestion` | Needs app | M2 |
| `rize ai metrics [--days]` | Classification accuracy. | `ai_metrics` | Yes | M2 |
| `rize ai retrain` / `reset-learned --days` / `retry <entry>` | Retrain, forget recent corrections, or reclassify one entry (confirmed where destructive). | `ai_retrain`, `ai_reset_learned`, `retry_classification` | Needs app | M2 |
| `rize agents board` | Coding agent jobs in flight (needs `advancedWorkflowTracking` on). | `agent_board` | Needs app | M2 |
| `rize agents report --from --to` / `entries` / `confirm <job>` | Billed agent time and confirming a job. | `agent_report`, `list_agent_entries`, `confirm_agent_job` | Yes | M2 |
| `rize extensions list` | Herdr and tmux bridge status; switch them with `settings set extensions.<id>`. Empty until `settings set advancedWorkflowTracking true`. | `list_extensions` | Needs app | M2 |
| `rize energy summary [--days]` / `history` / `reset` | Battery and energy history. | `get_energy_summary`, `query_energy_history`, `reset_energy_history` | Yes | M2 |
| `rize login-item status` / `enable` / `disable` | Launch at login. | `launch_at_login`, `set_launch_at_login` | Needs app on macOS | M2 |
| `rize update status` / `check` / `install` | App updates. | `update_status`, `check_for_updates`, `install_update` | Needs app | M2 |
| `rize threads --from --to` | Per-day work threads, as the Calendar shows them. | `thread_days` | Yes | Later |
| `rize activity --from --to [--full]` | Raw activity segments; titles only with `--full`. | `activity_snapshot` | Yes | Later |
| `rize status --watch` | Live status that redraws on every change. | new streaming subscription | Needs app | Later |
| `rize ai classify` / `models` | Classification and model downloads for a non-Apple model backend. | new | Later | Later |

## Left to the window

These app commands only serve what is on screen, so rize leaves them out:
`resize_pulse_panel`, `hide_pulse_panel`, `resize_reminder_panel`, `expand_break_reminder`,
`open_break_settings`, `render_invoice_preview`, `get_invoice_logo`, `preview_break_chime`,
`mark_segment_reviewed` and the debug-only `dev_sample_break_reminder`. Appearance preferences
stay reachable through `rize settings set`.

## Conventions

| Area | Rule |
| --- | --- |
| Output | Plain aligned tables, no color. Durations as `3h 25m`, times local. |
| `--json` | One envelope for every command, errors included: `schemaVersion`, `ok`, `data`, `error { code, message }`. Adding a field is compatible; removing or renaming one bumps `schemaVersion`. |
| Exit codes | 0 success, 1 operation failed, 2 invalid arguments (including an unconfirmed deletion), 3 app needed but not running, 4 incompatible versions (the running app speaks another rize protocol, or the stored data is from another app version). |
| Error codes | `INVALID_ARGUMENT`, `NOT_FOUND`, `AMBIGUOUS` (with `candidates`), `NO_DATA`, `APP_NOT_RUNNING`, `INCOMPATIBLE`, `FAILED`. `NO_DATA` means the app never ran on this machine. |
| Dates | A bare date covers the whole local day (`--to 2026-09-30` includes that day); offset-less times are local. `today`, `yesterday`, weekday names, `this-week`, `last-week`, `this-month` and `last-month` work for `--from`, `--to` and `--period`; `--last 7d` (or `90m`, `12h`, `2w`) runs up to now. Weeks start on Monday. Ranges default to today. |
| Names | Projects, clients, categories and timers take an id, an exact name or a unique case-insensitive prefix; two matches fail with `AMBIGUOUS` and list both. Entries take a full id or its last characters (lists show the last eight). |
| Confirmation | Deleting, voiding, finalizing, resetting and rebuilding prompt on a terminal and need `--yes` otherwise. |
| Paging | `entries list` takes `--limit` (default 50) and reports `truncated` when more match. |
| Privacy | Window titles and URLs only with `--full`. |
| Global flags | `--json`, `--full`, `--yes` (`-y`), `--data-dir` (another app data directory, such as a dev build's). `RIZE_APP` points rize at a specific app binary to launch, for dev builds. |
