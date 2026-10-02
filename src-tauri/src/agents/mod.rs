//! The agent bridge: sees which coding agents are running, in which project,
//! and whether they are working, waiting on the person or finished, so a day
//! with several agents in flight can be tracked and billed honestly (design
//! board `multitask-tracking`, stage 1).
//!
//! Everything runs inside the app process, with no daemon, LaunchAgent or CLI.
//! Each *extension* (Herdr, tmux; hooks later) is a source of pane
//! observations. It is on by default when its tool is detected, and a
//! person's explicit on or off in Settings always wins (`Settings::extensions`).
//! Sources only ever read metadata: the agent name, its state, the working
//! folder and whether its pane has focus. Terminal content is never read,
//! stored or sent.
//!
//! Layers, from the outside in:
//! - `herdr`, `tmux`: adapters that turn a source into `PaneObservation`s.
//! - `tracker`: observations to jobs, state transitions and focus time.
//! - `ledger`: jobs and the person's own entries to counted agent entries
//!   (`source = 'agent'`) under the guardrails in `accounting`.
//! - this module: the supervising thread, extension status and the events the
//!   frontend consumes.
//!
//! `ledger`, `accounting`, `spans` and `store` live in `openrize_core::agents`
//! (re-exported here), so `rize entries rebuild` can refresh the ledger with
//! the app closed.

pub mod detect;
pub mod herdr;
pub mod tmux;
pub mod tracker;

pub use openrize_core::agents::{accounting, ledger, store};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use tracker::{Context, LiveAgent, PaneObservation, PaneState, Tracker};

use crate::timers::now_epoch_ms;
use crate::AppState;

/// Emitted whenever the live agents or their jobs change. Payload: `Board`.
pub const EVENT_AGENTS_CHANGED: &str = "agents-changed";
/// Emitted when an extension's detection or connection changes.
pub const EVENT_EXTENSIONS_CHANGED: &str = "extensions-changed";

/// How often detection is repeated, so a tool installed while the app runs is
/// found and enabled without a restart.
const DETECT_EVERY: Duration = Duration::from_secs(10);
/// While a job runs, its building entry is extended this often.
const REFRESH_EVERY: Duration = Duration::from_secs(30);
/// A source that stops answering for this long has its panes closed where it
/// was last seen.
const OUTAGE_GRACE_MS: u64 = 15_000;
const POLL_FAST: Duration = Duration::from_secs(2);
const POLL_WITH_EVENTS: Duration = Duration::from_secs(10);
const RETRY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Herdr,
    Tmux,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Herdr => "herdr",
            Self::Tmux => "tmux",
        }
    }
}

/// Terminal emulators, by bundle id. The person's time counts as time in an
/// agent's pane only while one of these is in front.
const TERMINALS: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.github.wez.wezterm",
    "com.mitchellh.ghostty",
    "org.alacritty",
    "net.kovidgoyal.kitty",
    "dev.warp.Warp-Stable",
    "co.zeit.hyper",
    "com.raphaelamorim.rio",
    "io.alacritty",
    "org.tabby",
    "app.warp.Warp",
];
const TERMINAL_NAMES: &[&str] = &[
    "terminal",
    "iterm",
    "wezterm",
    "ghostty",
    "alacritty",
    "kitty",
    "warp",
    "hyper",
    "rio",
    "tabby",
];

pub fn is_terminal(app: &str, bundle_id: Option<&str>) -> bool {
    if let Some(id) = bundle_id {
        if TERMINALS.iter().any(|known| known.eq_ignore_ascii_case(id)) {
            return true;
        }
    }
    let name = app.to_ascii_lowercase();
    TERMINAL_NAMES
        .iter()
        .any(|known| name == *known || name.starts_with(&format!("{known} ")))
}

// --- Status ---------------------------------------------------------------

/// One extension as Settings lists it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub detected: bool,
    /// Where the tool was found.
    pub detail: Option<String>,
    /// What the person chose by hand: `None` follows auto-detection.
    pub preference: Option<bool>,
    /// Whether it is running now.
    pub enabled: bool,
    /// `off`, `notDetected`, `waiting` (on, no server yet), `connected` or
    /// `error`.
    pub connection: &'static str,
    pub agents: usize,
    pub error: Option<String>,
}

#[derive(Default, Clone)]
struct SourceStatus {
    ok: bool,
    last_ok: u64,
    error: Option<String>,
    /// The server's protocol, when it differs from what events were written for.
    protocol: Option<u64>,
}

#[derive(Default)]
struct Shared {
    live: Vec<LiveAgent>,
    sources: HashMap<String, SourceStatus>,
    detections: HashMap<&'static str, (bool, Option<String>)>,
    active: HashSet<String>,
}

/// Managed state: the supervisor's mailbox and what it last published.
pub struct AgentRuntime {
    tx: Mutex<Option<Sender<Msg>>>,
    shared: Mutex<Shared>,
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self {
            tx: Mutex::new(None),
            shared: Mutex::new(Shared::default()),
        }
    }
}

impl AgentRuntime {
    fn send(&self, msg: Msg) {
        if let Ok(tx) = self.tx.lock() {
            if let Some(tx) = tx.as_ref() {
                let _ = tx.send(msg);
            }
        }
    }

    pub fn live(&self) -> Vec<LiveAgent> {
        self.shared
            .lock()
            .map(|s| s.live.clone())
            .unwrap_or_default()
    }
}

enum Msg {
    Snapshot {
        source_id: String,
        source: Source,
        panes: Vec<PaneObservation>,
        protocol: Option<u64>,
    },
    Failed {
        source_id: String,
        error: String,
    },
    /// Detection or settings changed: reconcile now.
    Recheck,
    /// Recompute the ledger now (a job was confirmed, a range was rebuilt).
    Refresh,
}

/// Asks the supervisor to look at detection and settings again.
pub fn recheck(app: &AppHandle) {
    app.state::<AgentRuntime>().send(Msg::Recheck);
}

/// Asks the supervisor to recompute the agent ledger.
pub fn refresh(app: &AppHandle) {
    app.state::<AgentRuntime>().send(Msg::Refresh);
}

fn extension_meta(id: &str) -> (&'static str, &'static str) {
    match id {
        "herdr" => (
            "Herdr",
            "Reads which agents run in your Herdr panes and whether they are working, waiting on you or done.",
        ),
        _ => (
            "tmux",
            "Reads which agents run in your tmux panes and whether they are producing output.",
        ),
    }
}

/// Detection for every extension, fresh from the machine.
pub fn detect_all() -> Vec<(&'static str, bool, Option<String>)> {
    let herdr = herdr::detect();
    let tmux = tmux::detect();
    vec![
        ("herdr", herdr.detected, herdr.detail),
        ("tmux", tmux.detected, tmux.detail),
    ]
}

/// The extensions as Settings shows them, detected fresh.
pub fn extensions(app: &AppHandle) -> Vec<ExtensionStatus> {
    let settings = app.state::<AppState>().settings_snapshot();
    let runtime = app.state::<AgentRuntime>();
    let shared = runtime.shared.lock().ok();
    detect_all()
        .into_iter()
        .map(|(id, detected, detail)| {
            let (name, description) = extension_meta(id);
            let enabled = settings.extension_enabled(id, detected);
            let prefix = format!("{id}:");
            let (mut connected, mut error, mut agents) = (false, None, 0);
            if let Some(shared) = shared.as_ref() {
                for (source_id, status) in &shared.sources {
                    if source_id.starts_with(&prefix) && shared.active.contains(source_id) {
                        connected |= status.ok;
                        if error.is_none() {
                            error = status.error.clone();
                        }
                    }
                }
                agents = shared
                    .live
                    .iter()
                    .filter(|agent| agent.source == id)
                    .count();
            }
            let connection = if !enabled {
                "off"
            } else if connected {
                "connected"
            } else if !detected {
                "notDetected"
            } else if error
                .as_deref()
                .is_some_and(|e| !e.contains("could not connect"))
            {
                "error"
            } else {
                "waiting"
            };
            ExtensionStatus {
                id,
                name,
                description,
                detected,
                detail,
                preference: settings.extensions.get(id).copied(),
                enabled,
                connection,
                agents,
                error: if connection == "error" { error } else { None },
            }
        })
        .collect()
}

// --- The board ------------------------------------------------------------

/// The live flight board: what runs now, what waits on the person, and what
/// they have waited out today.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub live: Vec<LiveAgent>,
    pub running: usize,
    pub needs_you: usize,
    pub ready: usize,
    /// Agent turns today that wait for the person to confirm them.
    pub to_confirm: usize,
    /// Time agents waited on the person today, before they came.
    pub waited_ms: u64,
    pub jobs: Vec<ledger::JobView>,
    pub day_start: u64,
    pub day_end: u64,
}

pub fn board(app: &AppHandle) -> Result<Board, String> {
    let now = now_epoch_ms();
    let (day_start, day_end) = ledger::calendar_day(now);
    let report = {
        let state = app.state::<AppState>();
        let reader = state
            .activity_reader
            .lock()
            .map_err(|_| "reader lock poisoned".to_string())?;
        ledger::report(&reader, day_start, day_end, now)?
    };
    let live = app.state::<AgentRuntime>().live();
    let count = |state: PaneState| live.iter().filter(|agent| agent.state == state).count();
    Ok(Board {
        running: count(PaneState::Running),
        needs_you: count(PaneState::NeedsYou),
        ready: count(PaneState::Ready),
        to_confirm: report
            .jobs
            .iter()
            .filter(|job| {
                job.pending_ms > 0 && !job.confirmed && !matches!(job.phase, "running" | "needsYou")
            })
            .count(),
        waited_ms: report.ledger.waiting_ms,
        jobs: report.jobs,
        live,
        day_start,
        day_end,
    })
}

fn publish(app: &AppHandle) {
    if let Ok(board) = board(app) {
        crate::tray::set_review_dot(app, board.needs_you + board.ready > 0);
        let _ = app.emit(EVENT_AGENTS_CHANGED, board);
    }
}

// --- Workers --------------------------------------------------------------

struct Worker {
    stop: Arc<AtomicBool>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn sleep_unless_stopped(stop: &AtomicBool, wait: Duration) {
    let step = Duration::from_millis(200);
    let mut waited = Duration::ZERO;
    while waited < wait && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(step);
        waited += step;
    }
}

fn spawn_herdr(endpoint: herdr::Endpoint, tx: Sender<Msg>) -> Worker {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let source_id = format!("herdr:{}", endpoint.id);
    let spawned = std::thread::Builder::new()
        .name(format!("agents-{source_id}"))
        .spawn(move || {
            let (wake, woken) = mpsc::channel::<()>();
            let mut watching: Option<(Vec<String>, Arc<AtomicBool>)> = None;
            while !flag.load(Ordering::Relaxed) {
                match herdr::poll(&endpoint) {
                    Ok(poll) => {
                        // Events are only a wake-up, and only for the protocol
                        // they were written against: otherwise just poll.
                        if poll.protocol == herdr::PROTOCOL {
                            let ids = herdr::pane_ids(&poll.panes, &endpoint.id);
                            let alive = watching.as_ref().is_some_and(|(known, stop)| {
                                *known == ids && !stop.load(Ordering::Relaxed)
                            });
                            if !alive {
                                if let Some((_, old)) = watching.take() {
                                    old.store(true, Ordering::Relaxed);
                                }
                                let stop_watch = Arc::new(AtomicBool::new(false));
                                let (watch_endpoint, watch_ids) = (endpoint.clone(), ids.clone());
                                let (watch_stop, watch_wake) = (stop_watch.clone(), wake.clone());
                                std::thread::spawn(move || {
                                    let _ = herdr::watch(
                                        &watch_endpoint,
                                        &watch_ids,
                                        &watch_stop,
                                        &watch_wake,
                                    );
                                    // The subscription ended: poll on its own.
                                    watch_stop.store(true, Ordering::Relaxed);
                                });
                                watching = Some((ids, stop_watch));
                            }
                        }
                        let _ = tx.send(Msg::Snapshot {
                            source_id: source_id.clone(),
                            source: Source::Herdr,
                            panes: poll.panes,
                            protocol: (poll.protocol != herdr::PROTOCOL).then_some(poll.protocol),
                        });
                        let events = watching
                            .as_ref()
                            .is_some_and(|(_, stop)| !stop.load(Ordering::Relaxed));
                        let wait = if events { POLL_WITH_EVENTS } else { POLL_FAST };
                        if woken.recv_timeout(wait).is_ok() {
                            // Collapse a burst of events into one poll.
                            std::thread::sleep(Duration::from_millis(150));
                            while woken.try_recv().is_ok() {}
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Msg::Failed {
                            source_id: source_id.clone(),
                            error,
                        });
                        sleep_unless_stopped(&flag, RETRY);
                    }
                }
            }
            if let Some((_, old)) = watching.take() {
                old.store(true, Ordering::Relaxed);
            }
        });
    if let Err(error) = spawned {
        eprintln!("could not start the Herdr bridge: {error}");
    }
    Worker { stop }
}

fn spawn_tmux(server: tmux::Server, tx: Sender<Msg>) -> Worker {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let source_id = format!("tmux:{}", server.id);
    let spawned = std::thread::Builder::new()
        .name(format!("agents-{source_id}"))
        .spawn(move || {
            let (wake, woken) = mpsc::channel::<()>();
            let mut watcher: Option<tmux::Watcher> = None;
            let mut last_attach: Option<std::time::Instant> = None;
            while !flag.load(Ordering::Relaxed) {
                match tmux::poll(&server, now_epoch_ms()) {
                    Ok(panes) => {
                        // One control client per server, retried at most every 30 s.
                        let alive = watcher.as_mut().is_some_and(tmux::Watcher::is_alive);
                        if !alive
                            && !panes.is_empty()
                            && last_attach.is_none_or(|at| at.elapsed() > Duration::from_secs(30))
                        {
                            last_attach = Some(std::time::Instant::now());
                            watcher = tmux::Watcher::start(&server, wake.clone()).ok();
                        }
                        let _ = tx.send(Msg::Snapshot {
                            source_id: source_id.clone(),
                            source: Source::Tmux,
                            panes,
                            protocol: None,
                        });
                    }
                    Err(error) => {
                        let _ = tx.send(Msg::Failed {
                            source_id: source_id.clone(),
                            error,
                        });
                    }
                }
                if woken.recv_timeout(POLL_FAST).is_ok() {
                    std::thread::sleep(Duration::from_millis(150));
                    while woken.try_recv().is_ok() {}
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("could not start the tmux bridge: {error}");
    }
    Worker { stop }
}

// --- The supervisor -------------------------------------------------------

struct Supervisor {
    app: AppHandle,
    conn: Connection,
    tracker: Tracker,
    workers: HashMap<String, Worker>,
    tx: Sender<Msg>,
    rules: Vec<tracker::PathRule>,
    rules_at: u64,
    last_detect: Option<std::time::Instant>,
    last_refresh: std::time::Instant,
}

/// Starts the bridge. Safe to call once, from `setup`.
pub fn spawn(app: AppHandle, db_path: PathBuf) {
    let runtime = app.state::<AgentRuntime>();
    let (tx, rx) = mpsc::channel();
    if let Ok(mut slot) = runtime.tx.lock() {
        *slot = Some(tx.clone());
    }
    let spawned = std::thread::Builder::new()
        .name("agents-supervisor".to_string())
        .spawn(move || {
            let conn = match Connection::open(&db_path) {
                Ok(conn) => {
                    let _ = conn.busy_timeout(Duration::from_secs(5));
                    conn
                }
                Err(error) => {
                    eprintln!("agent bridge could not open the database: {error}");
                    return;
                }
            };
            // A job left open by a quit or crash ends where it was last seen.
            if let Err(error) = store::close_orphans(&conn) {
                eprintln!("agent bridge: {error}");
            }
            Supervisor {
                app,
                conn,
                tracker: Tracker::default(),
                workers: HashMap::new(),
                tx,
                rules: Vec::new(),
                rules_at: 0,
                last_detect: None,
                last_refresh: std::time::Instant::now(),
            }
            .run(&rx);
        });
    if let Err(error) = spawned {
        eprintln!("could not start the agent bridge: {error}");
    }
}

impl Supervisor {
    fn run(mut self, rx: &Receiver<Msg>) {
        loop {
            self.reconcile_if_due(false);
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(msg) => self.handle(msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if self.last_refresh.elapsed() >= REFRESH_EVERY {
                self.last_refresh = std::time::Instant::now();
                self.refresh_ledger(true);
            }
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Recheck => self.reconcile_if_due(true),
            Msg::Refresh => self.refresh_ledger(true),
            Msg::Failed { source_id, error } => self.failed(&source_id, error),
            Msg::Snapshot {
                source_id,
                source,
                panes,
                protocol,
            } => self.snapshot(&source_id, source, &panes, protocol),
        }
    }

    fn rules(&mut self, now: u64) -> &[tracker::PathRule] {
        if now.saturating_sub(self.rules_at) > 5_000 {
            if let Ok(rules) = crate::ai::store::load_rules(&self.conn) {
                self.rules = tracker::path_rules(&rules);
                self.rules_at = now;
            }
        }
        &self.rules
    }

    fn attending(&self) -> bool {
        let state = self.app.state::<AppState>();
        let Ok(store) = state.activity.lock() else {
            return false;
        };
        store
            .live_state(now_epoch_ms())
            .attention()
            .is_some_and(|(app, bundle)| is_terminal(app, bundle))
    }

    fn snapshot(
        &mut self,
        source_id: &str,
        source: Source,
        panes: &[PaneObservation],
        protocol: Option<u64>,
    ) {
        let now = now_epoch_ms();
        let attending = self.attending();
        let rules = self.rules(now).to_vec();
        let outcome = self.tracker.observe(
            &self.conn,
            source_id,
            source,
            panes,
            &Context {
                now,
                attending,
                rules: &rules,
            },
        );
        let runtime = self.app.state::<AgentRuntime>();
        let status_changed = if let Ok(mut shared) = runtime.shared.lock() {
            let before = shared
                .sources
                .get(source_id)
                .map(|s| (s.ok, s.error.clone(), s.protocol));
            let status = shared.sources.entry(source_id.to_string()).or_default();
            status.ok = true;
            status.last_ok = now;
            status.error = None;
            status.protocol = protocol;
            before != Some((true, None, protocol))
        } else {
            false
        };
        let mut count_changed = false;
        match outcome {
            Ok(outcome) => {
                count_changed = self.sync_live();
                if outcome.jobs || outcome.focus {
                    self.refresh_ledger(false);
                    // Carved entries are rebuilt by the AI worker's pass.
                    crate::ai::nudge(&self.app);
                }
                if outcome.any() {
                    publish(&self.app);
                }
            }
            Err(error) => eprintln!("agent bridge: {error}"),
        }
        // Settings shows how many agents each extension sees.
        if status_changed || count_changed {
            let _ = self.app.emit(EVENT_EXTENSIONS_CHANGED, ());
        }
    }

    fn failed(&mut self, source_id: &str, error: String) {
        let now = now_epoch_ms();
        let (was_ok, last_ok) = {
            let runtime = self.app.state::<AgentRuntime>();
            let Ok(mut shared) = runtime.shared.lock() else {
                return;
            };
            let status = shared.sources.entry(source_id.to_string()).or_default();
            let snapshot = (status.ok, status.last_ok);
            status.error = Some(error);
            snapshot
        };
        // Past the grace period the server is gone: its panes close where it
        // was last seen, so a quit never bills.
        if was_ok && now.saturating_sub(last_ok) > OUTAGE_GRACE_MS {
            if let Ok(mut shared) = self.app.state::<AgentRuntime>().shared.lock() {
                if let Some(status) = shared.sources.get_mut(source_id) {
                    status.ok = false;
                }
            }
            if let Ok(outcome) = self.tracker.forget_source(&self.conn, source_id, last_ok) {
                self.sync_live();
                if outcome.any() {
                    self.refresh_ledger(false);
                    publish(&self.app);
                }
            }
            let _ = self.app.emit(EVENT_EXTENSIONS_CHANGED, ());
        }
    }

    /// Publishes the live panes to readers; whether how many there are changed.
    fn sync_live(&mut self) -> bool {
        let live = self.tracker.live();
        if let Ok(mut shared) = self.app.state::<AgentRuntime>().shared.lock() {
            let changed = shared.live.len() != live.len();
            shared.live = live;
            return changed;
        }
        false
    }

    /// Recomputes today's and yesterday's agent ledger and writes its entries.
    fn refresh_ledger(&mut self, force_publish: bool) {
        let now = now_epoch_ms();
        let auto_accept = self.app.state::<AppState>().settings_snapshot().auto_accept;
        let (today_start, today_end) = ledger::calendar_day(now);
        let (yesterday_start, _) = ledger::calendar_day(today_start.saturating_sub(1));
        let mut changed = false;
        for (start, end) in [(yesterday_start, today_start), (today_start, today_end)] {
            match ledger::refresh(&mut self.conn, start, end, now, auto_accept) {
                Ok(did) => changed |= did,
                Err(error) => eprintln!("agent ledger: {error}"),
            }
        }
        if changed {
            let _ = self.app.emit(crate::EVENT_ENTRIES_CHANGED, ());
        }
        if changed || force_publish {
            publish(&self.app);
        }
    }

    /// Starts and stops source workers to match detection and the person's
    /// choices. Cheap; runs every few seconds and on demand.
    fn reconcile_if_due(&mut self, force: bool) {
        if !force
            && self
                .last_detect
                .is_some_and(|at| at.elapsed() < DETECT_EVERY)
        {
            return;
        }
        self.last_detect = Some(std::time::Instant::now());
        let settings = self.app.state::<AppState>().settings_snapshot();
        let mut wanted: HashSet<String> = HashSet::new();
        let mut herdr_endpoints: HashMap<String, herdr::Endpoint> = HashMap::new();
        let mut tmux_servers: HashMap<String, tmux::Server> = HashMap::new();
        let mut detections = HashMap::new();
        for (id, detected, detail) in detect_all() {
            detections.insert(id, (detected, detail));
            if !settings.extension_enabled(id, detected) {
                continue;
            }
            match id {
                "herdr" => {
                    for endpoint in herdr::endpoints() {
                        let source_id = format!("herdr:{}", endpoint.id);
                        wanted.insert(source_id.clone());
                        herdr_endpoints.insert(source_id, endpoint);
                    }
                }
                _ => {
                    for server in tmux::servers() {
                        let source_id = format!("tmux:{}", server.id);
                        wanted.insert(source_id.clone());
                        tmux_servers.insert(source_id, server);
                    }
                }
            }
        }

        let before = self
            .app
            .state::<AgentRuntime>()
            .shared
            .lock()
            .map(|shared| (shared.active.clone(), shared.detections.clone()))
            .ok();
        let stale: Vec<String> = self
            .workers
            .keys()
            .filter(|id| !wanted.contains(*id))
            .cloned()
            .collect();
        let mut closed = false;
        for id in stale {
            self.workers.remove(&id);
            if let Ok(outcome) = self.tracker.forget_source(&self.conn, &id, now_epoch_ms()) {
                closed |= outcome.any();
            }
            if let Ok(mut shared) = self.app.state::<AgentRuntime>().shared.lock() {
                shared.sources.remove(&id);
            }
        }
        for id in &wanted {
            if self.workers.contains_key(id) {
                continue;
            }
            let worker = if let Some(endpoint) = herdr_endpoints.remove(id) {
                spawn_herdr(endpoint, self.tx.clone())
            } else if let Some(server) = tmux_servers.remove(id) {
                spawn_tmux(server, self.tx.clone())
            } else {
                continue;
            };
            self.workers.insert(id.clone(), worker);
        }
        let changed = {
            let runtime = self.app.state::<AgentRuntime>();
            let Ok(mut shared) = runtime.shared.lock() else {
                return;
            };
            shared.active = wanted;
            shared.detections = detections;
            before.is_none_or(|(active, detected)| {
                active != shared.active || detected != shared.detections
            })
        };
        if closed {
            self.sync_live();
            self.refresh_ledger(false);
            publish(&self.app);
        }
        if changed {
            let _ = self.app.emit(EVENT_EXTENSIONS_CHANGED, ());
        }
    }
}
