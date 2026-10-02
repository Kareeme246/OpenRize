//! Finding the tools an extension talks to. A Finder-launched app has a bare
//! PATH, so the usual install folders are searched too.

use std::path::{Path, PathBuf};

/// Where command-line tools commonly live that a GUI app's PATH misses.
const EXTRA_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];
const HOME_DIRS: &[&str] = &[".local/bin", ".cargo/bin", "bin", ".bun/bin"];

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Windows has no executable bit; the `.exe` name says it.
#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// The first executable called `name` on PATH or in the usual install folders.
pub fn which(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    dirs.extend(EXTRA_DIRS.iter().map(PathBuf::from));
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.extend(HOME_DIRS.iter().map(|dir| home.join(dir)));
    }
    dirs.into_iter()
        .map(|dir| dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
        .find(|candidate| is_executable(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_tool_that_exists_and_not_one_that_does_not() {
        #[cfg(unix)]
        assert!(which("sh").is_some());
        assert!(which("definitely-not-an-installed-tool-xyz").is_none());
    }
}
