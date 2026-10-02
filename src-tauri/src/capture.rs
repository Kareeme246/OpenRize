//! What the user is looking at: the frontmost app, its window title, and a
//! browser's URL, plus the OS hooks the sampler needs around that.
//!
//! Each platform lives in its own file and provides the same items:
//! `read_active_window`, `user_is_away`, `AppNapAssertion`, and
//! `register_sleep_listeners`. macOS and Windows are implemented (Windows is
//! experimental); anything else compiles `unsupported.rs`, which captures
//! nothing.

#[cfg_attr(target_os = "macos", path = "capture/macos.rs")]
#[cfg_attr(windows, path = "capture/windows.rs")]
#[cfg_attr(
    not(any(target_os = "macos", windows)),
    path = "capture/unsupported.rs"
)]
mod platform;

pub use platform::{read_active_window, register_sleep_listeners, user_is_away, AppNapAssertion};

pub use openrize_core::capture::WindowSample;

/// The host a browser URL points at, without a leading `www.`.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn domain_of(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .host_str()
        .map(|host| host.trim_start_matches("www.").to_string())
}
