//! Read-only terminal client. All database access lives behind local IPC in
//! the on-demand service and shared Rust domain operations, never in commands.
#[cfg(not(unix))]
compile_error!("The OpenRize CLI currently requires Unix local sockets (macOS release target).");

mod ipc;
mod protocol;

use std::path::PathBuf;

use chrono::{DateTime, NaiveDateTime};
use clap::{Parser, Subcommand, ValueEnum};

use ipc::Endpoint;
use protocol::{Data, Operation, Request, Response};

#[derive(Parser, Debug)]
#[command(
    name = "openrize",
    version,
    about = "Read-only OpenRize queries, even with the GUI closed",
    after_help = "With no command, shows stored totals. The local service starts on demand and exits after 30 idle seconds.\nExit codes: 0 success, 1 operation/service error, 2 invalid arguments.\nExample: openrize entries list --from 2026-09-01T00:00:00 --to 2026-09-30T23:59:59Z --json"
)]
struct Args {
    /// Stable schemaVersion=1 JSON response (including errors)
    #[arg(long, global = true)]
    json: bool,
    /// Existing app data directory, for isolated/dev stores (never created)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Private (0700) IPC directory; defaults to the OpenRize CLI cache
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
    /// Opt-in: symlink this executable as <dir>/openrize, without replacing files
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
        /// ISO 8601 date-time, omitted offset = UTC; date-only inputs rejected
        #[arg(long, value_parser = parse_time)]
        from: u64,
        /// Inclusive ISO 8601 date-time at millisecond precision
        #[arg(long, value_parser = parse_time)]
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

fn parse_time(input: &str) -> Result<u64, String> {
    let error = || {
        "expected ISO 8601 date-time (e.g. 2026-09-01T00:00:00Z); omitted offset is UTC, precision <= milliseconds".to_string()
    };
    if !input.contains('T') {
        return Err(error());
    }
    let date = DateTime::parse_from_rfc3339(input)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S%.f").map(|dt| dt.and_utc())
        })
        .map_err(|_| error())?;
    if date.timestamp_subsec_nanos() % 1_000_000 != 0
        || date.timestamp_subsec_nanos() >= 1_000_000_000
    {
        return Err(error());
    }
    u64::try_from(date.timestamp_millis())
        .map_err(|_| "date-time must be on or after the Unix epoch".into())
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
        let result = paths(&args)
            .and_then(|(data, runtime)| Endpoint::new(&data, &runtime))
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
    let endpoint = match paths(args).and_then(|(data, runtime)| Endpoint::new(&data, &runtime)) {
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

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set; provide explicit directories".into())
}

fn absolute(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path)
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|e| e.to_string())
    }
}

fn paths(args: &Args) -> Result<(PathBuf, PathBuf), String> {
    // Matches Tauri 2 app_data_dir on macOS: data_dir/bundle_identifier.
    let identifier = openrize_core::APP_IDENTIFIER;
    let data = match &args.data_dir {
        Some(path) => absolute(path.clone())?,
        None if cfg!(target_os = "macos") => {
            home()?.join("Library/Application Support").join(identifier)
        }
        None => {
            return Err(
                "default store discovery currently supports macOS only; provide --data-dir".into(),
            )
        }
    };
    // Canonicalize existing stores so symlink/relative aliases reuse the same
    // service. Missing stores remain missing and yield NO_DATA, not migrations.
    let data = data.canonicalize().unwrap_or(data);
    let runtime = match &args.runtime_dir {
        Some(path) => absolute(path.clone())?,
        None => home()?.join("Library/Caches").join(identifier).join("cli"),
    };
    Ok((data, runtime))
}

fn install_path(dir: Option<PathBuf>) -> Result<Data, String> {
    let dir = absolute(match dir {
        Some(dir) => dir,
        None => home()?.join(".local/bin"),
    })?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let executable = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|e| e.to_string())?;
    let path = dir.join("openrize");
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
            println!("Next: openrize entries list --from <datetime> --to <datetime>");
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
    #[test]
    fn exact_dates() {
        assert_eq!(
            parse_time("2026-09-01T00:00:00").unwrap(),
            parse_time("2026-09-01T00:00:00Z").unwrap()
        );
        assert_eq!(
            parse_time("2026-09-01T01:00:00+01:00").unwrap(),
            parse_time("2026-09-01T00:00:00Z").unwrap()
        );
        for value in [
            "2026-09-01",
            "2026-09-01T00:00:00.0001Z",
            "1969-01-01T00:00:00Z",
            "2026-09-01T00:00:60Z",
        ] {
            assert!(parse_time(value).is_err(), "accepted {value}");
        }
    }
}
