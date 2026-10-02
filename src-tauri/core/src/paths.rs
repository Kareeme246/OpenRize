//! Where the installed desktop app keeps its files, for clients that run
//! without Tauri. These mirror Tauri 2's `PathResolver`, which resolves
//! `app_data_dir` to `data_dir()/<identifier>` and `app_cache_dir` to
//! `cache_dir()/<identifier>` with the same `dirs` crate:
//!
//! | | data | cache |
//! |---|---|---|
//! | macOS | `~/Library/Application Support` | `~/Library/Caches` |
//! | Windows | `%APPDATA%` (Roaming) | `%LOCALAPPDATA%` |
//! | Linux | `$XDG_DATA_HOME` or `~/.local/share` | `$XDG_CACHE_HOME` or `~/.cache` |
//!
//! Dev and screenshot instances use their own identifier, so clients reach
//! those stores only through an explicit directory.
use std::path::PathBuf;

use crate::APP_IDENTIFIER;

/// The activity database inside the app data directory.
pub const DATABASE_FILE: &str = "activity.db";

/// The installed app's data directory, or `None` when the OS has no home.
pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(APP_IDENTIFIER))
}

/// The installed app's cache directory, or `None` when the OS has no home.
pub fn app_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join(APP_IDENTIFIER))
}
