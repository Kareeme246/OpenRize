//! `rize --dry-run` end to end, with the app closed (the harness is
//! `offline.rs`'s): every command `rize` confirms can be previewed, the
//! preview resolves and refuses exactly as the real command does, and it
//! changes nothing.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use openrize_core::activity::{ActivityStore, LOCKED};
use openrize_core::protocol::DRY_RUN_UNSUPPORTED;
use serde_json::{json, Value};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Store {
    root: PathBuf,
}

impl Store {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rize-dry-run-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("data")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        // What the app's first launch leaves behind.
        ActivityStore::load(&root.join("data")).unwrap();
        Self { root }
    }

    fn rize(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rize"))
            .arg("--data-dir")
            .arg(self.root.join("data"))
            .args(args)
            // No app anywhere rize could find one.
            .env("RIZE_APP", self.root.join("missing-openrize"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .output()
            .unwrap()
    }

    /// Runs with `--json`: the envelope and the exit code.
    fn run(&self, args: &[&str]) -> (Value, i32) {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let output = self.rize(&all);
        let envelope = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "rize {args:?}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (envelope, output.status.code().unwrap())
    }

    /// Runs with `--json` and returns `data`, asserting success.
    fn ok(&self, args: &[&str]) -> Value {
        let (envelope, code) = self.run(args);
        assert_eq!(envelope["ok"], true, "rize {args:?}: {envelope}");
        assert_eq!(code, 0);
        envelope["data"].clone()
    }

    /// Runs with `--json`, asserting failure: the error code, the message and
    /// the exit code.
    fn fail(&self, args: &[&str]) -> (String, String, i32) {
        let (envelope, code) = self.run(args);
        assert_eq!(envelope["ok"], false, "rize {args:?}: {envelope}");
        (
            envelope["error"]["code"].as_str().unwrap().to_string(),
            envelope["error"]["message"].as_str().unwrap().to_string(),
            code,
        )
    }

    /// `rize <args> --dry-run --json`, with no `--yes`, asserting that it
    /// succeeded as a preview.
    fn dry(&self, args: &[&str]) -> Value {
        let mut all = args.to_vec();
        all.push("--dry-run");
        let data = self.ok(&all);
        assert_eq!(data["dryRun"], true, "{all:?}: {data}");
        data
    }

    /// Everything the seven commands could change, as `rize` lists it.
    fn snapshot(&self) -> Value {
        json!([
            self.ok(&[
                "entries",
                "list",
                "--from",
                "2026-08-01",
                "--to",
                "2026-10-01",
                "--limit",
                "100"
            ]),
            self.ok(&["projects", "list"]),
            self.ok(&["clients", "list"]),
            self.ok(&["categories", "list"]),
            self.ok(&["timers", "list"]),
        ])
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Seed {
    /// An approved entry on the project "Acme site".
    approved: String,
    /// A pending entry on the project "Solo" and the category "Admin".
    pending: String,
    /// A pending entry with no project or category.
    loose: String,
}

/// Acme with one project, a project nobody approved time on, a category in
/// use and one nobody uses, three entries on 2026-09-01, and a timer that has
/// been started.
fn seed(store: &Store) -> Seed {
    store.ok(&["clients", "add", "Acme"]);
    store.ok(&["projects", "add", "Acme site", "--client", "acme"]);
    store.ok(&["projects", "add", "Solo"]);
    store.ok(&["categories", "add", "Admin"]);
    store.ok(&["categories", "add", "Spare"]);
    let add = |from: &str, to: &str, extra: &[&str]| {
        let mut args = vec!["entries", "add", "--from", from, "--to", to, "-d", "Work"];
        args.extend_from_slice(extra);
        store.ok(&args)["id"].as_str().unwrap().to_string()
    };
    let approved = add(
        "2026-09-01T09:00",
        "2026-09-01T10:00",
        &["--project", "acme site"],
    );
    let pending = add(
        "2026-09-01T11:00",
        "2026-09-01T12:00",
        &["--project", "solo", "--category", "admin"],
    );
    store.ok(&["entries", "unapprove", &pending]);
    let loose = add("2026-09-01T13:00", "2026-09-01T14:00", &[]);
    store.ok(&["entries", "unapprove", &loose]);
    store.ok(&["timers", "new", "Deep work"]);
    store.ok(&["timers", "start", "deep work"]);
    Seed {
        approved,
        pending,
        loose,
    }
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect()
}

#[test]
fn every_confirmed_command_previews_and_changes_nothing() {
    let store = Store::new();
    let seed = seed(&store);
    let before = store.snapshot();
    let range = ["--from", "2026-09-01", "--to", "2026-09-01"];
    let rebuild = ["entries", "rebuild"]
        .into_iter()
        .chain(range)
        .collect::<Vec<_>>();
    let commands: [(&[&str], &str); 7] = [
        (&["timers", "reset", "deep work"], "timerReset"),
        (&["timers", "rm", "deep work"], "timerDelete"),
        (&["entries", "rm", &seed.loose], "entriesDelete"),
        (&rebuild, "entriesRebuild"),
        (&["projects", "rm", "solo"], "projectDelete"),
        (&["clients", "rm", "acme"], "clientDelete"),
        (&["categories", "rm", "spare"], "categoryDelete"),
    ];
    for (args, operation) in commands {
        // The real command refuses to run without --yes off a terminal...
        let (code, _, exit) = store.fail(args);
        assert_eq!((code.as_str(), exit), ("INVALID_ARGUMENT", 2), "{args:?}");
        // ...and the dry run needs neither --yes nor a prompt.
        let data = store.dry(args);
        assert_eq!(data["operation"], operation, "{args:?}");
        assert!(data["refused"].is_null(), "{args:?}: {data}");
        assert_eq!(store.snapshot(), before, "{args:?} changed something");
    }
}

#[test]
fn a_timer_reset_preview_matches_the_real_reset() {
    let store = Store::new();
    seed(&store);

    let preview = store.dry(&["timers", "reset", "deep"]);
    assert_eq!(preview["effect"], "reset");
    assert_eq!(preview["timer"]["label"], "Deep work");
    assert!(preview["timer"]["startedAt"].is_number());

    store.ok(&["timers", "reset", "deep", "--yes"]);
    let after = store.ok(&["timers", "list"]);
    assert_eq!(after[0]["id"], preview["timer"]["id"]);
    assert_eq!(after[0]["accumulatedMs"], 0);
    assert!(after[0]["startedAt"].is_null());

    let preview = store.dry(&["timers", "rm", "deep"]);
    assert_eq!(preview["effect"], "delete");
    store.ok(&["timers", "rm", "deep", "--yes"]);
    assert_eq!(store.ok(&["timers", "list"]), json!([]));
}

#[test]
fn an_entry_preview_agrees_with_the_real_delete() {
    let store = Store::new();
    let seed = seed(&store);

    // Approved entries are locked, and the preview says so in the real words.
    let preview = store.dry(&["entries", "rm", &seed.approved]);
    assert_eq!(
        preview["refused"],
        json!({ "code": "FAILED", "message": LOCKED })
    );
    assert!(preview["deletion"].is_null());
    let (code, message, exit) = store.fail(&["entries", "rm", &seed.approved, "--yes"]);
    assert_eq!(
        (code.as_str(), message.as_str(), exit),
        ("FAILED", LOCKED, 1)
    );

    // A pending entry lists what goes, and the real delete removes exactly it.
    let preview = store.dry(&["entries", "rm", &seed.loose, &seed.pending]);
    assert!(preview["refused"].is_null());
    let mut previewed = strings(&preview["deletion"]["deletedEntries"]);
    previewed.sort_unstable();
    let mut expected = vec![seed.loose.as_str(), seed.pending.as_str()];
    expected.sort_unstable();
    assert_eq!(previewed, expected);
    assert_eq!(preview["deletion"]["unlinkedEntries"], json!([]));

    let deleted = store.ok(&["entries", "rm", &seed.loose, &seed.pending, "--yes"]);
    let mut deleted = strings(&deleted["deleted"]).to_vec();
    deleted.sort_unstable();
    assert_eq!(deleted, expected);
    let left = store.ok(&["entries", "list", "--from", "2026-09-01"]);
    assert_eq!(left["entries"].as_array().unwrap().len(), 1);
    assert_eq!(left["entries"][0]["id"], seed.approved.as_str());
}

#[test]
fn a_project_preview_agrees_with_the_real_delete() {
    let store = Store::new();
    let seed = seed(&store);

    // Approved time on the project locks it.
    let preview = store.dry(&["projects", "rm", "acme site"]);
    let message = preview["refused"]["message"].as_str().unwrap().to_string();
    assert_eq!(preview["refused"]["code"], "FAILED");
    assert!(message.contains("approved entries"), "{message}");
    assert_eq!(
        store.fail(&["projects", "rm", "acme site", "--yes"]),
        ("FAILED".into(), message, 1)
    );

    // A project with only pending time can go; its entry stays, unlinked.
    let preview = store.dry(&["projects", "rm", "solo"]);
    assert!(preview["refused"].is_null());
    assert_eq!(preview["deletion"]["deletedEntries"], json!([]));
    assert_eq!(
        strings(&preview["deletion"]["unlinkedEntries"]),
        [seed.pending.as_str()]
    );
    assert_eq!(preview["deletion"]["projects"], json!([preview["id"]]));

    store.ok(&["projects", "rm", "solo", "--yes"]);
    let entry = store.ok(&["entries", "show", &seed.pending]);
    assert!(entry["detail"]["entry"]["projectId"].is_null());
    assert!(entry["detail"]["entry"]["categoryId"].is_string());
}

#[test]
fn a_category_preview_agrees_with_the_real_removal() {
    let store = Store::new();
    seed(&store);

    let used = store.dry(&["categories", "rm", "admin"]);
    assert_eq!(used["removal"], "archived");
    assert_eq!(used["category"]["name"], "Admin");
    assert!(used["deletion"].is_null());
    assert!(used["refused"].is_null());
    assert_eq!(
        store.ok(&["categories", "rm", "admin", "--yes"])["removal"],
        "archived"
    );

    let unused = store.dry(&["categories", "rm", "spare"]);
    assert_eq!(unused["removal"], "deleted");
    assert_eq!(unused["deletion"]["categories"][0]["id"], unused["id"]);
    assert_eq!(unused["deletion"]["categories"][0]["wasArchived"], false);
    assert_eq!(
        store.ok(&["categories", "rm", "spare", "--yes"])["removal"],
        "deleted"
    );
}

#[test]
fn a_client_preview_lists_the_projects_that_would_be_detached() {
    let store = Store::new();
    seed(&store);

    let preview = store.dry(&["clients", "rm", "acme"]);
    assert_eq!(preview["client"]["name"], "Acme");
    assert_eq!(preview["detachedProjects"].as_array().unwrap().len(), 1);
    assert_eq!(preview["detachedProjects"][0]["name"], "Acme site");
    assert!(preview["refused"].is_null());

    store.ok(&["clients", "rm", "acme", "--yes"]);
    let projects = store.ok(&["projects", "list"]);
    assert!(projects["projects"]
        .as_array()
        .unwrap()
        .iter()
        .all(|project| project["clientId"].is_null()));
}

#[test]
fn a_rebuild_preview_lists_the_entries_in_range_and_does_not_rebuild() {
    let store = Store::new();
    seed(&store);
    let before = store.snapshot();

    let preview = store.dry(&[
        "entries",
        "rebuild",
        "--from",
        "2026-09-01",
        "--to",
        "2026-09-01",
    ]);
    assert_eq!(preview["entries"].as_array().unwrap().len(), 3);
    assert!(preview["note"]
        .as_str()
        .unwrap()
        .starts_with("Closed entries are frozen"));
    assert_eq!(store.snapshot(), before);

    // The range is checked exactly as for the real command.
    assert_eq!(
        store.fail(&[
            "entries",
            "rebuild",
            "--from",
            "2026-09-02",
            "--to",
            "2026-09-01",
            "--dry-run"
        ]),
        (
            "INVALID_ARGUMENT".into(),
            "--to must be after --from".into(),
            2
        )
    );
}

#[test]
fn a_preview_resolves_references_like_the_real_command() {
    let store = Store::new();
    seed(&store);
    store.ok(&["timers", "new", "Deep dive"]);

    for (args, code) in [
        (&["timers", "rm", "deep"][..], "AMBIGUOUS"),
        (&["timers", "rm", "nothing"][..], "NOT_FOUND"),
        (&["entries", "rm", "ffffffff"][..], "NOT_FOUND"),
        (&["projects", "rm", "nothing"][..], "NOT_FOUND"),
        (&["clients", "rm", "nothing"][..], "NOT_FOUND"),
        (&["categories", "rm", "nothing"][..], "NOT_FOUND"),
    ] {
        let mut dry = args.to_vec();
        dry.push("--dry-run");
        let mut real = args.to_vec();
        real.push("--yes");
        let preview = store.fail(&dry);
        assert_eq!((preview.0.as_str(), preview.2), (code, 1), "{args:?}");
        assert_eq!(preview, store.fail(&real), "{args:?}");
    }
}

#[test]
fn other_commands_reject_dry_run_before_touching_the_data() {
    let store = Store::new();
    for args in [
        &["entries", "list"][..],
        &["projects", "add", "Nope"][..],
        &["status"][..],
        &["settings", "set", "weekly-target-hours", "32"][..],
    ] {
        let mut dry = args.to_vec();
        dry.push("--dry-run");
        assert_eq!(
            store.fail(&dry),
            ("INVALID_ARGUMENT".into(), DRY_RUN_UNSUPPORTED.into(), 2),
            "{args:?}"
        );
    }

    // Not even a missing database is looked at.
    fs::remove_file(store.root.join("data/activity.db")).unwrap();
    assert_eq!(
        store.fail(&["entries", "list", "--dry-run"]),
        ("INVALID_ARGUMENT".into(), DRY_RUN_UNSUPPORTED.into(), 2)
    );
    assert!(!store.root.join("data/activity.db").exists());
}

#[test]
fn the_human_dry_run_says_what_would_happen() {
    let store = Store::new();
    let seed = seed(&store);
    let say = |args: &[&str]| {
        let mut all = args.to_vec();
        all.push("--dry-run");
        let output = store.rize(&all);
        assert_eq!(output.status.code(), Some(0), "{all:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    let short = |id: &str| id[id.len() - 8..].to_string();

    let text = say(&["entries", "rm", &seed.loose, &seed.pending]);
    assert!(
        text.starts_with("Dry run: nothing was changed.\n"),
        "{text}"
    );
    assert!(text.contains("Would delete 2 entries: "), "{text}");
    assert!(text.contains(&short(&seed.loose)), "{text}");

    let text = say(&["projects", "rm", "solo"]);
    assert!(text.contains("Would delete project Solo"), "{text}");
    assert!(
        text.contains("Would unlink 1 entry from project Solo"),
        "{text}"
    );

    assert!(say(&["categories", "rm", "admin"]).contains("Would archive category Admin"));
    assert!(say(&["clients", "rm", "acme"]).contains("Would detach 1 project from client Acme"));
    assert!(say(&["timers", "reset", "deep"]).contains("Would reset timer Deep work ("));

    let text = say(&["entries", "rm", &seed.approved]);
    assert!(
        text.ends_with(&format!("The real command would fail: {LOCKED}\n")),
        "{text}"
    );

    let text = say(&[
        "entries",
        "rebuild",
        "--from",
        "2026-09-01",
        "--to",
        "2026-09-01",
    ]);
    assert!(text.contains("Would rebuild 3 entries between "), "{text}");
}
