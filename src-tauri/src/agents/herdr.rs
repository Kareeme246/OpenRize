//! The Herdr extension: reads which panes hold agents and what state each is
//! in, over Herdr's local socket API (newline-delimited JSON on
//! `~/.config/herdr/herdr.sock`, or `$HERDR_SOCKET_PATH`).
//!
//! Read-only metadata: `ping` and `agent.list`, plus an event subscription
//! that is only a wake-up signal (the state always comes from `agent.list`,
//! so an event shape that changes can never corrupt a job). This module never
//! calls `agent.read`, `pane.read` or `agent.send_keys`, and never starts,
//! stops or attaches a Herdr session.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::{json, Value};

use super::tracker::{PaneObservation, PaneState};

/// The wire protocol the event subscription was written against.
pub const PROTOCOL: u64 = 22;
const IO_TIMEOUT: Duration = Duration::from_secs(4);
/// Upper bound on one response line: an agent list is a few KB per pane.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Stable label for the server: `default`, a session name, or `custom`.
    pub id: String,
    pub path: PathBuf,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The sockets a local Herdr could be listening on. `HERDR_SOCKET_PATH` names
/// the one in use and replaces the defaults; otherwise the default server
/// plus every named session whose socket exists.
pub fn endpoints() -> Vec<Endpoint> {
    if let Some(path) = std::env::var_os("HERDR_SOCKET_PATH").filter(|p| !p.is_empty()) {
        return vec![Endpoint {
            id: "custom".to_string(),
            path: PathBuf::from(path),
        }];
    }
    let Some(root) = home().map(|home| home.join(".config").join("herdr")) else {
        return Vec::new();
    };
    let mut found = vec![Endpoint {
        id: "default".to_string(),
        path: root.join("herdr.sock"),
    }];
    if let Ok(sessions) = std::fs::read_dir(root.join("sessions")) {
        let mut named: Vec<Endpoint> = sessions
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path().join("herdr.sock");
                path.exists().then(|| Endpoint {
                    id: entry.file_name().to_string_lossy().into_owned(),
                    path,
                })
            })
            .collect();
        named.sort_by(|a, b| a.id.cmp(&b.id));
        found.extend(named);
    }
    found
}

/// What Herdr's presence looks like on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub detected: bool,
    /// Where it was found, for the Settings row.
    pub detail: Option<String>,
}

pub fn detect() -> Detection {
    if let Some(endpoint) = endpoints().into_iter().find(|e| e.path.exists()) {
        return Detection {
            detected: true,
            detail: Some(format!("socket {}", display_path(&endpoint.path))),
        };
    }
    match super::detect::which("herdr") {
        Some(binary) => Detection {
            detected: true,
            detail: Some(format!("binary {}", display_path(&binary))),
        },
        None => Detection {
            detected: false,
            detail: None,
        },
    }
}

fn display_path(path: &Path) -> String {
    match (home(), path.strip_prefix(home().unwrap_or_default())) {
        (Some(_), Ok(rest)) => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// A snapshot of the agents behind one socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Poll {
    pub protocol: u64,
    pub panes: Vec<PaneObservation>,
}

/// One request and its response over a fresh connection to the socket.
struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

impl Client {
    fn connect(path: &Path) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|error| {
            format!("could not connect to Herdr at {}: {error}", path.display())
        })?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
            .map_err(|error| error.to_string())?;
        let writer = stream.try_clone().map_err(|error| error.to_string())?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
            next: 1,
        })
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = format!("openrize-{}", self.next);
        self.next += 1;
        let mut line = json!({ "id": id, "method": method, "params": params }).to_string();
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .map_err(|error| format!("Herdr write failed: {error}"))?;
        loop {
            let reply = read_line(&mut self.reader)?;
            let value: Value = serde_json::from_str(&reply)
                .map_err(|error| format!("Herdr sent bad JSON: {error}"))?;
            // Anything that is not the answer to this request (an event that
            // raced it) is skipped.
            if value.get("id").and_then(Value::as_str) != Some(id.as_str()) {
                continue;
            }
            if let Some(error) = value.get("error") {
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(format!("Herdr {method} failed: {message}"));
            }
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| format!("Herdr {method} returned no result"));
        }
    }
}

fn read_line(reader: &mut BufReader<UnixStream>) -> Result<String, String> {
    let mut line = String::new();
    let read = reader
        .by_ref()
        .take(MAX_LINE_BYTES as u64)
        .read_line(&mut line)
        .map_err(|error| format!("Herdr read failed: {error}"))?;
    if read == 0 {
        return Err("Herdr closed the connection".to_string());
    }
    Ok(line)
}

/// Reads the agents behind a socket: `ping` for the protocol, `agent.list`
/// for the panes.
pub fn poll(endpoint: &Endpoint) -> Result<Poll, String> {
    let mut client = Client::connect(&endpoint.path)?;
    let pong = client.call("ping", json!({}))?;
    let protocol = pong.get("protocol").and_then(Value::as_u64).unwrap_or(0);
    let list = client.call("agent.list", json!({}))?;
    Ok(Poll {
        protocol,
        panes: parse_agent_list(&endpoint.id, &list),
    })
}

/// `AgentStatus` in Herdr's words, in the shared vocabulary.
pub fn state_of(status: &str) -> PaneState {
    match status {
        "working" => PaneState::Running,
        "blocked" => PaneState::NeedsYou,
        "done" => PaneState::Ready,
        "idle" => PaneState::Idle,
        _ => PaneState::Unknown,
    }
}

/// The panes of an `agent_list` result. Only metadata is read: the agent
/// name, state, working folder and whether the pane has focus.
pub fn parse_agent_list(endpoint_id: &str, result: &Value) -> Vec<PaneObservation> {
    let Some(agents) = result.get("agents").and_then(Value::as_array) else {
        return Vec::new();
    };
    agents
        .iter()
        .filter_map(|info| {
            let pane_id = info.get("pane_id").and_then(Value::as_str)?;
            let name = info
                .get("display_agent")
                .and_then(Value::as_str)
                .or_else(|| info.get("agent").and_then(Value::as_str))?;
            let status = info
                .get("agent_status")
                .and_then(Value::as_str)
                .unwrap_or("");
            let cwd = info
                .get("foreground_cwd")
                .and_then(Value::as_str)
                .or_else(|| info.get("cwd").and_then(Value::as_str))
                .map(str::to_string);
            Some(PaneObservation {
                key: format!("herdr:{endpoint_id}:{pane_id}"),
                agent: name.to_string(),
                state: state_of(status),
                cwd,
                focused: info
                    .get("focused")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                output_at: None,
            })
        })
        .collect()
}

/// The pane ids that hold agents, for the per-pane event subscriptions.
pub fn pane_ids(panes: &[PaneObservation], endpoint_id: &str) -> Vec<String> {
    let prefix = format!("herdr:{endpoint_id}:");
    panes
        .iter()
        .filter_map(|pane| pane.key.strip_prefix(&prefix).map(str::to_string))
        .collect()
}

/// Holds an event subscription open and signals `wake` for every event line.
/// Returns when the connection ends or `stop` is set. Events are only a
/// wake-up: a changed event shape cannot affect any state.
pub fn watch(
    endpoint: &Endpoint,
    panes: &[String],
    stop: &AtomicBool,
    wake: &Sender<()>,
) -> Result<(), String> {
    let mut client = Client::connect(&endpoint.path)?;
    let mut subscriptions: Vec<Value> = [
        "pane.agent_detected",
        "pane.focused",
        "pane.closed",
        "pane.exited",
        "tab.focused",
        "workspace.focused",
    ]
    .iter()
    .map(|kind| json!({ "type": kind }))
    .collect();
    subscriptions.extend(
        panes
            .iter()
            .map(|id| json!({ "type": "pane.agent_status_changed", "pane_id": id })),
    );
    client.call(
        "events.subscribe",
        json!({ "subscriptions": subscriptions }),
    )?;
    // Waiting for events: a short timeout so `stop` is noticed.
    client
        .reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| error.to_string())?;
    while !stop.load(Ordering::Relaxed) {
        let mut line = String::new();
        match client
            .reader
            .by_ref()
            .take(MAX_LINE_BYTES as u64)
            .read_line(&mut line)
        {
            Ok(0) => return Err("Herdr closed the event connection".to_string()),
            Ok(_) => {
                if wake.send(()).is_err() {
                    return Ok(());
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(format!("Herdr event read failed: {error}")),
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod fixture {
    //! A disposable synthetic Herdr: a Unix socket in a temp directory that
    //! answers the handful of requests the bridge makes, from a script. It is
    //! never pointed at a real server.

    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};

    pub struct FakeHerdr {
        pub path: PathBuf,
        pub agents: Arc<Mutex<Vec<Value>>>,
        /// Every method the bridge called, in order.
        pub calls: Arc<Mutex<Vec<String>>>,
        pub protocol: u64,
    }

    impl FakeHerdr {
        pub fn start(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "orz-herdr-{name}-{}-{}",
                std::process::id(),
                crate::timers::now_epoch_ms()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("herdr.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let fake = Self {
                path,
                agents: Arc::new(Mutex::new(Vec::new())),
                calls: Arc::new(Mutex::new(Vec::new())),
                protocol: super::PROTOCOL,
            };
            let agents = fake.agents.clone();
            let calls = fake.calls.clone();
            let protocol = fake.protocol;
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let agents = agents.clone();
                    let calls = calls.clone();
                    std::thread::spawn(move || serve(stream, &agents, &calls, protocol));
                }
            });
            fake
        }
    }

    fn serve(
        stream: UnixStream,
        agents: &Mutex<Vec<Value>>,
        calls: &Mutex<Vec<String>>,
        protocol: u64,
    ) {
        let mut writer = stream.try_clone().unwrap();
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let Ok(request) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let method = request["method"].as_str().unwrap_or("").to_string();
            calls.lock().unwrap().push(method.clone());
            let id = request["id"].clone();
            let result = match method.as_str() {
                "ping" => json!({ "type": "pong", "version": "0.9.3", "protocol": protocol }),
                "agent.list" => {
                    json!({ "type": "agent_list", "agents": agents.lock().unwrap().clone() })
                }
                "events.subscribe" => json!({ "type": "subscription_started" }),
                _ => {
                    let reply = json!({ "id": id, "error": { "code": "unknown_method", "message": "no such method" } });
                    let _ = writeln!(writer, "{reply}");
                    continue;
                }
            };
            let _ = writeln!(writer, "{}", json!({ "id": id, "result": result }));
            if method == "events.subscribe" {
                // Push one event, as a live server would on a state change.
                let _ = writeln!(
                    writer,
                    "{}",
                    json!({ "event": "pane.agent_status_changed", "data": { "pane_id": "p1", "workspace_id": "w1", "agent_status": "done" } })
                );
            }
        }
    }

    pub fn agent(pane: &str, status: &str, cwd: &str, focused: bool) -> Value {
        json!({
            "terminal_id": format!("t-{pane}"),
            "agent": "claude",
            "display_agent": "Claude Code",
            "agent_status": status,
            "workspace_id": "w1",
            "tab_id": "w1:1",
            "pane_id": pane,
            "focused": focused,
            "revision": 4,
            "cwd": "/Users/me",
            "foreground_cwd": cwd,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use super::fixture::{agent, FakeHerdr};
    use super::*;

    fn endpoint(fake: &FakeHerdr) -> Endpoint {
        Endpoint {
            id: "fixture".to_string(),
            path: fake.path.clone(),
        }
    }

    #[test]
    fn statuses_map_into_the_shared_vocabulary() {
        assert_eq!(state_of("working"), PaneState::Running);
        assert_eq!(state_of("blocked"), PaneState::NeedsYou);
        assert_eq!(state_of("done"), PaneState::Ready);
        assert_eq!(state_of("idle"), PaneState::Idle);
        assert_eq!(state_of("unknown"), PaneState::Unknown);
        assert_eq!(state_of("something new"), PaneState::Unknown);
    }

    #[test]
    fn polling_reads_only_state_folder_and_focus_over_ping_and_agent_list() {
        let fake = FakeHerdr::start("poll");
        *fake.agents.lock().unwrap() = vec![
            agent("p1", "working", "/Users/me/Code/OpenRize", true),
            agent("p2", "blocked", "/Users/me/Code/Acme", false),
        ];

        let poll = poll(&endpoint(&fake)).unwrap();

        assert_eq!(poll.protocol, PROTOCOL);
        assert_eq!(poll.panes.len(), 2);
        assert_eq!(poll.panes[0].key, "herdr:fixture:p1");
        assert_eq!(poll.panes[0].agent, "Claude Code");
        assert_eq!(poll.panes[0].state, PaneState::Running);
        assert_eq!(
            poll.panes[0].cwd.as_deref(),
            Some("/Users/me/Code/OpenRize")
        );
        assert!(poll.panes[0].focused);
        assert_eq!(poll.panes[1].state, PaneState::NeedsYou);
        assert!(!poll.panes[1].focused);
        // Nothing but ping and agent.list: no reads, no keys, no lifecycle.
        assert_eq!(*fake.calls.lock().unwrap(), vec!["ping", "agent.list"]);
        assert_eq!(pane_ids(&poll.panes, "fixture"), vec!["p1", "p2"]);
    }

    #[test]
    fn a_pane_without_an_agent_is_not_listed() {
        let result = json!({ "type": "agent_list", "agents": [
            { "pane_id": "p9", "agent": null, "display_agent": null, "agent_status": "idle", "focused": false }
        ] });
        assert!(parse_agent_list("x", &result).is_empty());
    }

    #[test]
    fn a_missing_socket_is_a_clear_error_not_a_panic() {
        let endpoint = Endpoint {
            id: "none".to_string(),
            path: PathBuf::from("/nonexistent/herdr.sock"),
        };
        let error = poll(&endpoint).unwrap_err();
        assert!(error.contains("could not connect"), "{error}");
    }

    #[test]
    fn the_event_subscription_only_wakes_the_poller() {
        let fake = FakeHerdr::start("watch");
        let (wake, woken) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let endpoint = endpoint(&fake);
        let flag = stop.clone();
        let handle =
            std::thread::spawn(move || watch(&endpoint, &["p1".to_string()], &flag, &wake));

        woken
            .recv_timeout(Duration::from_secs(3))
            .expect("an event wakes the poller");
        stop.store(true, Ordering::Relaxed);
        handle.join().unwrap().unwrap();
        assert_eq!(*fake.calls.lock().unwrap(), vec!["events.subscribe"]);
    }
}
