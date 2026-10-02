//! Human output. `--json` prints the app's answer as is (see `redact`);
//! everything here only formats it for a terminal.

use chrono::{Local, TimeZone};
use serde_json::Value;

use crate::{
    AppCommand, CategoriesCommand, ClientsCommand, Command, EntriesCommand, Group, Per,
    ProjectsCommand, SettingsCommand, TimersCommand, TrackCommand,
};

/// Keys that carry window titles or URLs, left out unless `--full`.
const PRIVATE_KEYS: [&str; 3] = ["title", "titles", "url"];

/// The muted categorical palette in `src/lib/palette.ts`; a new project or
/// category gets the color its name hashes to there.
const PALETTE: [&str; 10] = [
    "#75a4e5", "#56c2b1", "#52b788", "#df84b5", "#e4817d", "#9aa6b4", "#bfa181", "#707bf0",
    "#66b1df", "#9b87df",
];

/// FNV-1a over UTF-16 code units, as `slotFor` in `src/lib/palette.ts`.
pub fn color_for(name: &str) -> String {
    let mut hash: u32 = 0x811c9dc5;
    for unit in name.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x01000193);
    }
    PALETTE[hash as usize % PALETTE.len()].to_string()
}

/// Removes window titles and URLs.
pub fn redact(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for key in PRIVATE_KEYS {
                fields.remove(key);
            }
            fields.values_mut().for_each(redact);
        }
        Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

pub fn show(command: Option<&Command>, data: &Value, full: bool) {
    let mut data = data.clone();
    if !full {
        redact(&mut data);
    }
    let data = &data;
    match command {
        None | Some(Command::Status) => status(data),
        Some(Command::App { command }) => match command {
            AppCommand::Start { .. } => {
                let version = text(&data["appVersion"]);
                if data["launched"].as_bool() == Some(true) {
                    println!("OpenRize {version} is running in the background.");
                } else {
                    println!("OpenRize {version} was already running.");
                }
                if data["tracking"].as_bool() == Some(true) {
                    println!("Tracking is on.");
                }
            }
            AppCommand::Status => {
                let version = text(&data["appVersion"]);
                if data["running"].as_bool() == Some(true) {
                    println!("OpenRize {version} is running.");
                } else {
                    println!("OpenRize {version} is not running; nothing new is being tracked.");
                    println!("Start it with `rize app start`.");
                }
                println!("Data: {}", text(&data["dataDir"]));
            }
            AppCommand::Open { .. } => {}
            AppCommand::Quit => {
                if data["alreadyClosed"].as_bool() == Some(true) {
                    println!("OpenRize was not running.");
                } else {
                    println!("OpenRize quit.");
                }
            }
        },
        Some(Command::Track { command }) => match command {
            TrackCommand::Start | TrackCommand::Stop => {
                if data["captureEnabled"].as_bool() == Some(true) {
                    println!("Tracking is on.");
                } else {
                    println!("Tracking is off.");
                }
            }
            TrackCommand::Idle { .. } => println!(
                "Time counts as idle after {} minutes without input.",
                data["idleThresholdMinutes"]
            ),
        },
        Some(Command::Focus { .. }) => status(data),
        Some(Command::Timers { command }) => match command {
            TimersCommand::List
            | TimersCommand::New { .. }
            | TimersCommand::Start { .. }
            | TimersCommand::Pause { .. }
            | TimersCommand::Reset { .. }
            | TimersCommand::Rename { .. }
            | TimersCommand::Rm { .. } => timers(data),
        },
        Some(Command::Entries { command }) => match command {
            EntriesCommand::List { .. } => entries(data),
            EntriesCommand::Show { .. } => entry_detail(data, full),
            EntriesCommand::Add { .. } => {
                println!("Added entry {}.", short(&data["id"]));
            }
            EntriesCommand::Edit { .. } => {
                println!("Updated {}.", count(data, "entry", "entries"));
            }
            EntriesCommand::Approve { .. } => {
                println!("Approved {}.", count(data, "entry", "entries"));
            }
            EntriesCommand::Unapprove { .. } => {
                println!("Sent {} back to review.", count(data, "entry", "entries"));
            }
            EntriesCommand::Reject { .. } => {
                println!("Rejected the suggestion on {}.", short(&data["id"]));
            }
            EntriesCommand::Split { .. } => {
                let parts = data.as_array().cloned().unwrap_or_default();
                println!(
                    "Split into {}.",
                    parts.iter().map(short).collect::<Vec<_>>().join(" and ")
                );
            }
            EntriesCommand::Rm { .. } => {
                println!("Deleted {}.", count(&data["deleted"], "entry", "entries"));
            }
            EntriesCommand::Rebuild { .. } => {
                println!("Rebuilt {}.", count(data, "entry", "entries"));
            }
            EntriesCommand::Export { .. } => {
                println!(
                    "Wrote {} entries to {}.",
                    data["count"],
                    text(&data["path"])
                );
            }
        },
        Some(Command::Review { .. }) => {
            if data["entries"].as_array().is_some_and(Vec::is_empty) {
                println!("Nothing to review.");
            } else {
                entries(data);
            }
        }
        Some(Command::Report(report)) => self::report(data, report.per, report.by),
        Some(Command::Projects { command }) => match command {
            ProjectsCommand::List => projects(&data["projects"], &data["names"]),
            ProjectsCommand::Show { .. } => project(data),
            ProjectsCommand::Add { .. } | ProjectsCommand::Edit { .. } => {
                println!(
                    "Saved project {} ({}).",
                    text(&data["name"]),
                    text(&data["id"])
                );
            }
            ProjectsCommand::Rm { .. } => println!("Deleted the project."),
        },
        Some(Command::Clients { command }) => match command {
            ClientsCommand::List => clients(data),
            ClientsCommand::Show { .. } => {
                let client = &data["client"];
                println!("{}", text(&client["name"]));
                field("Email", &client["email"]);
                field("Address", &client["address"]);
                field(
                    "Rate",
                    &client["defaultRate"]
                        .as_f64()
                        .map_or(Value::Null, |rate| format!("{rate:.2}/h").into()),
                );
                field("Currency", &client["currency"]);
                field("Notes", &client["notes"]);
                println!();
                projects(&data["projects"], &data["names"]);
            }
            ClientsCommand::Add { .. } | ClientsCommand::Edit { .. } => {
                println!(
                    "Saved client {} ({}).",
                    text(&data["name"]),
                    text(&data["id"])
                );
            }
            ClientsCommand::Rm { .. } => println!("Deleted the client."),
        },
        Some(Command::Categories { command }) => match command {
            CategoriesCommand::List => categories(data),
            CategoriesCommand::Add { .. } | CategoriesCommand::Edit { .. } => {
                println!(
                    "Saved category {} ({}).",
                    text(&data["name"]),
                    text(&data["id"])
                );
            }
            CategoriesCommand::Rm { .. } => println!("Deleted the category."),
        },
        Some(Command::Settings { command }) => match command {
            SettingsCommand::List => {
                let mut lines = Vec::new();
                flatten("", data, &mut lines);
                for (key, value) in lines {
                    println!("{key} = {value}");
                }
            }
            SettingsCommand::Get { .. } => println!("{data}"),
            SettingsCommand::Set { key, .. } => {
                let path = crate::setting_path(key);
                let value = path
                    .split('.')
                    .try_fold(data, |value, part| value.get(part))
                    .unwrap_or(&Value::Null);
                println!("{path} = {value}");
            }
        },
        Some(Command::Paths) => {
            println!("Settings: {}", text(&data["configFile"]));
            println!("Data:     {}", text(&data["dataDir"]));
            println!("Database: {}", text(&data["databaseFile"]));
        }
        Some(Command::InstallPath { .. }) => {
            if cfg!(windows) {
                println!(
                    "{} is on your PATH{}. Open a new terminal to use rize.",
                    text(&data["path"]),
                    if data["unchanged"].as_bool() == Some(true) {
                        " already"
                    } else {
                        ""
                    }
                );
            } else {
                println!(
                    "{} points to {}. Add its folder to PATH if it is not there yet.",
                    text(&data["path"]),
                    text(&data["executable"])
                );
            }
        }
        Some(Command::Completions { .. }) => {}
    }
}

fn status(data: &Value) {
    let activity = &data["activity"];
    let version = text(&data["appVersion"]);
    if data["running"].as_bool() == Some(true) {
        let tracking = if activity["captureEnabled"].as_bool() != Some(true) {
            "tracking is off"
        } else if activity["trackingActive"].as_bool() == Some(true) {
            "tracking"
        } else {
            "outside tracking hours"
        };
        println!("OpenRize {version} is running, {tracking}.");
        if let Some(current) = activity["current"].as_object() {
            let started = current["startedAt"].as_u64().unwrap_or(0);
            let name = match current["kind"].as_str() {
                Some("focus") | Some("break") => current["label"]
                    .as_str()
                    .unwrap_or(current["app"].as_str().unwrap_or("")),
                _ => current["app"].as_str().unwrap_or(""),
            };
            let title = current
                .get("title")
                .and_then(Value::as_str)
                .filter(|title| !title.is_empty())
                .map(|title| format!(" - {}", clip(title, 60)))
                .unwrap_or_default();
            println!(
                "Now:    {name}{title}, {}",
                duration(now().saturating_sub(started))
            );
        }
    } else {
        println!("OpenRize {version} is not running; nothing new is being tracked.");
        println!("        Start it with `rize app start`.");
    }
    let mut today = vec![format!("{} tracked", duration(ms(&activity["trackedMs"])))];
    if ms(&activity["focusMs"]) > 0 {
        today.push(format!("{} focus", duration(ms(&activity["focusMs"]))));
    }
    if ms(&activity["breakMs"]) > 0 {
        today.push(format!("{} on breaks", duration(ms(&activity["breakMs"]))));
    }
    match data["pendingEntries"].as_u64().unwrap_or(0) {
        0 => {}
        1 => today.push("1 entry to review".into()),
        n => today.push(format!("{n} entries to review")),
    }
    println!("Today:  {}", today.join(", "));
    let running: Vec<String> = data["timers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|timer| !timer["startedAt"].is_null())
        .map(|timer| format!("{} {}", text(&timer["label"]), duration(elapsed(timer))))
        .collect();
    if !running.is_empty() {
        println!("Timers: {}", running.join(", "));
    }
}

fn timers(data: &Value) {
    let rows: Vec<Vec<String>> = data
        .as_array()
        .into_iter()
        .flatten()
        .map(|timer| {
            vec![
                text(&timer["label"]),
                duration(elapsed(timer)),
                if timer["startedAt"].is_null() {
                    "paused".into()
                } else {
                    "running".into()
                },
            ]
        })
        .collect();
    if rows.is_empty() {
        println!("No timers. Create one with `rize timers new <label>`.");
    } else {
        table(&["TIMER", "ELAPSED", "STATE"], rows);
    }
}

fn entries(data: &Value) {
    let names = &data["names"];
    let list = data["entries"].as_array().cloned().unwrap_or_default();
    let mut total = 0;
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|entry| {
            let start = ms(&entry["startedAt"]);
            let end = ms(&entry["endedAt"]);
            total += end.saturating_sub(start);
            vec![
                short(&entry["id"]),
                day(start),
                format!("{}-{}", clock(start), clock(end)),
                duration(end.saturating_sub(start)),
                text(&entry["status"]),
                name(names, &entry["projectId"]),
                name(names, &entry["categoryId"]),
                clip(&text(&entry["description"]), 50),
            ]
        })
        .collect();
    if rows.is_empty() {
        println!("No entries in this range.");
        return;
    }
    table(
        &[
            "ID",
            "DATE",
            "TIME",
            "DURATION",
            "STATUS",
            "PROJECT",
            "CATEGORY",
            "DESCRIPTION",
        ],
        rows,
    );
    println!(
        "\n{}, {} total{}",
        count(&Value::Array(list), "entry", "entries"),
        duration(total),
        if data["truncated"].as_bool() == Some(true) {
            "; more match, so narrow the range or raise --limit"
        } else {
            ""
        }
    );
}

fn entry_detail(data: &Value, full: bool) {
    let detail = &data["detail"];
    let names = &data["names"];
    let entry = &detail["entry"];
    let start = ms(&entry["startedAt"]);
    let end = ms(&entry["endedAt"]);
    println!("Entry {}", text(&entry["id"]));
    println!(
        "  {} {}-{}, {}",
        day(start),
        clock(start),
        clock(end),
        duration(end.saturating_sub(start))
    );
    let or_none = |value: String| {
        Value::String(if value.is_empty() {
            "none".into()
        } else {
            value
        })
    };
    field("Status", &entry["status"]);
    field("Project", &or_none(name(names, &entry["projectId"])));
    field("Category", &or_none(name(names, &entry["categoryId"])));
    field("Billable", &yes_no(&entry["billable"]).into());
    field("Source", &entry["source"]);
    field("Description", &entry["description"]);
    let apps = detail["apps"].as_array().cloned().unwrap_or_default();
    if !apps.is_empty() {
        println!("\nApps");
        table(
            &["APP", "TIME", "SHARE"],
            apps.iter()
                .map(|app| {
                    vec![
                        text(&app["app"]),
                        duration(ms(&app["durationMs"])),
                        format!("{:.0}%", app["percentage"].as_f64().unwrap_or(0.0)),
                    ]
                })
                .collect(),
        );
    }
    if full {
        let titles = detail["titles"].as_array().cloned().unwrap_or_default();
        if !titles.is_empty() {
            println!("\nWindows");
            table(
                &["APP", "TIME", "TITLE"],
                titles
                    .iter()
                    .map(|title| {
                        vec![
                            text(&title["app"]),
                            duration(ms(&title["durationMs"])),
                            clip(&text(&title["title"]), 80),
                        ]
                    })
                    .collect(),
            );
        }
    }
    let events = detail["events"].as_array().cloned().unwrap_or_default();
    if !events.is_empty() {
        println!("\nHistory");
        table(
            &["WHEN", "EVENT", "BY"],
            events
                .iter()
                .map(|event| {
                    let at = ms(&event["at"]);
                    vec![
                        format!("{} {}", day(at), clock(at)),
                        text(&event["kind"]),
                        text(&event["actor"]),
                    ]
                })
                .collect(),
        );
    }
}

fn report(data: &Value, per: Per, by: Group) {
    let none = match by {
        Group::Project => "No project",
        Group::Client => "No client",
        Group::Category => "Uncategorized",
        _ => "All",
    };
    let names = &data["names"];
    let boundaries: Vec<u64> = data["boundaries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect();
    let buckets = boundaries.len().saturating_sub(1);
    // group key -> (per-bucket ms, entries, billable ms)
    let mut groups: Vec<(String, Vec<u64>, u64, u64)> = Vec::new();
    for cell in data["cells"].as_array().into_iter().flatten() {
        let label = match cell["key"].as_str() {
            Some(key) => names[key].as_str().unwrap_or(key).to_string(),
            None => none.into(),
        };
        let index = match groups.iter().position(|(name, ..)| *name == label) {
            Some(index) => index,
            None => {
                groups.push((label, vec![0; buckets], 0, 0));
                groups.len() - 1
            }
        };
        let group = &mut groups[index];
        if let Some(slot) = group
            .1
            .get_mut(cell["bucket"].as_u64().unwrap_or(0) as usize)
        {
            *slot += ms(&cell["ms"]);
        }
        group.2 += cell["entries"].as_u64().unwrap_or(0);
        group.3 += ms(&cell["billableMs"]);
    }
    if groups.is_empty() {
        println!("No time in this range.");
        return;
    }
    groups.sort_by_key(|(_, times, ..)| std::cmp::Reverse(times.iter().sum::<u64>()));
    if matches!(per, Per::Total) {
        let rows = groups
            .iter()
            .map(|(name, times, entries, billable)| {
                vec![
                    name.clone(),
                    duration(times.iter().sum()),
                    entries.to_string(),
                    duration(*billable),
                ]
            })
            .collect();
        table(&["GROUP", "TIME", "ENTRIES", "BILLABLE"], rows);
    } else {
        let labels: Vec<String> = boundaries
            .iter()
            .take(buckets)
            .map(|start| {
                let format = match per {
                    Per::Day => "%a %-d",
                    Per::Week => "%b %-d",
                    _ => "%b %Y",
                };
                local(*start).format(format).to_string()
            })
            .collect();
        let mut headers = vec!["GROUP".to_string()];
        headers.extend(labels);
        headers.push("TOTAL".into());
        let rows = groups
            .iter()
            .map(|(name, times, ..)| {
                let mut row = vec![name.clone()];
                row.extend(times.iter().map(|time| match time {
                    0 => "-".to_string(),
                    time => duration(*time),
                }));
                row.push(duration(times.iter().sum()));
                row
            })
            .collect();
        let headers: Vec<&str> = headers.iter().map(String::as_str).collect();
        table(&headers, rows);
    }
    let total: u64 = groups
        .iter()
        .map(|(_, times, ..)| times.iter().sum::<u64>())
        .sum();
    println!("\nTotal {}", duration(total));
}

fn projects(list: &Value, names: &Value) {
    let rows: Vec<Vec<String>> = list
        .as_array()
        .into_iter()
        .flatten()
        .map(|project| {
            vec![
                text(&project["name"]),
                name(names, &project["clientId"]),
                text(&project["status"]),
                project["hourlyRate"]
                    .as_f64()
                    .map(|rate| format!("{rate:.2}/h"))
                    .unwrap_or_default(),
                yes_no(&project["billableDefault"]),
            ]
        })
        .collect();
    if rows.is_empty() {
        println!("No projects.");
    } else {
        table(&["PROJECT", "CLIENT", "STATUS", "RATE", "BILLABLE"], rows);
    }
}

fn project(data: &Value) {
    let names = &data["names"];
    let project = &data["project"];
    let stats = &data["stats"];
    println!("{}", text(&project["name"]));
    field("Client", &Value::String(name(names, &project["clientId"])));
    field("Status", &project["status"]);
    field(
        "Rate",
        &project["hourlyRate"]
            .as_f64()
            .map_or(Value::Null, |rate| format!("{rate:.2}/h").into()),
    );
    field("Billable", &yes_no(&project["billableDefault"]).into());
    if text(&project["budgetKind"]) != "none" && !project["budgetKind"].is_null() {
        let budget = format!(
            "{} {} ({})",
            project["budgetValue"],
            text(&project["budgetKind"]),
            text(&project["budgetPeriod"])
        );
        field("Budget", &budget.into());
    }
    field("Description", &project["description"]);
    if stats.is_object() {
        let month = format!(
            "{}, {} billable, {} ready to invoice",
            duration(ms(&stats["monthMs"])),
            duration(ms(&stats["billableMonthMs"])),
            duration(ms(&stats["unbilledMs"]))
        );
        field("This month", &month.into());
        field("All time", &duration(ms(&stats["totalMs"])).into());
    }
    let rules = data["rules"].as_array().cloned().unwrap_or_default();
    if !rules.is_empty() {
        println!("\nRules");
        table(
            &["MATCH", "PATTERN", "ORIGIN"],
            rules
                .iter()
                .map(|rule| {
                    vec![
                        text(&rule["matchKind"]),
                        text(&rule["pattern"]),
                        text(&rule["origin"]),
                    ]
                })
                .collect(),
        );
    }
}

fn clients(data: &Value) {
    let rows: Vec<Vec<String>> = data
        .as_array()
        .into_iter()
        .flatten()
        .map(|client| {
            vec![
                text(&client["name"]),
                text(&client["email"]),
                client["defaultRate"]
                    .as_f64()
                    .map(|rate| format!("{rate:.2}/h"))
                    .unwrap_or_default(),
                text(&client["currency"]),
                if client["archivedAt"].is_null() {
                    ""
                } else {
                    "archived"
                }
                .into(),
            ]
        })
        .collect();
    if rows.is_empty() {
        println!("No clients.");
    } else {
        table(&["CLIENT", "EMAIL", "RATE", "CURRENCY", ""], rows);
    }
}

fn categories(data: &Value) {
    let rows: Vec<Vec<String>> = data
        .as_array()
        .into_iter()
        .flatten()
        .map(|category| {
            vec![
                text(&category["name"]),
                yes_no(&category["countsAsWork"]),
                yes_no(&category["billableDefault"]),
                if category["archived"].as_bool() == Some(true) {
                    "archived"
                } else {
                    ""
                }
                .into(),
            ]
        })
        .collect();
    table(&["CATEGORY", "WORK", "BILLABLE", ""], rows);
}

fn flatten(prefix: &str, value: &Value, out: &mut Vec<(String, String)>) {
    match value.as_object() {
        Some(fields) if !fields.is_empty() || prefix.is_empty() => {
            for (key, value) in fields {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&path, value, out);
            }
        }
        _ => out.push((prefix.to_string(), value.to_string())),
    }
}

/// Columns padded to their widest cell; the last column is not padded.
fn table(headers: &[&str], rows: Vec<Vec<String>>) {
    let mut widths: Vec<usize> = headers
        .iter()
        .map(|header| header.chars().count())
        .collect();
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let line = |cells: Vec<String>| {
        let last = cells.len().saturating_sub(1);
        let text: Vec<String> = cells
            .into_iter()
            .enumerate()
            .map(|(index, cell)| {
                if index == last {
                    cell
                } else {
                    format!("{cell:<width$}", width = widths[index])
                }
            })
            .collect();
        println!("{}", text.join("  ").trim_end());
    };
    line(headers.iter().map(|header| header.to_string()).collect());
    for row in rows {
        line(row);
    }
}

/// One `Label:  value` line of a detail view; empty values are left out.
fn field(label: &str, value: &Value) {
    if !value.is_null() && value != "" {
        println!("  {:<13}{}", format!("{label}:"), text(value));
    }
}

fn name(names: &Value, id: &Value) -> String {
    match id.as_str() {
        Some(id) => names[id].as_str().unwrap_or(id).to_string(),
        None => String::new(),
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn yes_no(value: &Value) -> String {
    if value.as_bool() == Some(true) {
        "yes"
    } else {
        "no"
    }
    .into()
}

/// The last eight characters of an id, which `rize entries` accepts back.
fn short(value: &Value) -> String {
    let id = value
        .as_str()
        .or_else(|| value["id"].as_str())
        .unwrap_or_default();
    id[id.len().saturating_sub(8)..].to_string()
}

fn count(value: &Value, one: &str, many: &str) -> String {
    match value.as_array().map_or(1, Vec::len) {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

fn clip(text: &str, max: usize) -> String {
    let text = text.replace(['\n', '\t'], " ");
    if text.chars().count() <= max {
        return text;
    }
    let mut clipped: String = text.chars().take(max.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

fn ms(value: &Value) -> u64 {
    value.as_u64().unwrap_or(0)
}

fn elapsed(timer: &Value) -> u64 {
    let running = timer["startedAt"]
        .as_u64()
        .map_or(0, |started| now().saturating_sub(started));
    ms(&timer["accumulatedMs"]) + running
}

fn now() -> u64 {
    Local::now().timestamp_millis() as u64
}

fn duration(ms: u64) -> String {
    let minutes = ms / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, 0) => format!("{}s", ms / 1000),
        (0, minutes) => format!("{minutes}m"),
        (hours, minutes) => format!("{hours}h {minutes:02}m"),
    }
}

fn local(ms: u64) -> chrono::DateTime<Local> {
    Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .unwrap_or_default()
}

fn day(ms: u64) -> String {
    local(ms).format("%a %b %-d").to_string()
}

fn clock(ms: u64) -> String {
    local(ms).format("%H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_titles_and_urls_everywhere() {
        let mut value = json!({
            "current": { "app": "Safari", "title": "secret", "url": "https://x" },
            "titles": [{ "title": "secret" }],
            "segments": [{ "title": "secret", "app": "Code" }],
        });
        redact(&mut value);
        assert_eq!(
            value,
            json!({ "current": { "app": "Safari" }, "segments": [{ "app": "Code" }] })
        );
    }

    #[test]
    fn colors_match_the_frontend_palette() {
        // What slotFor picks in src/lib/palette.ts for the same names.
        assert_eq!(color_for("Acme"), "#9aa6b4");
        assert_eq!(color_for("Deep work"), "#e4817d");
    }

    #[test]
    fn durations() {
        assert_eq!(duration(42_000), "42s");
        assert_eq!(duration(25 * 60_000), "25m");
        assert_eq!(duration(65 * 60_000), "1h 05m");
    }
}
