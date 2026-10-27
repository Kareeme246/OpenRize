//! Choosing a window title, and recording why none could be read.
//!
//! The OS calls live in `macos.rs`. What they should do with the answers does
//! not depend on the OS, so it lives here where a test can reach it: the order
//! titles are tried in, which results count as a title, which browsers we
//! expect to read titles from, and how often a failure is worth a log line.

use std::collections::HashMap;

/// Where a window's title can come from, in the order they are tried. The
/// first window an app reports as focused is the right one almost always, but
/// not for a popup, a picture-in-picture window or a detached DevTools panel:
/// there the focused window can be missing or untitled while the main window
/// still has the title the user is working under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TitleSource {
    FocusedWindow,
    MainWindow,
    /// The window that contains the focused control.
    FocusedElementWindow,
    /// The app's window list, front to back.
    WindowList,
}

impl TitleSource {
    pub const IN_ORDER: [TitleSource; 4] = [
        TitleSource::FocusedWindow,
        TitleSource::MainWindow,
        TitleSource::FocusedElementWindow,
        TitleSource::WindowList,
    ];
}

/// What one attempt at a title produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attempt {
    Title(String),
    /// A window answered but its title is empty or blank.
    Empty,
    /// The OS refused or had nothing to say; carries its error code.
    Failed(i32),
}

/// The outcome of walking every source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub title: Option<String>,
    /// What each source that was tried said, in the order tried.
    pub attempts: Vec<(TitleSource, Attempt)>,
}

impl Resolution {
    /// The first error code any source reported, the most useful single fact
    /// when no title came back.
    pub fn first_error(&self) -> Option<i32> {
        self.attempts.iter().find_map(|(_, attempt)| match attempt {
            Attempt::Failed(code) => Some(*code),
            _ => None,
        })
    }
}

/// Whether a title is worth keeping: an empty or blank title is "no title",
/// not a successful read that happens to say nothing.
pub fn is_usable(title: &str) -> bool {
    !title.trim().is_empty()
}

/// The first usable title among windows listed in order, with the error or
/// emptiness of the front window kept for the log when none is usable.
pub fn pick_from_windows(windows: impl IntoIterator<Item = Attempt>) -> Attempt {
    let mut first_miss = None;
    for attempt in windows {
        match attempt {
            Attempt::Title(title) if is_usable(&title) => return Attempt::Title(title),
            Attempt::Title(_) => first_miss = first_miss.or(Some(Attempt::Empty)),
            other => first_miss = first_miss.or(Some(other)),
        }
    }
    first_miss.unwrap_or(Attempt::Empty)
}

/// Tries each source in order and stops at the first usable title. `read` is
/// called lazily, so a focused window with a title costs one lookup.
pub fn resolve(mut read: impl FnMut(TitleSource) -> Attempt) -> Resolution {
    let mut attempts = Vec::new();
    for source in TitleSource::IN_ORDER {
        let attempt = match read(source) {
            Attempt::Title(title) if !is_usable(&title) => Attempt::Empty,
            other => other,
        };
        if let Attempt::Title(title) = &attempt {
            let title = title.clone();
            attempts.push((source, attempt));
            return Resolution {
                title: Some(title),
                attempts,
            };
        }
        attempts.push((source, attempt));
    }
    Resolution {
        title: None,
        attempts,
    }
}

/// The browser engines whose window titles we expect to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserFamily {
    Firefox,
    Chromium,
}

impl BrowserFamily {
    pub fn label(self) -> &'static str {
        match self {
            BrowserFamily::Firefox => "firefox-family",
            BrowserFamily::Chromium => "chromium-family",
        }
    }
}

/// Every browser bundle id we name as covered. Titles are read the same way
/// for any app, so an id missing here still gets a title when macOS allows it;
/// what the list buys is that a failure is logged under the browser's family
/// and a browser that is not listed is logged as unlisted, which says plainly
/// that nobody has checked it. Add an id here when someone has.
///
/// Ids for Firefox, Firefox Developer Edition, Zen and Chrome were read off
/// installed bundles; the rest are the publishers' documented identifiers.
pub const BROWSERS: &[(&str, BrowserFamily)] = &[
    ("org.mozilla.firefox", BrowserFamily::Firefox),
    (
        "org.mozilla.firefoxdeveloperedition",
        BrowserFamily::Firefox,
    ),
    ("org.mozilla.nightly", BrowserFamily::Firefox),
    ("app.zen-browser.zen", BrowserFamily::Firefox),
    (
        "io.gitlab.librewolf-community.librewolf",
        BrowserFamily::Firefox,
    ),
    ("one.ablaze.floorp", BrowserFamily::Firefox),
    ("net.waterfox.waterfox", BrowserFamily::Firefox),
    ("org.torproject.torbrowser", BrowserFamily::Firefox),
    ("net.mullvad.mullvadbrowser", BrowserFamily::Firefox),
    ("com.google.Chrome", BrowserFamily::Chromium),
    ("com.google.Chrome.beta", BrowserFamily::Chromium),
    ("com.google.Chrome.dev", BrowserFamily::Chromium),
    ("com.google.Chrome.canary", BrowserFamily::Chromium),
    ("org.chromium.Chromium", BrowserFamily::Chromium),
    ("com.brave.Browser", BrowserFamily::Chromium),
    ("com.brave.Browser.beta", BrowserFamily::Chromium),
    ("com.brave.Browser.dev", BrowserFamily::Chromium),
    ("com.brave.Browser.nightly", BrowserFamily::Chromium),
    ("com.microsoft.edgemac", BrowserFamily::Chromium),
    ("com.microsoft.edgemac.Beta", BrowserFamily::Chromium),
    ("com.microsoft.edgemac.Dev", BrowserFamily::Chromium),
    ("com.microsoft.edgemac.Canary", BrowserFamily::Chromium),
    ("company.thebrowser.Browser", BrowserFamily::Chromium),
    ("company.thebrowser.dia", BrowserFamily::Chromium),
    ("com.vivaldi.Vivaldi", BrowserFamily::Chromium),
    ("com.operasoftware.Opera", BrowserFamily::Chromium),
    ("com.operasoftware.OperaGX", BrowserFamily::Chromium),
    ("ai.perplexity.comet", BrowserFamily::Chromium),
    ("com.sigmaos.sigmaos.macos", BrowserFamily::Chromium),
];

pub fn browser_family(bundle_id: &str) -> Option<BrowserFamily> {
    BROWSERS
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(bundle_id))
        .map(|(_, family)| *family)
}

/// Lets a repeating failure through once per interval per key, and counts what
/// it held back so the next line can say how often it happened. The sampler
/// runs once a second; without this a permission that is simply off would fill
/// the log.
pub struct FailureLog {
    interval_ms: u64,
    seen: HashMap<String, Seen>,
}

struct Seen {
    last_logged_ms: u64,
    held_back: u32,
}

impl FailureLog {
    pub fn new(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            seen: HashMap::new(),
        }
    }

    /// `Some(held_back)` when the failure under `key` should be logged now,
    /// where `held_back` is how many identical ones were skipped since the
    /// last line; `None` when it should stay quiet.
    pub fn admit(&mut self, key: &str, now_ms: u64) -> Option<u32> {
        match self.seen.get_mut(key) {
            None => {
                self.seen.insert(
                    key.to_string(),
                    Seen {
                        last_logged_ms: now_ms,
                        held_back: 0,
                    },
                );
                Some(0)
            }
            Some(seen) if now_ms.saturating_sub(seen.last_logged_ms) >= self.interval_ms => {
                let held_back = seen.held_back;
                seen.last_logged_ms = now_ms;
                seen.held_back = 0;
                Some(held_back)
            }
            Some(seen) => {
                seen.held_back = seen.held_back.saturating_add(1);
                None
            }
        }
    }
}

/// What to log when no source gave a title, or `None` when saying so would
/// only be noise. An app with no window at all (a menu bar app, Finder with
/// every window closed) answers "no value" and is not a problem; a refused
/// call, or any trouble reading a browser, is.
pub fn failure_to_log(bundle_id: Option<&str>, resolution: &Resolution) -> Option<String> {
    const NO_VALUE: i32 = -25212;
    if resolution.title.is_some() {
        return None;
    }
    let listed = bundle_id.and_then(browser_family).is_some();
    let only_missing = resolution
        .attempts
        .iter()
        .all(|(_, attempt)| matches!(attempt, Attempt::Failed(NO_VALUE) | Attempt::Empty));
    if only_missing && !listed {
        return None;
    }
    Some(failure_key(bundle_id, resolution))
}

/// The key a failure is rate limited under, which doubles as its log text.
pub fn failure_key(bundle_id: Option<&str>, resolution: &Resolution) -> String {
    let family = bundle_id
        .and_then(browser_family)
        .map_or("not a listed browser", BrowserFamily::label);
    let how = match resolution.first_error() {
        Some(code) => format!("AXError {code}"),
        None => "every window had an empty title".to_string(),
    };
    format!("{} ({family}): {how}", bundle_id.unwrap_or("unknown app"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(text: &str) -> Attempt {
        Attempt::Title(text.to_string())
    }

    #[test]
    fn the_focused_window_wins_and_nothing_else_is_asked() {
        let mut asked = Vec::new();
        let resolution = resolve(|source| {
            asked.push(source);
            title("Inbox - Mail")
        });
        assert_eq!(resolution.title.as_deref(), Some("Inbox - Mail"));
        assert_eq!(asked, vec![TitleSource::FocusedWindow]);
    }

    #[test]
    fn a_missing_focused_window_falls_back_to_the_main_window() {
        let resolution = resolve(|source| match source {
            TitleSource::FocusedWindow => Attempt::Failed(-25212),
            TitleSource::MainWindow => title("Docs - Zen"),
            _ => panic!("asked past the main window"),
        });
        assert_eq!(resolution.title.as_deref(), Some("Docs - Zen"));
        assert_eq!(
            resolution.attempts,
            vec![
                (TitleSource::FocusedWindow, Attempt::Failed(-25212)),
                (TitleSource::MainWindow, title("Docs - Zen")),
            ]
        );
    }

    #[test]
    fn an_empty_title_is_not_a_success() {
        // A popup is focused and answers with no title; the main window has it.
        let resolution = resolve(|source| match source {
            TitleSource::FocusedWindow => title(""),
            TitleSource::MainWindow => title("Pull request - Chrome"),
            _ => panic!("asked past the main window"),
        });
        assert_eq!(resolution.title.as_deref(), Some("Pull request - Chrome"));
        assert_eq!(
            resolution.attempts[0],
            (TitleSource::FocusedWindow, Attempt::Empty)
        );
    }

    #[test]
    fn a_blank_title_counts_as_empty() {
        assert!(!is_usable(""));
        assert!(!is_usable("  \t\n"));
        assert!(is_usable(" a "));
        let resolution = resolve(|source| match source {
            TitleSource::FocusedWindow => title("   "),
            TitleSource::MainWindow => Attempt::Empty,
            TitleSource::FocusedElementWindow => title("Arc"),
            TitleSource::WindowList => panic!("asked past the focused element"),
        });
        assert_eq!(resolution.title.as_deref(), Some("Arc"));
    }

    #[test]
    fn the_chain_tries_every_source_in_order_before_giving_up() {
        let mut asked = Vec::new();
        let resolution = resolve(|source| {
            asked.push(source);
            Attempt::Failed(-25211)
        });
        assert_eq!(resolution.title, None);
        assert_eq!(asked, TitleSource::IN_ORDER.to_vec());
        assert_eq!(resolution.first_error(), Some(-25211));
    }

    #[test]
    fn every_window_empty_is_no_title_and_no_error() {
        let resolution = resolve(|_| Attempt::Empty);
        assert_eq!(resolution.title, None);
        assert_eq!(resolution.first_error(), None);
    }

    #[test]
    fn the_window_list_skips_untitled_windows() {
        let picked = pick_from_windows([
            Attempt::Empty,
            title("  "),
            title("DevTools"),
            title("Page"),
        ]);
        assert_eq!(picked, title("DevTools"));
    }

    #[test]
    fn a_window_list_with_no_title_reports_the_front_windows_problem() {
        assert_eq!(
            pick_from_windows([Attempt::Failed(-25204), Attempt::Empty]),
            Attempt::Failed(-25204)
        );
        assert_eq!(
            pick_from_windows([title(" "), Attempt::Failed(-1)]),
            Attempt::Empty
        );
        assert_eq!(pick_from_windows([]), Attempt::Empty);
    }

    #[test]
    fn firefox_and_chromium_browsers_are_named_explicitly() {
        for id in [
            "org.mozilla.firefox",
            "org.mozilla.firefoxdeveloperedition",
            "org.mozilla.nightly",
            "app.zen-browser.zen",
            "one.ablaze.floorp",
            "net.waterfox.waterfox",
        ] {
            assert_eq!(browser_family(id), Some(BrowserFamily::Firefox), "{id}");
        }
        for id in [
            "com.google.Chrome",
            "com.google.Chrome.canary",
            "org.chromium.Chromium",
            "com.brave.Browser",
            "com.microsoft.edgemac",
            "company.thebrowser.Browser",
            "company.thebrowser.dia",
            "com.vivaldi.Vivaldi",
            "com.operasoftware.Opera",
        ] {
            assert_eq!(browser_family(id), Some(BrowserFamily::Chromium), "{id}");
        }
    }

    #[test]
    fn an_unlisted_browser_is_visibly_unlisted() {
        assert_eq!(browser_family("com.example.NewBrowser"), None);
        assert_eq!(browser_family("com.apple.Safari"), None);
        let resolution = resolve(|_| Attempt::Failed(-25212));
        assert_eq!(
            failure_key(Some("com.example.NewBrowser"), &resolution),
            "com.example.NewBrowser (not a listed browser): AXError -25212"
        );
        assert_eq!(
            failure_key(Some("app.zen-browser.zen"), &resolution),
            "app.zen-browser.zen (firefox-family): AXError -25212"
        );
    }

    #[test]
    fn only_trouble_is_logged() {
        let no_window = resolve(|_| Attempt::Failed(-25212));
        let refused = resolve(|_| Attempt::Failed(-25211));
        let found = resolve(|_| title("Page"));
        assert_eq!(failure_to_log(Some("com.apple.finder"), &no_window), None);
        assert_eq!(failure_to_log(Some("com.apple.finder"), &found), None);
        assert!(failure_to_log(Some("com.apple.finder"), &refused).is_some());
        // A browser with nothing to read is worth a line even without an error.
        assert!(failure_to_log(Some("org.mozilla.firefox"), &no_window).is_some());
        assert_eq!(failure_to_log(Some("org.mozilla.firefox"), &found), None);
    }

    #[test]
    fn the_browser_table_has_no_duplicates() {
        let mut ids: Vec<String> = BROWSERS.iter().map(|(id, _)| id.to_lowercase()).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before);
    }

    #[test]
    fn a_repeating_failure_is_logged_once_per_interval_with_a_count() {
        let mut log = FailureLog::new(60_000);
        assert_eq!(log.admit("zen: AXError -25211", 0), Some(0));
        for second in 1..60 {
            assert_eq!(log.admit("zen: AXError -25211", second * 1000), None);
        }
        assert_eq!(log.admit("zen: AXError -25211", 60_000), Some(59));
        assert_eq!(log.admit("zen: AXError -25211", 61_000), None);
    }

    #[test]
    fn different_failures_are_limited_separately() {
        let mut log = FailureLog::new(60_000);
        assert_eq!(log.admit("a", 0), Some(0));
        assert_eq!(log.admit("b", 1_000), Some(0));
        assert_eq!(log.admit("a", 2_000), None);
    }
}
