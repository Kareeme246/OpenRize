//! Read-only terminal client. All database access lives behind local IPC in
//! the on-demand service and shared Rust domain operations, never in commands.

mod ipc;
mod protocol;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use clap::{Parser, Subcommand, ValueEnum};

use ipc::Endpoint;
use openrize_core::paths;
use protocol::{Data, Operation, Request, Response};

#[derive(Parser, Debug)]
#[command(
    name = "rize",
    version,
    about = "Read-only OpenRize queries, even with the GUI closed",
    after_help = "With no command, shows stored totals. The local service starts on demand and exits after 30 idle seconds.\nExit codes: 0 success, 1 operation/service error, 2 invalid arguments.\nExample: rize entries list --from 2026-09-01 --to 2026-09-30 --json"
)]
struct Args {
    /// Stable schemaVersion=1 JSON response (including errors)
    #[arg(long, global = true)]
    json: bool,
    /// Existing app data directory, for isolated/dev stores (never created)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Private IPC location: a 0700 socket directory (Unix) or pipe namespace (Windows)
    #[arg(long, global = true)]
    runtime_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Stored entry totals only; does not start capture or report live GUI state
    Status,
    /// Query stored time entries without window titles or URLs
    Entries {
        #[command(subcommand)]
        command: EntriesCommand,
    },
    /// Opt-in: symlink this executable as <dir>/rize, without replacing files
    InstallPath {
        /// Destination directory; defaults to ~/.local/bin (add it to PATH yourself)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    #[command(name = "__serve", hide = true)]
    Serve,
}

#[derive(Subcommand, Debug)]
enum EntriesCommand {
    /// Newest first, bounded. Both inclusive bounds filter entry START time.
    List {
        /// Local date (from its start) or ISO 8601 date-time; no offset = local time
        #[arg(long, value_parser = parse_from)]
        from: u64,
        /// Local date (through its end) or ISO 8601 date-time, inclusive
        #[arg(long, value_parser = parse_to)]
        to: u64,
        #[arg(long, value_enum)]
        status: Option<EntryStatus>,
        /// 1..500, default 50; truncated=true if more entries match
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=500))]
        limit: u32,
        /// Opt into description (max 4096 characters), project ID, and billable
        #[arg(long)]
        full: bool,
    },
}

#[derive(ValueEnum, Clone, Debug)]
enum EntryStatus {
    Pending,
    Approved,
}

fn parse_from(input: &str) -> Result<u64, String> {
    parse_time(input, false)
}

fn parse_to(input: &str) -> Result<u64, String> {
    parse_time(input, true)
}

/// A bare date covers the whole local day, so `--from` takes its first and
/// `--to` its last millisecond. Date-times without an offset are local too.
fn parse_time(input: &str, end_of_day: bool) -> Result<u64, String> {
    let error = || {
        "expected a date (2026-09-01) or ISO 8601 date-time (2026-09-01T09:00:00, local unless it has an offset), precision <= milliseconds".to_string()
    };
    let utc = if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        let day = if end_of_day {
            date.succ_opt().ok_or_else(error)?
        } else {
            date
        };
        let midnight = local(day.and_time(NaiveTime::MIN)).ok_or_else(error)?;
        midnight - chrono::Duration::milliseconds(i64::from(end_of_day))
    } else if !input.contains('T') {
        return Err(error());
    } else if let Ok(date) = DateTime::parse_from_rfc3339(input) {
        date.with_timezone(&chrono::Utc)
    } else {
        let naive =
            NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S%.f").map_err(|_| error())?;
        local(naive).ok_or_else(|| format!("{input} does not exist in local time"))?
    };
    if utc.timestamp_subsec_nanos() % 1_000_000 != 0
        || utc.timestamp_subsec_nanos() >= 1_000_000_000
    {
        return Err(error());
    }
    u64::try_from(utc.timestamp_millis())
        .map_err(|_| "date-time must be on or after the Unix epoch".into())
}

/// A local wall-clock time in UTC. A time repeated by a DST change resolves
/// to its first occurrence; one skipped by it does not exist.
fn local(naive: NaiveDateTime) -> Option<DateTime<chrono::Utc>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(time) | LocalResult::Ambiguous(time, _) => {
            Some(time.with_timezone(&chrono::Utc))
        }
        LocalResult::None => None,
    }
}

fn main() {
    let wants_json = std::env::args_os().any(|arg| arg == "--json");
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let code = error.exit_code();
            if code == 0 {
                print!("{error}");
            } else {
                emit(
                    &Response::error("INVALID_ARGUMENT", error.to_string()),
                    wants_json,
                );
            }
            std::process::exit(code);
        }
    };
    if matches!(args.command, Some(Command::Serve)) {
        let result = data_dir(&args)
            .and_then(|data| Endpoint::new(&data, args.runtime_dir.as_deref()))
            .and_then(|endpoint| endpoint.serve());
        if let Err(error) = result {
            eprintln!("SERVICE_UNAVAILABLE: {}", quoted(&error));
            std::process::exit(1);
        }
        return;
    }
    let (response, exit) = execute(&args);
    emit(&response, args.json);
    std::process::exit(exit);
}

fn execute(args: &Args) -> (Response, i32) {
    if let Some(Command::InstallPath { dir }) = &args.command {
        return match install_path(dir.clone()) {
            Ok(data) => (Response::success(data), 0),
            Err(error) => (Response::error("INSTALL_FAILED", error), 1),
        };
    }
    let operation = match &args.command {
        None | Some(Command::Status) => Operation::Status {},
        Some(Command::Entries {
            command:
                EntriesCommand::List {
                    from,
                    to,
                    status,
                    limit,
                    full,
                },
        }) => {
            if from > to || *to >= i64::MAX as u64 {
                return (
                    Response::error(
                        "INVALID_ARGUMENT",
                        "--from must not exceed --to; bounds must fit epoch milliseconds",
                    ),
                    2,
                );
            }
            let status = status.as_ref().map(|s| {
                match s {
                    EntryStatus::Pending => "pending",
                    EntryStatus::Approved => "approved",
                }
                .to_string()
            });
            Operation::Entries {
                from: *from,
                to: *to,
                status,
                limit: *limit,
                full: *full,
            }
        }
        Some(Command::InstallPath { .. } | Command::Serve) => unreachable!(),
    };
    let endpoint =
        match data_dir(args).and_then(|data| Endpoint::new(&data, args.runtime_dir.as_deref())) {
            Ok(endpoint) => endpoint,
            Err(error) => return (Response::error("INVALID_PATH", error), 1),
        };
    match endpoint.call(&Request {
        protocol_version: protocol::VERSION,
        operation,
    }) {
        Ok(response) => {
            let exit = i32::from(!response.ok);
            (response, exit)
        }
        Err(error) => (Response::error("SERVICE_UNAVAILABLE", error), 1),
    }
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    std::path::absolute(path).map_err(|e| e.to_string())
}

fn data_dir(args: &Args) -> Result<PathBuf, String> {
    let data = match &args.data_dir {
        Some(path) => absolute(path)?,
        None => paths::app_data_dir().ok_or("no home directory; provide --data-dir")?,
    };
    // Canonicalize existing stores so symlink/relative aliases reuse the same
    // service. Missing stores remain missing and yield NO_DATA, not migrations.
    // Windows canonical paths are verbatim (`\\?\`), which SQLite should not
    // have to parse, so there the absolute path is the key.
    if cfg!(unix) {
        Ok(data.canonicalize().unwrap_or(data))
    } else {
        Ok(data)
    }
}

#[cfg(windows)]
fn install_path(_dir: Option<PathBuf>) -> Result<Data, String> {
    Err("not available on Windows, where symlinks need extra privileges; `cargo install` already puts rize on PATH".into())
}

#[cfg(unix)]
fn install_path(dir: Option<PathBuf>) -> Result<Data, String> {
    let dir = absolute(&match dir {
        Some(dir) => dir,
        None => std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is not set; pass --dir")?
            .join(".local/bin"),
    })?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let executable = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|e| e.to_string())?;
    let path = dir.join("rize");
    match std::fs::symlink_metadata(&path) {
        Ok(meta)
            if meta.file_type().is_symlink()
                && std::fs::read_link(&path).ok().as_ref() == Some(&executable) => {}
        Ok(_) => {
            return Err(format!(
                "{} already exists; nothing was replaced",
                path.display()
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(&executable, &path).map_err(|e| e.to_string())?;
        }
        Err(e) => return Err(e.to_string()),
    }
    Ok(Data::Installation {
        executable: executable.display().to_string(),
        path: path.display().to_string(),
    })
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn duration(ms: u64) -> String {
    format!(
        "{}h {:02}m {:02}.{:03}s",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

fn timestamp(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| format!("{ms} epoch-ms"))
}

fn emit(response: &Response, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string(response).expect("serializable response")
        );
        return;
    }
    if let Some(error) = &response.error {
        println!("{}: {}", error.code, quoted(&error.message));
        return;
    }
    match &response.data {
        Some(Data::Status(status)) => {
            println!("Stored entries (all time): {}\nPending review: {}\nTracked: {}\nBillable: {}", status.entries, status.pending, duration(status.tracked_ms), duration(status.billable_ms));
            if status.entries == 0 { println!("0 results"); }
            println!("Next: rize entries list --from <date> --to <date>");
        }
        Some(Data::Entries(page)) => {
            println!("{} results{}", page.count, if page.truncated { " (truncated; narrow the range or increase --limit, max 500)" } else { "" });
            for entry in &page.entries {
                println!("{}  {}  {}  {}", quoted(&entry.id), timestamp(entry.started_at), duration(entry.duration_ms), quoted(&entry.status));
                if let Some(detail) = &entry.detail {
                    println!("  description={}{}  project={}  billable={}", quoted(&detail.description), if detail.description_truncated { " (truncated at 4096 characters)" } else { "" }, detail.project_id.as_deref().map(quoted).unwrap_or_else(|| "none".into()), detail.billable);
                }
            }
        }
        Some(Data::Installation { path, .. }) => println!("Installed {}. Add its directory to PATH if needed; no shell configuration was changed.", quoted(path)),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn local_midnight(day: u32) -> u64 {
        let date = NaiveDate::from_ymd_opt(2026, 9, day).unwrap();
        local(date.and_time(NaiveTime::MIN))
            .unwrap()
            .timestamp_millis() as u64
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(
            parse_from("2026-09-01T01:00:00+01:00").unwrap(),
            parse_from("2026-09-01T00:00:00Z").unwrap()
        );
        assert_eq!(
            parse_from("2026-09-01T00:00:00").unwrap(),
            local_midnight(1)
        );
        assert_eq!(parse_from("2026-09-01").unwrap(), local_midnight(1));
        assert_eq!(parse_to("2026-09-01").unwrap(), local_midnight(2) - 1);
        assert_eq!(parse_to("2026-09-01T00:00:00").unwrap(), local_midnight(1));
        for value in [
            "2026-09-32",
            "09/01/2026",
            "2026-09-01T00:00:00.0001Z",
            "1969-01-01T00:00:00Z",
            "1969-01-01",
            "2026-09-01T00:00:60Z",
        ] {
            assert!(parse_from(value).is_err(), "accepted {value}");
        }
    }
}
