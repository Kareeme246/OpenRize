//! Keeps `llama-server` below normal priority and from outliving the app, even after a crash:
//! Windows uses a kill-on-close Job Object; macOS has none, so the app re-executes itself as a
//! `--llama-watchdog` that waits on both pids' exit events and kills the server if the app goes.

use std::process::{Child, Command};

pub const WATCHDOG_FLAG: &str = "--llama-watchdog";

#[cfg_attr(target_os = "macos", path = "guard/macos.rs")]
#[cfg_attr(windows, path = "guard/windows.rs")]
#[cfg_attr(not(any(target_os = "macos", windows)), path = "guard/unsupported.rs")]
mod platform;

/// Dropping it releases the tie (Windows: the child dies with the handle; macOS: the watchdog
/// is reaped). Hold it as long as the child.
pub use platform::LifetimeGuard;

/// Before spawn: creation flags for priority and no console window.
pub fn prepare(command: &mut Command) {
    platform::prepare(command);
}

/// After spawn. The caller kills the child if this fails: an unguarded server could outlive the app.
pub fn attach(child: &Child) -> Result<LifetimeGuard, String> {
    platform::attach(child)
}

/// `[program, --llama-watchdog, <app pid>, <child pid>]` is the watchdog's whole command line.
fn watchdog_request(args: impl IntoIterator<Item = String>) -> Option<(u32, u32)> {
    let mut args = args.into_iter().skip(1);
    if args.next()? != WATCHDOG_FLAG {
        return None;
    }
    let app = args.next()?.parse().ok()?;
    let child = args.next()?.parse().ok()?;
    args.next().is_none().then_some((app, child))
}

/// Called first thing in `main`: if this process was started as the watchdog, runs it and returns
/// the exit code, else `None` and the app starts normally.
pub fn run_if_requested(args: impl IntoIterator<Item = String>) -> Option<i32> {
    let (app, child) = watchdog_request(args)?;
    Some(platform::run_watchdog(app, child))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_watchdog_command_line_is_recognised() {
        assert_eq!(
            watchdog_request(args(&["openrize", WATCHDOG_FLAG, "123", "456"])),
            Some((123, 456))
        );
    }

    #[test]
    fn anything_else_starts_the_app() {
        for list in [
            &["openrize"][..],
            &["openrize", "--background"],
            &["openrize", WATCHDOG_FLAG],
            &["openrize", WATCHDOG_FLAG, "123"],
            &["openrize", WATCHDOG_FLAG, "abc", "456"],
            &["openrize", WATCHDOG_FLAG, "-1", "456"],
            &["openrize", WATCHDOG_FLAG, "1", "2", "3"],
            &["openrize", "--background", WATCHDOG_FLAG, "1", "2"],
        ] {
            assert_eq!(watchdog_request(args(list)), None, "{list:?}");
        }
    }
}
