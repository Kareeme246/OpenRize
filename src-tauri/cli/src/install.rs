//! `rize install-path`: puts this rize on PATH without replacing anything.

use std::path::PathBuf;

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub executable: String,
    /// The symlink (macOS, Linux) or the folder added to PATH (Windows).
    pub path: String,
    pub unchanged: bool,
}

fn executable() -> Result<PathBuf, String> {
    let path = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|e| e.to_string())?;
    // Windows canonical paths are verbatim (`\\?\C:\...`), which PATH and
    // most shells do not accept.
    Ok(PathBuf::from(
        path.to_string_lossy()
            .strip_prefix(r"\\?\")
            .map(str::to_owned)
            .unwrap_or_else(|| path.to_string_lossy().into_owned()),
    ))
}

/// Symlinks `<dir>/rize` (default `~/.local/bin`) to this executable.
#[cfg(unix)]
pub fn install(dir: Option<PathBuf>) -> Result<Installation, String> {
    let dir = std::path::absolute(match dir {
        Some(dir) => dir,
        None => std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is not set; pass --dir")?
            .join(".local/bin"),
    })
    .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let executable = executable()?;
    let path = dir.join("rize");
    let unchanged = match std::fs::symlink_metadata(&path) {
        Ok(meta)
            if meta.file_type().is_symlink()
                && std::fs::read_link(&path).ok().as_ref() == Some(&executable) =>
        {
            true
        }
        Ok(_) => {
            return Err(format!(
                "{} already exists; nothing was replaced",
                path.display()
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(&executable, &path).map_err(|e| e.to_string())?;
            false
        }
        Err(e) => return Err(e.to_string()),
    };
    Ok(Installation {
        executable: executable.display().to_string(),
        path: path.display().to_string(),
        unchanged,
    })
}

#[cfg(unix)]
pub fn uninstall() -> Result<String, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    let path = PathBuf::from(home).join(".local/bin/rize");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => std::fs::remove_file(&path).map_err(|error| error.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(format!("No installed rize at {}", path.display()));
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(format!("Removed {}", path.display()))
}

/// Adds this executable's folder to the user's PATH (via `openrize_core::user_path`).
#[cfg(windows)]
pub fn install(dir: Option<PathBuf>) -> Result<Installation, String> {
    if dir.is_some() {
        return Err("--dir is not used on Windows; rize adds its own folder to PATH".into());
    }
    let executable = executable()?;
    let folder = executable
        .parent()
        .ok_or("rize has no folder")?
        .display()
        .to_string();
    let changed = openrize_core::user_path::ensure_folder(&folder)?;
    Ok(Installation {
        executable: executable.display().to_string(),
        path: folder,
        unchanged: !changed,
    })
}

/// Removes this executable's folder from the user's PATH, however it is
/// written there and wherever it appears. Nothing else in PATH changes.
#[cfg(windows)]
pub fn uninstall() -> Result<String, String> {
    let folder = executable()?
        .parent()
        .ok_or("rize has no folder")?
        .display()
        .to_string();
    let changed = openrize_core::user_path::remove_folder(&folder)?;
    Ok(if changed {
        format!("Removed {folder} from PATH")
    } else {
        format!("{folder} was not on PATH")
    })
}
