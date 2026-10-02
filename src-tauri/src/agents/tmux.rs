//! The tmux extension: reads which panes run an agent and whether they are
//! producing output, from the tmux servers on this machine.
//!
//! tmux alone cannot tell "waiting for an answer" from "finished" without
//! reading the screen, which this bridge never does (`capture-pane` is never
//! called, and the control client below is attached with `no-output`, so it
//! is never even sent pane output). It reports what tmux itself knows: the
//! pane's command, its working folder, when its window last had activity and
//! which pane is in front. A pane with output in the last 15 s is running;
//! the tracker turns a longer silence into "ready" (an estimate, labeled so).
//!
//! Each server also gets one read-only, size-ignoring control-mode client
//! (`tmux -C`) subscribed to pane changes, so a change wakes the poller at
//! once instead of waiting for the next tick.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use super::tracker::{PaneObservation, PaneState};

/// Output within this long ago means the agent is working.
pub const RUNNING_WINDOW_MS: u64 = 15_000;
/// A field separator that cannot appear in a path or a command name.
const SEP: char = '\u{1f}';
/// tmux was given this long to answer a one-shot command.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

/// Processes that are coding agents. The name tmux reports for the pane is the
/// only signal, so a generic runtime such as `node` is deliberately absent: a
/// dev server in a project folder would otherwise look like an agent.
const AGENTS: &[&str] = &[
    "claude",
    "codex",
    "gemini",
    "opencode",
    "aider",
    "cursor-agent",
    "amp",
    "goose",
    "droid",
    "kimi",
    "qwen",
    "copilot",
    "crush",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// The socket's file name inside tmux's directory: `default`, or the name
    /// given with `tmux -L`.
    pub id: String,
    pub socket: PathBuf,
}

#[cfg(unix)]
fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// `$TMUX_TMPDIR/tmux-<uid>` (default `/tmp/tmux-<uid>`).
#[cfg(unix)]
pub fn socket_dir() -> PathBuf {
    let base = std::env::var_os("TMUX_TMPDIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    base.join(format!("tmux-{}", uid()))
}

/// tmux has no native Windows build, so there are no servers to find.
#[cfg(windows)]
pub fn servers() -> Vec<Server> {
    Vec::new()
}

/// The tmux servers with a socket on disk. A stale socket is harmless: asking
/// it for panes just fails.
#[cfg(unix)]
pub fn servers() -> Vec<Server> {
    let Ok(entries) = std::fs::read_dir(socket_dir()) else {
        return Vec::new();
    };
    let mut found: Vec<Server> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            use std::os::unix::fs::FileTypeExt;
            entry.file_type().is_ok_and(|kind| kind.is_socket())
        })
        .map(|entry| Server {
            id: entry.file_name().to_string_lossy().into_owned(),
            socket: entry.path(),
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

pub fn binary() -> Option<PathBuf> {
    super::detect::which("tmux")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub detected: bool,
    pub detail: Option<String>,
}

pub fn detect() -> Detection {
    let servers = servers();
    if !servers.is_empty() {
        return Detection {
            detected: true,
            detail: Some(format!(
                "{} server{} running",
                servers.len(),
                if servers.len() == 1 { "" } else { "s" }
            )),
        };
    }
    match binary() {
        Some(path) => Detection {
            detected: true,
            detail: Some(format!("binary {}", path.display())),
        },
        None => Detection {
            detected: false,
            detail: None,
        },
    }
}

fn tmux() -> Command {
    let path = binary().unwrap_or_else(|| PathBuf::from("tmux"));
    let mut command = Command::new(path);
    command.stdin(Stdio::null()).stderr(Stdio::null());
    command
}

/// Runs one tmux command against a server and returns its stdout.
fn run(server: &Server, args: &[&str]) -> Result<String, String> {
    let mut child = tmux()
        .arg("-S")
        .arg(&server.socket)
        .args(args)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not run tmux: {error}"))?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    use std::io::Read;
                    let _ = stdout.read_to_string(&mut out);
                }
                return if status.success() {
                    Ok(out)
                } else {
                    Err(format!(
                        "tmux {} failed",
                        args.first().copied().unwrap_or("")
                    ))
                };
            }
            Ok(None) if started.elapsed() > COMMAND_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("tmux did not answer".to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn pane_format() -> String {
    [
        "#{pane_id}",
        "#{session_name}",
        "#{pane_current_command}",
        "#{pane_current_path}",
        "#{window_activity}",
        "#{pane_active}",
        "#{window_active}",
    ]
    .join(&SEP.to_string())
}

/// Reads the agent panes of one server.
pub fn poll(server: &Server, now: u64) -> Result<Vec<PaneObservation>, String> {
    let panes = run(server, &["list-panes", "-a", "-F", &pane_format()])?;
    // Control clients (ours) do not count as someone being at a terminal.
    let clients = run(
        server,
        &[
            "list-clients",
            "-F",
            &format!("#{{client_session}}{SEP}#{{client_control_mode}}"),
        ],
    )
    .unwrap_or_default();
    Ok(parse_panes(&server.id, &panes, &clients, now))
}

/// Sessions with at least one real (non-control) client attached.
fn attended_sessions(clients: &str) -> HashMap<String, usize> {
    let mut sessions = HashMap::new();
    for line in clients.lines() {
        let mut fields = line.split(SEP);
        let (Some(session), Some(control)) = (fields.next(), fields.next()) else {
            continue;
        };
        if control != "1" {
            *sessions.entry(session.to_string()).or_insert(0) += 1;
        }
    }
    sessions
}

pub fn is_agent_command(command: &str) -> bool {
    let name = command
        .rsplit('/')
        .next()
        .unwrap_or(command)
        .to_ascii_lowercase();
    AGENTS.contains(&name.as_str())
}

pub fn parse_panes(server_id: &str, panes: &str, clients: &str, now: u64) -> Vec<PaneObservation> {
    let attended = attended_sessions(clients);
    panes
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(SEP).collect();
            let [pane_id, session, command, path, activity, pane_active, window_active] =
                fields.as_slice()
            else {
                return None;
            };
            if !is_agent_command(command) {
                return None;
            }
            let output_at = activity.parse::<u64>().ok().map(|secs| secs * 1000);
            let running = output_at.is_some_and(|at| now.saturating_sub(at) <= RUNNING_WINDOW_MS);
            Some(PaneObservation {
                key: format!("tmux:{server_id}:{pane_id}"),
                agent: command.rsplit('/').next().unwrap_or(command).to_string(),
                state: if running {
                    PaneState::Running
                } else {
                    PaneState::Quiet
                },
                cwd: (!path.is_empty()).then(|| (*path).to_string()),
                focused: *pane_active == "1"
                    && *window_active == "1"
                    && attended.get(*session).copied().unwrap_or(0) > 0,
                output_at,
            })
        })
        .collect()
}

/// A control-mode client that signals `wake` whenever tmux reports a pane
/// change. Dropping it ends the client.
pub struct Watcher {
    child: Child,
    stop: Arc<AtomicBool>,
}

impl Watcher {
    /// Attaches read-only, ignoring size and pane output, then subscribes to
    /// each pane's command and activity. Waits briefly first: a command sent
    /// the instant `-C` attaches makes it exit.
    pub fn start(server: &Server, wake: Sender<()>) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let mut child = tmux()
            .arg("-S")
            .arg(&server.socket)
            .args([
                "-C",
                "attach-session",
                "-f",
                "read-only,ignore-size,no-output",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start the tmux control client: {error}"))?;
        let mut stdin = child.stdin.take().ok_or("no control client stdin")?;
        let stdout = child.stdout.take().ok_or("no control client stdout")?;
        let alive = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let _ = writeln!(
                stdin,
                "refresh-client -B agents:%*:#{{pane_current_command}}{SEP}#{{window_activity}}"
            );
            // Keep stdin open for as long as the client should live.
            while !alive.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.starts_with("%subscription-changed") && wake.send(()).is_err() {
                    break;
                }
            }
        });
        Ok(Self { child, stop })
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(fields: [&str; 7]) -> String {
        fields.join(&SEP.to_string())
    }

    #[test]
    fn only_known_agent_processes_are_panes_of_interest() {
        assert!(is_agent_command("claude"));
        assert!(is_agent_command("/opt/homebrew/bin/codex"));
        assert!(is_agent_command("Aider"));
        // A dev server in a project folder is not an agent.
        assert!(!is_agent_command("node"));
        assert!(!is_agent_command("zsh"));
        assert!(!is_agent_command("vim"));
    }

    #[test]
    fn a_pane_with_recent_output_is_running_and_a_quiet_one_is_not() {
        let now = 1_000_000_000_000;
        let panes = [
            row([
                "%3",
                "work",
                "claude",
                "/Users/me/Code/OpenRize",
                "999999995",
                "1",
                "1",
            ]),
            row([
                "%4",
                "work",
                "codex",
                "/Users/me/Code/Acme",
                "999999900",
                "0",
                "1",
            ]),
            row(["%5", "work", "zsh", "/Users/me", "999999999", "0", "1"]),
        ]
        .join("\n");
        let clients = format!("work{SEP}0\nwork{SEP}1\n");

        let parsed = parse_panes("default", &panes, &clients, now);

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].key, "tmux:default:%3");
        assert_eq!(parsed[0].state, PaneState::Running);
        assert_eq!(parsed[0].agent, "claude");
        assert_eq!(parsed[0].output_at, Some(999_999_995_000));
        assert!(parsed[0].focused, "active pane with a real client attached");
        assert_eq!(parsed[1].state, PaneState::Quiet);
        assert!(!parsed[1].focused, "inactive pane");
    }

    #[test]
    fn our_own_control_client_never_counts_as_someone_looking() {
        let now = 1_000_000_000_000;
        let panes = row(["%3", "work", "claude", "/p", "999999999", "1", "1"]);
        // Only a control client is attached to the session.
        let clients = format!("work{SEP}1\n");

        let parsed = parse_panes("default", &panes, &clients, now);

        assert!(!parsed[0].focused);
    }

    #[test]
    fn malformed_rows_are_skipped() {
        assert!(parse_panes("d", "garbage\n\n%1\tclaude", "", 0).is_empty());
    }
}
