//! Whether the first-run permissions screen has been dealt with (allowed or
//! skipped). Kept as a marker file next to the settings, not in them: it is
//! about this install, not a preference the CLI should list.

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

/// A dev build asked to see the screen again. Kept in memory, never by deleting
/// the marker: config_dir() is shared with the installed app when XDG_CONFIG_HOME
/// is unset, so removing it would change the release app too.
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
