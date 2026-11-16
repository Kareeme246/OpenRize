//! macOS: `~/.local/bin/rize` is a symlink to the `rize` bundled next to the app
//! executable. An installed app checks it at every launch, so a removed link
//! heals itself; a file or a link that is not the app's own is never replaced.

use std::path::Path;

pub fn install_bundled_cli() {
    // Dev and drive builds must never write the developer's real link.
    if cfg!(debug_assertions) {
        return;
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let bundled = std::env::current_exe()?.with_file_name("rize");
        if !is_installed(&bundled) || !bundled.is_file() {
            return Ok(());
        }
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        let directory = std::path::PathBuf::from(home).join(".local/bin");
        let destination = directory.join("rize");
        match std::fs::symlink_metadata(&destination) {
            Ok(existing) if existing.is_symlink() => {
                let target = std::fs::read_link(&destination)?;
                if target == bundled {
                    return Ok(());
                }
                if !is_app_link(&target) {
                    return Err(format!("it links to {}, not to OpenRize", target.display()).into());
                }
            }
            Ok(_) => return Err("it is not a link this app made".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        std::fs::create_dir_all(&directory)?;
        let temporary = directory.join(format!(".rize-{}", std::process::id()));
        let _ = std::fs::remove_file(&temporary);
        std::os::unix::fs::symlink(&bundled, &temporary)?;
        std::fs::rename(&temporary, &destination)?;
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("left ~/.local/bin/rize as it is: {error}");
    }
}

/// A build folder, a mounted disk image and an App Translocation copy are not
/// installs: a link into one would dangle, or name whatever is mounted there next.
fn is_installed(exe: &Path) -> bool {
    !exe.starts_with("/Volumes")
        && !exe.components().any(|part| {
            let part = part.as_os_str();
            part.eq_ignore_ascii_case("target") || part == "AppTranslocation"
        })
}

/// Whether a link's target is the `rize` inside an OpenRize app bundle, which
/// only this app (or `rize install-path` from one) puts there.
fn is_app_link(target: &Path) -> bool {
    target.ends_with("Contents/MacOS/rize")
        && target
            .ancestors()
            .nth(3)
            .and_then(Path::file_name)
            .is_some_and(|bundle| bundle.eq_ignore_ascii_case("openrize.app"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_app_in_a_lasting_folder_is_an_install() {
        for exe in [
            "/Applications/openrize.app/Contents/MacOS/rize",
            "/Users/ada/Applications/openrize.app/Contents/MacOS/rize",
            "/opt/apps/openrize.app/Contents/MacOS/rize",
        ] {
            assert!(is_installed(Path::new(exe)), "{exe}");
        }
        for exe in [
            "/Users/ada/OpenRize/src-tauri/target/debug/rize",
            "/Users/ada/OpenRize/src-tauri/Target/release/bundle/macos/openrize.app/Contents/MacOS/rize",
            "/Volumes/openrize/openrize.app/Contents/MacOS/rize",
            "/private/var/folders/ab/cd/T/AppTranslocation/1234-5678/d/openrize.app/Contents/MacOS/rize",
        ] {
            assert!(!is_installed(Path::new(exe)), "{exe}");
        }
    }

    #[test]
    fn only_a_link_into_an_openrize_bundle_is_replaced() {
        for target in [
            "/Applications/openrize.app/Contents/MacOS/rize",
            "/Users/ada/Applications/OpenRize.app/Contents/MacOS/rize",
            "/Volumes/openrize/openrize.app/Contents/MacOS/rize",
        ] {
            assert!(is_app_link(Path::new(target)), "{target}");
        }
        for target in [
            "/Users/ada/tools/rize",
            "/Applications/Rize.app/Contents/MacOS/rize",
            "/Applications/openrize.app/Contents/MacOS/openrize",
            "/Applications/openrize.app/Contents/Resources/Contents/MacOS/rize",
            "/Users/ada/OpenRize/src-tauri/target/debug/rize",
            "Contents/MacOS/rize",
        ] {
            assert!(!is_app_link(Path::new(target)), "{target}");
        }
    }
}
