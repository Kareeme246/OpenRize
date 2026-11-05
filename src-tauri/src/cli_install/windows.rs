//! Windows: the installer puts rize's folder on the user PATH once (its NSIS
//! POSTINSTALL hook runs `rize install-path`). This checks that entry again at
//! every launch, so one that was removed comes back, as the macOS link does.

use std::path::Path;

pub fn install_bundled_cli() {
    // Dev and drive builds must never write the developer's real PATH: skip
    // debug builds and release builds run from a Cargo `target` folder.
    if cfg!(debug_assertions) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if !is_installed(&exe) || !exe.with_file_name("rize.exe").is_file() {
        return;
    }
    // The registry work and the settings broadcast can wait on other programs,
    // so they run beside startup rather than in it.
    let spawned = std::thread::Builder::new()
        .name("rize-path".into())
        .spawn(move || {
            let Some(folder) = exe.parent() else {
                return;
            };
            if let Err(error) =
                openrize_core::user_path::ensure_folder(&folder.display().to_string())
            {
                eprintln!("could not add rize to PATH: {error}");
            }
        });
    if let Err(error) = spawned {
        eprintln!("could not start the rize PATH check: {error}");
    }
}

fn is_installed(exe: &Path) -> bool {
    !exe.components()
        .any(|part| part.as_os_str().eq_ignore_ascii_case("target"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_folders_are_not_installs() {
        assert!(!is_installed(Path::new(
            r"C:\src\OpenRize\src-tauri\target\debug\openrize.exe"
        )));
        assert!(!is_installed(Path::new(
            r"C:\src\OpenRize\src-tauri\Target\release\openrize.exe"
        )));
        assert!(is_installed(Path::new(
            r"C:\Users\Ada\AppData\Local\OpenRize\openrize.exe"
        )));
        assert!(is_installed(Path::new(
            r"C:\Program Files\OpenRize\openrize.exe"
        )));
    }
}
