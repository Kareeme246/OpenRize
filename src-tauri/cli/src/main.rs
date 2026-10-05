//! rize: OpenRize from the terminal. Every operation is the app's own; rize
//! parses arguments, reaches the app (see `client`) and prints the answer.
//! `COMMANDS.md` lists every command and the app command behind it.

mod client;
mod dates;
mod install;
mod render;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use clap::{Args as ClapArgs, CommandFactory, Parser, Subcommand, ValueEnum};
use openrize_core::models::{
    NewCategory, NewClient, NewProject, NewTimeEntry, UpdateCategory, UpdateClient, UpdateProject,
    UpdateTimeEntry,
};
use openrize_core::protocol::{code, Operation, Response};
use openrize_core::EntryFilter;
use serde_json::{json, Value};

use client::Client;

#[derive(Parser, Debug)]
#[command(
    name = "rize",
    version,
    about = "OpenRize from the terminal: see and edit your tracked time, with the app open or closed",
    after_help = "With no command, shows today's status.\n\
        The app does the tracking; rize reads and edits what it stored. With the app closed,\n\
        everything but tracking still works and the app picks the changes up at its next launch.\n\n\
        Times are local: a date (2026-09-01), a date-time (2026-09-01T09:00), today, yesterday,\n\
        a weekday (monday), this-week, last-week, this-month or last-month.\n\
        Projects, clients, categories and timers take an id, a name or the start of one name.\n\n\
        Exit codes: 0 success, 1 failed, 2 invalid arguments, 3 the app must be running, 4 app and rize versions differ."
)]
struct Args {
    /// Print one JSON envelope: schemaVersion, ok, data, error
    #[arg(long, global = true)]
    json: bool,
    /// Include window titles and URLs
    #[arg(long, global = true)]
    full: bool,
    /// Confirm deletions and resets without asking
    #[arg(long, short = 'y', global = true)]
    yes: bool,
    /// Another app data directory, such as a dev build's
    #[arg(long, global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Whether the app is running and tracking, today's totals, and timers
    Status,
    /// Start, show or quit the app
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// Turn automatic tracking on or off
    Track {
        #[command(subcommand)]
        command: TrackCommand,
    },
    /// Manual focus sessions (needs the app running)
    Focus {
        #[command(subcommand)]
        command: FocusCommand,
    },
    /// Stopwatch timers
    Timers {
        #[command(subcommand)]
        command: TimersCommand,
    },
    /// List, add, edit, approve and export time entries
    Entries {
        #[command(subcommand)]
        command: EntriesCommand,
    },
    /// Entries waiting for review (today unless a range is given)
    Review {
        #[command(flatten)]
        range: RangeArgs,
    },
    /// Totals grouped by project, client, category, app or status
    Report(ReportArgs),
    /// List, show, add, edit and delete projects
    Projects {
        #[command(subcommand)]
        command: ProjectsCommand,
    },
    /// List, show, add, edit and delete clients
    Clients {
        #[command(subcommand)]
        command: ClientsCommand,
    },
    /// List, add, edit and delete categories
    Categories {
        #[command(subcommand)]
        command: CategoriesCommand,
    },
    /// Read or change preferences by key, such as breaks.enabled
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    /// Where the settings file and database live
    Paths,
    /// Put rize on PATH: a symlink in ~/.local/bin (macOS), the user PATH (Windows)
    InstallPath {
        /// Folder for the symlink (macOS and Linux)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Remove the installed rize command from ~/.local/bin
    UninstallPath,
    /// Print a shell completion script
    Completions { shell: clap_complete::Shell },
}

#[derive(Subcommand, Debug)]
enum AppCommand {
    /// Launch the app in the background (menu bar only), if it is not running
    Start {
        /// Also turn tracking on
        #[arg(long)]
        track: bool,
        /// Open the window too
        #[arg(long)]
        show: bool,
    },
    /// Whether the app is running, and its version
    Status,
    /// Show the app's window
    Open {
        /// Open on today's review queue
        #[arg(long)]
        review: bool,
    },
    /// Quit the app (ends the current activity, as the menu's Quit does)
    Quit,
}

#[derive(Subcommand, Debug)]
enum TrackCommand {
    /// Turn tracking on, starting the app if needed
    Start,
    /// Turn tracking off
    Stop,
    /// Minutes without input before time counts as idle
    Idle { minutes: u64 },
}

#[derive(Subcommand, Debug)]
enum FocusCommand {
    /// Start a focus session
    Start { label: Option<String> },
    /// End the focus session
    Stop,
}

#[derive(Subcommand, Debug)]
enum TimersCommand {
    /// Every timer with its elapsed time
    List,
    /// Create a timer
    New { label: String },
    /// Start or resume a timer
    Start { timer: String },
    /// Pause a running timer
    Pause { timer: String },
    /// Set a timer back to zero (asks first)
    Reset { timer: String },
    /// Rename a timer
    Rename { timer: String, label: String },
    /// Delete a timer (asks first)
    Rm { timer: String },
}

#[derive(ClapArgs, Debug, Default)]
struct RangeArgs {
    /// today, yesterday, a weekday, this-week, last-week, this-month or last-month
    #[arg(long, conflicts_with_all = ["from", "to", "last"])]
    period: Option<String>,
    /// Start: a date, a date-time or a word such as monday
    #[arg(long)]
    from: Option<String>,
    /// End, inclusive for a date: --to 2026-09-30 covers that whole day
    #[arg(long)]
    to: Option<String>,
    /// The time up to now, such as 90m, 12h, 7d or 2w
    #[arg(long, conflicts_with_all = ["from", "to"])]
    last: Option<String>,
}

#[derive(ClapArgs, Debug, Default)]
struct FilterArgs {
    /// A project, or none
    #[arg(long)]
    project: Option<String>,
    /// A client, or none
    #[arg(long)]
    client: Option<String>,
    /// A category, or none
    #[arg(long)]
    category: Option<String>,
    /// An app name or website domain
    #[arg(long)]
    app: Option<String>,
    #[arg(long, value_enum)]
    status: Option<Status>,
    #[arg(long)]
    billable: Option<bool>,
    /// Words that must all appear in the description or a window title
    #[arg(long)]
    search: Option<String>,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Status {
    Pending,
    Approved,
}

#[derive(Subcommand, Debug)]
enum EntriesCommand {
    /// Entries in a range (today by default), newest first
    List {
        #[command(flatten)]
        range: RangeArgs,
        #[command(flatten)]
        filter: FilterArgs,
        /// 1 to 5000
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// One entry with its apps, events and AI suggestions
    Show { entry: String },
    /// Add an entry by hand
    Add {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long, short = 'd', default_value = "")]
        description: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        billable: Option<bool>,
    },
    /// Change one or more entries; `none` clears a project or category
    Edit {
        #[arg(required = true)]
        entries: Vec<String>,
        #[arg(long, short = 'd')]
        description: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        billable: Option<bool>,
        /// New start time
        #[arg(long)]
        from: Option<String>,
        /// New end time
        #[arg(long)]
        to: Option<String>,
    },
    /// Approve entries, or with --all-pending every pending entry in a range
    Approve {
        entries: Vec<String>,
        #[arg(long, conflicts_with = "entries")]
        all_pending: bool,
        #[command(flatten)]
        range: RangeArgs,
    },
    /// Send approved entries back to review
    Unapprove {
        #[arg(required = true)]
        entries: Vec<String>,
    },
    /// Reject an entry's AI suggestion
    Reject { entry: String },
    /// Split an entry in two at a time
    Split {
        entry: String,
        #[arg(long)]
        at: String,
    },
    /// Delete entries (asks first)
    Rm {
        #[arg(required = true)]
        entries: Vec<String>,
    },
    /// Rebuild entries from recorded activity (asks first)
    Rebuild {
        #[command(flatten)]
        range: RangeArgs,
    },
    /// Write entries as CSV or JSON to stdout or a file
    Export {
        #[arg(long, value_enum, default_value = "csv")]
        format: Format,
        /// File to write; stdout by default
        #[arg(long)]
        out: Option<PathBuf>,
        #[command(flatten)]
        range: RangeArgs,
        #[command(flatten)]
        filter: FilterArgs,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Format {
    Csv,
    Json,
}

#[derive(ClapArgs, Debug)]
struct ReportArgs {
    #[arg(long, value_enum, default_value = "project")]
    by: Group,
    /// One column per day, week or month, or a single total
    #[arg(long, value_enum, default_value = "total")]
    per: Per,
    #[command(flatten)]
    range: RangeArgs,
    #[command(flatten)]
    filter: FilterArgs,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Group {
    Project,
    Client,
    Category,
    App,
    Status,
    None,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Per {
    Day,
    Week,
    Month,
    Total,
}

#[derive(ClapArgs, Debug, Default)]
struct ProjectFields {
    /// A client, or none
    #[arg(long)]
    client: Option<String>,
    /// A hex color such as #75a4e5
    #[arg(long)]
    color: Option<String>,
    #[arg(long)]
    description: Option<String>,
    /// Hourly rate
    #[arg(long)]
    rate: Option<f64>,
    /// New entries default to billable
    #[arg(long)]
    billable: Option<bool>,
    /// Words that point activity at this project, one per line or comma
    #[arg(long)]
    hints: Option<String>,
    /// active or archived
    #[arg(long)]
    status: Option<String>,
    /// none, hours or amount
    #[arg(long)]
    budget_kind: Option<String>,
    #[arg(long)]
    budget: Option<f64>,
    /// total or monthly
    #[arg(long)]
    budget_period: Option<String>,
    /// Due date
    #[arg(long)]
    due: Option<String>,
}

#[derive(Subcommand, Debug)]
enum ProjectsCommand {
    /// Every project with its client and rate
    List,
    /// A project with this month's time, budget and matching rules
    Show { project: String },
    /// Create a project
    Add {
        name: String,
        #[command(flatten)]
        fields: ProjectFields,
    },
    /// Change a project; --client none removes its client
    Edit {
        project: String,
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        fields: ProjectFields,
    },
    /// Delete a project (asks first)
    Rm { project: String },
}

#[derive(ClapArgs, Debug, Default)]
struct ClientFields {
    #[arg(long)]
    email: Option<String>,
    #[arg(long)]
    address: Option<String>,
    /// Default hourly rate
    #[arg(long)]
    rate: Option<f64>,
    /// Currency code, such as USD
    #[arg(long)]
    currency: Option<String>,
    #[arg(long)]
    notes: Option<String>,
}

#[derive(Subcommand, Debug)]
enum ClientsCommand {
    /// Every client
    List,
    /// A client and its projects
    Show { client: String },
    /// Create a client
    Add {
        name: String,
        #[command(flatten)]
        fields: ClientFields,
    },
    /// Change a client
    Edit {
        client: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        archived: Option<bool>,
        #[command(flatten)]
        fields: ClientFields,
    },
    /// Delete a client (asks first)
    Rm { client: String },
}

#[derive(ClapArgs, Debug, Default)]
struct CategoryFields {
    /// A hex color such as #75a4e5
    #[arg(long)]
    color: Option<String>,
    #[arg(long)]
    description: Option<String>,
    /// New entries in it default to billable
    #[arg(long)]
    billable: Option<bool>,
    /// Counts toward work totals
    #[arg(long)]
    work: Option<bool>,
}

#[derive(Subcommand, Debug)]
enum CategoriesCommand {
    /// Every category
    List,
    /// Create a category
    Add {
        name: String,
        #[command(flatten)]
        fields: CategoryFields,
    },
    /// Change a category
    Edit {
        category: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        archived: Option<bool>,
        #[command(flatten)]
        fields: CategoryFields,
    },
    /// Delete a category (asks first)
    Rm { category: String },
}

#[derive(Subcommand, Debug)]
enum SettingsCommand {
    /// Every preference with its value
    List,
    /// One preference, by dotted key
    Get { key: String },
    /// Change one preference; the value is JSON (true, 40, "dark") or plain text
    Set { key: String, value: String },
}

/// What a command prints, and how.
enum Outcome {
    /// An answer from the app, rendered by `render`.
    Answer(Response),
    /// Text rize produced itself (completions, an export written to stdout).
    Raw(String),
}

fn main() {
    let wants_json = std::env::args_os().any(|arg| arg == "--json");
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            if error.exit_code() == 0 {
                print!("{error}");
                std::process::exit(0);
            }
            if wants_json {
                print_json(
                    &Response::error(code::INVALID_ARGUMENT, error.to_string()),
                    true,
                );
            } else {
                eprint!("{error}");
            }
            std::process::exit(2);
        }
    };
    let response = match run(&args) {
        Ok(Outcome::Raw(text)) => {
            print!("{text}");
            return;
        }
        Ok(Outcome::Answer(response)) => response,
        Err(response) => response,
    };
    if args.json {
        print_json(&response, args.full);
    } else if let Some(error) = &response.error {
        eprintln!("{}", error.message);
        for candidate in &error.candidates {
            eprintln!("  {candidate}");
        }
    } else if let Some(data) = &response.data {
        render::show(args.command.as_ref(), data, args.full);
    }
    std::process::exit(exit_code(&response));
}

fn exit_code(response: &Response) -> i32 {
    match response.code() {
        None => 0,
        Some(code::INVALID_ARGUMENT) => 2,
        Some(code::APP_NOT_RUNNING) => 3,
        Some(code::INCOMPATIBLE) => 4,
        Some(_) => 1,
    }
}

fn print_json(response: &Response, full: bool) {
    let mut value = serde_json::to_value(response).unwrap_or(Value::Null);
    if !full {
        render::redact(&mut value);
    }
    println!("{value}");
}

fn invalid(message: impl Into<String>) -> Response {
    Response::error(code::INVALID_ARGUMENT, message)
}

fn run(args: &Args) -> Result<Outcome, Response> {
    let command = match &args.command {
        Some(Command::Completions { shell }) => {
            let mut script = Vec::new();
            clap_complete::generate(*shell, &mut Args::command(), "rize", &mut script);
            return Ok(Outcome::Raw(String::from_utf8_lossy(&script).into_owned()));
        }
        Some(Command::InstallPath { dir }) => {
            return install::install(dir.clone())
                .map(|installation| Outcome::Answer(Response::success(installation)))
                .map_err(|error| Response::error(code::FAILED, error));
        }
        Some(Command::UninstallPath) => {
            return install::uninstall()
                .map(Outcome::Raw)
                .map_err(|error| Response::error(code::FAILED, error));
        }
        command => command,
    };
    let client = Client::new(args.data_dir.as_deref()).map_err(invalid)?;
    let today = dates::today();
    let ask = |operation: Operation| Ok(Outcome::Answer(client.request(operation)));
    let time = |input: &str| dates::instant(input, false, today).map_err(invalid);
    let confirm = |question: &str| confirm(args.yes, question);

    let Some(command) = command else {
        return ask(Operation::Status {
            day_start: dates::day_start(today),
        });
    };
    match command {
        Command::Status => ask(Operation::Status {
            day_start: dates::day_start(today),
        }),
        Command::App { command } => match command {
            AppCommand::Start { track, show } => {
                let launched = client
                    .start_app(*show)
                    .map_err(|error| Response::error(code::FAILED, error))?;
                if *track {
                    let response = client.request(Operation::TrackSet { enabled: true });
                    if !response.ok {
                        return Err(response);
                    }
                }
                if *show && !launched {
                    let response = client.request(Operation::AppOpen { review: false });
                    if !response.ok {
                        return Err(response);
                    }
                }
                let hello = client
                    .hello()
                    .map_err(|error| Response::error(code::FAILED, error))?;
                Ok(Outcome::Answer(Response::success(json!({
                    "launched": launched,
                    "tracking": *track,
                    "appVersion": hello.app_version,
                }))))
            }
            AppCommand::Status => ask(Operation::Hello {}),
            AppCommand::Open { review } => {
                if !client.running() {
                    client
                        .start_app(true)
                        .map_err(|error| Response::error(code::FAILED, error))?;
                    if !*review {
                        return Ok(Outcome::Answer(Response::success(json!({}))));
                    }
                }
                ask(Operation::AppOpen { review: *review })
            }
            AppCommand::Quit => {
                if !client.running() {
                    return Ok(Outcome::Answer(Response::success(
                        json!({ "alreadyClosed": true }),
                    )));
                }
                Ok(Outcome::Answer(client.quit_app()))
            }
        },
        Command::Track { command } => match command {
            TrackCommand::Start => {
                client
                    .start_app(false)
                    .map_err(|error| Response::error(code::FAILED, error))?;
                ask(Operation::TrackSet { enabled: true })
            }
            TrackCommand::Stop => ask(Operation::TrackSet { enabled: false }),
            TrackCommand::Idle { minutes } => ask(Operation::TrackIdle { minutes: *minutes }),
        },
        Command::Focus { command } => match command {
            FocusCommand::Start { label } => ask(Operation::FocusStart {
                label: label.clone(),
            }),
            FocusCommand::Stop => ask(Operation::FocusStop {}),
        },
        Command::Timers { command } => match command {
            TimersCommand::List => ask(Operation::TimersList {}),
            TimersCommand::New { label } => ask(Operation::TimerCreate {
                label: label.clone(),
            }),
            TimersCommand::Start { timer } => ask(Operation::TimerStart {
                timer: timer.clone(),
            }),
            TimersCommand::Pause { timer } => ask(Operation::TimerPause {
                timer: timer.clone(),
            }),
            TimersCommand::Reset { timer } => {
                confirm(&format!("Reset timer \"{timer}\" to zero?"))?;
                ask(Operation::TimerReset {
                    timer: timer.clone(),
                })
            }
            TimersCommand::Rename { timer, label } => ask(Operation::TimerRename {
                timer: timer.clone(),
                label: label.clone(),
            }),
            TimersCommand::Rm { timer } => {
                confirm(&format!("Delete timer \"{timer}\"?"))?;
                ask(Operation::TimerDelete {
                    timer: timer.clone(),
                })
            }
        },
        Command::Entries { command } => match command {
            EntriesCommand::List {
                range,
                filter,
                limit,
            } => ask(Operation::EntriesList {
                filter: entry_filter(range, filter, today)?,
                limit: *limit,
            }),
            EntriesCommand::Show { entry } => ask(Operation::EntryShow {
                entry: entry.clone(),
            }),
            EntriesCommand::Add {
                from,
                to,
                description,
                project,
                category,
                billable,
            } => ask(Operation::EntryCreate {
                entry: NewTimeEntry {
                    started_at: time(from)?,
                    ended_at: dates::instant(to, true, today).map_err(invalid)?,
                    description: description.clone(),
                    category_id: category.clone(),
                    project_id: project.clone(),
                    billable: *billable,
                    review: false,
                },
            }),
            EntriesCommand::Edit {
                entries,
                description,
                project,
                category,
                billable,
                from,
                to,
            } => {
                let patch = UpdateTimeEntry {
                    description: description.clone(),
                    category_id: category.clone(),
                    project_id: project.clone(),
                    started_at: from.as_deref().map(time).transpose()?,
                    ended_at: to
                        .as_deref()
                        .map(|to| dates::instant(to, true, today).map_err(invalid))
                        .transpose()?,
                    status: None,
                    billable: *billable,
                };
                if serde_json::to_value(&patch)
                    .ok()
                    .and_then(|value| {
                        value
                            .as_object()
                            .map(|fields| fields.values().all(Value::is_null))
                    })
                    .unwrap_or(true)
                {
                    return Err(invalid("nothing to change; pass a field such as --project"));
                }
                ask(Operation::EntriesEdit {
                    entries: entries.clone(),
                    patch,
                })
            }
            EntriesCommand::Approve {
                entries,
                all_pending,
                range,
            } => {
                if !*all_pending {
                    if entries.is_empty() {
                        return Err(invalid("name entries to approve, or pass --all-pending"));
                    }
                    return ask(Operation::EntriesApprove {
                        entries: entries.clone(),
                    });
                }
                let mut filter = entry_filter(range, &FilterArgs::default(), today)?;
                filter.status = Some("pending".into());
                let pending = client.request(Operation::EntriesList {
                    filter,
                    limit: 5_000,
                });
                let Some(list) = pending
                    .data
                    .as_ref()
                    .and_then(|data| data["entries"].as_array())
                else {
                    return Err(pending);
                };
                let ids: Vec<String> = list
                    .iter()
                    .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
                    .collect();
                if ids.is_empty() {
                    return Ok(Outcome::Answer(Response::success(json!([]))));
                }
                ask(Operation::EntriesApprove { entries: ids })
            }
            EntriesCommand::Unapprove { entries } => ask(Operation::EntriesUnapprove {
                entries: entries.clone(),
            }),
            EntriesCommand::Reject { entry } => ask(Operation::EntryReject {
                entry: entry.clone(),
            }),
            EntriesCommand::Split { entry, at } => ask(Operation::EntrySplit {
                entry: entry.clone(),
                at: time(at)?,
            }),
            EntriesCommand::Rm { entries } => {
                let count = entries.len();
                confirm(&format!(
                    "Delete {count} {}?",
                    if count == 1 { "entry" } else { "entries" }
                ))?;
                ask(Operation::EntriesDelete {
                    entries: entries.clone(),
                })
            }
            EntriesCommand::Rebuild { range } => {
                let (from, to) = span(range, "today", today)?;
                confirm(
                    "Rebuild entries from recorded activity? Edits to automatic entries in this range are replaced.",
                )?;
                ask(Operation::EntriesRebuild { from, to })
            }
            EntriesCommand::Export {
                format,
                out,
                range,
                filter,
            } => {
                let format = match format {
                    Format::Csv => "csv",
                    Format::Json => "json",
                };
                let response = client.request(Operation::EntriesExport {
                    filter: entry_filter(range, filter, today)?,
                    format: format.into(),
                });
                let Some(content) = response
                    .data
                    .as_ref()
                    .and_then(|data| data["content"].as_str())
                    .map(str::to_owned)
                else {
                    return Err(response);
                };
                match out {
                    Some(path) => {
                        std::fs::write(path, &content).map_err(|error| {
                            Response::error(code::FAILED, format!("{}: {error}", path.display()))
                        })?;
                        let count = response
                            .data
                            .as_ref()
                            .map_or(Value::Null, |data| data["count"].clone());
                        Ok(Outcome::Answer(Response::success(json!({
                            "path": path.display().to_string(),
                            "count": count,
                        }))))
                    }
                    None if args.json => Ok(Outcome::Answer(response)),
                    None => Ok(Outcome::Raw(content)),
                }
            }
        },
        Command::Review { range } => {
            let mut filter = entry_filter(range, &FilterArgs::default(), today)?;
            filter.status = Some("pending".into());
            ask(Operation::EntriesList {
                filter,
                limit: 5_000,
            })
        }
        Command::Report(report) => {
            let filter = entry_filter(&report.range, &report.filter, today)?;
            let per = match report.per {
                Per::Day => "day",
                Per::Week => "week",
                Per::Month => "month",
                Per::Total => "total",
            };
            let boundaries =
                dates::boundaries(filter.start_ms, filter.end_ms, per).map_err(invalid)?;
            let group_by = match report.by {
                Group::Project => "project",
                Group::Client => "client",
                Group::Category => "category",
                Group::App => "app",
                Group::Status => "status",
                Group::None => "none",
            };
            ask(Operation::Report {
                filter,
                boundaries,
                group_by: group_by.into(),
            })
        }
        Command::Projects { command } => match command {
            ProjectsCommand::List => ask(Operation::ProjectsList {}),
            ProjectsCommand::Show { project } => {
                let month = dates::named("this-month", today).expect("this-month");
                ask(Operation::ProjectShow {
                    project: project.clone(),
                    range_start: dates::day_start(month.start),
                    range_end: dates::day_start(month.end),
                    month_start: dates::day_start(month.start),
                })
            }
            ProjectsCommand::Add { name, fields } => ask(Operation::ProjectCreate {
                project: NewProject {
                    client_id: fields.client.clone(),
                    name: name.clone(),
                    color: fields
                        .color
                        .clone()
                        .unwrap_or_else(|| render::color_for(name)),
                    description: fields.description.clone(),
                    ai_hints: fields.hints.clone(),
                    status: fields.status.clone(),
                    due_date: fields.due.as_deref().map(time).transpose()?,
                    budget_kind: fields.budget_kind.clone(),
                    budget_value: fields.budget,
                    budget_period: fields.budget_period.clone(),
                    billable_default: fields.billable,
                    hourly_rate: fields.rate,
                },
            }),
            ProjectsCommand::Edit {
                project,
                name,
                fields,
            } => ask(Operation::ProjectEdit {
                project: project.clone(),
                patch: UpdateProject {
                    client_id: fields
                        .client
                        .as_ref()
                        .map(|client| (client != "none").then(|| client.clone())),
                    name: name.clone(),
                    color: fields.color.clone(),
                    description: fields.description.clone().map(Some),
                    ai_hints: fields.hints.clone().map(Some),
                    status: fields.status.clone(),
                    due_date: fields.due.as_deref().map(time).transpose()?.map(Some),
                    budget_kind: fields.budget_kind.clone(),
                    budget_value: fields.budget.map(Some),
                    budget_period: fields.budget_period.clone(),
                    billable_default: fields.billable,
                    hourly_rate: fields.rate.map(Some),
                },
            }),
            ProjectsCommand::Rm { project } => {
                confirm(&format!("Delete project \"{project}\"?"))?;
                ask(Operation::ProjectDelete {
                    project: project.clone(),
                })
            }
        },
        Command::Clients { command } => match command {
            ClientsCommand::List => ask(Operation::ClientsList {}),
            ClientsCommand::Show { client: reference } => ask(Operation::ClientShow {
                client: reference.clone(),
            }),
            ClientsCommand::Add { name, fields } => ask(Operation::ClientCreate {
                client: NewClient {
                    name: name.clone(),
                    email: fields.email.clone(),
                    address: fields.address.clone(),
                    default_rate: fields.rate,
                    currency: fields.currency.clone(),
                    notes: fields.notes.clone(),
                },
            }),
            ClientsCommand::Edit {
                client: reference,
                name,
                archived,
                fields,
            } => ask(Operation::ClientEdit {
                client: reference.clone(),
                patch: UpdateClient {
                    name: name.clone(),
                    email: fields.email.clone().map(Some),
                    address: fields.address.clone().map(Some),
                    default_rate: fields.rate.map(Some),
                    currency: fields.currency.clone().map(Some),
                    notes: fields.notes.clone().map(Some),
                    archived: *archived,
                },
            }),
            ClientsCommand::Rm { client: reference } => {
                confirm(&format!("Delete client \"{reference}\"?"))?;
                ask(Operation::ClientDelete {
                    client: reference.clone(),
                })
            }
        },
        Command::Categories { command } => match command {
            CategoriesCommand::List => ask(Operation::CategoriesList {}),
            CategoriesCommand::Add { name, fields } => ask(Operation::CategoryCreate {
                category: NewCategory {
                    name: name.clone(),
                    color: fields
                        .color
                        .clone()
                        .unwrap_or_else(|| render::color_for(name)),
                    description: fields.description.clone(),
                    ai_prompt: None,
                    billable_default: fields.billable,
                    counts_as_work: fields.work,
                    sort: None,
                },
            }),
            CategoriesCommand::Edit {
                category,
                name,
                archived,
                fields,
            } => ask(Operation::CategoryEdit {
                category: category.clone(),
                patch: UpdateCategory {
                    name: name.clone(),
                    color: fields.color.clone(),
                    description: fields.description.clone(),
                    ai_prompt: None,
                    billable_default: fields.billable,
                    counts_as_work: fields.work,
                    archived: *archived,
                    sort: None,
                },
            }),
            CategoriesCommand::Rm { category } => {
                confirm(&format!("Delete category \"{category}\"?"))?;
                ask(Operation::CategoryDelete {
                    category: category.clone(),
                })
            }
        },
        Command::Settings { command } => match command {
            SettingsCommand::List => ask(Operation::SettingsGet {}),
            SettingsCommand::Get { key } => {
                let response = client.request(Operation::SettingsGet {});
                let Some(settings) = &response.data else {
                    return Err(response);
                };
                let path = setting_path(key);
                let value = path
                    .split('.')
                    .try_fold(settings, |value, part| value.get(part))
                    .ok_or_else(|| invalid(format!("unknown setting {key}")))?;
                Ok(Outcome::Answer(Response::success(value)))
            }
            SettingsCommand::Set { key, value } => ask(Operation::SettingsSet {
                key: setting_path(key),
                value: serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.clone())),
            }),
        },
        Command::Paths => ask(Operation::Paths {}),
        Command::InstallPath { .. } | Command::UninstallPath | Command::Completions { .. } => {
            unreachable!()
        }
    }
}

/// `tracking-hours.start` and `trackingHours.start` name the same setting.
fn setting_path(key: &str) -> String {
    key.split('.')
        .map(|part| {
            let mut camel = String::with_capacity(part.len());
            let mut upper = false;
            for c in part.chars() {
                if c == '-' || c == '_' {
                    upper = true;
                } else if upper {
                    camel.extend(c.to_uppercase());
                    upper = false;
                } else {
                    camel.push(c);
                }
            }
            camel
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn span(
    range: &RangeArgs,
    default: &str,
    today: chrono::NaiveDate,
) -> Result<(u64, u64), Response> {
    dates::range(
        range.period.as_deref(),
        range.from.as_deref(),
        range.to.as_deref(),
        range.last.as_deref(),
        default,
        today,
    )
    .map_err(invalid)
}

fn entry_filter(
    range: &RangeArgs,
    filter: &FilterArgs,
    today: chrono::NaiveDate,
) -> Result<EntryFilter, Response> {
    let (start_ms, end_ms) = span(range, "today", today)?;
    Ok(EntryFilter {
        start_ms,
        end_ms,
        category_id: filter.category.clone(),
        project_id: filter.project.clone(),
        client_id: filter.client.clone(),
        app: filter.app.clone(),
        status: filter.status.map(|status| {
            match status {
                Status::Pending => "pending",
                Status::Approved => "approved",
            }
            .into()
        }),
        billable: filter.billable,
        search: filter.search.clone(),
    })
}

/// Asks on a terminal; elsewhere only `--yes` goes ahead, so a script never
/// waits on a prompt or deletes by accident.
fn confirm(yes: bool, question: &str) -> Result<(), Response> {
    if yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Err(invalid(format!("{question} Pass --yes to confirm.")));
    }
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    if matches!(answer.trim(), "y" | "Y" | "yes") {
        Ok(())
    } else {
        Err(Response::error(code::FAILED, "Nothing was changed."))
    }
}
