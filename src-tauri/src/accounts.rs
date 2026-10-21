//! Local accounts, the app side. `openrize_core::accounts` owns the files (the
//! registry, the folders, the move of existing data); this file is what the
//! running app does with them.
//!
//! **A switch is a restart.** Everything the app opens - the store lock,
//! `AppState`, the samplers, the AI worker, the breaks engine, the `rize`
//! endpoint - is opened once at launch from one folder (`lib.rs` `setup`), so
//! switching writes down which account to open, leaves a `Handoff`, and calls
//! `AppHandle::request_restart`, the path the updater already takes. Nothing
//! here teaches a store to change folders or holds a handle across accounts.
//!
//! **Work in progress stays with its account.** A switch pauses every running
//! stopwatch and ends a live break (`pause_for_switch`), and records what it
//! paused in the account's own database under [`SWITCH_PAUSED_KEY`]. The next
//! account sees nothing of it. When the user returns, the banner offers to
//! resume it; nothing ever resumes by itself.
//!
//! **The switch takes at least [`SWITCH_MIN_MS`]**, measured from the command,
//! so the "Switching to..." screen is never a flash.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use openrize_core::accounts::{
    self, Account, Adopted, DataSummary, Handoff, Launch, Registry, WindowBounds,
};
use openrize_core::state::lock_stores;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

use crate::breaks::engine::{Source, SwitchedBreak};
use crate::timers::now_epoch_ms;
use crate::{breaks, commands, AppState, EVENT_ACCOUNTS_CHANGED};

/// How long a switch takes at the least, from the moment it is asked for. A
/// switch that looks instant feels broken.
pub const SWITCH_MIN_MS: u64 = 3_000;

/// Key in the account's database `settings` table: the work its last switch
/// paused (see [`Record`]).
const SWITCH_PAUSED_KEY: &str = "switch_paused";

/// The folder this process opened, managed once at launch.
pub struct ActiveAccount {
    root: PathBuf,
    /// The folder every store was opened in: `root`, or the account's.
    dir: PathBuf,
    /// The open account's id, or `None` for No account.
    id: Option<String>,
    /// The account the registry named whose data was gone, until dismissed.
    missing: Mutex<Option<Account>>,
    /// The launch finished moving No account's data into the account. Read once.
    just_adopted: AtomicBool,
}

impl ActiveAccount {
    pub fn new(root: PathBuf, launch: &Launch) -> Self {
        Self {
            root,
            dir: launch.resolved.dir.clone(),
            id: launch
                .resolved
                .account
                .as_ref()
                .map(|account| account.id.clone()),
            missing: Mutex::new(launch.resolved.missing.clone()),
            just_adopted: AtomicBool::new(launch.adopted == Some(Adopted::Moved)),
        }
    }
}

/// The folder the stores are open in.
pub fn data_dir(app: &AppHandle) -> PathBuf {
    active(app).dir.clone()
}

/// Puts the main window back where the switch found it.
pub fn restore_window(app: &AppHandle, bounds: WindowBounds) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_size(PhysicalSize::new(bounds.width, bounds.height));
        let _ = window.set_position(PhysicalPosition::new(bounds.x, bounds.y));
    }
}

/// Whether the main window is showing, and where. The position is the outer
/// one and the size the inner one, the pair `restore_window` sets, so a switch
/// never nudges the window. A maximized, full-screen or minimized window has
/// no bounds to put back.
fn main_window(app: &AppHandle) -> (bool, Option<WindowBounds>) {
    let Some(window) = app.get_webview_window("main") else {
        return (false, None);
    };
    let visible = window.is_visible().unwrap_or(false);
    let ordinary = !window.is_maximized().unwrap_or(false)
        && !window.is_fullscreen().unwrap_or(false)
        && !window.is_minimized().unwrap_or(false);
    let bounds = match (window.outer_position(), window.inner_size()) {
        (Ok(position), Ok(size)) if ordinary => Some(WindowBounds {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }),
        _ => None,
    };
    (visible, bounds)
}

fn active(app: &AppHandle) -> tauri::State<'_, ActiveAccount> {
    app.state::<ActiveAccount>()
}

fn changed(app: &AppHandle) {
    let _ = app.emit(EVENT_ACCOUNTS_CHANGED, ());
}

// --- What the frontend lists ------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub size_bytes: u64,
    /// Listed, but its data folder is gone.
    pub missing: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsState {
    /// The open account's id, or `None` for No account.
    pub current: Option<String>,
    pub accounts: Vec<AccountView>,
    /// The account this launch could not open because its data is gone.
    pub missing: Option<Account>,
    /// This launch finished moving No account's data into the account. True
    /// once: the first read clears it.
    pub just_adopted: bool,
    /// A newer release wrote the accounts; they can be seen but not changed.
    pub read_only: bool,
}

#[tauri::command]
pub fn accounts_state(app: AppHandle) -> AccountsState {
    let active = active(&app);
    let registry = Registry::peek(&active.root);
    let accounts = registry
        .accounts
        .iter()
        .map(|account| AccountView {
            id: account.id.clone(),
            name: account.name.clone(),
            created_at: account.created_at,
            size_bytes: accounts::folder_size(&accounts::data_dir(&active.root, Some(&account.id))),
            missing: !accounts::is_healthy(&active.root, &account.id),
        })
        .collect();
    AccountsState {
        current: active.id.clone(),
        accounts,
        missing: active
            .missing
            .lock()
            .ok()
            .and_then(|missing| missing.clone()),
        just_adopted: active.just_adopted.swap(false, Ordering::SeqCst),
        read_only: registry.is_read_only(),
    }
}

/// What No account holds, for the question of moving it into a new account.
/// `None` when an account is open, or when there is nothing to move.
#[tauri::command]
pub fn existing_data_summary(app: AppHandle) -> Result<Option<DataSummary>, String> {
    if active(&app).id.is_some() {
        return Ok(None);
    }
    let state = app.state::<AppState>();
    let conn = state.activity_reader.lock().map_err(|e| e.to_string())?;
    let summary = accounts::summarize(&conn);
    Ok((!summary.is_empty()).then_some(summary))
}

#[derive(Debug, Serialize)]
pub struct Labeled {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakPreview {
    pub label: String,
    pub remaining_ms: u64,
}

/// What a switch right now would pause.
#[derive(Debug, Serialize)]
pub struct SwitchPreview {
    pub stopwatches: Vec<Labeled>,
    #[serde(rename = "break")]
    pub break_: Option<BreakPreview>,
}

#[tauri::command]
pub fn switch_preview(app: AppHandle) -> Result<SwitchPreview, String> {
    let timers = app
        .state::<AppState>()
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .snapshot()?;
    Ok(SwitchPreview {
        stopwatches: timers
            .into_iter()
            .filter(|timer| timer.is_running())
            .map(|timer| Labeled {
                id: timer.id,
                label: timer.label,
            })
            .collect(),
        break_: breaks::running_break(&app).map(|running| BreakPreview {
            label: running.label,
            remaining_ms: running.remaining_ms,
        }),
    })
}

// --- Switching --------------------------------------------------------------

/// Writes the handoff the next launch reads, with the window as it is now.
fn write_handoff(app: &AppHandle, to: Option<&str>, adopt: bool) -> Result<(), String> {
    let (show, window) = main_window(app);
    Handoff {
        to: to.map(str::to_string),
        adopt,
        show,
        window,
    }
    .write(&active(app).root)
}

/// Restarts once [`SWITCH_MIN_MS`] has passed since `started`. The frontend
/// has the "Switching" screen up until then, and the command has already
/// returned, so it stays painted.
fn restart_when_due(app: AppHandle, started: Instant, to: Option<String>, adopt: bool) {
    std::thread::spawn(move || {
        if let Some(rest) = Duration::from_millis(SWITCH_MIN_MS).checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
        if !adopt {
            // Whatever was started during the wait (the tray, `rize`) pauses
            // too, so nothing keeps counting against data the next account
            // cannot see. A move pauses nothing: the work goes with its data.
            if let Err(error) = pause_for_switch(&app) {
                eprintln!("could not pause the work before switching: {error}");
            }
        }
        // The window may have moved while the screen was up. The handoff
        // written when the switch was asked for still stands if this fails.
        if let Err(error) = write_handoff(&app, to.as_deref(), adopt) {
            eprintln!("could not update the handoff: {error}");
        }
        app.request_restart();
    });
}

fn editable(root: &Path) -> Result<(), String> {
    if Registry::peek(root).is_read_only() {
        return Err(
            "These accounts were made by a newer version of OpenRize. Update OpenRize to change them."
                .to_string(),
        );
    }
    Ok(())
}

/// Opens another account, or No account for `None` (Sign out), by restarting.
/// Returns at once; the restart follows after the minimum time.
#[tauri::command]
pub fn switch_account(app: AppHandle, to: Option<String>) -> Result<(), String> {
    let started = Instant::now();
    let (root, current) = {
        let active = active(&app);
        (active.root.clone(), active.id.clone())
    };
    if to == current {
        return Ok(());
    }
    editable(&root)?;
    if let Some(id) = &to {
        if !accounts::is_healthy(&root, id) {
            return Err("That account's data folder is missing.".to_string());
        }
    }
    pause_for_switch(&app)?;
    accounts::set_current(&root, to.as_deref())?;
    write_handoff(&app, to.as_deref(), false)?;
    restart_when_due(app, started, to, false);
    Ok(())
}

/// Adds an account and opens it by restarting. With `move_existing`, No
/// account's data moves into it (renamed, not copied) as the next launch
/// starts; that only makes sense from No account.
#[tauri::command]
pub fn create_account(
    app: AppHandle,
    name: String,
    move_existing: bool,
) -> Result<Account, String> {
    let started = Instant::now();
    let (root, current) = {
        let active = active(&app);
        (active.root.clone(), active.id.clone())
    };
    editable(&root)?;
    if move_existing && current.is_some() {
        return Err("Only the data with no account can be moved into a new account.".to_string());
    }
    if move_existing {
        let account = accounts::register(&root, &name)?;
        if let Err(error) = write_handoff(&app, Some(&account.id), true) {
            let _ = accounts::remove_entry(&root, &account.id);
            return Err(error);
        }
        restart_when_due(app, started, Some(account.id.clone()), true);
        return Ok(account);
    }
    let account = accounts::create(&root, &name)?;
    let opened = pause_for_switch(&app)
        .and_then(|()| accounts::set_current(&root, Some(&account.id)))
        .and_then(|()| write_handoff(&app, Some(&account.id), false));
    if let Err(error) = opened {
        // Nothing opened it, so it would only be clutter.
        let _ = accounts::remove_entry(&root, &account.id);
        return Err(error);
    }
    restart_when_due(app, started, Some(account.id.clone()), false);
    Ok(account)
}

#[tauri::command]
pub fn rename_account(app: AppHandle, id: String, name: String) -> Result<Account, String> {
    let account = accounts::rename(&active(&app).root, &id, &name)?;
    changed(&app);
    Ok(account)
}

/// Moves an account's folder to the Trash and forgets it. An account whose
/// folder is already gone is only forgotten. Never deletes for good.
#[tauri::command]
pub fn delete_account(app: AppHandle, id: String) -> Result<(), String> {
    let root = active(&app).root.clone();
    if active(&app).id.as_deref() == Some(id.as_str()) {
        return Err("Switch to another account first.".to_string());
    }
    editable(&root)?;
    if Registry::peek(&root).get(&id).is_none() {
        return Err(format!("There is no account {id}."));
    }
    let dir = accounts::data_dir(&root, Some(&id));
    if dir.exists() {
        // `rize` holds the folder's lock while it edits an account's data.
        match lock_stores(&dir, false).map_err(|error| error.to_string())? {
            Some(lock) => drop(lock),
            None => {
                return Err(
                    "rize is working on this account right now. Try again in a moment.".to_string(),
                )
            }
        }
        move_to_trash(&dir)?;
    }
    accounts::remove_entry(&root, &id)?;
    changed(&app);
    Ok(())
}

/// The Trash, or an error: there is no fallback to deleting for good.
fn move_to_trash(path: &Path) -> Result<(), String> {
    #[allow(unused_mut)]
    let mut context = trash::TrashContext::default();
    // Finder, the default, asks for permission to control it and plays its
    // sound; this one needs neither.
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context
        .delete(path)
        .map_err(|error| format!("Couldn't move the account to the Trash: {error}"))
}

/// The user saw the missing-account dialog and chose to keep it listed.
#[tauri::command]
pub fn dismiss_missing_account(app: AppHandle) {
    if let Ok(mut missing) = active(&app).missing.lock() {
        *missing = None;
    }
    changed(&app);
}

// --- Work paused by a switch ------------------------------------------------

/// What a switch paused, kept in the account's own database as JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    /// When the latest switch away paused something.
    at: u64,
    /// Stopwatches that were running.
    #[serde(default)]
    stopwatches: Vec<String>,
    #[serde(default, rename = "break")]
    break_: Option<RecordedBreak>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordedBreak {
    /// When the break ended; it can be taken up again that day only.
    at: u64,
    label: String,
    source: String,
    schedule_id: Option<String>,
    remaining_ms: u64,
    /// The stopwatches (id, label) the break had paused.
    paused_stopwatches: Vec<(String, String)>,
}

impl Record {
    fn is_empty(&self) -> bool {
        self.stopwatches.is_empty() && self.break_.is_none()
    }

    /// Adds what a later switch away paused, when the first was never resumed.
    fn merge(&mut self, later: Record) {
        self.at = later.at;
        for id in later.stopwatches {
            if !self.stopwatches.contains(&id) {
                self.stopwatches.push(id);
            }
        }
        if later.break_.is_some() {
            self.break_ = later.break_;
        }
    }
}

impl RecordedBreak {
    fn from_ended(ended: SwitchedBreak, at: u64) -> Self {
        Self {
            at,
            label: ended.label,
            source: ended.source.as_str().to_string(),
            schedule_id: ended.schedule_id,
            remaining_ms: ended.remaining_ms,
            paused_stopwatches: ended.paused,
        }
    }

    fn into_ended(self) -> SwitchedBreak {
        SwitchedBreak {
            label: self.label,
            source: Source::parse(&self.source).unwrap_or(Source::Manual),
            schedule_id: self.schedule_id,
            remaining_ms: self.remaining_ms,
            paused: self.paused_stopwatches,
        }
    }

    /// A break ended earlier in the day has run its course by the next.
    fn resumable(&self, now: u64) -> bool {
        self.remaining_ms > 0 && breaks::same_local_day(self.at, now)
    }
}

fn read_record(app: &AppHandle) -> Option<Record> {
    let value = app
        .state::<AppState>()
        .activity
        .lock()
        .ok()?
        .read_setting(SWITCH_PAUSED_KEY)?;
    serde_json::from_str(&value).ok()
}

fn write_record(app: &AppHandle, record: &Record) -> Result<(), String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    if record.is_empty() {
        return store
            .conn()
            .execute("DELETE FROM settings WHERE key = ?1", [SWITCH_PAUSED_KEY])
            .map(|_| ())
            .map_err(|e| e.to_string());
    }
    let json = serde_json::to_string(record).map_err(|e| e.to_string())?;
    store.write_setting(SWITCH_PAUSED_KEY, &json)
}

/// Pauses everything running in the open account and records it there: every
/// running stopwatch is paused the ordinary way (its run becomes a timer
/// segment) and a live break ends now, its own stopwatches staying paused.
/// Asked again after a switch away that was never resumed, it adds to the
/// record. Does nothing when nothing is running.
pub fn pause_for_switch(app: &AppHandle) -> Result<(), String> {
    let now = now_epoch_ms();
    let ended = breaks::pause_for_switch(app);
    let mut paused = Record {
        at: now,
        stopwatches: breaks::pause_running_timers(app)
            .into_iter()
            .map(|(id, _)| id)
            .collect(),
        break_: None,
    };
    if let Some(ended) = ended {
        if ended.remaining_ms > 0 {
            paused.break_ = Some(RecordedBreak::from_ended(ended, now));
        } else {
            // Over already: only its stopwatches are left to take up.
            paused
                .stopwatches
                .extend(ended.paused.into_iter().map(|(id, _)| id));
        }
    }
    if paused.is_empty() {
        return Ok(());
    }
    let mut record = read_record(app).unwrap_or_default();
    record.merge(paused);
    write_record(app, &record)
}

/// A stopwatch started by hand is no longer paused work to take up.
pub fn forget_paused_stopwatch(app: &AppHandle, id: &str) {
    let Some(mut record) = read_record(app) else {
        return;
    };
    let before = record.clone();
    record.stopwatches.retain(|paused| paused != id);
    if let Some(recorded) = &mut record.break_ {
        recorded
            .paused_stopwatches
            .retain(|(paused, _)| paused != id);
    }
    if record != before {
        if let Err(error) = write_record(app, &record) {
            eprintln!("could not update the paused work: {error}");
        }
    }
}

/// What is still worth offering: stopwatches that exist and are not running,
/// and a break only while it can be taken up.
struct Offer {
    at: u64,
    stopwatches: Vec<Labeled>,
    ended: Option<SwitchedBreak>,
}

fn offer(app: &AppHandle) -> Result<Option<Offer>, String> {
    let Some(record) = read_record(app) else {
        return Ok(None);
    };
    let timers = app
        .state::<AppState>()
        .store
        .lock()
        .map_err(|e| e.to_string())?
        .snapshot()?;
    let now = now_epoch_ms();
    let stopwatches: Vec<Labeled> = record
        .stopwatches
        .iter()
        .filter_map(|id| timers.iter().find(|timer| &timer.id == id))
        .filter(|timer| !timer.is_running())
        .map(|timer| Labeled {
            id: timer.id.clone(),
            label: timer.label.clone(),
        })
        .collect();
    let ended = record
        .break_
        .filter(|recorded| recorded.resumable(now))
        .map(RecordedBreak::into_ended);
    if stopwatches.is_empty() && ended.is_none() {
        // Nothing left to offer; don't ask again.
        write_record(app, &Record::default())?;
        return Ok(None);
    }
    Ok(Some(Offer {
        at: record.at,
        stopwatches,
        ended,
    }))
}

/// The banner's content.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PausedOnSwitch {
    pub at: u64,
    pub stopwatches: Vec<Labeled>,
    #[serde(rename = "break")]
    pub break_: Option<BreakPreview>,
}

#[tauri::command]
pub fn paused_on_switch(app: AppHandle) -> Result<Option<PausedOnSwitch>, String> {
    Ok(offer(&app)?.map(|offer| PausedOnSwitch {
        at: offer.at,
        stopwatches: offer.stopwatches,
        break_: offer.ended.map(|ended| BreakPreview {
            label: ended.label,
            remaining_ms: ended.remaining_ms,
        }),
    }))
}

/// Takes up the paused stopwatches and the break, from now.
#[tauri::command]
pub fn resume_paused(app: AppHandle) -> Result<(), String> {
    let Some(offer) = offer(&app)? else {
        return Ok(());
    };
    // Cleared first: whatever happens below, it is not offered twice.
    write_record(&app, &Record::default())?;
    for stopwatch in &offer.stopwatches {
        // A stopwatch deleted in the meantime is simply gone.
        let _ = commands::start_timer_by_id(&app, &stopwatch.id);
    }
    match &offer.ended {
        Some(ended) => breaks::resume_switched_break(&app, ended),
        None => Ok(()),
    }
}

/// Keeps everything paused and stops offering.
#[tauri::command]
pub fn dismiss_paused(app: AppHandle) -> Result<(), String> {
    write_record(&app, &Record::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded(at: u64, remaining_ms: u64) -> RecordedBreak {
        RecordedBreak {
            at,
            label: "Lunch".into(),
            source: "scheduled".into(),
            schedule_id: Some("lunch".into()),
            remaining_ms,
            paused_stopwatches: vec![("t2".into(), "Design".into())],
        }
    }

    #[test]
    fn a_repeated_switch_away_adds_to_the_record_instead_of_replacing_it() {
        let mut record = Record {
            at: 100,
            stopwatches: vec!["t1".into()],
            break_: Some(recorded(100, 8 * 60_000)),
        };
        record.merge(Record {
            at: 200,
            stopwatches: vec!["t1".into(), "t3".into()],
            break_: None,
        });
        assert_eq!(record.at, 200);
        assert_eq!(record.stopwatches, ["t1", "t3"]);
        // A break paused earlier is still offered; it keeps its own time.
        assert_eq!(record.break_.as_ref().map(|b| b.at), Some(100));

        record.merge(Record {
            at: 300,
            stopwatches: Vec::new(),
            break_: Some(recorded(300, 60_000)),
        });
        assert_eq!(
            record.break_.map(|b| (b.at, b.remaining_ms)),
            Some((300, 60_000))
        );
    }

    #[test]
    fn the_record_reads_back_as_written_and_holds_only_what_it_needs() {
        let record = Record {
            at: 5,
            stopwatches: vec!["t1".into()],
            break_: Some(recorded(5, 1)),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), record);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["at", "break", "stopwatches"]);
        // A record from before it had a break still reads.
        let older: Record = serde_json::from_str(r#"{"at": 1}"#).unwrap();
        assert!(older.is_empty());
    }

    #[test]
    fn a_paused_break_is_resumable_on_the_same_local_day_only() {
        let hour = 60 * 60 * 1000;
        let at = 1_790_000_000_000;
        let paused = recorded(at, 8 * 60_000);
        assert!(paused.resumable(at + 1_000));
        assert!(!paused.resumable(at + 48 * hour));
        // A break with no time left is not offered at all.
        assert!(!recorded(at, 0).resumable(at));
    }

    #[test]
    fn a_break_survives_the_record_with_its_source_and_stopwatches() {
        let ended = recorded(5, 7).into_ended();
        assert_eq!(ended.source, Source::Scheduled);
        assert_eq!(ended.paused, [("t2".to_string(), "Design".to_string())]);
        let again = RecordedBreak::from_ended(ended, 5);
        assert_eq!(again, recorded(5, 7));
    }
}
