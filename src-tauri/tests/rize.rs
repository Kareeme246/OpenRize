//! `rize` end to end with the app closed: the real CLI (the sidecar build.rs
//! stages) runs the real app binary headless against a fresh data directory.
//! The running-app path shares every operation with it; only the transport
//! differs.
#![cfg(any(target_os = "macos", windows))]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Store {
    root: PathBuf,
}

impl Store {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rize-e2e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("data")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        // What a first launch leaves behind; the headless run migrates it.
        fs::write(root.join("data/activity.db"), b"").unwrap();
        Self { root }
    }

    fn rize(&self, args: &[&str]) -> Output {
        Command::new(env!("RIZE_SIDECAR"))
            .arg("--data-dir")
            .arg(self.root.join("data"))
            .args(args)
            .env("RIZE_APP", env!("CARGO_BIN_EXE_openrize"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .output()
            .unwrap()
    }

    /// Runs with `--json` and returns `data`, asserting success.
    fn ok(&self, args: &[&str]) -> Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let output = self.rize(&all);
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "rize {args:?}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert_eq!(envelope["ok"], true, "rize {args:?}: {envelope}");
        assert_eq!(output.status.code(), Some(0));
        envelope["data"].clone()
    }

    /// Runs with `--json` and returns the error code and exit code.
    fn err(&self, args: &[&str]) -> (String, i32) {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let output = self.rize(&all);
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(envelope["ok"], false, "rize {args:?}: {envelope}");
        (
            envelope["error"]["code"].as_str().unwrap().to_string(),
            output.status.code().unwrap(),
        )
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn edits_and_reports_with_the_app_closed() {
    let store = Store::new();
    let hello = store.ok(&["app", "status"]);
    assert_eq!(hello["running"], false);
    assert_eq!(hello["appVersion"], env!("CARGO_PKG_VERSION"));

    store.ok(&["clients", "add", "Acme", "--currency", "USD"]);
    let project = store.ok(&[
        "projects",
        "add",
        "Acme site",
        "--client",
        "acme",
        "--rate",
        "120",
    ]);
    assert_eq!(project["hourlyRate"], 120.0);
    assert!(project["clientId"].is_string());
    assert_eq!(
        store.err(&["projects", "show", "nothing"]),
        ("NOT_FOUND".into(), 1)
    );

    let entry = store.ok(&[
        "entries",
        "add",
        "--from",
        "2026-09-01T09:00",
        "--to",
        "2026-09-01T10:30",
        "-d",
        "Homepage",
        "--project",
        "acme",
    ]);
    let id = entry["id"].as_str().unwrap().to_string();
    let short = &id[id.len() - 8..];
    let list = store.ok(&["entries", "list", "--from", "2026-09-01"]);
    assert_eq!(list["entries"].as_array().unwrap().len(), 1);
    assert_eq!(list["names"][project["id"].as_str().unwrap()], "Acme site");

    let edited = store.ok(&[
        "entries",
        "edit",
        short,
        "-d",
        "Homepage copy",
        "--project",
        "none",
    ]);
    assert_eq!(edited[0]["description"], "Homepage copy");
    assert!(edited[0]["projectId"].is_null());
    store.ok(&["entries", "unapprove", short]);
    let pending = store.ok(&["review", "--from", "2026-09-01"]);
    assert_eq!(pending["entries"].as_array().unwrap().len(), 1);
    let approved = store.ok(&[
        "entries",
        "approve",
        "--all-pending",
        "--from",
        "2026-09-01",
    ]);
    assert_eq!(approved[0]["status"], "approved");

    let report = store.ok(&["report", "--from", "2026-09-01", "--by", "status"]);
    assert_eq!(report["cells"][0]["ms"], 90 * 60_000);
    let export = store.ok(&[
        "entries",
        "export",
        "--from",
        "2026-09-01",
        "--format",
        "csv",
    ]);
    assert!(export["content"]
        .as_str()
        .unwrap()
        .contains("Homepage copy"));

    // Deleting asks; a script must say --yes.
    assert_eq!(
        store.err(&["entries", "rm", short]),
        ("INVALID_ARGUMENT".into(), 2)
    );
    store.ok(&["entries", "rm", short, "--yes"]);
    assert_eq!(
        store.err(&["entries", "show", short]),
        ("NOT_FOUND".into(), 1)
    );
}

#[test]
fn settings_timers_and_what_needs_the_app() {
    let store = Store::new();
    let settings = store.ok(&["settings", "set", "weekly-target-hours", "32"]);
    assert_eq!(settings["weeklyTargetHours"], 32);
    assert_eq!(store.ok(&["settings", "get", "weeklyTargetHours"]), 32);
    assert_eq!(
        store.err(&["settings", "set", "breaks.enabled", "maybe"]),
        ("INVALID_ARGUMENT".into(), 2)
    );
    assert_eq!(
        store.err(&["settings", "set", "nope", "1"]),
        ("INVALID_ARGUMENT".into(), 2)
    );

    store.ok(&["timers", "new", "Deep work"]);
    store.ok(&["timers", "new", "Deep dive"]);
    assert_eq!(
        store.err(&["timers", "start", "deep"]),
        ("AMBIGUOUS".into(), 1)
    );
    let timers = store.ok(&["timers", "start", "deep work"]);
    assert!(timers[0]["startedAt"].is_number());

    let status = store.ok(&["status"]);
    assert_eq!(status["running"], false);
    assert_eq!(
        store.err(&["focus", "start"]),
        ("APP_NOT_RUNNING".into(), 3)
    );
    assert_eq!(store.ok(&["app", "quit"])["alreadyClosed"], true);
}

#[test]
fn no_data_means_the_app_was_never_installed() {
    let store = Store::new();
    fs::remove_file(store.root.join("data/activity.db")).unwrap();
    let output = store.rize(&["entries", "list"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(
        "This laptop doesn't have any rize data. Are you sure you've installed the app before?"
    ));
}
