//! The reminder chime, one system sound per platform.

#[cfg_attr(target_os = "macos", path = "sound/macos.rs")]
#[cfg_attr(windows, path = "sound/windows.rs")]
#[cfg_attr(not(any(target_os = "macos", windows)), path = "sound/unsupported.rs")]
mod platform;

pub fn play_chime() {
    platform::play_chime();
}
