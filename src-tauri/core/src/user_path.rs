//! The Windows user PATH (`HKCU\Environment`), edited by `rize install-path` and `uninstall-path`
//! and checked by the app at every launch.

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

    pub fn with(path: &str, folder: &str) -> String {
        match path.trim_end_matches(';') {
            _ if contains(path, folder) => path.to_owned(),
            "" => folder.to_owned(),
            rest => format!("{rest};{folder}"),
        }
    }

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

/// The text of a stored `REG_SZ` or `REG_EXPAND_SZ` value, only when writing it back would lose
/// nothing: an unpaired surrogate or an embedded NUL does not survive the round trip.
fn stored_text(wide: &[u16]) -> Option<String> {
    let end = wide
        .iter()
        .rposition(|unit| *unit != 0)
        .map_or(0, |last| last + 1);
    String::from_utf16(&wide[..end])
        .ok()
        .filter(|text| !text.contains('\0'))
}

#[cfg(windows)]
fn edit(change: impl FnOnce(&str) -> Option<String>) -> Result<bool, String> {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    use windows_registry::{Type, CURRENT_USER};

    /// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`: the value does not exist.
    const NOT_FOUND: i32 = 0x8007_0002_u32 as i32;

    let key = CURRENT_USER
        .create("Environment")
        .map_err(|e| e.to_string())?;
    // Only a value that is not there is an empty PATH. One that cannot be read exactly is left
    // alone: writing over it would replace the user's whole PATH with this one folder.
    let (ty, current) = match key.get_value("Path") {
        Ok(value) => match (value.ty(), stored_text(value.as_wide())) {
            (ty @ (Type::String | Type::ExpandString), Some(text)) => (ty, text),
            (ty, _) => {
                return Err(format!(
                    "the user PATH ({ty:?}) is not text rize can rewrite; it was left unchanged"
                ))
            }
        },
        // REG_EXPAND_SZ, as Windows writes it, so `%USERPROFILE%`-style
        // entries keep expanding.
        Err(error) if error.code().0 == NOT_FOUND => (Type::ExpandString, String::new()),
        Err(error) => return Err(format!("could not read the user PATH: {error}")),
    };
    let Some(next) = change(&current) else {
        return Ok(false);
    };
    match ty {
        Type::String => key.set_string("Path", &next),
        _ => key.set_expand_string("Path", &next),
    }
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

#[cfg(windows)]
pub fn ensure_folder(folder: &str) -> Result<bool, String> {
    edit(|path| (!path_list::contains(path, folder)).then(|| path_list::with(path, folder)))
}

#[cfg(windows)]
pub fn remove_folder(folder: &str) -> Result<bool, String> {
    edit(|path| path_list::contains(path, folder).then(|| path_list::without(path, folder)))
}

#[cfg(test)]
mod tests {
    use super::stored_text;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn a_stored_path_is_read_exactly_or_not_at_all() {
        let path = r"C:\bin;%USERPROFILE%\Tools é";
        assert_eq!(stored_text(&wide(path)).as_deref(), Some(path));
        // With its terminator, or the two some writers leave.
        assert_eq!(
            stored_text(&wide(&format!("{path}\0"))).as_deref(),
            Some(path)
        );
        assert_eq!(
            stored_text(&wide(&format!("{path}\0\0"))).as_deref(),
            Some(path)
        );
        assert_eq!(stored_text(&[]).as_deref(), Some(""));
        assert_eq!(stored_text(&[0]).as_deref(), Some(""));

        // An unpaired surrogate, and text after a NUL, cannot be written back as they were.
        let mut broken = wide(path);
        broken.push(0xD800);
        assert_eq!(stored_text(&broken), None);
        assert_eq!(stored_text(&wide("C:\\bin\0D:\\tools")), None);
    }
}
