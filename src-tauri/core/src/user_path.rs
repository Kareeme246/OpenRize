//! The Windows user PATH (`HKCU\Environment`), edited by `rize install-path` and `uninstall-path`
//! and checked by the app at every launch.

/// The user PATH as a list of folders, compared the way Windows does:
/// ignoring case and a trailing backslash.
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

/// Writes the user PATH and tells running programs. `change` gets the current
/// PATH and returns the new one, or `None` to leave it alone. Returns whether
/// the PATH was written.
fn edit(change: impl FnOnce(&str) -> Option<String>) -> Result<bool, String> {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    use windows_registry::CURRENT_USER;

    let key = CURRENT_USER
        .create("Environment")
        .map_err(|e| e.to_string())?;
    let current = key.get_string("Path").unwrap_or_default();
    let Some(next) = change(&current) else {
        return Ok(false);
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
    Ok(true)
}

/// Adds `folder` to the user PATH unless already there. Returns whether the PATH was written.
pub fn ensure_folder(folder: &str) -> Result<bool, String> {
    edit(|path| (!path_list::contains(path, folder)).then(|| path_list::with(path, folder)))
}

/// Removes every entry for `folder` from the user PATH. Returns whether the PATH was written.
pub fn remove_folder(folder: &str) -> Result<bool, String> {
    edit(|path| path_list::contains(path, folder).then(|| path_list::without(path, folder)))
}
