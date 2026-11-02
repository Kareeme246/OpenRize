//! Where the installed desktop app keeps its files, for clients that run
//! without Tauri. These mirror Tauri 2's `PathResolver`, which resolves
//! `app_local_data_dir` to `data_local_dir()/<identifier>` and `app_cache_dir`
//! to `cache_dir()/<identifier>` with the same `dirs` crate:
//!
//! | | data | cache |
//! |---|---|---|
//! | macOS | `~/Library/Application Support` | `~/Library/Caches` |
//! | Windows | `%LOCALAPPDATA%` | `%LOCALAPPDATA%` |
//! | Linux | `$XDG_DATA_HOME` or `~/.local/share` | `$XDG_CACHE_HOME` or `~/.cache` |
//!
//! Everything stays on the machine, so Windows data is in Local AppData, not
//! the Roaming folder (`dirs::data_dir`) that profiles and folder redirection
//! sync elsewhere and that earlier builds used. That is the only platform where
//! `data_local_dir` and `data_dir` differ; on the others Tauri's
//! `app_local_data_dir` and `app_data_dir` are the same folder, so nothing
//! changed there. See `data_location` for how existing Windows data is moved.
//!
//! Dev and screenshot instances use their own identifier, so clients reach
//! those stores only through an explicit directory.
use std::path::PathBuf;

use crate::APP_IDENTIFIER;

/// The activity database inside the app data directory.
pub const DATABASE_FILE: &str = "activity.db";

/// Held exclusively by whichever process owns the stores: the running app for
/// its whole life, or `rize` while it edits them with the app closed.
pub const LOCK_FILE: &str = "app.lock";

/// Local accounts, beside the data of No account (the data directory itself).
/// The registry lists them; see `accounts.rs`.
pub const ACCOUNTS_FILE: &str = "accounts.json";

/// The restart handoff a switch leaves for the next launch; deleted once read.
pub const SWITCH_FILE: &str = "switch.json";

/// Where Windows keeps settings (`settings.rs`), inside the data directory. It
/// is not data: `data_location` never counts, moves or removes it.
pub const CONFIG_DIR: &str = "config";

/// One folder per account, named by its id.
pub const ACCOUNTS_DIR: &str = "accounts";

/// The installed app's data directory, or `None` when the OS has no home.
/// Never creates or moves anything: until the app has moved an older Windows
/// install's data out of the roaming folder, that folder is the answer, so
/// `rize` agrees with what the app will open (`data_location::choose`).
pub fn app_data_dir() -> Option<PathBuf> {
    let local = local_data_dir()?;
    Some(match legacy_data_dir() {
        Some(legacy) => crate::data_location::choose(&legacy, &local),
        None => local,
    })
}

/// Where the data lives from now on: Tauri's `app_local_data_dir`.
pub fn local_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(APP_IDENTIFIER))
}

/// The roaming folder earlier Windows builds kept the data in (Tauri's
/// `app_data_dir`). `None` elsewhere, where there was nothing to move.
pub fn legacy_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        dirs::data_dir().map(|dir| dir.join(APP_IDENTIFIER))
    } else {
        None
    }
}

/// The installed app's cache directory, or `None` when the OS has no home.
pub fn app_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join(APP_IDENTIFIER))
}
