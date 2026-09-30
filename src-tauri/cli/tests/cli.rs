//! Real executable + local service coverage, without launching Tauri or a GUI.
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::{json, Value};

#[path = "../src/protocol.rs"]
#[allow(dead_code)]
mod protocol;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    data: PathBuf,
    runtime: PathBuf,
    service: Option<Child>,
}

impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target");
        let root = root.canonicalize().unwrap();
        let data = root.join(format!(
            "cli-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let runtime = root.join("i");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            data,
            runtime,
            service: None,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_openrize"));
        command
            .arg("--data-dir")
            .arg(&self.data)
            .arg("--runtime-dir")
            .arg(&self.runtime);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.command().arg("--json").args(args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn database(&self) -> Connection {
        let conn = Connection::open(self.data.join("activity.db")).unwrap();
        // Exact time_entries fields consumed by the shared row mapper. No UI,
        // ML, or capture state is required by the independently installed CLI.
        conn.execute_batch(
            "CREATE TABLE projects(id TEXT PRIMARY KEY, client_id TEXT);
            CREATE TABLE time_entries (
              id TEXT PRIMARY KEY, started_at INTEGER, ended_at INTEGER, description TEXT,
              category_id TEXT, project_id TEXT, status TEXT, approved_by TEXT, source TEXT,
              billable INTEGER, invoice_id TEXT, created_at INTEGER, updated_at INTEGER,
              deleted_at INTEGER, description_origin TEXT);",
        )
        .unwrap();
        conn
    }

    fn socket(&self) -> PathBuf {
        let mut hash = 0xcbf29ce484222325_u64;
        for byte in self.data.as_os_str().as_encoded_bytes() {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
        self.runtime.join(format!("v1-{hash:016x}.sock"))
    }

    fn start(&mut self) {
        self.service = Some(
            self.command()
                .arg("__serve")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while UnixStream::connect(self.socket()).is_err() {
            assert!(Instant::now() < deadline, "service did not start");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut child) = self.service.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_file(self.socket());
        let _ = fs::remove_file(self.socket().with_extension("lock"));
        let _ = fs::remove_dir_all(&self.data);
    }
}

fn insert(conn: &Connection, id: &str, start: i64, status: &str, deleted: bool) {
    conn.execute("INSERT INTO time_entries VALUES (?1, ?2, ?2 + 1000, ?3, NULL, NULL, ?4, NULL, 'manual', 1, NULL, 0, 0, ?5, 'user')",
        rusqlite::params![id, start, "PRIVATE \u{1b}[31m window / url", status, deleted.then_some(1)]).unwrap();
}

#[test]
fn actual_cli_read_surface_and_privacy() {
    let mut fixture = Fixture::new();
    let conn = fixture.database();
    insert(&conn, "lower", 1000, "pending", false);
    insert(&conn, "upper", 2000, "approved", false);
    insert(&conn, "outside", 2001, "pending", false);
    insert(&conn, "deleted", 1500, "pending", true);
    drop(conn);
    fixture.start();
    let before = fs::read(fixture.data.join("activity.db")).unwrap();
    let status = fixture.json(&[], 0);
    assert_eq!(status["schemaVersion"], 1);
    assert_eq!(status["data"]["value"]["entries"], 3);
    assert_eq!(status["data"]["value"]["pending"], 2);
    assert_eq!(status["data"]["value"]["trackedMs"], 3000);
    let query = [
        "entries",
        "list",
        "--from",
        "1970-01-01T00:00:01",
        "--to",
        "1970-01-01T01:00:02+01:00",
    ];
    let page = fixture.json(&query, 0);
    let entries = page["data"]["value"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["id"], "upper");
    assert_eq!(entries[1]["id"], "lower");
    assert_eq!(entries[0].as_object().unwrap().len(), 4);
    assert!(!page.to_string().contains("PRIVATE"));
    let mut full_query = query.to_vec();
    full_query.push("--full");
    assert!(fixture.json(&full_query, 0).to_string().contains("PRIVATE"));
    let mut limited = query.to_vec();
    limited.extend(["--limit", "1"]);
    assert_eq!(
        fixture.json(&limited, 0)["data"]["value"]["truncated"],
        true
    );
    let mut pending = query.to_vec();
    pending.extend(["--status", "pending"]);
    assert_eq!(
        fixture.json(&pending, 0)["data"]["value"]["entries"][0]["id"],
        "lower"
    );
    let human = fixture.run(&full_query);
    assert!(!human.stdout.contains(&0x1b), "terminal control escaped");
    assert_eq!(
        before,
        fs::read(fixture.data.join("activity.db")).unwrap(),
        "CLI changed database bytes"
    );
    let readonly =
        openrize_core::readonly::open_database(&fixture.data.join("activity.db")).unwrap();
    assert!(readonly.execute("DELETE FROM time_entries", []).is_err());
}

#[test]
fn live_wal_updates_and_bounded_sensitive_detail() {
    let mut fixture = Fixture::new();
    let conn = fixture.database();
    conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
    fixture.start();
    assert_eq!(fixture.json(&["status"], 0)["data"]["value"]["entries"], 0);
    insert(&conn, "live", 1000, "pending", false);
    assert_eq!(fixture.json(&["status"], 0)["data"]["value"]["entries"], 1);
    conn.execute(
        "UPDATE time_entries SET description = ?1",
        ["🙂".repeat(5000)],
    )
    .unwrap();
    let page = fixture.json(
        &[
            "entries",
            "list",
            "--from",
            "1970-01-01T00:00:01Z",
            "--to",
            "1970-01-01T00:00:01Z",
            "--full",
        ],
        0,
    );
    let detail = &page["data"]["value"]["entries"][0]["detail"];
    assert_eq!(
        detail["description"].as_str().unwrap().chars().count(),
        4096
    );
    assert_eq!(detail["descriptionTruncated"], true);
}

#[test]
fn crashed_service_socket_is_recovered() {
    let mut fixture = Fixture::new();
    fixture.database();
    fixture.start();
    let mut child = fixture.service.take().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(fixture.socket().exists());
    fixture.start();
    assert_eq!(fixture.json(&["status"], 0)["ok"], true);
}

#[test]
fn empty_results_and_argument_errors() {
    let mut fixture = Fixture::new();
    fixture.database();
    fixture.start();
    let query = [
        "entries",
        "list",
        "--from",
        "1970-01-01T00:00:00Z",
        "--to",
        "1970-01-01T00:00:00Z",
    ];
    let empty = fixture.json(&query, 0);
    assert_eq!(empty["data"]["value"]["count"], 0);
    assert_eq!(empty["data"]["value"]["entries"], json!([]));
    assert!(String::from_utf8(fixture.run(&query).stdout)
        .unwrap()
        .contains("0 results"));
    for args in [
        vec!["--unknown"],
        vec!["entries", "approve", "id"],
        vec![
            "entries",
            "list",
            "--from",
            "2026-09-01",
            "--to",
            "2026-09-02",
        ],
        vec![
            "entries",
            "list",
            "--from",
            "1970-01-01T00:00:00Z",
            "--to",
            "1970-01-01T00:00:01Z",
            "--limit",
            "501",
        ],
    ] {
        assert_eq!(fixture.json(&args, 2)["error"]["code"], "INVALID_ARGUMENT");
    }
}

#[test]
fn protocol_limits_version_and_unknown_operations() {
    let mut fixture = Fixture::new();
    fixture.database();
    fixture.start();
    for request in [
        json!({"protocolVersion": 2, "operation": {"kind": "status"}}),
        json!({"protocolVersion": 1, "operation": {"kind": "delete"}}),
        json!({"protocolVersion": 1, "operation": {"kind": "status", "extra": true}}),
    ] {
        let mut stream = UnixStream::connect(fixture.socket()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        protocol::write_frame(&mut stream, &request, protocol::MAX_REQUEST).unwrap();
        let response: Value = protocol::read_frame(&mut stream, protocol::MAX_RESPONSE).unwrap();
        assert_eq!(response["ok"], false);
        assert_eq!(
            response["error"]["code"],
            if request["protocolVersion"] == 2 {
                "PROTOCOL_MISMATCH"
            } else {
                "INVALID_REQUEST"
            }
        );
    }
    let mut stream = UnixStream::connect(fixture.socket()).unwrap();
    stream
        .write_all(&((protocol::MAX_REQUEST + 1) as u32).to_be_bytes())
        .unwrap();
    let response: Value = protocol::read_frame(&mut stream, protocol::MAX_RESPONSE).unwrap();
    assert_eq!(response["error"]["code"], "INVALID_REQUEST");
}

#[test]
fn on_demand_service_without_gui_and_concurrent_startup() {
    let fixture = Fixture::new();
    // No pre-existing service, GUI, or database. Two real CLI invocations race
    // to start one local service and both get a structured, non-creating error.
    let first = fixture
        .command()
        .args(["--json", "status"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let second = fixture.json(&["status"], 1);
    let first_output = first.wait_with_output().unwrap();
    let first_response: Value = serde_json::from_slice(&first_output.stdout).unwrap();
    assert_eq!(first_response["error"]["code"], "NO_DATA");
    assert_eq!(second["error"]["code"], "NO_DATA");
    assert!(!fixture.data.join("activity.db").exists());
    assert_eq!(
        fs::metadata(fixture.socket()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // Wait for the intentional detached service to self-expire before cleanup.
    let deadline = Instant::now() + Duration::from_secs(40);
    while fixture.socket().exists() {
        assert!(
            Instant::now() < deadline,
            "headless service failed to self-expire"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn optional_path_install_is_explicit_idempotent_and_non_destructive() {
    let fixture = Fixture::new();
    let bin = fixture.data.join("bin");
    let args = ["install-path", "--dir", bin.to_str().unwrap()];
    assert_eq!(fixture.json(&args, 0)["ok"], true);
    assert_eq!(fixture.json(&args, 0)["ok"], true);
    assert!(bin.join("openrize").is_symlink());
    fs::remove_file(bin.join("openrize")).unwrap();
    fs::write(bin.join("openrize"), "existing executable").unwrap();
    assert_eq!(fixture.json(&args, 1)["error"]["code"], "INSTALL_FAILED");
    assert_eq!(
        fs::read_to_string(bin.join("openrize")).unwrap(),
        "existing executable"
    );
}

#[test]
fn unsafe_runtime_directory_is_rejected() {
    let fixture = Fixture::new();
    // Keep the runtime path below sockaddr_un's limit so this specifically
    // exercises permissions, not the separate long-path guard.
    let unsafe_dir = fixture.runtime.with_file_name("u");
    fs::create_dir(&unsafe_dir).unwrap();
    fs::set_permissions(&unsafe_dir, fs::Permissions::from_mode(0o755)).unwrap();
    let output = fixture
        .command()
        .args(["--runtime-dir", unsafe_dir.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    // Clap rejects duplicate flags rather than accepting ambiguous routing.
    assert_eq!(output.status.code(), Some(2));
    let output = Command::new(env!("CARGO_BIN_EXE_openrize"))
        .arg("--data-dir")
        .arg(&fixture.data)
        .arg("--runtime-dir")
        .arg(&unsafe_dir)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "SERVICE_UNAVAILABLE");
    fs::remove_dir(unsafe_dir).unwrap();
}
