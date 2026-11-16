//! Every AI child starts from an empty environment. `llama-server` reads a `LLAMA_ARG_*` variable
//! for each option missing from its command line, MCP stdio servers among them, so an inherited
//! variable could run a command under the app's identity.

use std::ffi::OsStr;
use std::process::Command;

/// The only variables a child gets. None on macOS, where the server and the sidecar both run
/// without any; on Windows the system root that Winsock and the GPU driver cannot load without,
/// and where temporary and per-user files go.
#[cfg(not(windows))]
const KEPT: &[&str] = &[];
#[cfg(windows)]
const KEPT: &[&str] = &[
    "SystemRoot",
    "SystemDrive",
    "windir",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "LOCALAPPDATA",
];

pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    for name in KEPT {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_child_inherits_nothing_but_the_kept_variables() {
        std::env::set_var("LLAMA_ARG_MCP_SERVERS_JSON", "inherited");
        std::env::set_var("LLAMA_API_KEY", "inherited");
        assert!(std::env::var_os("PATH").is_some(), "nothing to inherit");

        let output = command("/usr/bin/env").output().unwrap();
        assert!(output.status.success());
        let printed = String::from_utf8(output.stdout).unwrap();
        let names: Vec<&str> = printed
            .lines()
            .filter_map(|line| line.split_once('=').map(|(name, _)| name))
            .collect();
        assert!(!printed.contains("inherited"), "{printed}");
        for name in &names {
            assert!(KEPT.contains(name), "the child inherited {name}");
        }
        // The same program without the helper does inherit, so the check above can fail.
        let inherited = Command::new("/usr/bin/env").output().unwrap();
        assert!(String::from_utf8_lossy(&inherited.stdout).contains("LLAMA_ARG_MCP_SERVERS_JSON="));
    }
}
