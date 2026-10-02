//! Installer behavior with local release fixtures. Apple codesign checks are
//! tested both with a recorded mock and against macOS's real `/usr/bin/codesign`,
//! which must reject a CLI that OpenRize did not sign and notarize.
#![cfg(target_os = "macos")]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const ASSET: &str = "openrize.app.tar.gz";

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    assets: PathBuf,
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
        let assets = root.join("assets");
        let mocks = root.join("commands");
        for dir in [&home, &assets, &mocks, &root.join("tmp")] {
            fs::create_dir_all(dir).unwrap();
        }
        let macos = assets.join("openrize.app/Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_rize"), macos.join("rize")).unwrap();
        fs::write(macos.join("openrize"), "desktop app").unwrap();
        pack(&assets);
        let fixture = Self {
            root,
            home,
            assets,
            mocks,
        };
        fixture.mock("uname", "case \"$1\" in -s) printf '%s\\n' \"${TEST_OS:-Darwin}\";; -m) printf '%s\\n' \"${TEST_ARCH:-arm64}\";; esac");
        fixture.mock(
            "curl",
            r#"printf '%s\n' "$*" >> "$TEST_ROOT/downloads"
[[ "${TEST_FAIL_DOWNLOAD:-0}" == 0 ]] || exit 22
url='' output=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output) output="$2"; shift 2;;
    https://*) url="$1"; shift;;
    *) shift;;
  esac
done
cp "$TEST_ASSETS/${url##*/}" "$output""#,
        );
        fixture.mock(
            "codesign",
            "printf '%s\\n' \"$*\" >> \"$TEST_ROOT/checks\"; exit \"${TEST_FAIL_SIGNATURE:-0}\"",
        );
        fixture.mock(
            "cargo",
            "printf 'installer must not invoke cargo\\n' >&2; exit 99",
        );
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
            .args(["-s", "--"])
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", self.mocks.display()),
            )
            .env("HOME", &self.home)
            .env("TMPDIR", self.root.join("tmp"))
            .env("TEST_ROOT", &self.root)
            .env("TEST_ASSETS", &self.assets)
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

    fn binary(&self) -> PathBuf {
        self.home.join(".local/bin/rize")
    }
}

/// Archives `openrize.app` the way the release's updater bundle is laid out.
fn pack(assets: &Path) {
    assert!(Command::new("/usr/bin/tar")
        .current_dir(assets)
        .args(["-czf", ASSET, "openrize.app"])
        .status()
        .unwrap()
        .success());
}

fn version_of(binary: &Path) -> String {
    let output = Command::new(binary).arg("--version").output().unwrap();
    assert_success(&output);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
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
fn piped_installer_uses_default_path_without_rust_or_gui() {
    let fixture = Fixture::new();
    assert_success(&fixture.run(fixture.command()));
    assert_eq!(
        version_of(&fixture.binary()),
        format!("rize {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        fs::metadata(fixture.binary()).unwrap().permissions().mode() & 0o777,
        0o755
    );
    // Only the CLI comes out of the app archive.
    assert_eq!(
        fs::read_dir(fixture.binary().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    let checks = fs::read_to_string(fixture.root.join("checks")).unwrap();
    assert_eq!(checks.lines().count(), 1);
    assert!(checks.starts_with("--verify --strict --check-notarization "));
    assert!(checks.contains(r#"certificate leaf[subject.OU] = "Z899WY5Y94""#));
    assert!(!fixture.home.join(".zshrc").exists());
    assert_eq!(fs::read_dir(fixture.root.join("tmp")).unwrap().count(), 0);
    let repeat = fixture.run(fixture.command());
    assert_success(&repeat);
    assert!(String::from_utf8(repeat.stdout)
        .unwrap()
        .contains("already installed"));
    let requests = fs::read_to_string(fixture.root.join("downloads")).unwrap();
    assert!(requests.contains("--proto =https --proto-redir =https"));
    assert!(requests.contains(
        "https://github.com/Kareeme246/OpenRize/releases/latest/download/openrize.app.tar.gz"
    ));
}

#[test]
fn concurrent_installations_never_clobber_each_other() {
    let fixture = Fixture::new();
    let mut children = Vec::new();
    for _ in 0..2 {
        let mut child = fixture.command().spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(include_bytes!("../../../install.sh"))
            .unwrap();
        children.push(child);
    }
    let outputs: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();
    assert!(outputs.iter().any(|output| output.status.success()));
    assert_eq!(
        fs::read(fixture.binary()).unwrap(),
        fs::read(env!("CARGO_BIN_EXE_rize")).unwrap()
    );
    assert_eq!(
        fs::metadata(fixture.binary()).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn download_and_apple_verification_fail_closed() {
    for failure in ["TEST_FAIL_DOWNLOAD", "TEST_FAIL_SIGNATURE"] {
        let fixture = Fixture::new();
        let mut command = fixture.command();
        command.env(failure, "1");
        let output = fixture.run(command);
        assert_eq!(output.status.code(), Some(1), "accepted {failure}");
        assert!(!fixture.binary().exists());
        assert_eq!(fs::read_dir(fixture.root.join("tmp")).unwrap().count(), 0);
        if failure == "TEST_FAIL_DOWNLOAD" {
            assert!(!fixture.root.join("checks").exists());
        }
    }
}

#[test]
fn real_codesign_rejects_a_cli_openrize_did_not_notarize() {
    // The locally built CLI carries only the linker's ad-hoc signature.
    let fixture = Fixture::new();
    fs::remove_file(fixture.mocks.join("codesign")).unwrap();
    let output = fixture.run(fixture.command());
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not notarized and signed by OpenRize")
    );
    assert!(!fixture.binary().exists());
    assert_eq!(fs::read_dir(fixture.root.join("tmp")).unwrap().count(), 0);
}

#[test]
fn release_without_the_cli_or_a_valid_archive_fails_without_installation() {
    for corrupt in [false, true] {
        let fixture = Fixture::new();
        if corrupt {
            fs::write(fixture.assets.join(ASSET), b"not a valid archive").unwrap();
        } else {
            // Releases before the CLI shipped carry only the desktop app.
            fs::remove_file(fixture.assets.join("openrize.app/Contents/MacOS/rize")).unwrap();
            pack(&fixture.assets);
        }
        let output = fixture.run(fixture.command());
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("does not include the CLI"));
        assert!(!fixture.binary().exists());
        assert!(!fixture.root.join("checks").exists());
        assert_eq!(fs::read_dir(fixture.root.join("tmp")).unwrap().count(), 0);
    }
}

#[test]
fn existing_executable_and_symlink_are_never_replaced() {
    for symlink in [false, true] {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.binary().parent().unwrap()).unwrap();
        let existing = fixture.home.join("existing");
        fs::write(&existing, "do not replace").unwrap();
        if symlink {
            std::os::unix::fs::symlink(&existing, fixture.binary()).unwrap();
        } else {
            fs::copy(&existing, fixture.binary()).unwrap();
        }
        let output = fixture.run(fixture.command());
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            fs::read_to_string(fixture.binary()).unwrap(),
            "do not replace"
        );
        assert_eq!(fixture.binary().is_symlink(), symlink);
    }
}

#[test]
fn pinned_version_custom_destination_and_invalid_inputs() {
    let fixture = Fixture::new();
    let destination = fixture.root.join("custom bin");
    let mut command = fixture.command();
    command
        .arg("--dir")
        .arg(&destination)
        .args(["--version", "0.8.10"]);
    assert_success(&fixture.run(command));
    assert!(destination.join("rize").is_file());
    assert!(fs::read_to_string(fixture.root.join("downloads"))
        .unwrap()
        .contains("/download/v0.8.10/openrize.app.tar.gz"));
    for args in [
        vec!["--unknown"],
        vec!["--version", "../main"],
        vec!["--dir"],
    ] {
        let mut command = fixture.command();
        command.args(args);
        assert_eq!(fixture.run(command).status.code(), Some(2));
    }
    for (os, arch) in [("Linux", "arm64"), ("Darwin", "x86_64")] {
        let unsupported = Fixture::new();
        let mut command = unsupported.command();
        command.env("TEST_OS", os).env("TEST_ARCH", arch);
        assert_eq!(unsupported.run(command).status.code(), Some(1));
        assert!(!unsupported.root.join("downloads").exists());
    }
}
