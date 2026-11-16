//! What a window says about itself (its AppUserModelID, its relaunch display name) is its owner's
//! to choose, so it is checked here before the shell is asked to act on it. No Windows calls, so
//! the tests run on every platform.

const NAME_LIMIT: usize = 128;
/// `MAX_PATH`, which a path with its resource id, or the longest AppUserModelID, fits in.
const CLAIM_LIMIT: usize = 260;

/// A display name a window gave, kept short and printable.
pub fn display_name(text: &str) -> Option<String> {
    let name: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_LIMIT)
        .collect();
    let name = name.trim_end();
    (!name.is_empty()).then(|| name.to_string())
}

/// Whether `SHLoadIndirectString` may be given this `@...` string. It opens the file the string
/// names, wherever that is, so only two forms pass: an installed package's own resource
/// (`@{PackageFullName?ms-resource:...}`, never a `.pri` path), and `@<file>,-<id>` for a file
/// under one of `roots`, the Windows and Program Files folders only an administrator can write.
/// A network or device path, or one that climbs out with `..`, never does.
pub fn is_safe_indirect(text: &str, roots: &[String]) -> bool {
    if text.len() > CLAIM_LIMIT || text.chars().any(char::is_control) {
        return false;
    }
    let Some(body) = text.strip_prefix('@') else {
        return false;
    };
    if let Some(reference) = body.strip_prefix('{') {
        return is_package_resource(reference);
    }
    body.rsplit_once(',').is_some_and(|(file, resource)| {
        let id = resource.strip_prefix('-').unwrap_or(resource);
        (1..=10).contains(&id.len())
            && id.bytes().all(|digit| digit.is_ascii_digit())
            && is_under_a_root(file, roots)
    })
}

/// The rest of `@{PackageFullName?ms-resource:...}`. A package full name
/// (`Name_1.2.3.4_x64__publisherid`) has no separator or colon, so this is never a file path.
fn is_package_resource(reference: &str) -> bool {
    reference
        .strip_suffix('}')
        .and_then(|inner| inner.split_once('?'))
        .is_some_and(|(package, resource)| {
            !package.is_empty()
                && package
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-~".contains(c))
                && resource.to_ascii_lowercase().starts_with("ms-resource:")
                && !resource.contains(['{', '}'])
        })
}

/// The variables Windows itself writes such paths with; the shell expands them.
const ROOT_VARIABLES: [&str; 5] = [
    "%systemroot%",
    "%windir%",
    "%programfiles%",
    "%programfiles(x86)%",
    "%programw6432%",
];

fn is_under_a_root(file: &str, roots: &[String]) -> bool {
    let file = file.to_ascii_lowercase();
    let inside = |rest: &str| {
        rest.strip_prefix('\\')
            .is_some_and(|rest| rest.split('\\').all(is_plain_component))
    };
    ROOT_VARIABLES
        .iter()
        .any(|variable| file.strip_prefix(variable).is_some_and(inside))
        || roots.iter().any(|root| {
            let root = root.trim_end_matches('\\').to_ascii_lowercase();
            is_drive_path(&root) && file.strip_prefix(&root).is_some_and(inside)
        })
}

/// `c:\...`, and so not a network (`\\host`), device (`\\.\`, `\\?\`) or relative path.
fn is_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":\\"
}

/// One folder or file name that stays where it is: not `.` or `..` (Windows also drops trailing
/// dots and spaces, which would make one), no stream (`:`), variable (`%`) or wildcard.
fn is_plain_component(component: &str) -> bool {
    !component.is_empty()
        && !component.ends_with(['.', ' '])
        && !component.contains(['/', ':', '%', '*', '?', '"', '<', '>', '|'])
}

/// Whether an AppUserModelID may be looked up as `shell:AppsFolder\<id>`: a plain id
/// (`Company.Product`, `Family_publisherid!App`), or the form Windows derives from an executable
/// in a known folder, `{known folder GUID}\relative\path.exe`. Anything else with a separator or
/// a colon could name something outside the apps folder.
pub fn is_shell_app_id(app_id: &str) -> bool {
    if app_id.is_empty() || app_id.len() > CLAIM_LIMIT || app_id.chars().any(char::is_control) {
        return false;
    }
    match app_id.split_once('\\') {
        None => !app_id.contains(['/', ':']),
        Some((folder, path)) => is_guid(folder) && path.split('\\').all(is_plain_component),
    }
}

/// `{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}`.
fn is_guid(text: &str) -> bool {
    let groups = text
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .map(|inner| inner.split('-').collect::<Vec<_>>());
    groups.is_some_and(|groups| {
        groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
            && groups
                .iter()
                .all(|group| group.bytes().all(|digit| digit.is_ascii_hexdigit()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<String> {
        [
            r"C:\WINDOWS",
            r"C:\Program Files",
            r"C:\Program Files (x86)\",
            "",
            r"\\server\share",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn indirect_strings_load_only_from_packages_and_system_folders() {
        for text in [
            r"@%SystemRoot%\system32\shell32.dll,-21770",
            r"@%windir%\System32\mmc.exe,-100",
            r"@%ProgramFiles%\Windows Defender\EppManifest.dll,-1000",
            r"@%ProgramFiles(x86)%\Vendor\app.exe,3",
            r"@C:\Windows\System32\wsecedit.dll,-718",
            r"@c:\program files\Vendor App\res.dll,-5",
            r"@C:\Program Files (x86)\Vendor\res.dll,-5",
            "@{Microsoft.WindowsCalculator_11.2405.2.0_x64__8wekyb3d8bbwe?ms-resource://Microsoft.WindowsCalculator/Resources/AppStoreName}",
            "@{Microsoft.Windows.Photos_2024.1.0.0_neutral_~_8wekyb3d8bbwe?ms-resource:AppName}",
        ] {
            assert!(is_safe_indirect(text, &roots()), "{text}");
        }
        for text in [
            // Anywhere a process without rights could have put a file, or another machine.
            r"@C:\temp\probe.dll,-101",
            r"@C:\Users\ada\AppData\Local\Temp\x.dll,-1",
            r"@\\attacker\share\x.dll,-101",
            r"@\\?\C:\Windows\System32\shell32.dll,-1",
            r"@\\.\pipe\x,-1",
            r"@%TEMP%\x.dll,-1",
            r"@%USERPROFILE%\x.dll,-1",
            r"@x.dll,-1",
            r"@D:\Windows\System32\shell32.dll,-1",
            // Out of a trusted folder again.
            r"@%SystemRoot%\..\temp\x.dll,-1",
            r"@C:\Windows\System32\..\..\temp\x.dll,-1",
            r"@C:\Windows\.. \temp\x.dll,-1",
            r"@C:\Windows\System32\.\x.dll,-1",
            r"@C:\Windows/../temp/x.dll,-1",
            r"@C:\WindowsEvil\x.dll,-1",
            r"@C:\Program Files (x86)Evil\x.dll,-1",
            r"@%SystemRoot%\%TEMP%\x.dll,-1",
            r"@%SystemRoot%\x.dll:stream,-1",
            r"@%SystemRoot%\\server\share\x.dll,-1",
            r"@%SystemRoot%,-1",
            // No resource, or not a number.
            r"@%SystemRoot%\system32\shell32.dll",
            r"@%SystemRoot%\system32\shell32.dll,-",
            r"@%SystemRoot%\system32\shell32.dll,-12a",
            r"@%SystemRoot%\system32\shell32.dll,-1;v2",
            // A resource file by path, where a package belongs.
            r"@{C:\temp\resources.pri?ms-resource://x/y}",
            r"@{\\attacker\share\resources.pri?ms-resource://x/y}",
            "@{Some.Package_1.0.0.0_x64__abc?file://x}",
            "@{Some.Package_1.0.0.0_x64__abc}",
            "@{?ms-resource:x}",
            "@{Some.Package_1.0.0.0_x64__abc?ms-resource:x",
            "@",
            "Calculator",
            "@%SystemRoot%\\system32\\shell32.dll,-1\u{0}",
        ] {
            assert!(!is_safe_indirect(text, &roots()), "{text}");
        }
        let long = format!(r"@%SystemRoot%\{},-1", "a".repeat(300));
        assert!(!is_safe_indirect(&long, &roots()));
        assert!(!is_safe_indirect(r"@C:\Windows\x.dll,-1", &[]));
    }

    #[test]
    fn only_ids_that_stay_in_the_apps_folder_are_looked_up() {
        for app_id in [
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
            "Microsoft.Office.WINWORD.EXE.15",
            "Chrome._crx_abcdefghijklmnopabcdefghijklmnop",
            "com.squirrel.slack.slack",
            "My Company.My App",
            r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe",
            r"{6D809377-6AF0-444B-8957-A3773F02200E}\Git\git-bash.exe",
        ] {
            assert!(is_shell_app_id(app_id), "{app_id}");
        }
        for app_id in [
            "",
            r"C:\Users\ada\app.exe",
            r"\\attacker\share\app.exe",
            r"..\..\Desktop",
            r"Some.App\..\other",
            r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\..\x.exe",
            r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\server\x.exe",
            r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\",
            r"{not-a-guid}\x.exe",
            "shell:Desktop",
            "a/b",
            "a\u{0}b",
        ] {
            assert!(!is_shell_app_id(app_id), "{app_id}");
        }
        assert!(!is_shell_app_id(&"a".repeat(300)));
    }

    #[test]
    fn display_names_are_short_and_printable() {
        assert_eq!(display_name("  Mail  ").as_deref(), Some("Mail"));
        assert_eq!(
            display_name("Ma\u{1b}[2Jil\u{7}").as_deref(),
            Some("Ma[2Jil")
        );
        assert_eq!(display_name(" \u{1b} "), None);
        let long = display_name(&"é".repeat(500)).unwrap();
        assert_eq!(long.chars().count(), 128);
    }
}
