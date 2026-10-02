//! The curl installer against a local app bundle fixture. Apple codesign
//! checks are tested both with a recorded mock and against macOS's real
//! `/usr/bin/codesign`, which must reject a rize OpenRize did not notarize.
//! Every run names its app with `--app`, so the machine's own installed
//! OpenRize never takes part.
#![cfg(target_os = "macos")]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const NO_APP: &str = "No rize tracking information found. Are you sure you've installed the app?";

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    app: PathBuf,
    mocks: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target")
            .canonicalize()
            .unwrap()
            .join(format!(
                "installer-tests-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        let home = root.join("home with spaces");
        let app = root.join("Applications/openrize.app");
        let mocks = root.join("commands");
        let macos = app.join("Contents/MacOS");
        for dir in [&home, &mocks, &macos] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::copy(env!("CARGO_BIN_EXE_rize"), macos.join("rize")).unwrap();
        fs::write(macos.join("openrize"), "desktop app").unwrap();
        let fixture = Self {
            root,
            home,
            app,
            mocks,
        };
        fixture.mock(
            "uname",
            "case \"$1\" in -s) printf '%s\\n' \"${TEST_OS:-Darwin}\";; esac",
        );
        fixture.mock(
            "codesign",
            "printf '%s\\n' \"$*\" >> \"$TEST_ROOT/checks\"; exit \"${TEST_FAIL_SIGNATURE:-0}\"",
        );
        for tool in ["curl", "tar", "cargo", "sudo"] {
            fixture.mock(tool, "printf 'installer must not run this\\n' >&2; exit 99");
        }
        fixture
    }

    fn mock(&self, name: &str, body: &str) {
        let path = self.mocks.join(name);
        fs::write(&path, format!("#!/bin/bash\nset -euo pipefail\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new("/bin/bash");
        command
            .args(["-s", "--", "--app"])
            .arg(&self.app)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", self.mocks.display()),
            )
            .env("HOME", &self.home)
            .env("TEST_ROOT", &self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, mut command: Command) -> Output {
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(include_bytes!("../../../install.sh"))
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn link(&self) -> PathBuf {
        self.home.join(".local/bin/rize")
    }

    fn cli(&self) -> PathBuf {
        self.app.join("Contents/MacOS/rize")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn links_the_apps_rize_into_the_default_path() {
    let fixture = Fixture::new();
    let output = fixture.run(fixture.command());
    assert_success(&output);
    assert!(fixture.link().is_symlink());
    assert_eq!(fs::read_link(fixture.link()).unwrap(), fixture.cli());
    let version = Command::new(fixture.link())
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("rize {}", env!("CARGO_PKG_VERSION"))
    );
    let checks = fs::read_to_string(fixture.root.join("checks")).unwrap();
    assert_eq!(checks.lines().count(), 1);
    assert!(checks.starts_with("--verify --strict --check-notarization "));
    assert!(checks.contains(r#"certificate leaf[subject.OU] = "Z899WY5Y94""#));
    assert!(checks.trim_end().ends_with(fixture.cli().to_str().unwrap()));
    assert!(!fixture.home.join(".zshrc").exists());
    let repeat = fixture.run(fixture.command());
    assert_success(&repeat);
    assert!(String::from_utf8(repeat.stdout)
        .unwrap()
        .contains("already installed"));
}

#[test]
fn concurrent_installations_never_clobber_each_other() {
    let fixture = Fixture::new();
    let children: Vec<_> = (0..3)
        .map(|_| {
            let mut child = fixture.command().spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(include_bytes!("../../../install.sh"))
                .unwrap();
            child
        })
        .collect();
    for child in children {
        assert_success(&child.wait_with_output().unwrap());
    }
    assert_eq!(fs::read_link(fixture.link()).unwrap(), fixture.cli());
}

#[test]
fn a_missing_app_or_one_without_rize_links_nothing() {
    let fixture = Fixture::new();
    let mut command = fixture.command();
    command.args(["--app", "/nonexistent/openrize.app"]);
    let output = fixture.run(command);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains(NO_APP));

    // Apps from before rize shipped have no CLI inside.
    fs::remove_file(fixture.cli()).unwrap();
    let output = fixture.run(fixture.command());
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not include rize yet"));
    assert!(!fixture.link().exists());
    assert!(!fixture.root.join("checks").exists());
}

#[test]
fn apple_verification_fails_closed() {
    let fixture = Fixture::new();
    let mut command = fixture.command();
    command.env("TEST_FAIL_SIGNATURE", "1");
    let output = fixture.run(command);
    assert_eq!(output.status.code(), Some(1));
    assert!(!fixture.link().exists());
}

#[test]
fn real_codesign_rejects_a_rize_openrize_did_not_notarize() {
    // The locally built CLI carries only the linker's ad-hoc signature.
    let fixture = Fixture::new();
    fs::remove_file(fixture.mocks.join("codesign")).unwrap();
    let output = fixture.run(fixture.command());
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not notarized and signed by OpenRize")
    );
    assert!(!fixture.link().exists());
}

#[test]
fn existing_files_and_symlinks_are_never_replaced() {
    for symlink in [false, true] {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.link().parent().unwrap()).unwrap();
        let existing = fixture.home.join("existing");
        fs::write(&existing, "do not replace").unwrap();
        if symlink {
            std::os::unix::fs::symlink(&existing, fixture.link()).unwrap();
        } else {
            fs::copy(&existing, fixture.link()).unwrap();
        }
        let output = fixture.run(fixture.command());
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            fs::read_to_string(fixture.link()).unwrap(),
            "do not replace"
        );
        assert_eq!(fixture.link().is_symlink(), symlink);
    }
}

#[test]
fn custom_destination_and_invalid_inputs() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("custom bin");
    let mut command = fixture.command();
    command.arg("--dir").arg(&destination);
    assert_success(&fixture.run(command));
    assert_eq!(
        fs::read_link(destination.join("rize")).unwrap(),
        fixture.cli()
    );
    for args in [vec!["--unknown"], vec!["--dir"], vec!["--app"]] {
        let mut command = fixture.command();
        command.args(args);
        assert_eq!(fixture.run(command).status.code(), Some(2));
    }
    let mut command = fixture.command();
    command.env("TEST_OS", "Linux");
    assert_eq!(fixture.run(command).status.code(), Some(1));
}
