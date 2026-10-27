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

/// The user PATH as a list of folders, compared the way Windows does:
/// ignoring case and a trailing backslash.
#[cfg(windows)]
mod path_list {
    fn same(entry: &str, folder: &str) -> bool {
        entry
            .trim()
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(folder.trim_end_matches('\\'))
    }

    pub fn contains(path: &str, folder: &str) -> bool {
        path.split(';').any(|entry| same(entry, folder))
    }

    /// `path` with `folder` last, unchanged if it is already listed.
    pub fn with(path: &str, folder: &str) -> String {
        match path.trim_end_matches(';') {
            _ if contains(path, folder) => path.to_owned(),
            "" => folder.to_owned(),
            rest => format!("{rest};{folder}"),
        }
    }

    /// `path` without any entry for `folder`, other entries untouched.
    pub fn without(path: &str, folder: &str) -> String {
        path.split(';')
            .filter(|entry| !same(entry, folder))
            .collect::<Vec<_>>()
            .join(";")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const FOLDER: &str = r"C:\Users\Ada\AppData\Local\OpenRize";

        #[test]
        fn finds_the_folder_however_it_is_written() {
            for entry in [
                FOLDER,
                r"C:\Users\Ada\AppData\Local\OpenRize\",
                r"c:\users\ada\appdata\local\openrize",
            ] {
                assert!(contains(&format!(r"C:\bin;{entry};D:\tools"), FOLDER));
            }
            assert!(!contains(
                r"C:\bin;C:\Users\Ada\AppData\Local\OpenRize2",
                FOLDER
            ));
            assert!(!contains("", FOLDER));
        }

        #[test]
        fn adds_once_after_the_existing_entries() {
            assert_eq!(with("", FOLDER), FOLDER);
            assert_eq!(with(r"C:\bin;", FOLDER), format!(r"C:\bin;{FOLDER}"));
            let once = with(r"C:\bin", FOLDER);
            assert_eq!(with(&once, FOLDER), once);
        }

        #[test]
        fn removes_only_the_folder() {
            let path = format!(r"%USERPROFILE%\bin;{FOLDER};D:\tools;{FOLDER}\");
            assert_eq!(without(&path, FOLDER), r"%USERPROFILE%\bin;D:\tools");
            assert_eq!(without(FOLDER, FOLDER), "");
            assert_eq!(without(r"C:\bin", FOLDER), r"C:\bin");
        }
    }
}

/// Writes the user PATH (`HKCU\Environment`) and tells running programs.
/// Returns this rize's folder and whether `change` left the PATH as it was
/// (it returns `None` to do so).
#[cfg(windows)]
fn edit_user_path(
    change: impl FnOnce(&str, &str) -> Option<String>,
) -> Result<(String, bool), String> {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    use windows_registry::CURRENT_USER;

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
    let Some(next) = change(&current, &folder) else {
        return Ok((folder, true));
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
    Ok((folder, false))
}

/// Adds this executable's folder to the user's PATH.
#[cfg(windows)]
pub fn install(dir: Option<PathBuf>) -> Result<Installation, String> {
    if dir.is_some() {
        return Err("--dir is not used on Windows; rize adds its own folder to PATH".into());
    }
    let (folder, unchanged) = edit_user_path(|path, folder| {
        (!path_list::contains(path, folder)).then(|| path_list::with(path, folder))
    })?;
    Ok(Installation {
        executable: executable()?.display().to_string(),
        path: folder,
        unchanged,
    })
}

/// Removes this executable's folder from the user's PATH, however it is
/// written there and wherever it appears. Nothing else in PATH changes.
#[cfg(windows)]
pub fn uninstall() -> Result<String, String> {
    let (folder, unchanged) = edit_user_path(|path, folder| {
        path_list::contains(path, folder).then(|| path_list::without(path, folder))
    })?;
    Ok(if unchanged {
        format!("{folder} was not on PATH")
    } else {
        format!("Removed {folder} from PATH")
    })
}
