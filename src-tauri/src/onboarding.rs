//! Whether the first-run permissions screen has been dealt with.
//!
//! The screen itself is in the frontend, and which step it shows follows the
//! Accessibility state the monitor reports (`permissions`). All that has to
//! outlive a launch is one fact: the user has been through it, by allowing
//! access or by skipping. It is kept as a marker file next to the settings
//! rather than in them, because it is about this Mac and this install, not
//! about an account or a preference the CLI should list.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use openrize_core::settings::config_dir;

const MARKER: &str = "permissions-onboarding-done";

fn marker(dir: &Path) -> PathBuf {
    dir.join(MARKER)
}

fn is_done_in(dir: &Path) -> bool {
    marker(dir).is_file()
}

fn mark_done_in(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(marker(dir), b"")
}

/// A developer asked to see the screen again (dev builds only, see
/// dev_tools.rs). Kept in memory, never by deleting the marker: config_dir() is
/// shared with the installed app when a dev build runs without XDG_CONFIG_HOME,
/// so removing the marker would change the release app too. Lost on relaunch.
#[derive(Default)]
pub struct Rerun(AtomicBool);

impl Rerun {
    pub fn set(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn needed(rerun: bool, dir: &Path) -> bool {
    rerun || !is_done_in(dir)
}

/// Whether the screen may still be shown. Existing installs have no marker, so
/// they see it once after the update, and only if Accessibility is off.
#[tauri::command]
pub fn onboarding_needed(rerun: tauri::State<'_, Rerun>) -> bool {
    needed(rerun.is_set(), &config_dir())
}

/// The user skipped, or finished. Never shown again, unless a developer asks.
#[tauri::command]
pub fn finish_onboarding(rerun: tauri::State<'_, Rerun>) -> Result<(), String> {
    rerun.clear();
    mark_done_in(&config_dir()).map_err(|error| format!("Couldn't save your choice: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openrize-onboarding-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_fresh_install_has_not_been_through_it() {
        let dir = scratch("fresh");
        assert!(!is_done_in(&dir));
    }

    #[test]
    fn finishing_is_remembered_and_repeatable() {
        let dir = scratch("done");
        mark_done_in(&dir).unwrap();
        assert!(is_done_in(&dir));
        mark_done_in(&dir).unwrap();
        assert!(is_done_in(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_rerun_is_needed_even_when_the_marker_exists() {
        let dir = scratch("rerun");
        mark_done_in(&dir).unwrap();
        assert!(needed(true, &dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_finished_install_without_a_rerun_is_not_needed() {
        let dir = scratch("finished");
        mark_done_in(&dir).unwrap();
        assert!(!needed(false, &dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
