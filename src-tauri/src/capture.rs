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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSample {
    pub app: String,
    pub title: String,
    pub bundle_id: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
    /// The app holds a power assertion keeping the display awake, which is
    /// how a browser playing video or a call app tells macOS someone is
    /// watching. Watching gives no keyboard or mouse input, so this is the
    /// only sign the user is still there. Always false on Windows, which
    /// does not attribute display requests to a process without admin
    /// rights.
    pub keeps_display_awake: bool,
}

/// The host a browser URL points at, without a leading `www.`.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn domain_of(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .host_str()
        .map(|host| host.trim_start_matches("www.").to_string())
}
