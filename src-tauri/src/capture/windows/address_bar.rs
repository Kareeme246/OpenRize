//! A browser window's URL, read from its address bar through UI Automation.
//!
//! Every UI Automation call crosses into the browser's process and can block
//! for as long as that process is busy or hung, so none of them run on the
//! activity sampler. A worker thread owns the COM apartment and the
//! automation object; the sampler asks it for a window and takes whatever
//! answer is ready, waiting briefly only the first time a window is seen.
//!
//! What the sampler is told is smoothed by [`UrlTracker`], which holds no
//! COM state: a read that fails keeps the last URL for a grace period, a new
//! URL must be read twice before it replaces the old one, and a window whose
//! bar cannot be found is retried rarely instead of every second.

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
    UIA_HasKeyboardFocusPropertyId, UIA_ValuePatternId, UIA_ValueValuePropertyId,
    UIA_CONTROLTYPE_ID,
};

/// A failed read keeps the last URL this long before the window has none.
const GRACE: Duration = Duration::from_secs(8);
/// While the worker is stuck nothing can be confirmed, so the last URL is
/// kept longer than `GRACE` before it is given up.
const STALLED_GRACE: Duration = Duration::from_secs(30);
/// Reads of a different URL that must agree before it replaces the shown one.
const SWITCH_READS: u8 = 2;
/// Consecutive misses after which a window is only retried every `BACKOFF`.
const MISSES_BEFORE_BACKOFF: u32 = 3;
const BACKOFF: Duration = Duration::from_secs(15);
/// A window the sampler stopped asking about is forgotten after this long.
const FORGET_AFTER: Duration = Duration::from_secs(60);
const MAX_WINDOWS: usize = 8;

/// How long locating the address bar may take before the read is a miss.
const READ_DEADLINE: Duration = Duration::from_millis(800);
/// A read running longer than this is treated as a stuck worker.
const STALL_AFTER: Duration = Duration::from_millis(1500);
/// How long the sampler waits for the first read of a window it has not
/// seen lately, so its first sample already carries the URL.
const FIRST_READ_WAIT: Duration = Duration::from_millis(80);
/// Per-call limits on the browser answering, in milliseconds.
const CONNECTION_TIMEOUT_MS: u32 = 500;
const TRANSACTION_TIMEOUT_MS: u32 = 1000;

/// How far the address bar search may go before giving up on a window.
const SEARCH_MAX_DEPTH: usize = 12;
const SEARCH_MAX_NODES: usize = 600;

/// What one read of a window's address bar found.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Reading {
    /// The bar's text, normalized; `None` when it holds no web page.
    Page(Option<String>),
    /// The user is typing in the bar, so its text is not the page's.
    Typing,
    /// No bar could be found or read.
    Missing,
}

/// The smoothed answer for one browser window. Times are passed in so the
/// whole state machine can be tested without a clock.
#[derive(Debug, Default)]
struct UrlTracker {
    shown: Option<String>,
    /// The last time the bar was read, whatever it held.
    confirmed: Option<Instant>,
    /// A different URL seen on the last reads, and how many agreed.
    pending: Option<(Option<String>, u8)>,
    requested: Option<Instant>,
    misses: u32,
    retry_at: Option<Instant>,
}

impl UrlTracker {
    /// The sampler asked about this window. After a long gap the old URL says
    /// nothing about the window now (or the handle was reused), so it goes.
    fn touch(&mut self, now: Instant) {
        if self
            .requested
            .is_some_and(|last| now.duration_since(last) > GRACE)
        {
            self.shown = None;
            self.confirmed = None;
            self.pending = None;
        }
        self.requested = Some(now);
    }

    /// Whether the worker should read this window now.
    fn due(&self, now: Instant) -> bool {
        self.retry_at.is_none_or(|at| now >= at)
    }

    /// No read has vouched for `shown` lately, so a result is worth waiting for.
    fn unread(&self, now: Instant) -> bool {
        !self.fresh(now, GRACE)
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
            Reading::Typing => self.pending = None,
            Reading::Page(url) => self.see(url, now),
        }
        self.misses = 0;
        self.retry_at = None;
        self.confirmed = Some(now);
    }

    fn see(&mut self, url: Option<String>, now: Instant) {
        // Nothing to protect once the previous URL has lapsed or never was.
        if !self.fresh(now, GRACE) || url == self.shown {
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

/// The URL shown in a browser window, without ever waiting long for it.
pub(super) fn url_of(window: HWND) -> Option<String> {
    static READER: OnceLock<Option<Arc<Shared>>> = OnceLock::new();
    let shared = READER.get_or_init(Shared::start).as_ref()?;
    shared.url_of(window.0 as isize)
}

/// What the sampler and the worker thread share. Windows are keyed by the
/// handle's value, since an `HWND` is not `Send`.
struct Shared {
    state: Mutex<State>,
    /// Signalled when the sampler wants a window read.
    wake: Condvar,
    /// Signalled when the worker finishes a read.
    done: Condvar,
}

#[derive(Default)]
struct State {
    windows: HashMap<isize, UrlTracker>,
    wanted: Option<isize>,
    /// Requests handed to the worker, and the last one it has finished.
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

    fn url_of(&self, window: isize) -> Option<String> {
        let now = Instant::now();
        let mut state = self.lock();
        state.forget_idle(now);
        let stalled = state
            .busy_since
            .is_some_and(|since| now.duration_since(since) > STALL_AFTER);
        let tracker = state.windows.entry(window).or_default();
        tracker.touch(now);
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

    /// The worker thread: reads the window the sampler asked about last.
    fn run(&self) {
        let mut reader = Reader::default();
        loop {
            let (window, ticket) = {
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
                (window, state.issued)
            };
            let reading = reader.read(window);

            let mut state = self.lock();
            state.busy_since = None;
            state.finished = ticket;
            if let Some(tracker) = state.windows.get_mut(&window) {
                tracker.record(reading, Instant::now());
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

/// The worker's UI Automation objects and the address bars it has found.
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
                // The element went away (tab torn off, window rebuilt).
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

struct Automation {
    automation: IUIAutomation,
    /// Properties of the controls the search looks at, fetched with the
    /// children so the search needs one call per parent, not several per child.
    search: IUIAutomationCacheRequest,
    /// The two things read from the bar, fetched in one call.
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
            let value = automation.CreateCacheRequest().ok()?;
            value.AddProperty(UIA_HasKeyboardFocusPropertyId).ok()?;
            value.AddProperty(UIA_ValueValuePropertyId).ok()?;
            value.AddPattern(UIA_ValuePatternId).ok()?;
            let controls = automation.ControlViewCondition().ok()?;
            Some(Self {
                automation,
                search,
                value,
                controls,
            })
        }
    }

    /// The bar's text, or `None` when the element can no longer be read.
    ///
    /// # Safety
    /// `bar` must be a live UI Automation element.
    unsafe fn read(&self, bar: &IUIAutomationElement) -> Option<Reading> {
        unsafe {
            let bar = bar.BuildUpdatedCache(&self.value).ok()?;
            // While the user types in the bar it holds their text, not the page.
            if bar.CachedHasKeyboardFocus().ok()?.as_bool() {
                return Some(Reading::Typing);
            }
            let pattern: IUIAutomationValuePattern =
                bar.GetCachedPatternAs(UIA_ValuePatternId).ok()?;
            let text = pattern.CachedValue().ok()?.to_string();
            Some(Reading::Page(normalize_address(&text)))
        }
    }

    /// The address bar of `window`, searched for without descending into the
    /// page (a Document): walking web content would make Chromium turn on
    /// full accessibility for every tab.
    ///
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

    /// Breadth-first search of `window`'s controls, ending when `visit`
    /// finds something, the tree runs out, or a limit is hit.
    ///
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

/// What a search does with a control it has just met.
enum Step {
    Descend,
    Skip,
    Found,
}

/// One control met during a search, with the properties fetched alongside it.
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

/// What a control in a browser window is to the search for the address bar.
#[derive(Debug, PartialEq, Eq)]
enum Match {
    /// Certainly the address bar.
    Address,
    /// A text field that is certainly not the address bar.
    Other,
    /// A text field of no known kind; the first one stands in when no
    /// control is certainly the address bar.
    Fallback,
    /// Not a text field; the search looks inside it.
    Not,
}

/// Judges a control by identifiers that browsers do not translate, so no
/// language's UI strings are needed:
///
/// - Firefox family (Firefox, Zen, LibreWolf, Floorp): the URL bar's input has
///   the AutomationId `urlbar-input`. It is exposed as a ComboBox, not an
///   Edit, which is why a search for the first Edit never found it.
/// - Chromium family: the omnibox is an Edit of the `OmniboxViewViews` view
///   class (forks prefix it, e.g. `BraveOmniboxViewViews`). The find bar and
///   other search fields are plain `Textfield`s.
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

/// Address bars hide the scheme ("github.com/openrize"), and an empty or
/// search-text bar is no page at all. Only web pages count, as on macOS.
fn normalize_address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    let url = if text.contains("://") {
        text.to_string()
    } else {
        format!("https://{text}")
    };
    let parsed = url::Url::parse(&url).ok()?;
    let web = matches!(parsed.scheme(), "http" | "https");
    let host = parsed.host_str()?;
    (web && (host.contains('.') || host == "localhost")).then_some(url)
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

    /// A tracker read once at `start`, showing `url`.
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
    }

    #[test]
    fn address_bars_are_told_from_other_text_fields() {
        let edit = UIA_EditControlTypeId;
        let combo = UIA_ComboBoxControlTypeId;
        // Firefox family: the URL bar is a combo box with a fixed id.
        assert_eq!(
            classify(combo, "urlbar-input", "urlbar-input textbox-input"),
            Match::Address
        );
        // Chromium family, in any language and under any fork's prefix.
        assert_eq!(
            classify(edit, "view_1017", "OmniboxViewViews"),
            Match::Address
        );
        assert_eq!(classify(edit, "", "BraveOmniboxViewViews"), Match::Address);
        // Find bars and other fields are not the bar.
        assert_eq!(classify(edit, "", "Textfield"), Match::Other);
        assert_eq!(classify(edit, "findbar-textbox", ""), Match::Other);
        assert_eq!(classify(combo, "zoom-menu", ""), Match::Other);
        // An unknown Edit is only a fallback; other controls are walked into.
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
        // The bar coming back with the same page needs no second look.
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
        // Disagreeing candidates do not add up either.
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
            tracker.record(Reading::Typing, start + secs(second));
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
        // Still missing: another long wait. Found: back to every tick.
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
        // A quick return keeps what was known.
        let mut tracker = showing("https://a.example/", start);
        tracker.touch(start + secs(2));
        assert_eq!(
            tracker.url(start + secs(2), false).as_deref(),
            Some("https://a.example/")
        );
    }

    /// Visible top-level windows of the browsers in `BROWSERS`.
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

    /// Prints the text fields the search sees in each open browser window and
    /// what reading the one it picks gives. Run by hand:
    /// `cargo test --lib address_bar::tests::dump -- --ignored --nocapture`
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

    /// Asks `url_of` about each open browser window once a second for a few
    /// seconds, as the sampler does. Run by hand:
    /// `cargo test --lib address_bar::tests::live_urls -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the live UI of the browsers that are open"]
    fn live_urls() {
        for (window, exe) in browser_windows() {
            for tick in 0..4 {
                let started = Instant::now();
                let url = url_of(window);
                println!(
                    "{exe} {:#x} tick {tick}: {url:?} (sampler waited {:?})",
                    window.0 as isize,
                    started.elapsed()
                );
                std::thread::sleep(secs(1));
            }
        }
    }
}
