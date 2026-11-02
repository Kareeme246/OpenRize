//! Keeps the `rize` CLI bundled with the app reachable from the shell. It is
//! checked at every launch, so a link or PATH entry the user removed comes back,
//! and it always points at the app that is running.
//!
//! - macOS: a symlink at `~/.local/bin/rize` to the bundled binary.
//! - Windows: this folder on the user PATH (`rize.exe` sits next to the app).
//! - Elsewhere: nothing to install.

#[cfg_attr(target_os = "macos", path = "cli_install/macos.rs")]
#[cfg_attr(windows, path = "cli_install/windows.rs")]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    path = "cli_install/unsupported.rs"
)]
mod imp;

/// Called unconditionally at startup; each platform decides what, if anything,
/// to do.
pub fn install_bundled_cli() {
    imp::install_bundled_cli();
}
