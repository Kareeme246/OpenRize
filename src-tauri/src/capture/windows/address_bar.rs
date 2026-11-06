//! A browser window's URL, read from its address bar through UI Automation on a
//! worker thread, since UIA calls can block for as long as the browser is hung.
//! [`UrlTracker`] smooths reads: grace period, two-read confirmation, retry backoff.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest,
    IUIAutomationCondition, IUIAutomationElement, IUIAutomationValuePattern, TreeScope_Children,
    UIA_AutomationIdPropertyId, UIA_ClassNamePropertyId, UIA_ComboBoxControlTypeId,
    UIA_ControlTypePropertyId, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
    UIA_HasKeyboardFocusPropertyId, UIA_NamePropertyId, UIA_ValuePatternId,
    UIA_ValueValuePropertyId, UIA_CONTROLTYPE_ID,
};

const GRACE: Duration = Duration::from_secs(8);
/// While the worker is stuck nothing can be confirmed, so the last URL outlives `GRACE`.
const STALLED_GRACE: Duration = Duration::from_secs(30);
const SWITCH_READS: u8 = 2;
const MISSES_BEFORE_BACKOFF: u32 = 3;
const BACKOFF: Duration = Duration::from_secs(15);
const FORGET_AFTER: Duration = Duration::from_secs(60);
const MAX_WINDOWS: usize = 8;

const READ_DEADLINE: Duration = Duration::from_millis(800);
const STALL_AFTER: Duration = Duration::from_millis(1500);
/// How long the sampler waits for the first read, so its first sample carries the URL.
const FIRST_READ_WAIT: Duration = Duration::from_millis(80);
const CONNECTION_TIMEOUT_MS: u32 = 500;
const TRANSACTION_TIMEOUT_MS: u32 = 1000;

const SEARCH_MAX_DEPTH: usize = 12;
const SEARCH_MAX_NODES: usize = 600;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Reading {
    Page(Option<String>),
    /// The bar has focus: its trimmed text is what the user is editing.
    Typing(Option<String>),
    Missing,
}

/// Smoothed answer per window; times are passed in so tests need no clock.
#[derive(Debug, Default)]
struct UrlTracker {
    shown: Option<String>,
    confirmed: Option<Instant>,
    pending: Option<(Option<String>, u8)>,
    title: Option<String>,
    /// The title changed, so `shown` may be the old page: replace it without a second read.
    retitled: bool,
    requested: Option<Instant>,
    misses: u32,
    retry_at: Option<Instant>,
    /// Fixed for a window's life, so it is kept even when `touch` forgets the page.
    private: Option<bool>,
}

impl UrlTracker {
    /// After a long gap the old URL (or a reused handle) means nothing, so it goes.
    fn touch(&mut self, now: Instant) {
        if self
            .requested
            .is_some_and(|last| now.duration_since(last) > GRACE)
        {
            self.shown = None;
            self.confirmed = None;
            self.pending = None;
            self.title = None;
            self.retitled = false;
        }
        self.requested = Some(now);
    }

    /// A new title means a navigation and the bar already shows it: skip confirmation.
    fn retitle(&mut self, title: &str) {
        if self.title.as_deref().is_some_and(|last| last != title) {
            self.retitled = true;
        }
        if self.title.as_deref() != Some(title) {
            self.title = Some(title.to_string());
        }
    }

    fn due(&self, now: Instant) -> bool {
        self.retry_at.is_none_or(|at| now >= at)
    }

    fn unread(&self, now: Instant) -> bool {
        self.retitled || !self.fresh(now, GRACE)
    }

    fn fresh(&self, now: Instant, grace: Duration) -> bool {
        self.confirmed
            .is_some_and(|at| now.duration_since(at) <= grace)
    }

    fn url(&self, now: Instant, stalled: bool) -> Option<String> {
        let grace = if stalled { STALLED_GRACE } else { GRACE };
        self.fresh(now, grace).then(|| self.shown.clone()).flatten()
    }

    fn record(&mut self, reading: Reading, now: Instant) {
        match reading {
            Reading::Missing => {
                self.misses += 1;
                if self.misses >= MISSES_BEFORE_BACKOFF {
                    self.retry_at = Some(now + BACKOFF);
                }
                return;
            }
            Reading::Typing(text) => {
                if self.editing(text.as_deref()) {
                    self.pending = None;
                } else {
                    // A new tab's empty bar, or text unrelated to the page.
                    self.see(None, now);
                }
            }
            Reading::Page(url) => self.see(url, now),
        }
        self.retitled = false;
        self.misses = 0;
        self.retry_at = None;
        self.confirmed = Some(now);
    }

    /// Whether the focused bar holds an edit of the page being shown. An empty bar
    /// or unrelated text does not keep the previous URL.
    fn editing(&self, text: Option<&str>) -> bool {
        let Some(text) = text.map(str::trim) else {
            return true;
        };
        if text.is_empty() {
            return false;
        }
        let Some(shown) = &self.shown else {
            return true;
        };
        if self.retitled {
            return false;
        }
        normalize_address(text).is_some() || {
            let (shown, text) = (bare(shown), text.to_lowercase());
            shown.contains(&text) || text.contains(&shown)
        }
    }

    fn see(&mut self, url: Option<String>, now: Instant) {
        // Nothing to protect once the previous URL has lapsed or never was,
        // and a retitled window is a different page.
        if self.retitled || !self.fresh(now, GRACE) || url == self.shown {
            self.shown = url;
            self.pending = None;
            return;
        }
        let agreed = match self.pending.take() {
            Some((seen, count)) if seen == url => count + 1,
            _ => 1,
        };
        if agreed >= SWITCH_READS {
            self.shown = url;
        } else {
            self.pending = Some((url, agreed));
        }
    }
}

/// The URL shown in a browser window, without ever waiting long. A changed title
/// makes the next read replace it unconfirmed, and the sampler waits briefly for it.
pub(super) fn url_of_titled(window: HWND, title: &str) -> Option<String> {
    shared_url_of(window, Some(title))
}

fn shared_url_of(window: HWND, title: Option<&str>) -> Option<String> {
    shared()?.url_of(window.0 as isize, title)
}

static READER: OnceLock<Option<Arc<Shared>>> = OnceLock::new();

fn shared() -> Option<&'static Arc<Shared>> {
    READER.get_or_init(Shared::start).as_ref()
}

/// Whether a browser window is private. The title settles most (Edge `[InPrivate]`,
/// Firefox `Private Browsing`); Chrome's Incognito is only in the accessible name,
/// which the worker reads once. Never waits: until then the title's answer stands.
pub(super) fn is_private_window(window: HWND, executable: &str, title: &str) -> bool {
    openrize_core::capture::title_marks_private(executable, title)
        || READER
            .get()
            .and_then(Option::as_ref)
            .is_some_and(|shared| shared.is_private(window.0 as isize))
}

/// Shared by the sampler and the worker, keyed by handle value since `HWND` is not `Send`.
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    done: Condvar,
}

#[derive(Default)]
struct State {
    windows: HashMap<isize, UrlTracker>,
    wanted: Option<isize>,
    issued: u64,
    finished: u64,
    busy_since: Option<Instant>,
}

impl Shared {
    fn start() -> Option<Arc<Self>> {
        let shared = Arc::new(Self {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            done: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("address-bar".into())
            .spawn(move || worker.run());
        match spawned {
            Ok(_) => Some(shared),
            Err(error) => {
                eprintln!("could not start the address bar reader: {error}");
                None
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn url_of(&self, window: isize, title: Option<&str>) -> Option<String> {
        let now = Instant::now();
        let mut state = self.lock();
        state.forget_idle(now);
        let stalled = state
            .busy_since
            .is_some_and(|since| now.duration_since(since) > STALL_AFTER);
        let tracker = state.windows.entry(window).or_default();
        tracker.touch(now);
        if let Some(title) = title {
            tracker.retitle(title);
        }
        let (due, unread) = (tracker.due(now), tracker.unread(now));
        if due {
            state.wanted = Some(window);
            state.issued += 1;
            let ticket = state.issued;
            self.wake.notify_one();
            if unread && !stalled {
                state = self
                    .done
                    .wait_timeout_while(state, FIRST_READ_WAIT, |state| state.finished < ticket)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
        state
            .windows
            .get(&window)
            .and_then(|tracker| tracker.url(Instant::now(), stalled))
    }

    fn is_private(&self, window: isize) -> bool {
        self.lock()
            .windows
            .get(&window)
            .is_some_and(|tracker| tracker.private == Some(true))
    }

    fn run(&self) {
        let mut reader = Reader::default();
        loop {
            let (window, ticket, learn_private) = {
                let mut state = self.lock();
                let window = loop {
                    if let Some(window) = state.wanted.take() {
                        break window;
                    }
                    state = self
                        .wake
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                };
                state.busy_since = Some(Instant::now());
                let learn_private = state
                    .windows
                    .get(&window)
                    .is_some_and(|tracker| tracker.private.is_none());
                (window, state.issued, learn_private)
            };
            let reading = reader.read(window);
            let private = if learn_private {
                reader.window_private(window)
            } else {
                None
            };

            let mut state = self.lock();
            state.busy_since = None;
            state.finished = ticket;
            if let Some(tracker) = state.windows.get_mut(&window) {
                tracker.record(reading, Instant::now());
                if private.is_some() {
                    tracker.private = private;
                }
            }
            reader
                .bars
                .retain(|window, _| state.windows.contains_key(window));
            self.done.notify_all();
        }
    }
}

impl State {
    fn forget_idle(&mut self, now: Instant) {
        if self.windows.len() > MAX_WINDOWS {
            self.windows.retain(|_, tracker| {
                tracker
                    .requested
                    .is_some_and(|at| now.duration_since(at) <= FORGET_AFTER)
            });
        }
    }
}

#[derive(Default)]
struct Reader {
    automation: Option<Automation>,
    bars: HashMap<isize, IUIAutomationElement>,
}

impl Reader {
    fn read(&mut self, window: isize) -> Reading {
        if self.automation.is_none() {
            self.automation = Automation::new();
        }
        let Some(automation) = &self.automation else {
            return Reading::Missing;
        };
        if let Some(bar) = self.bars.get(&window) {
            // SAFETY: COM calls on a live element; a failure ends the read.
            match unsafe { automation.read(bar) } {
                Some(reading) => return reading,
                None => {
                    self.bars.remove(&window);
                }
            }
        }
        let deadline = Instant::now() + READ_DEADLINE;
        // SAFETY: COM calls on live UI Automation objects; every failure
        // ends the search. The handle is only given to UI Automation, which
        // rejects a stale one.
        let bar = unsafe { automation.find_bar(HWND(window as *mut c_void), deadline) };
        let Some(bar) = bar else {
            return Reading::Missing;
        };
        // SAFETY: as above.
        match unsafe { automation.read(&bar) } {
            Some(reading) => {
                self.bars.insert(window, bar);
                reading
            }
            None => Reading::Missing,
        }
    }
}

impl Reader {
    /// `None` when unreadable, so the next read retries.
    fn window_private(&mut self, window: isize) -> Option<bool> {
        let automation = self.automation.as_ref()?;
        // SAFETY: a COM call on a live automation object; the handle is only
        // given to UI Automation, which rejects a stale one.
        let name = unsafe { automation.window_name(HWND(window as *mut c_void)) }?;
        Some(openrize_core::capture::chromium_name_marks_private(&name))
    }
}

struct Automation {
    automation: IUIAutomation,
    named: IUIAutomationCacheRequest,
    /// Properties of the controls the search looks at, fetched with the
    /// children so the search needs one call per parent, not several per child.
    search: IUIAutomationCacheRequest,
    value: IUIAutomationCacheRequest,
    controls: IUIAutomationCondition,
}

impl Automation {
    fn new() -> Option<Self> {
        // SAFETY: joins this thread to the multithreaded apartment for its
        // whole life; a repeated call is harmless (S_FALSE). The cache
        // requests only name properties and patterns.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                    .or_else(|_| CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER))
                    .ok()?;
            if let Ok(automation) = automation.cast::<IUIAutomation2>() {
                // Never wait long on one browser, and never move its focus.
                let _ = automation.SetConnectionTimeout(CONNECTION_TIMEOUT_MS);
                let _ = automation.SetTransactionTimeout(TRANSACTION_TIMEOUT_MS);
                let _ = automation.SetAutoSetFocus(false);
            }
            let search = automation.CreateCacheRequest().ok()?;
            for property in [
                UIA_ControlTypePropertyId,
                UIA_AutomationIdPropertyId,
                UIA_ClassNamePropertyId,
            ] {
                search.AddProperty(property).ok()?;
            }
            let named = automation.CreateCacheRequest().ok()?;
            named.AddProperty(UIA_NamePropertyId).ok()?;
            let value = automation.CreateCacheRequest().ok()?;
            value.AddProperty(UIA_HasKeyboardFocusPropertyId).ok()?;
            value.AddProperty(UIA_ValueValuePropertyId).ok()?;
            value.AddPattern(UIA_ValuePatternId).ok()?;
            let controls = automation.ControlViewCondition().ok()?;
            Some(Self {
                automation,
                named,
                search,
                value,
                controls,
            })
        }
    }

    /// The window's own UIA name; for Chrome it includes the Incognito marker the title lacks.
    /// # Safety
    /// `window` is only handed to UI Automation, which rejects a stale handle.
    unsafe fn window_name(&self, window: HWND) -> Option<String> {
        unsafe {
            let root = self
                .automation
                .ElementFromHandleBuildCache(window, &self.named)
                .ok()?;
            root.CachedName().ok().map(|name| name.to_string())
        }
    }

    /// # Safety
    /// `bar` must be a live UI Automation element.
    unsafe fn read(&self, bar: &IUIAutomationElement) -> Option<Reading> {
        unsafe {
            let bar = bar.BuildUpdatedCache(&self.value).ok()?;
            // While the user types in the bar it holds their text, not the page.
            let text = bar
                .GetCachedPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()
                .and_then(|pattern| pattern.CachedValue().ok())
                .map(|text| text.to_string());
            if bar.CachedHasKeyboardFocus().ok()?.as_bool() {
                return Some(Reading::Typing(text.map(|text| text.trim().to_string())));
            }
            Some(Reading::Page(normalize_address(&text?)))
        }
    }

    /// Skips the page (a Document): walking web content makes Chromium enable full accessibility.
    /// # Safety
    /// As for [`Self::walk`].
    unsafe fn find_bar(&self, window: HWND, deadline: Instant) -> Option<IUIAutomationElement> {
        let mut plain = None;
        let mut found = unsafe {
            self.walk(window, deadline, &mut |control| match classify(
                control.kind,
                &control.id,
                &control.class,
            ) {
                Match::Address => Step::Found,
                Match::Other => Step::Skip,
                Match::Fallback => {
                    plain.get_or_insert_with(|| control.element.clone());
                    Step::Skip
                }
                Match::Not if control.is_page() => Step::Skip,
                Match::Not => Step::Descend,
            })
        };
        found.take().or(plain)
    }

    /// Breadth-first search of `window`'s controls until `visit` finds something or a limit is hit.
    /// # Safety
    /// `window` is only handed to UI Automation, which rejects a stale handle.
    unsafe fn walk(
        &self,
        window: HWND,
        deadline: Instant,
        visit: &mut dyn FnMut(&Control) -> Step,
    ) -> Option<IUIAutomationElement> {
        unsafe {
            let root = self
                .automation
                .ElementFromHandleBuildCache(window, &self.search)
                .ok()?;
            let mut level = vec![root];
            let mut visited = 0;
            for _ in 0..SEARCH_MAX_DEPTH {
                let mut next = Vec::new();
                for parent in &level {
                    if Instant::now() >= deadline {
                        return None;
                    }
                    let Ok(children) =
                        parent.FindAllBuildCache(TreeScope_Children, &self.controls, &self.search)
                    else {
                        continue;
                    };
                    for index in 0..children.Length().unwrap_or(0) {
                        let Ok(element) = children.GetElement(index) else {
                            continue;
                        };
                        visited += 1;
                        if visited > SEARCH_MAX_NODES {
                            return None;
                        }
                        let control = Control {
                            kind: element.CachedControlType().unwrap_or_default(),
                            id: element.CachedAutomationId().unwrap_or_default().to_string(),
                            class: element.CachedClassName().unwrap_or_default().to_string(),
                            element,
                        };
                        match visit(&control) {
                            Step::Found => return Some(control.element),
                            Step::Skip => {}
                            Step::Descend => next.push(control.element),
                        }
                    }
                }
                if next.is_empty() {
                    return None;
                }
                level = next;
            }
            None
        }
    }
}

enum Step {
    Descend,
    Skip,
    Found,
}

struct Control {
    element: IUIAutomationElement,
    kind: UIA_CONTROLTYPE_ID,
    id: String,
    class: String,
}

impl Control {
    fn is_page(&self) -> bool {
        self.kind == UIA_DocumentControlTypeId
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Match {
    Address,
    Other,
    /// A text field of no known kind; the first stands in when none is certain.
    Fallback,
    Not,
}

/// Judges a control by identifiers browsers do not translate. Firefox family: the
/// AutomationId `urlbar-input` (a ComboBox, not an Edit). Chromium: an Edit of class
/// `OmniboxViewViews` (forks prefix it); the find bar and others are plain `Textfield`s.
fn classify(kind: UIA_CONTROLTYPE_ID, id: &str, class: &str) -> Match {
    let text_field = kind == UIA_EditControlTypeId || kind == UIA_ComboBoxControlTypeId;
    if !text_field {
        return Match::Not;
    }
    if id == "urlbar-input" || class.contains("OmniboxView") {
        Match::Address
    } else if kind != UIA_EditControlTypeId
        || class == "Textfield"
        || id.to_ascii_lowercase().contains("find")
    {
        // Other combo boxes (page and browser settings) are never the bar.
        Match::Other
    } else {
        Match::Fallback
    }
}

/// Address bars hide the scheme; an empty or search-text bar is no page. Only web pages count.
fn normalize_address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let scheme = text.contains("://");
    let url = if scheme {
        text.to_string()
    } else {
        format!("https://{text}")
    };
    let parsed = url::Url::parse(&url).ok()?;
    let web = matches!(parsed.scheme(), "http" | "https");
    let host = parsed.host_str()?;
    // A dotless host ("jira", "server:8080") is a page only when the text
    // says so with a scheme or a port; a lone word is a search.
    let named = host.contains('.') || host == "localhost" || scheme || parsed.port().is_some();
    (web && named).then_some(url)
}

/// A URL without scheme, "www." and trailing slash, lower-cased, as the bar shows it while edited.
fn bare(url: &str) -> String {
    let url = url.to_lowercase();
    let url = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest);
    let url = url.strip_prefix("www.").unwrap_or(url);
    url.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn page(url: &str) -> Reading {
        Reading::Page(Some(url.to_string()))
    }

    fn typing(text: &str) -> Reading {
        Reading::Typing(Some(text.to_string()))
    }

    fn showing(url: &str, start: Instant) -> UrlTracker {
        let mut tracker = UrlTracker::default();
        tracker.touch(start);
        tracker.record(page(url), start);
        tracker
    }

    #[test]
    fn address_bar_text_becomes_a_web_url() {
        assert_eq!(
            normalize_address("github.com/openrize").as_deref(),
            Some("https://github.com/openrize")
        );
        assert_eq!(
            normalize_address("http://localhost:1420/").as_deref(),
            Some("http://localhost:1420/")
        );
        assert_eq!(
            normalize_address("  https://github.com/Kareeme246/OpenRize\n").as_deref(),
            Some("https://github.com/Kareeme246/OpenRize")
        );
        assert_eq!(normalize_address(""), None);
        assert_eq!(normalize_address("   "), None);
        assert_eq!(normalize_address("how to rust"), None);
        assert_eq!(normalize_address("edge://settings"), None);
        assert_eq!(normalize_address("about:blank"), None);
        assert_eq!(normalize_address("file:///C:/notes.html"), None);
        assert_eq!(normalize_address("newtab"), None);
        assert_eq!(normalize_address("rust"), None);
        assert_eq!(normalize_address("jira"), None);
        assert_eq!(normalize_address("jira/"), None);
    }

    #[test]
    fn dotless_hosts_need_a_scheme_or_a_port() {
        assert_eq!(
            normalize_address("http://jira/").as_deref(),
            Some("http://jira/")
        );
        assert_eq!(
            normalize_address("https://intranet/wiki").as_deref(),
            Some("https://intranet/wiki")
        );
        assert_eq!(
            normalize_address("server:8080").as_deref(),
            Some("https://server:8080")
        );
        assert_eq!(
            normalize_address("localhost:3000").as_deref(),
            Some("https://localhost:3000")
        );
        assert_eq!(normalize_address("chrome://newtab"), None);
        assert_eq!(normalize_address("ftp://server/"), None);
    }

    #[test]
    fn address_bars_are_told_from_other_text_fields() {
        let edit = UIA_EditControlTypeId;
        let combo = UIA_ComboBoxControlTypeId;
        assert_eq!(
            classify(combo, "urlbar-input", "urlbar-input textbox-input"),
            Match::Address
        );
        assert_eq!(
            classify(edit, "view_1017", "OmniboxViewViews"),
            Match::Address
        );
        assert_eq!(classify(edit, "", "BraveOmniboxViewViews"), Match::Address);
        assert_eq!(classify(edit, "", "Textfield"), Match::Other);
        assert_eq!(classify(edit, "findbar-textbox", ""), Match::Other);
        assert_eq!(classify(combo, "zoom-menu", ""), Match::Other);
        assert_eq!(classify(edit, "", "SomeEdit"), Match::Fallback);
        assert_eq!(classify(UIA_DocumentControlTypeId, "", ""), Match::Not);
    }

    #[test]
    fn first_read_shows_at_once() {
        let start = Instant::now();
        let tracker = showing("https://a.example/", start);
        assert_eq!(
            tracker.url(start, false).as_deref(),
            Some("https://a.example/")
        );
    }

    #[test]
    fn failed_reads_keep_the_url_for_a_grace_period() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        tracker.record(Reading::Missing, start + secs(1));
        tracker.record(Reading::Missing, start + secs(2));
        assert_eq!(
            tracker.url(start + secs(3), false).as_deref(),
            Some("https://a.example/")
        );
        assert_eq!(tracker.url(start + GRACE + secs(1), false), None);
        tracker.record(page("https://a.example/"), start + GRACE + secs(2));
        assert_eq!(
            tracker.url(start + GRACE + secs(2), false).as_deref(),
            Some("https://a.example/")
        );
    }

    #[test]
    fn a_different_url_must_be_read_twice() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        tracker.record(page("https://b.example/"), start + secs(1));
        assert_eq!(
            tracker.url(start + secs(1), false).as_deref(),
            Some("https://a.example/")
        );
        tracker.record(page("https://b.example/"), start + secs(2));
        assert_eq!(
            tracker.url(start + secs(2), false).as_deref(),
            Some("https://b.example/")
        );
    }

    #[test]
    fn a_one_read_blip_does_not_switch_the_url() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        for (second, reading) in [
            (1, page("https://b.example/")),
            (2, page("https://a.example/")),
            (3, page("https://c.example/")),
            (4, page("https://a.example/")),
        ] {
            tracker.record(reading, start + secs(second));
            assert_eq!(
                tracker.url(start + secs(second), false).as_deref(),
                Some("https://a.example/")
            );
        }
        tracker.record(page("https://b.example/"), start + secs(5));
        tracker.record(page("https://c.example/"), start + secs(6));
        assert_eq!(
            tracker.url(start + secs(6), false).as_deref(),
            Some("https://a.example/")
        );
    }

    #[test]
    fn leaving_the_web_is_also_read_twice() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        tracker.record(Reading::Page(None), start + secs(1));
        assert!(tracker.url(start + secs(1), false).is_some());
        tracker.record(Reading::Page(None), start + secs(2));
        assert_eq!(tracker.url(start + secs(2), false), None);
    }

    #[test]
    fn typing_in_the_bar_keeps_the_previous_url() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        tracker.record(page("https://b.example/"), start + secs(1));
        // Focus in the bar drops a half-seen switch and holds the URL, even
        // past the grace period, for as long as the typing lasts.
        for second in 2..30 {
            tracker.record(typing("a.exam"), start + secs(second));
            assert_eq!(
                tracker.url(start + secs(second), false).as_deref(),
                Some("https://a.example/")
            );
        }
        tracker.record(page("https://b.example/"), start + secs(30));
        assert_eq!(
            tracker.url(start + secs(30), false).as_deref(),
            Some("https://a.example/")
        );
    }

    #[test]
    fn an_empty_focused_bar_is_a_new_tab_with_no_url() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        // Ctrl+T focuses an empty bar: no page, read twice like any change.
        tracker.record(typing(""), start + secs(1));
        assert!(tracker.url(start + secs(1), false).is_some());
        tracker.record(typing(""), start + secs(2));
        assert_eq!(tracker.url(start + secs(2), false), None);
        tracker.record(typing("b.example"), start + secs(3));
        assert_eq!(tracker.url(start + secs(3), false), None);
    }

    #[test]
    fn text_unrelated_to_the_page_does_not_keep_its_url() {
        let start = Instant::now();
        let mut tracker = showing("https://www.github.com/openrize", start);
        for text in [
            "git",
            "github.com/open",
            "github.com/openrize/issues",
            "other.org",
        ] {
            tracker.record(typing(text), start + secs(1));
            assert!(tracker.url(start + secs(1), false).is_some(), "{text}");
        }
        // A search typed over it, or text that cannot be read, is not an edit
        // of the page; unreadable text is given the benefit of the doubt.
        tracker.record(Reading::Typing(None), start + secs(2));
        assert!(tracker.url(start + secs(2), false).is_some());
        tracker.record(typing("how to rust"), start + secs(3));
        assert!(tracker.url(start + secs(3), false).is_some());
        tracker.record(typing("how to rust"), start + secs(4));
        assert_eq!(tracker.url(start + secs(4), false), None);
    }

    #[test]
    fn a_new_title_replaces_the_url_on_the_next_read() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        tracker.retitle("A");
        tracker.touch(start + secs(1));
        tracker.retitle("A");
        assert!(!tracker.unread(start + secs(1)));
        tracker.touch(start + secs(2));
        tracker.retitle("B");
        assert!(tracker.unread(start + secs(2)));
        assert_eq!(
            tracker.url(start + secs(2), false).as_deref(),
            Some("https://a.example/")
        );
        tracker.record(page("https://b.example/"), start + secs(2));
        assert_eq!(
            tracker.url(start + secs(2), false).as_deref(),
            Some("https://b.example/")
        );
        assert!(!tracker.unread(start + secs(2)));
        tracker.record(page("https://c.example/"), start + secs(3));
        assert_eq!(
            tracker.url(start + secs(3), false).as_deref(),
            Some("https://b.example/")
        );
    }

    #[test]
    fn a_new_title_under_a_focused_bar_drops_the_old_url_at_once() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        tracker.retitle("A");
        tracker.retitle("New Tab");
        tracker.record(typing("a.exam"), start + secs(1));
        assert_eq!(tracker.url(start + secs(1), false), None);
    }

    #[test]
    fn a_lapsed_url_is_replaced_without_a_second_read() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        let later = start + GRACE + secs(5);
        tracker.record(page("https://b.example/"), later);
        assert_eq!(
            tracker.url(later, false).as_deref(),
            Some("https://b.example/")
        );
    }

    #[test]
    fn a_stalled_worker_keeps_the_last_url_longer() {
        let start = Instant::now();
        let tracker = showing("https://a.example/", start);
        let later = start + GRACE + secs(10);
        assert_eq!(tracker.url(later, false), None);
        assert_eq!(
            tracker.url(later, true).as_deref(),
            Some("https://a.example/")
        );
        assert_eq!(tracker.url(start + STALLED_GRACE + secs(1), true), None);
    }

    #[test]
    fn a_window_without_a_bar_is_retried_rarely() {
        let start = Instant::now();
        let mut tracker = UrlTracker::default();
        tracker.touch(start);
        for second in 0..u64::from(MISSES_BEFORE_BACKOFF) {
            assert!(tracker.due(start + secs(second)));
            tracker.record(Reading::Missing, start + secs(second));
        }
        let last = start + secs(u64::from(MISSES_BEFORE_BACKOFF) - 1);
        assert!(!tracker.due(last + secs(1)));
        assert!(!tracker.due(last + BACKOFF - secs(1)));
        assert!(tracker.due(last + BACKOFF));
        tracker.record(Reading::Missing, last + BACKOFF);
        assert!(!tracker.due(last + BACKOFF + secs(1)));
        tracker.record(page("https://a.example/"), last + BACKOFF * 2);
        assert!(tracker.due(last + BACKOFF * 2 + secs(1)));
    }

    #[test]
    fn a_window_asked_about_again_after_a_gap_starts_over() {
        let start = Instant::now();
        let mut tracker = showing("https://a.example/", start);
        assert!(!tracker.unread(start));
        let back = start + GRACE + secs(1);
        tracker.touch(back);
        assert!(tracker.unread(back));
        assert_eq!(tracker.url(back, false), None);
        let mut tracker = showing("https://a.example/", start);
        tracker.touch(start + secs(2));
        assert_eq!(
            tracker.url(start + secs(2), false).as_deref(),
            Some("https://a.example/")
        );
    }

    fn browser_windows() -> Vec<(HWND, String)> {
        use windows::core::BOOL;
        use windows::Win32::Foundation::LPARAM;
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindowVisible};

        unsafe extern "system" fn collect(window: HWND, state: LPARAM) -> BOOL {
            // SAFETY: `state` is the `Vec` passed below, alive for the call.
            let found = unsafe { &mut *(state.0 as *mut Vec<(HWND, String)>) };
            // SAFETY: a plain query on a window handle.
            if unsafe { IsWindowVisible(window) }.as_bool() {
                let path = super::super::process_id(window).and_then(super::super::process_path);
                if let Some(path) = path {
                    let exe = super::super::file_name(&path).to_lowercase();
                    if super::super::BROWSERS.contains(&exe.as_str()) {
                        found.push((window, exe));
                    }
                }
            }
            BOOL(1)
        }

        let mut windows: Vec<(HWND, String)> = Vec::new();
        // SAFETY: `collect` only runs during this call, while `windows` lives.
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize));
        }
        windows
    }

    /// Prints the text fields the search sees in each open browser window and what
    /// reading the picked one gives. Run: `cargo test --lib address_bar::tests::dump -- --ignored
    /// --nocapture`
    #[test]
    #[ignore = "reads the live UI of the browsers that are open"]
    fn dump() {
        let automation = Automation::new().expect("UI Automation");
        for (window, exe) in browser_windows() {
            println!(
                "== {exe} hwnd={:#x} title={:?}",
                window.0 as isize,
                super::super::window_title(window)
            );
            let started = Instant::now();
            let mut seen = 0;
            // SAFETY: COM calls on live objects.
            unsafe {
                automation.walk(window, started + secs(5), &mut |c| {
                    seen += 1;
                    let verdict = classify(c.kind, &c.id, &c.class);
                    if verdict != Match::Not {
                        println!(
                            "  #{seen} {verdict:?} type={} id={:?} class={:?}",
                            c.kind.0, c.id, c.class
                        );
                    }
                    if c.is_page() {
                        Step::Skip
                    } else {
                        Step::Descend
                    }
                });
            }
            println!("  walked {seen} controls in {:?}", started.elapsed());
            let started = Instant::now();
            // SAFETY: as above.
            let reading = unsafe {
                automation
                    .find_bar(window, started + secs(5))
                    .and_then(|bar| automation.read(&bar))
            };
            println!("  find+read took {:?}: {reading:?}", started.elapsed());
        }
    }

    /// Asks `shared_url_of` about each open browser window once a second, as the sampler does.
    /// Run: `cargo test --lib address_bar::tests::live_urls -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the live UI of the browsers that are open"]
    fn live_urls() {
        for (window, exe) in browser_windows() {
            for tick in 0..4 {
                let started = Instant::now();
                let url = shared_url_of(window, None);
                println!(
                    "{exe} {:#x} tick {tick}: {url:?} (sampler waited {:?})",
                    window.0 as isize,
                    started.elapsed()
                );
                std::thread::sleep(secs(1));
            }
        }
    }

    /// Prints each open browser window's executable, title, accessible name and privacy.
    /// Open a private window in each browser first. Run:
    /// `cargo test --lib address_bar::tests::live_private -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the live UI of the browsers that are open"]
    fn live_private() {
        let automation = Automation::new().expect("UI Automation");
        for (window, exe) in browser_windows() {
            let title = super::super::window_title(window);
            if title.is_empty() {
                continue;
            }
            // The sampler's path: the worker learns the window's name on its
            // first reads, so ask a few times as the sampler would.
            let mut private = false;
            for _ in 0..4 {
                let _ = shared_url_of(window, Some(&title));
                std::thread::sleep(Duration::from_millis(400));
                private = is_private_window(window, &exe, &title);
            }
            // SAFETY: a COM call on a live automation object.
            let name = unsafe { automation.window_name(window) };
            println!(
                "{exe} {:#x}\n  title    = {title:?}\n  uia name = {name:?}\n  title says private: {}\n  name says private:  {}\n  classified private: {private}",
                window.0 as isize,
                openrize_core::capture::title_marks_private(&exe, &title),
                name.as_deref()
                    .is_some_and(openrize_core::capture::chromium_name_marks_private),
            );
        }
    }
}
