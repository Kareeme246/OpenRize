//! Keeps the bundled `rize` reachable from the shell, rechecked every launch:
//! a macOS symlink at `~/.local/bin/rize`, or this folder on the Windows user
//! PATH. Elsewhere nothing is installed.

#[cfg_attr(target_os = "macos", path = "cli_install/macos.rs")]
#[cfg_attr(windows, path = "cli_install/windows.rs")]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    path = "cli_install/unsupported.rs"
)]
mod imp;

pub fn install_bundled_cli() {
    imp::install_bundled_cli();
}
