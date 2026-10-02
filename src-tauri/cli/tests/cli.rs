//! What rize decides on its own, before or without reaching the app. The
//! app's answers are covered end to end in `src-tauri/tests/rize.rs`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rize-cli-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("data")).unwrap();
        Self { root }
    }

    fn rize(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rize"))
            .arg("--data-dir")
            .arg(self.root.join("data"))
            .args(args)
            // Never the developer's own build next to this binary.
            .env("RIZE_APP", self.root.join("missing-openrize"))
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> (Value, i32) {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let output = self.rize(&all);
        (
            serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
                panic!("not JSON: {}", String::from_utf8_lossy(&output.stdout))
            }),
            output.status.code().unwrap(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn code(envelope: &Value) -> &str {
    envelope["error"]["code"].as_str().unwrap_or_default()
}

#[test]
fn invalid_arguments_exit_2_with_an_envelope() {
    let fixture = Fixture::new();
    for args in [
        vec!["entries", "list", "--from", "someday"],
        vec!["entries", "list", "--from", "today", "--to", "yesterday"],
        vec!["entries", "list", "--last", "5y"],
        vec!["report", "--by", "planet"],
        vec!["entries", "edit", "abcd1234"],
        vec!["nonsense"],
    ] {
        let (envelope, exit) = fixture.json(&args);
        assert_eq!(exit, 2, "{args:?}: {envelope}");
        assert_eq!(code(&envelope), "INVALID_ARGUMENT", "{args:?}");
        assert_eq!(envelope["schemaVersion"], 2);
    }
}

#[test]
fn without_any_data_rize_says_the_app_is_missing() {
    let fixture = Fixture::new();
    let output = fixture.rize(&["entries", "list"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "This laptop doesn't have any rize data. Are you sure you've installed the app before?"
    );
    let (envelope, _) = fixture.json(&["status"]);
    assert_eq!(code(&envelope), "NO_DATA");
}

#[test]
fn live_only_commands_need_the_running_app() {
    let fixture = Fixture::new();
    for args in [vec!["focus", "start"], vec!["focus", "stop"]] {
        let (envelope, exit) = fixture.json(&args);
        assert_eq!(exit, 3, "{args:?}");
        assert_eq!(code(&envelope), "APP_NOT_RUNNING");
    }
}

#[test]
fn destructive_commands_need_yes_outside_a_terminal() {
    let fixture = Fixture::new();
    for args in [
        vec!["entries", "rm", "abcd1234"],
        vec!["timers", "rm", "Deep work"],
        vec!["projects", "rm", "Acme"],
        vec!["entries", "rebuild"],
    ] {
        let (envelope, exit) = fixture.json(&args);
        assert_eq!(exit, 2, "{args:?}");
        assert!(envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--yes"));
    }
    // With --yes it goes on to the app, which is missing here.
    let (envelope, _) = fixture.json(&["entries", "rm", "abcd1234", "--yes"]);
    assert_eq!(code(&envelope), "NO_DATA");
}

#[test]
fn a_store_without_its_app_binary_fails_clearly() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("data/activity.db"), b"").unwrap();
    let (envelope, exit) = fixture.json(&["timers", "list"]);
    assert_eq!(exit, 1);
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("missing-openrize"));
}

#[test]
fn completions_and_help_need_nothing() {
    let fixture = Fixture::new();
    let output = fixture.rize(&["completions", "zsh"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("#compdef rize"));
    let help = fixture.rize(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    for command in [
        "app",
        "track",
        "entries",
        "report",
        "settings",
        "install-path",
    ] {
        assert!(text.contains(command), "help lacks {command}");
    }
}

#[cfg(unix)]
#[test]
fn install_path_links_without_replacing() {
    let fixture = Fixture::new();
    let bin = fixture.root.join("bin");
    let (envelope, exit) = fixture.json(&["install-path", "--dir", bin.to_str().unwrap()]);
    assert_eq!(exit, 0, "{envelope}");
    let link = bin.join("rize");
    assert_eq!(
        fs::read_link(&link).unwrap(),
        PathBuf::from(env!("CARGO_BIN_EXE_rize"))
            .canonicalize()
            .unwrap()
    );
    let (again, _) = fixture.json(&["install-path", "--dir", bin.to_str().unwrap()]);
    assert_eq!(again["data"]["unchanged"], true);
    fs::remove_file(&link).unwrap();
    fs::write(&link, "mine").unwrap();
    let (refused, exit) = fixture.json(&["install-path", "--dir", bin.to_str().unwrap()]);
    assert_eq!(exit, 1);
    assert!(refused["error"]["message"]
        .as_str()
        .unwrap()
        .contains("nothing was replaced"));
    assert_eq!(fs::read_to_string(&link).unwrap(), "mine");
}
