# rize command reference

Every command `rize` has or is planned to have, the app command it maps to, and whether it
works while the app is closed. Keep this file current: a PR that adds, changes or removes a
command, or changes the app command behind one, updates its row here.

## How rize reaches the data

- **App running:** rize sends the request to the app's local endpoint (a Unix socket on macOS,
  a named pipe on Windows), and the app runs the same command function its window uses. The app
  stays the only writer while it runs, and open windows and the tray update at once.
- **App closed:** rize runs the same domain code from `openrize-core` against the app's data
  directly. Everything that reads or edits stored data still works; nothing new is tracked until
  the app runs again.
- **No data at all:** rize exits with "No rize tracking information found. Are you sure you've
  installed the app?"

`rize app start` only launches the app (in the background, no window). Tracking is its own
command: `rize track start`, or `rize app start --track` to do both.

## Status

- **Shipped:** in the current binary.
- **M1:** the first launch.
- **M2:** the release after it.
- **Later:** planned, not scheduled.

## Commands

"Needs app" means the command depends on live app state, so rize says the app is not running
and suggests `rize app start`. "Starts app" means the command launches the app itself, because
that is what it asks for.

| Command | What it does | App command | App closed | Status |
| --- | --- | --- | --- | --- |
| `rize` / `rize status [--full]` | Whether the app is running, tracking state, current activity, today's totals, unreviewed count, running timers and break state. Titles and URLs only with `--full`. | `activity_snapshot`, `list_timers`, `break_state` | Stored totals and timers; no live activity | Shipped (stored totals only), M1 |
| `rize app start [--track] [--show]` | Launch the app in the background. `--track` also turns tracking on; `--show` opens the window. Waits until the endpoint answers. | launches the app | Starts app | M1 |
| `rize app status` | Running or not, app version, protocol version, data directory. | handshake | Yes | M1 |
| `rize app open [page]` | Show the window, optionally on a page (timesheet, review, calendar, projects). | `open_main_window` + a page argument | Starts app | M1 |
| `rize app quit` | Close the open segment and exit, as the tray's Quit does. | new | Nothing to do | M1 |
| `rize track start` | Turn tracking on, launching the app if needed. | `set_capture_enabled` | Starts app | M1 |
| `rize track stop` | Turn tracking off. | `set_capture_enabled` | Yes (saved for next launch) | M1 |
| `rize track idle <minutes>` | Set the idle threshold. | `set_idle_threshold` | Yes | M1 |
| `rize focus start [label]` / `rize focus stop` | Start or end a manual focus session. | `start_session`, `stop_session` | Needs app | M1 |
| `rize timers list` | All timers with elapsed time and state. | `list_timers` | Yes | M1 |
| `rize timers new <label>` | Create a timer. | `create_timer` | Yes | M1 |
| `rize timers start` / `pause` / `reset` / `rm <timer>` | Control or delete a timer by id or label. | `start_timer`, `pause_timer`, `reset_timer`, `delete_timer` | Yes | M1 |
| `rize timers rename <timer> <label>` | Rename a timer. | `rename_timer` | Yes | M1 |
| `rize entries list` | Filter by range, project, client, category, app, status, billable, search and scope. Paged, newest first. | `query_time_entries` | Yes | Shipped (range and status only), M1 |
| `rize entries show <id>` | One entry with its events, segments and AI suggestion. | `get_entry_detail` | Yes | M1 |
| `rize entries add --from --to [fields]` | Create a manual entry. | `create_time_entry` | Yes | M1 |
| `rize entries edit <id...> [fields]` | Change description, project, category, billable, start or end on one or many entries. | `update_time_entry`, `update_time_entries` | Yes | M1 |
| `rize entries approve` / `unapprove <id...>` | Approve or reopen entries; `--pending --from --to` approves a whole range. | `approve_time_entries`, `unapprove_time_entries` | Yes | M1 |
| `rize entries reject <id>` | Reject an AI suggestion. | `reject_time_entry` | Yes | M1 |
| `rize entries split <id> --at <time>` | Split one entry in two. | `split_time_entry` | Yes | M1 |
| `rize entries rm <id...>` | Delete entries (confirmed). | `delete_time_entry`, `delete_time_entries` | Yes | M1 |
| `rize entries rebuild --from --to` | Rebuild entries from raw activity (confirmed). | `rebuild_time_entries` | Yes | M1 |
| `rize entries export --format csv\|json [--out file]` | Export filtered entries to stdout or a file. | `export_time_entries`, split so it returns bytes | Yes | M1 |
| `rize review` | Today's pending queue, the list the Review sheet shows. | `query_time_entries` (pending) | Yes | M1 |
| `rize report --by project\|client\|category\|app\|status --per day\|week\|month` | Totals for a range, with the filters of `entries list`. | `entry_rollup` | Yes | M1 |
| `rize projects list` / `show <project>` | Projects with budget, rate, stats and rules. | `list_projects`, `project_stats`, `project_rules` | Yes | M1 |
| `rize projects add` / `edit` / `rm` | Manage projects. | `create_project`, `update_project`, `delete_project` | Yes | M1 |
| `rize clients list` / `show` / `add` / `edit` / `rm` | Manage clients. | `list_clients`, `create_client`, `update_client`, `delete_client` | Yes | M1 |
| `rize categories list` / `add` / `edit` / `rm` | Manage categories. | `list_categories`, `create_category`, `update_category`, `delete_category` | Yes | M1 |
| `rize settings list` / `get <key>` / `set <key> <value>` | Every preference by dotted key, such as `tracking-hours.start` or `breaks.enabled`. Writes one field at a time. | `get_settings` + a new field-level patch | Yes (applies on next launch) | M1 |
| `rize paths` | Settings file, data directory and database path. | `storage_paths` | Yes | M1 |
| `rize install-path` | Put `rize` on PATH: a symlink on macOS, the user PATH on Windows. | client only | Yes | Shipped (macOS), M1 |
| `rize completions bash\|zsh\|fish\|powershell` | Print a shell completion script. | client only | Yes | M1 |
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
| `rize agents board` | Coding agent jobs in flight. | `agent_board` | Needs app | M2 |
| `rize agents report --from --to` / `entries` / `confirm <job>` | Billed agent time and confirming a job. | `agent_report`, `list_agent_entries`, `confirm_agent_job` | Yes | M2 |
| `rize extensions list` | Herdr and tmux bridge status; switch them with `settings set extensions.<id>`. | `list_extensions` | Needs app | M2 |
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
| Output | Aligned tables on a terminal; color off with `NO_COLOR` or `--no-color`. Durations as `3h 25m`, times local. |
| `--json` | One envelope for every command, errors included: `schemaVersion`, `ok`, `data`, `error { code, message }`. Adding a field is compatible; removing or renaming one bumps `schemaVersion`. |
| Exit codes | 0 success, 1 operation failed, 2 invalid arguments (shipped); 3 app needed but not running, 4 app and rize versions incompatible (M1). |
| Dates | A bare date covers the whole local day; offset-less times are local (shipped). `today`, `yesterday`, weekday names, `this-week`, `last-week`, `this-month`, `last-month` and `--last 7d` (M1). |
| Names | Projects, clients, categories and timers take an id, an exact name or a unique case-insensitive prefix; two matches fail with `AMBIGUOUS` and list both. |
| Confirmation | Deleting, voiding, finalizing, resetting and rebuilding prompt on a terminal and need `--yes` otherwise. |
| Paging | Lists take `--limit` (default 50) and report `truncated` when more match. |
| Privacy | Window titles and URLs only with `--full`. |
