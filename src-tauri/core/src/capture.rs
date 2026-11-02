//! What one sample of the foreground window records. The app owns the OS
//! capture; the store only needs the shape.
//!
//! This is also where a sample is judged *private*: a private, incognito or
//! InPrivate browser window. The OS code only has to hand over the facts
//! (the window title, the executable or bundle id, and on macOS the answer to
//! "what mode is this window in"); what those facts mean lives here, where a
//! test on any OS can reach it.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSample {
    pub app: String,
    pub title: String,
    pub bundle_id: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
    /// The window is a private browsing window. The time still counts and
    /// can become a time entry; what the window showed (title, address) is
    /// kept only until that entry has been classified, then erased (see
    /// `activity::redact`).
    pub private: bool,
    /// The app holds a power assertion keeping the display awake, which is
    /// how a browser playing video or a call app tells macOS someone is
    /// watching. Watching gives no keyboard or mouse input, so this is the
    /// only sign the user is still there. Windows does not attribute display
    /// requests to a process without admin rights, so it infers this: the
    /// system holds a display request while the foreground app (or a process
    /// of the same app) is playing audio or recording, or is fullscreen.
    pub keeps_display_awake: bool,
}

impl WindowSample {
    /// The sample as the store keeps it. A private window's address loses its
    /// query and fragment at once: they hold what the person searched for or
    /// opened, and a rule can match the host and path without them.
    pub fn for_storage(mut self) -> Self {
        if self.private {
            self.url = self.url.as_deref().map(strip_url_detail);
        }
        self
    }
}

/// The title a private segment carries once its detail has been erased.
pub const PRIVATE_TITLE: &str = "Private window";

/// The browser engines that mark a private window in different ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Firefox,
    Chromium,
}

/// Which engine an executable (Windows, `firefox.exe`) or bundle id (macOS)
/// runs on, or `None` for an app that is not a known browser.
fn engine_of(app_id: &str) -> Option<Engine> {
    let id = app_id.to_ascii_lowercase();
    const FIREFOX_EXE: [&str; 5] = [
        "firefox.exe",
        "zen.exe",
        "librewolf.exe",
        "floorp.exe",
        "waterfox.exe",
    ];
    const FIREFOX_BUNDLES: [&str; 6] = [
        "org.mozilla.",
        "app.zen-browser.",
        "io.gitlab.librewolf-community.",
        "one.ablaze.floorp",
        "net.waterfox.",
        "net.mullvad.",
    ];
    const CHROMIUM_EXE: [&str; 9] = [
        "chrome.exe",
        "msedge.exe",
        "brave.exe",
        "vivaldi.exe",
        "opera.exe",
        "chromium.exe",
        "thorium.exe",
        "arc.exe",
        "comet.exe",
    ];
    const CHROMIUM_BUNDLES: [&str; 8] = [
        "com.google.chrome",
        "com.brave.browser",
        "com.microsoft.edgemac",
        "company.thebrowser.",
        "com.vivaldi.",
        "com.operasoftware.",
        "org.chromium.",
        "ai.perplexity.comet",
    ];
    if FIREFOX_EXE.contains(&id.as_str()) || FIREFOX_BUNDLES.iter().any(|p| id.starts_with(p)) {
        Some(Engine::Firefox)
    } else if CHROMIUM_EXE.contains(&id.as_str())
        || CHROMIUM_BUNDLES.iter().any(|p| id.starts_with(p))
    {
        Some(Engine::Chromium)
    } else {
        None
    }
}

/// What Firefox (and Zen, LibreWolf, Floorp, Waterfox) append to the title of
/// a private window, in every language Zen ships. Read from the
/// `browser-main-private-suffix-for-content` string of Zen 1.x's
/// `browser.ftl`; Firefox uses the same strings. The window title is
/// `<page> — <brand> <suffix>` on Windows and `<page> — <suffix>` on macOS,
/// and an empty page gives `<brand> <suffix>`, so a private window always ends
/// with the suffix, while a normal one ends with the brand.
const FIREFOX_PRIVATE_SUFFIXES: [&str; 37] = [
    "التصفح الخاص",
    "Поверително сърфиране",
    "Privatno pretraživanje",
    "Navegació privada",
    "Anonymní prohlížení",
    "Pori Preifat",
    "Privat browsing",
    "Privater Modus",
    "Ιδιωτική περιήγηση",
    "Private Browsing",
    "Navegación privada",
    "مرور ناشناس",
    "Yksityinen selaus",
    "Navigation privée",
    "גלישה פרטית",
    "Privát böngészés",
    "Penjelajahan Pribadi",
    "Huliðsvafur",
    "Navigazione anonima",
    "プライベートブラウジング",
    "사생활 보호 모드",
    "Privat nettlesing",
    "Privénavigatie",
    "Tryb prywatny",
    "Navegação privativa",
    "Navegação privada",
    "Navigare privată",
    "Приватный просмотр",
    "Súkromné prehliadanie",
    "Privat surfning",
    "การท่องเว็บแบบส่วนตัว",
    "Gizli Gezinti",
    "Приватний перегляд",
    "Duyệt web riêng tư",
    "隐私浏览",
    "隱私瀏覽模式",
    "隱私瀏覽",
];

/// Whether a window title says the window is private, for the browser `app_id`
/// names. Locale-dependent for everything except Edge, whose `[InPrivate]` is
/// the same in every language it ships (checked in its locale files).
pub fn title_marks_private(app_id: &str, title: &str) -> bool {
    let title = title.trim();
    match engine_of(app_id) {
        Some(Engine::Firefox) => FIREFOX_PRIVATE_SUFFIXES
            .iter()
            .any(|suffix| title.ends_with(suffix)),
        Some(Engine::Chromium) => chromium_name_marks_private(title),
        None => false,
    }
}

/// Whether a Chromium window's name (its title, or the name the accessibility
/// tree gives the window) marks it private. Edge puts `[InPrivate]` between the
/// page and the product name; Chrome's accessible window name ends in
/// `(Incognito)`; Brave, Vivaldi and Opera call theirs private.
pub fn chromium_name_marks_private(name: &str) -> bool {
    let name = name.trim();
    name.contains("[InPrivate]")
        || name.ends_with("(Incognito)")
        || name.ends_with(" - Incognito")
        || name.ends_with("(Private)")
        || name.ends_with(" - Private")
}

/// What an `osascript` query of a browser window answered: its URL, and
/// whether the window is in incognito mode.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrowserReply {
    pub url: Option<String>,
    pub incognito: bool,
}

/// Reads the reply of the macOS browser query: the page URL on the first line
/// and the window `mode` ("normal" or "incognito") on the second. Safari has
/// no mode, so its reply is the URL alone.
pub fn parse_browser_reply(stdout: &str) -> BrowserReply {
    let mut lines = stdout.lines().map(str::trim);
    let url = lines
        .next()
        .filter(|line| line.starts_with("http://") || line.starts_with("https://"))
        .map(str::to_string);
    let incognito = lines.next().is_some_and(|mode| mode == "incognito");
    BrowserReply { url, incognito }
}

/// A URL without its query and fragment. Searches, tokens and document ids
/// live there, and nothing needs them to classify a private window.
pub fn strip_url_detail(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    url[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_inprivate_is_marked_in_every_locale() {
        // Read off a live InPrivate window.
        let title = "about:blank - [InPrivate] - Microsoft\u{200b} Edge";
        assert!(title_marks_private("msedge.exe", title));
        assert!(title_marks_private("com.microsoft.edgemac", title));
        assert!(!title_marks_private(
            "msedge.exe",
            "about:blank - Profile 1 - Microsoft\u{200b} Edge"
        ));
    }

    #[test]
    fn firefox_family_private_windows_end_with_the_private_suffix() {
        // Read off a live Firefox private window, and Zen's own strings.
        assert!(title_marks_private(
            "firefox.exe",
            "Mozilla Firefox Private Browsing"
        ));
        assert!(title_marks_private(
            "zen.exe",
            "Some page \u{2014} Zen Browser Private Browsing"
        ));
        assert!(title_marks_private(
            "app.zen-browser.zen",
            "Some page \u{2014} Private Browsing"
        ));
        assert!(title_marks_private(
            "zen.exe",
            "Seite \u{2014} Zen Browser Privater Modus"
        ));
        assert!(title_marks_private(
            "firefox.exe",
            "ページ \u{2014} Mozilla Firefox プライベートブラウジング"
        ));
        // A normal window ends with the brand, even when the page talks about
        // private browsing.
        assert!(!title_marks_private(
            "zen.exe",
            "*BEST* Method \u{2014} Zen Browser"
        ));
        assert!(!title_marks_private(
            "zen.exe",
            "How to use Private Browsing \u{2014} Zen Browser"
        ));
    }

    #[test]
    fn chromium_incognito_is_marked_by_the_window_name() {
        assert!(title_marks_private(
            "chrome.exe",
            "New Tab - Google Chrome (Incognito)"
        ));
        assert!(title_marks_private("brave.exe", "Page - Brave (Private)"));
        assert!(!title_marks_private(
            "chrome.exe",
            "New Tab - Google Chrome"
        ));
        assert!(chromium_name_marks_private(
            "Inbox - Google Chrome - Incognito"
        ));
        assert!(!chromium_name_marks_private("Inbox - Google Chrome"));
    }

    #[test]
    fn an_app_that_is_not_a_browser_is_never_private() {
        assert!(!title_marks_private(
            "notepad.exe",
            "notes - [InPrivate] - Private Browsing"
        ));
        assert!(!title_marks_private("", "Private Browsing"));
    }

    #[test]
    fn the_macos_reply_carries_the_url_and_the_window_mode() {
        let reply = parse_browser_reply("https://example.com/a?q=1\nincognito\n");
        assert_eq!(reply.url.as_deref(), Some("https://example.com/a?q=1"));
        assert!(reply.incognito);

        let reply = parse_browser_reply("https://example.com/\nnormal");
        assert!(!reply.incognito);
        assert!(reply.url.is_some());

        // Safari: the URL alone. A page that is not the web has no URL.
        let reply = parse_browser_reply("https://example.com/");
        assert!(!reply.incognito);
        assert_eq!(parse_browser_reply("chrome://newtab/\nincognito").url, None);
        assert!(parse_browser_reply("chrome://newtab/\nincognito").incognito);
        assert_eq!(parse_browser_reply(""), BrowserReply::default());
    }

    #[test]
    fn a_stripped_url_keeps_the_host_and_path_only() {
        assert_eq!(
            strip_url_detail("https://example.com/a/b?q=secret#frag"),
            "https://example.com/a/b"
        );
        assert_eq!(
            strip_url_detail("https://example.com/a#x"),
            "https://example.com/a"
        );
        assert_eq!(
            strip_url_detail("https://example.com/"),
            "https://example.com/"
        );
    }
}
