//! No lifetime tie off Windows and macOS; the runtime is not offered there.

use std::process::{Child, Command};

pub struct LifetimeGuard;

pub fn prepare(_command: &mut Command) {}

pub fn attach(_child: &Child) -> Result<LifetimeGuard, String> {
    Ok(LifetimeGuard)
}

pub fn run_watchdog(_app: u32, _child: u32) -> i32 {
    0
}
