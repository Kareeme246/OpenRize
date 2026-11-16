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
    /// The sample as the store keeps it: no URL keeps its query or fragment, which hold what the
    /// person searched for or opened and the tokens of sign-in and share links.
    pub fn for_storage(mut self) -> Self {
        self.url = self.url.as_deref().map(strip_url_detail);
        self
    }
}

pub const PRIVATE_TITLE: &str = "Private window";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Firefox,
    Chromium,
    Safari,
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
    // Exact, so a Safari web app ("com.apple.Safari.WebApp.<id>") stays an ordinary app.
    const SAFARI_BUNDLES: [&str; 2] = ["com.apple.safari", "com.apple.safaritechnologypreview"];
    if SAFARI_BUNDLES.contains(&id.as_str()) {
        Some(Engine::Safari)
    } else if FIREFOX_EXE.contains(&id.as_str())
        || FIREFOX_BUNDLES.iter().any(|p| id.starts_with(p))
    {
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

/// What Safari's accessibility title adds to a private window, in every locale Safari 27 ships:
/// the text after the window's own title in its `%@, Private Browsing` format
/// (`-[BrowserWindow accessibilityTitle]`, Safari.framework `Localizable.strings`). Thai is the one
/// locale that puts it before the title instead.
const SAFARI_PRIVATE_SUFFIXES: [&str; 36] = [
    "\u{60c} التصفح الخاص",
    ", navegació privada",
    ", anonymní procházení",
    ", privat browser",
    ", „Privates Surfen“",
    ", Ιδιωτική περιήγηση",
    ", Private Browsing",
    ", navegación privada",
    " con navegación privada",
    ", yksityinen selaus",
    ", navigation privée",
    ", בגלישה פרטית",
    ", गोपनीय ब्राउज़िंग",
    ", privatno pretraživanje",
    ", privát böngészés",
    ", Penelusuran Pribadi",
    ", navigazione privata",
    "、プライベートブラウズ",
    ", 개인정보 보호 브라우징",
    ", Pelayaran Peribadi",
    ", privémodus",
    ", Privat nettlesing",
    ", przeglądanie prywatne",
    ", Navegação Privada",
    ", navegação privada",
    ", navigare privată",
    ", частный доступ",
    ", anonymné prezeranie",
    ", zasebno brskanje",
    ", privat surfning",
    ", Özel Dolaşma",
    ", приватний перегляд",
    ", Duyệt riêng tư",
    "，无痕浏览",
    "，「私密瀏覽」",
    "，私密瀏覽",
];
const SAFARI_PRIVATE_PREFIX_TH: &str = "ท่องเว็บแบบส่วนตัวเรื่อง";

/// Locale-dependent for everything except Edge, whose `[InPrivate]` is the same in every language.
pub fn title_marks_private(app_id: &str, title: &str) -> bool {
    let title = title.trim();
    match engine_of(app_id) {
        Some(Engine::Firefox) => FIREFOX_PRIVATE_SUFFIXES
            .iter()
            .any(|suffix| title.ends_with(suffix)),
        Some(Engine::Chromium) => chromium_name_marks_private(title),
        Some(Engine::Safari) => {
            title.starts_with(SAFARI_PRIVATE_PREFIX_TH)
                || SAFARI_PRIVATE_SUFFIXES
                    .iter()
                    .any(|suffix| title.ends_with(suffix))
        }
        None => false,
    }
}

/// Whether a macOS window counts as private. A browser whose mode cannot be told is taken as
/// private, never as normal: a Chromium browser that did not say "normal" (not asked, no
/// Automation permission, hung, timed out; its title marker depends on the language), and a
/// Firefox-family or Safari window with no readable title. Those two say it in the title alone,
/// so an unanswered script costs them the URL, not the title.
pub fn window_is_private(app_id: &str, title: &str, answer: Option<Mode>) -> bool {
    let Some(engine) = engine_of(app_id) else {
        return false;
    };
    if answer == Some(Mode::Private) || title_marks_private(app_id, title) {
        return true;
    }
    match engine {
        Engine::Chromium => answer != Some(Mode::Normal),
        Engine::Firefox | Engine::Safari => title.trim().is_empty(),
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

/// What a browser said about its front window when asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    Normal,
    Private,
    /// It did not say: the script failed, was refused or timed out, or the answer made no sense.
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrowserReply {
    pub url: Option<String>,
    pub mode: Mode,
}

/// Line 1 is the page URL (empty when there is none), line 2 the window mode ("normal" or
/// "incognito"). Without a mode the reply is not trusted for a URL either.
pub fn parse_browser_reply(stdout: &str) -> BrowserReply {
    let mut lines = stdout.lines().map(str::trim);
    let url = lines
        .next()
        .filter(|line| line.starts_with("http://") || line.starts_with("https://"))
        .map(strip_url_detail);
    let mode = match lines.next() {
        Some("normal") => Mode::Normal,
        Some("incognito") => Mode::Private,
        _ => Mode::Unknown,
    };
    BrowserReply {
        url: url.filter(|_| mode != Mode::Unknown),
        mode,
    }
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
    fn the_macos_reply_carries_the_url_without_its_detail_and_the_window_mode() {
        let reply = parse_browser_reply("https://example.com/a?q=1#top\nincognito\n");
        assert_eq!(reply.url.as_deref(), Some("https://example.com/a"));
        assert_eq!(reply.mode, Mode::Private);

        let reply = parse_browser_reply("https://example.com/\nnormal");
        assert_eq!(reply.mode, Mode::Normal);
        assert_eq!(reply.url.as_deref(), Some("https://example.com/"));

        // A page that is not the web, or no page at all, has no URL.
        let reply = parse_browser_reply("chrome://newtab/\nincognito");
        assert_eq!((reply.url, reply.mode), (None, Mode::Private));
        let reply = parse_browser_reply("\nnormal");
        assert_eq!((reply.url, reply.mode), (None, Mode::Normal));
    }

    #[test]
    fn a_reply_without_a_mode_is_unknown_and_gives_no_url() {
        for stdout in [
            "",
            "https://example.com/a",
            "https://example.com/a\n",
            "https://example.com/a\nmissing value",
            "missing value, \n, normal",
        ] {
            assert_eq!(
                parse_browser_reply(stdout),
                BrowserReply::default(),
                "{stdout:?}"
            );
        }
        assert_eq!(BrowserReply::default().mode, Mode::Unknown);
    }

    #[test]
    fn a_safari_private_window_is_marked_by_its_accessibility_title() {
        for id in ["com.apple.Safari", "com.apple.SafariTechnologyPreview"] {
            assert!(title_marks_private(id, "Apple, Private Browsing"));
            assert!(title_marks_private(
                id,
                "Private Browsing, Private Browsing"
            ));
            assert!(title_marks_private(id, "Apple, „Privates Surfen“"));
            assert!(title_marks_private(id, "Apple、プライベートブラウズ"));
            assert!(title_marks_private(id, "Apple，无痕浏览"));
            assert!(title_marks_private(id, "Apple con navegación privada"));
            assert!(title_marks_private(id, "ท่องเว็บแบบส่วนตัวเรื่องApple"));
            assert!(!title_marks_private(id, "Apple"));
            assert!(!title_marks_private(id, "Private Browsing - Apple Support"));
        }
        // Another browser showing a page with that title is not Safari's marker.
        assert!(!title_marks_private(
            "com.google.Chrome",
            "Tips, Private Browsing"
        ));
        assert!(!title_marks_private(
            "com.apple.Safari.WebApp.1234",
            "Tips, Private Browsing"
        ));
    }

    #[test]
    fn safari_is_private_by_its_title_and_when_it_cannot_be_read_or_asked() {
        let safari = "com.apple.Safari";
        assert!(!window_is_private(safari, "Apple", Some(Mode::Normal)));
        assert!(window_is_private(
            safari,
            "Apple, Private Browsing",
            Some(Mode::Normal)
        ));
        // Asked and no answer (Automation refused, Safari hung): the title still settles it.
        assert!(!window_is_private(safari, "Apple", Some(Mode::Unknown)));
        assert!(window_is_private(
            safari,
            "Apple, Private Browsing",
            Some(Mode::Unknown)
        ));
        // No Accessibility title: nothing says the window is normal.
        assert!(window_is_private(safari, "", Some(Mode::Normal)));
        assert!(window_is_private(safari, "  ", None));
        // Safari Technology Preview is not asked; its title still settles it.
        let preview = "com.apple.SafariTechnologyPreview";
        assert!(!window_is_private(preview, "Apple", None));
        assert!(window_is_private(preview, "Apple, Private Browsing", None));
    }

    #[test]
    fn chromium_is_normal_only_when_it_says_so() {
        let chrome = "com.google.Chrome";
        assert!(!window_is_private(
            chrome,
            "Inbox - Google Chrome",
            Some(Mode::Normal)
        ));
        assert!(window_is_private(
            chrome,
            "Inbox - Google Chrome",
            Some(Mode::Private)
        ));
        assert!(window_is_private(
            chrome,
            "Inbox - Google Chrome",
            Some(Mode::Unknown)
        ));
        // The title marker still counts when the answer lags behind a window switch.
        assert!(window_is_private(
            chrome,
            "Inbox - Google Chrome (Incognito)",
            Some(Mode::Normal)
        ));
        // Vivaldi, Opera and the like are not asked, and their title marker is per language.
        assert!(window_is_private(
            "com.vivaldi.Vivaldi",
            "Inbox - Vivaldi",
            None
        ));
        assert!(window_is_private("com.operasoftware.Opera", "", None));
    }

    #[test]
    fn firefox_is_private_by_its_title_or_when_it_has_none() {
        let zen = "app.zen-browser.zen";
        assert!(!window_is_private(zen, "Page \u{2014} Zen Browser", None));
        assert!(window_is_private(
            zen,
            "Page \u{2014} Private Browsing",
            None
        ));
        assert!(window_is_private(zen, "", None));
    }

    #[test]
    fn an_app_that_is_not_a_browser_is_never_private_whatever_was_read() {
        for answer in [None, Some(Mode::Unknown), Some(Mode::Private)] {
            assert!(!window_is_private("com.apple.finder", "", answer));
            assert!(!window_is_private(
                "com.microsoft.VSCode",
                "x, Private Browsing",
                answer
            ));
        }
    }

    #[test]
    fn every_stored_url_loses_its_query_and_fragment() {
        let sample = |private| WindowSample {
            app: "Browser".to_string(),
            title: "Page".to_string(),
            bundle_id: None,
            url: Some("https://example.com/reset?token=abc#step".to_string()),
            domain: Some("example.com".to_string()),
            private,
            keeps_display_awake: false,
        };
        for private in [false, true] {
            let stored = sample(private).for_storage();
            assert_eq!(stored.url.as_deref(), Some("https://example.com/reset"));
            assert_eq!(stored.domain.as_deref(), Some("example.com"));
        }
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
