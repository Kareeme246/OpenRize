//! What one sample of the foreground window records, and where a sample is judged private (a
//! private, incognito or InPrivate browser window), so tests on any OS can reach it.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSample {
    pub app: String,
    pub title: String,
    pub bundle_id: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
    /// The window is a private browsing window. The time still counts; its title and address
    /// are erased once the entry is classified (see `activity::redact`).
    pub private: bool,
    /// The app holds a power assertion keeping the display awake (video, calls): the only sign of
    /// presence without input. Windows infers it from audio, recording or fullscreen, as it cannot
    /// attribute display requests to a process without admin rights.
    pub keeps_display_awake: bool,
}

impl WindowSample {
    /// The sample as the store keeps it: a private window's URL loses its query and fragment,
    /// which hold what the person searched for or opened.
    pub fn for_storage(mut self) -> Self {
        if self.private {
            self.url = self.url.as_deref().map(strip_url_detail);
        }
        self
    }
}

/// The title a private segment carries once its detail has been erased.
pub const PRIVATE_TITLE: &str = "Private window";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Firefox,
    Chromium,
}

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

/// What Firefox-family browsers append to a private window's title, in every locale Zen ships
/// (from Zen 1.x `browser.ftl`). A private window always ends with it; a normal one with the brand.
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

/// Locale-dependent for everything except Edge, whose `[InPrivate]` is the same in every language.
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

/// Edge puts `[InPrivate]` between page and product name; Chrome's accessible name ends in
/// `(Incognito)`; Brave, Vivaldi and Opera call theirs private.
pub fn chromium_name_marks_private(name: &str) -> bool {
    let name = name.trim();
    name.contains("[InPrivate]")
        || name.ends_with("(Incognito)")
        || name.ends_with(" - Incognito")
        || name.ends_with("(Private)")
        || name.ends_with(" - Private")
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrowserReply {
    pub url: Option<String>,
    pub incognito: bool,
}

/// Line 1 is the page URL, line 2 the window mode ("normal" or "incognito"); Safari has no mode.
pub fn parse_browser_reply(stdout: &str) -> BrowserReply {
    let mut lines = stdout.lines().map(str::trim);
    let url = lines
        .next()
        .filter(|line| line.starts_with("http://") || line.starts_with("https://"))
        .map(str::to_string);
    let incognito = lines.next().is_some_and(|mode| mode == "incognito");
    BrowserReply { url, incognito }
}

/// A URL without its query and fragment, which hold searches, tokens and document ids.
pub fn strip_url_detail(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    url[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_inprivate_is_marked_in_every_locale() {
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
