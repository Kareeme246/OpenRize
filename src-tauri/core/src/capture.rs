//! What one sample of the foreground window records. The app owns the OS
//! capture; the store only needs the shape.

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
    /// only sign the user is still there. Windows does not attribute display
    /// requests to a process without admin rights, so it infers this: the
    /// system holds a display request while the foreground app (or a process
    /// of the same app) is playing audio or recording, or is fullscreen.
    pub keeps_display_awake: bool,
}
