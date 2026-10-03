//! `rize install-path`: puts this rize on PATH without replacing anything.

use std::path::PathBuf;

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub executable: String,
    /// The symlink (macOS, Linux) or the folder added to PATH (Windows).
    pub path: String,
    /// Already set up by an earlier run; nothing changed.
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

#[cfg(windows)]
pub fn uninstall() -> Result<String, String> {
    Err("rize uninstall-path is only available on macOS and Linux".into())
}

/// Adds this executable's folder to the user's PATH.
#[cfg(windows)]
pub fn install(dir: Option<PathBuf>) -> Result<Installation, String> {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    use windows_registry::CURRENT_USER;

    if dir.is_some() {
        return Err("--dir is not used on Windows; rize adds its own folder to PATH".into());
    }
    let executable = executable()?;
    let folder = executable
        .parent()
        .ok_or("rize has no folder")?
        .display()
        .to_string();
    let key = CURRENT_USER
        .create("Environment")
        .map_err(|e| e.to_string())?;
    let current = key.get_string("Path").unwrap_or_default();
    let unchanged = current.split(';').any(|entry| {
        entry
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(folder.trim_end_matches('\\'))
    });
    if !unchanged {
        let next = match current.trim_end_matches(';') {
            "" => folder.clone(),
            rest => format!("{rest};{folder}"),
        };
        // REG_EXPAND_SZ, as Windows writes it, so `%USERPROFILE%`-style
        // entries keep expanding.
        key.set_expand_string("Path", &next)
            .map_err(|e| e.to_string())?;
        // SAFETY: a broadcast with a static string; the result is ignored.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(w!("Environment").as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                5000,
                None,
            );
        }
    }
    Ok(Installation {
        executable: executable.display().to_string(),
        path: folder,
        unchanged,
    })
}
