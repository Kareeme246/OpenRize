//! Installer behavior with local release fixtures. Apple signature/notarization
//! commands are mocked here; actual signing remains the release workflow's gate.
#![cfg(target_os = "macos")]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const ASSET: &str = "openrize-cli-darwin-aarch64.zip";

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
        fs::create_dir(assets.join("openrize-cli")).unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_openrize"),
            assets.join("openrize-cli/openrize"),
        )
        .unwrap();
        fs::write(assets.join("openrize-cli/LICENSE"), "fixture license\n").unwrap();
        assert!(Command::new("/usr/bin/zip")
            .current_dir(&assets)
            .args(["-qr", ASSET, "openrize-cli"])
            .status()
            .unwrap()
            .success());
        let checksum = Command::new("/usr/bin/shasum")
            .current_dir(&assets)
            .args(["-a", "256", ASSET])
            .output()
            .unwrap();
        assert!(checksum.status.success());
        fs::write(assets.join(format!("{ASSET}.sha256")), checksum.stdout).unwrap();
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
            "printf 'signature\\n' >> \"$TEST_ROOT/checks\"; exit \"${TEST_FAIL_SIGNATURE:-0}\"",
        );
        fixture.mock(
            "spctl",
            "printf 'notarization\\n' >> \"$TEST_ROOT/checks\"; exit \"${TEST_FAIL_NOTARY:-0}\"",
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
        self.home.join(".local/bin/openrize")
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
fn piped_installer_uses_default_path_without_rust_or_gui() {
    let fixture = Fixture::new();
    assert_success(&fixture.run(fixture.command()));
    let version = Command::new(fixture.binary())
        .arg("--version")
        .output()
        .unwrap();
    assert_success(&version);
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("openrize {}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        fs::metadata(fixture.binary()).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        fs::read_to_string(fixture.binary().with_file_name("openrize.LICENSE")).unwrap(),
        "fixture license\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("checks")).unwrap(),
        "signature\nnotarization\n"
    );
    assert!(!fixture.home.join(".zshrc").exists());
    assert_eq!(fs::read_dir(fixture.root.join("tmp")).unwrap().count(), 0);
    let repeat = fixture.run(fixture.command());
    assert_success(&repeat);
    assert!(String::from_utf8(repeat.stdout)
        .unwrap()
        .contains("already installed"));
    let requests = fs::read_to_string(fixture.root.join("downloads")).unwrap();
    assert!(requests.contains("--proto =https --proto-redir =https"));
    assert!(requests.contains("https://github.com/Kareeme246/OpenRize/releases/latest/download/"));
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
        fs::read(env!("CARGO_BIN_EXE_openrize")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(fixture.binary().with_file_name("openrize.LICENSE")).unwrap(),
        "fixture license\n"
    );
    assert_eq!(
        fs::metadata(fixture.binary()).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn checksum_download_and_apple_verification_fail_closed() {
    for failure in [
        "checksum",
        "TEST_FAIL_DOWNLOAD",
        "TEST_FAIL_SIGNATURE",
        "TEST_FAIL_NOTARY",
    ] {
        let fixture = Fixture::new();
        let mut command = fixture.command();
        if failure == "checksum" {
            fs::write(
                fixture.assets.join(format!("{ASSET}.sha256")),
                format!("{}  {ASSET}\n", "0".repeat(64)),
            )
            .unwrap();
        } else {
            command.env(failure, "1");
        }
        let output = fixture.run(command);
        assert_eq!(output.status.code(), Some(1), "accepted {failure}");
        assert!(!fixture.binary().exists());
        assert_eq!(fs::read_dir(fixture.root.join("tmp")).unwrap().count(), 0);
        if ["checksum", "TEST_FAIL_DOWNLOAD"].contains(&failure) {
            assert!(!fixture.root.join("checks").exists());
        }
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
        assert!(!fixture.binary().with_file_name("openrize.LICENSE").exists());
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
        .args(["--version", "0.8.1"]);
    assert_success(&fixture.run(command));
    assert!(destination.join("openrize").is_file());
    assert!(fs::read_to_string(fixture.root.join("downloads"))
        .unwrap()
        .contains("/download/v0.8.1/"));
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
