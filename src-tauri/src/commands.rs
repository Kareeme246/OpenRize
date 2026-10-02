//! IPC surface.

use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::activity::{self, ActivitySnapshot};
use crate::ai::metrics::AiMetrics;
use crate::ai::{self, AiRuntime, AiStatus};
use crate::breaks;
use crate::invoices::{self, profile, BillableEntry, DraftInput, Invoice, InvoiceSummary};
use crate::login_item::{self, LoginItemState};
use crate::models::{
    AppRecord, Category, Client, EntryDetail, NewCategory, NewClient, NewProject, NewTimeEntry,
    Project, RuleSuggestion, TimeEntry, UpdateCategory, UpdateClient, UpdateProject,
    UpdateTimeEntry,
};
use crate::projects::{HintPreview, ImportSummary, ProjectRule, ProjectStats, ProjectSuggestion};
use crate::reports::{self, EntryFilter, ExportFormat, ExportResult, GroupBy, RollupCell};
use crate::settings::Settings;
use crate::timers::{now_epoch_ms, Timer};
use crate::tray;
use crate::updater::{self, UpdateStatus, UpdaterState};
use crate::AppState;

type Timers = Result<Vec<Timer>, String>;

pub(crate) fn refresh_tray(app: &AppHandle, timers: &[Timer]) {
    if let Err(error) = tray::refresh(app, timers) {
        eprintln!("tray refresh failed: {error}");
    }
}

// --- Timers -------------------------------------------------------------

#[tauri::command]
pub fn list_timers(app: AppHandle) -> Timers {
    let state = app.state::<AppState>();
    let store = state.store.lock().map_err(|error| error.to_string())?;
    store.snapshot()
}

#[tauri::command]
pub fn create_timer(app: AppHandle, label: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.create(&label)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn start_timer(app: AppHandle, id: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.start(&id)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn pause_timer(app: AppHandle, id: String) -> Timers {
    pause_timer_by_id(&app, &id)
}

pub fn pause_timer_by_id(app: &AppHandle, id: &str) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.pause(id)?
    };
    refresh_tray(app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn reset_timer(app: AppHandle, id: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.reset(&id)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn rename_timer(app: AppHandle, id: String, label: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.rename(&id, &label)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

#[tauri::command]
pub fn delete_timer(app: AppHandle, id: String) -> Timers {
    let timers = {
        let state = app.state::<AppState>();
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.delete(&id)?
    };
    refresh_tray(&app, &timers);
    Ok(timers)
}

// --- Activity capture ---------------------------------------------------

#[tauri::command]
pub fn activity_snapshot(app: AppHandle, since_ms: u64) -> Result<ActivitySnapshot, String> {
    activity::snapshot_for(&app, since_ms)
}

#[tauri::command]
pub fn set_capture_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.set_capture_enabled(enabled, crate::timers::now_epoch_ms())?;
    }
    activity::emit_full(&app);
    Ok(())
}

#[tauri::command]
pub fn set_idle_threshold(app: AppHandle, minutes: u64) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.set_idle_threshold_ms(minutes.saturating_mul(60_000))?;
    }
    activity::emit_full(&app);
    Ok(())
}

#[tauri::command]
pub fn start_session(app: AppHandle, kind: String, label: Option<String>) -> Result<(), String> {
    let now = now_epoch_ms();
    let changed = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.start_session(&kind, label.as_deref(), now)?;
        store.rebuild_range(
            now.saturating_sub(crate::activity::REBUILD_WINDOW_MS),
            now,
            now,
        )?
    };
    activity::emit_full(&app);
    if changed {
        entries_changed(&app);
    }
    crate::ai::nudge(&app);
    Ok(())
}

#[tauri::command]
pub fn stop_session(app: AppHandle) -> Result<(), String> {
    let now = now_epoch_ms();
    let changed = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.stop_session(now)?;
        store.rebuild_range(
            now.saturating_sub(crate::activity::REBUILD_WINDOW_MS),
            now,
            now,
        )?
    };
    activity::emit_full(&app);
    if changed {
        entries_changed(&app);
    }
    crate::ai::nudge(&app);
    Ok(())
}

#[tauri::command]
pub fn mark_segment_reviewed(app: AppHandle, id: i64) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        store.mark_reviewed(id)?;
    }
    activity::emit_full(&app);
    Ok(())
}

// --- P1: Categories -----------------------------------------------------

#[tauri::command]
pub fn list_categories(app: AppHandle) -> Result<Vec<Category>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_categories()
}

#[tauri::command]
pub fn create_category(app: AppHandle, category: NewCategory) -> Result<Category, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.create_category(category, now)
}

#[tauri::command]
pub fn update_category(
    app: AppHandle,
    id: String,
    patch: UpdateCategory,
) -> Result<Category, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.update_category(&id, patch, now)
}

#[tauri::command]
pub fn delete_category(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.delete_category(&id, now)
}

// --- P1: Projects -------------------------------------------------------

#[tauri::command]
pub fn list_projects(app: AppHandle) -> Result<Vec<Project>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_projects()
}

#[tauri::command]
pub fn create_project(app: AppHandle, project: NewProject) -> Result<Project, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.create_project(project, now)
}

#[tauri::command]
pub fn update_project(app: AppHandle, id: String, patch: UpdateProject) -> Result<Project, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.update_project(&id, patch, now)
}

#[tauri::command]
pub fn delete_project(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.delete_project(&id, now)
}

// --- P1: Clients --------------------------------------------------------

#[tauri::command]
pub fn list_clients(app: AppHandle) -> Result<Vec<Client>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_clients()
}

#[tauri::command]
pub fn create_client(app: AppHandle, client: NewClient) -> Result<Client, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.create_client(client, now)
}

#[tauri::command]
pub fn update_client(app: AppHandle, id: String, patch: UpdateClient) -> Result<Client, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.update_client(&id, patch, now)
}

#[tauri::command]
pub fn delete_client(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.delete_client(&id, now)
}

// --- Invoices -----------------------------------------------------------

/// Runs an invoice mutation on the writer connection.
fn with_writer<T>(
    app: &AppHandle,
    run: impl FnOnce(&mut rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    run(store.conn_mut())
}

#[tauri::command]
pub fn list_invoices(app: AppHandle) -> Result<Vec<InvoiceSummary>, String> {
    with_reader(&app, invoices::list)
}

#[tauri::command]
pub fn get_invoice(app: AppHandle, id: String) -> Result<Invoice, String> {
    with_reader(&app, |conn| invoices::get(conn, &id))
}

#[tauri::command]
pub fn list_billable_entries(
    app: AppHandle,
    client_id: String,
    start_ms: u64,
    end_ms: u64,
    invoice_id: Option<String>,
) -> Result<Vec<BillableEntry>, String> {
    with_reader(&app, |conn| {
        invoices::billable_entries(conn, &client_id, start_ms, end_ms, invoice_id.as_deref())
    })
}

/// The draft resolved and priced, without storing it.
#[tauri::command]
pub fn quote_invoice(app: AppHandle, draft: DraftInput) -> Result<Invoice, String> {
    with_reader(&app, |conn| invoices::quote(conn, &draft))
}

/// The DRAFT-watermarked PDF bytes for an unsaved or saved draft.
#[tauri::command]
pub fn render_invoice_preview(app: AppHandle, draft: DraftInput) -> Result<Response, String> {
    with_reader(&app, |conn| invoices::render_draft(conn, &draft)).map(Response::new)
}

/// The archived PDF bytes of a finalized invoice.
#[tauri::command]
pub fn get_invoice_pdf(app: AppHandle, id: String) -> Result<Response, String> {
    with_reader(&app, |conn| invoices::stored_pdf(conn, &id)).map(Response::new)
}

#[tauri::command]
pub fn save_invoice_draft(app: AppHandle, draft: DraftInput) -> Result<Invoice, String> {
    let saved = with_writer(&app, |conn| {
        invoices::save_draft(conn, &draft, now_epoch_ms())
    })?;
    entries_changed(&app);
    Ok(saved)
}

#[tauri::command]
pub fn finalize_invoice(app: AppHandle, id: String) -> Result<Invoice, String> {
    let done = with_writer(&app, |conn| invoices::finalize(conn, &id, now_epoch_ms()))?;
    entries_changed(&app);
    Ok(done)
}

#[tauri::command]
pub fn set_invoice_paid(app: AppHandle, id: String, paid: bool) -> Result<(), String> {
    with_writer(&app, |conn| {
        invoices::set_paid(conn, &id, paid, now_epoch_ms())
    })?;
    entries_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn void_invoice(app: AppHandle, id: String) -> Result<(), String> {
    with_writer(&app, |conn| invoices::void(conn, &id, now_epoch_ms()))?;
    entries_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_draft_invoice(app: AppHandle, id: String) -> Result<(), String> {
    with_writer(&app, |conn| invoices::delete_draft(conn, &id))?;
    entries_changed(&app);
    Ok(())
}

/// Asks where to save an invoice PDF and writes it: a finalized invoice's
/// archived bytes (`id`), or a fresh DRAFT render of `draft`. Returns the saved
/// path, or `None` when the user cancels.
#[tauri::command]
pub async fn export_invoice_pdf(
    app: AppHandle,
    id: Option<String>,
    draft: Option<DraftInput>,
) -> Result<Option<String>, String> {
    let (bytes, name) = with_reader(&app, |conn| match (&id, &draft) {
        (Some(id), _) => {
            let invoice = invoices::get(conn, id)?;
            let name = invoices::file_name(
                invoice.summary.number.as_deref(),
                &invoice.summary.client_name,
            );
            Ok((invoices::stored_pdf(conn, id)?, name))
        }
        (None, Some(draft)) => Ok((
            invoices::render_draft(conn, draft)?,
            invoices::file_name(None, &draft.bill_to_name),
        )),
        (None, None) => Err("Nothing to export".to_string()),
    })?;
    let Some(picked) = app
        .dialog()
        .file()
        .set_file_name(&name)
        .add_filter("PDF", &["pdf"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let mut path = picked.into_path().map_err(|e| e.to_string())?;
    if path.extension().is_none() {
        path.set_extension("pdf");
    }
    let temp = path.with_extension("pdf.openrize-tmp");
    std::fs::write(&temp, &bytes).map_err(|e| format!("Could not save the PDF: {e}"))?;
    std::fs::rename(&temp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("Could not save the PDF: {e}")
    })?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn get_invoice_profile(app: AppHandle) -> Result<profile::InvoiceProfile, String> {
    with_reader(&app, profile::get)
}

#[tauri::command]
pub fn update_invoice_profile(
    app: AppHandle,
    profile: profile::ProfileInput,
) -> Result<profile::InvoiceProfile, String> {
    with_writer(&app, |conn| profile::update(conn, profile, now_epoch_ms()))
}

/// The logo image bytes (PNG), or an empty response when none is set.
#[tauri::command]
pub fn get_invoice_logo(app: AppHandle) -> Result<Response, String> {
    with_reader(&app, |conn| {
        Ok(profile::logo_bytes(conn)?.unwrap_or_default())
    })
    .map(Response::new)
}

/// Stores the raw bytes of the request body as the logo (PNG or JPEG in).
#[tauri::command]
pub fn set_invoice_logo(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("Expected the logo image bytes".into());
    };
    with_writer(&app, |conn| profile::set_logo(conn, bytes, now_epoch_ms()))
}

#[tauri::command]
pub fn clear_invoice_logo(app: AppHandle) -> Result<(), String> {
    with_writer(&app, |conn| profile::clear_logo(conn, now_epoch_ms()))
}

// --- P1: Time Entries ---------------------------------------------------

/// Tells views to refetch, and wakes the AI worker: an entry mutation can
/// queue work (an approval queues its embedding, a rebuild a classification).
pub(crate) fn entries_changed(app: &AppHandle) {
    let _ = app.emit(crate::EVENT_ENTRIES_CHANGED, ());
    ai::nudge(app);
}

#[tauri::command]
pub fn list_time_entries(
    app: AppHandle,
    start_ms: u64,
    end_ms: u64,
) -> Result<Vec<TimeEntry>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_time_entries(start_ms, end_ms)
}

#[tauri::command]
pub fn get_entry_detail(app: AppHandle, id: String) -> Result<EntryDetail, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.get_entry_detail(&id)
}

#[tauri::command]
pub fn update_time_entry(
    app: AppHandle,
    id: String,
    patch: UpdateTimeEntry,
) -> Result<TimeEntry, String> {
    let now = now_epoch_ms();
    let entry = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.update_time_entry(&id, patch, now)?
    };
    entries_changed(&app);
    Ok(entry)
}

/// Returns the approved entries, so a caller adopts them without a refetch.
#[tauri::command]
pub fn approve_time_entries(app: AppHandle, ids: Vec<String>) -> Result<Vec<TimeEntry>, String> {
    let now = now_epoch_ms();
    let entries = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.approve_time_entries(&ids, "user", now)?;
        store.time_entries(&ids)?
    };
    entries_changed(&app);
    Ok(entries)
}

/// Returns the entries, back to pending, so a caller adopts them without a
/// refetch.
#[tauri::command]
pub fn unapprove_time_entries(app: AppHandle, ids: Vec<String>) -> Result<Vec<TimeEntry>, String> {
    let now = now_epoch_ms();
    let entries = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.unapprove_time_entries(&ids, now)?;
        store.time_entries(&ids)?
    };
    entries_changed(&app);
    Ok(entries)
}

/// One patch applied to many entries (My Timesheet's bulk bar). Returns the
/// updated entries.
#[tauri::command]
pub fn update_time_entries(
    app: AppHandle,
    ids: Vec<String>,
    patch: UpdateTimeEntry,
) -> Result<Vec<TimeEntry>, String> {
    let now = now_epoch_ms();
    let entries = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        let mut updated = Vec::with_capacity(ids.len());
        for id in &ids {
            updated.push(store.update_time_entry(id, patch.clone(), now)?);
        }
        updated
    };
    entries_changed(&app);
    Ok(entries)
}

#[tauri::command]
pub fn reject_time_entry(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.reject_time_entry(&id, now)?;
    }
    entries_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn split_time_entry(
    app: AppHandle,
    id: String,
    at_ms: u64,
) -> Result<(TimeEntry, TimeEntry), String> {
    let now = now_epoch_ms();
    let res = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.split_time_entry(&id, at_ms, now)?
    };
    entries_changed(&app);
    Ok(res)
}

#[tauri::command]
pub fn delete_time_entry(app: AppHandle, id: String) -> Result<(), String> {
    delete_time_entries(app, vec![id])
}

#[tauri::command]
pub fn delete_time_entries(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    let now = now_epoch_ms();
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.delete_time_entries(&ids, now)?;
    }
    entries_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn create_time_entry(app: AppHandle, entry: NewTimeEntry) -> Result<TimeEntry, String> {
    let now = now_epoch_ms();
    let created = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.create_manual_entry(entry, now)?
    };
    entries_changed(&app);
    Ok(created)
}

#[tauri::command]
pub fn rebuild_time_entries(
    app: AppHandle,
    start_ms: u64,
    end_ms: u64,
) -> Result<Vec<TimeEntry>, String> {
    let now = now_epoch_ms();
    let entries = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.rebuild_time_entries_in_range(start_ms, end_ms, now)?
    };
    // A past day may never have had its agent time written: recompute the
    // days the range touches (the sessions it just rebuilt are its basis).
    app.state::<AppState>()
        .refresh_agent_days(start_ms, end_ms, now);
    entries_changed(&app);
    Ok(entries)
}

// --- Agents -------------------------------------------------------------

/// The extensions with fresh detection. Asking also nudges the bridge to pick
/// up a tool that was just installed.
#[tauri::command]
pub fn list_extensions(app: AppHandle) -> Vec<crate::agents::ExtensionStatus> {
    crate::agents::recheck(&app);
    crate::agents::extensions(&app)
}

#[tauri::command]
pub fn agent_board(app: AppHandle) -> Result<crate::agents::Board, String> {
    crate::agents::board(&app)
}

/// Jobs, counted agent time and what was left out, for `[start_ms, end_ms)`.
#[tauri::command]
pub fn agent_report(
    app: AppHandle,
    start_ms: u64,
    end_ms: u64,
) -> Result<crate::agents::ledger::Report, String> {
    let state = app.state::<AppState>();
    let reader = state
        .activity_reader
        .lock()
        .map_err(|_| "reader lock poisoned".to_string())?;
    crate::agents::ledger::report(&reader, start_ms, end_ms, now_epoch_ms())
}

/// Each day's threads (per-project visits, agent rails, focus stats) for the
/// days between consecutive `boundaries`.
#[tauri::command]
pub fn thread_days(
    app: AppHandle,
    boundaries: Vec<u64>,
) -> Result<Vec<crate::threads::DayThreads>, String> {
    if boundaries.len() < 2 || boundaries.len() > crate::threads::MAX_DAYS + 1 {
        return Err(format!(
            "expected 2 to {} day boundaries",
            crate::threads::MAX_DAYS + 1
        ));
    }
    let state = app.state::<AppState>();
    let reader = state
        .activity_reader
        .lock()
        .map_err(|_| "reader lock poisoned".to_string())?;
    let now = now_epoch_ms();
    boundaries
        .windows(2)
        .map(|pair| {
            if pair[1] <= pair[0] {
                return Err("day boundaries must increase".to_string());
            }
            crate::threads::day(&reader, pair[0], pair[1], now)
        })
        .collect()
}

/// The person confirms an agent turn: its time counts even though they did
/// not supervise it.
#[tauri::command]
pub fn confirm_agent_job(app: AppHandle, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let store = state.activity.lock().map_err(|e| e.to_string())?;
        if !crate::agents::store::set_confirmed(store.conn(), &id)? {
            return Err("That agent turn no longer exists".to_string());
        }
    }
    crate::agents::refresh(&app);
    Ok(())
}

/// Agent entries (counted agent time) in a range. They are kept out of
/// `list_time_entries`, so no work total can pick them up by accident.
#[tauri::command]
pub fn list_agent_entries(
    app: AppHandle,
    start_ms: u64,
    end_ms: u64,
) -> Result<Vec<TimeEntry>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_agent_entries(start_ms, end_ms)
}

// --- P1: Apps -----------------------------------------------------------

#[tauri::command]
pub fn list_apps(app: AppHandle) -> Result<Vec<AppRecord>, String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    store.list_apps()
}

#[tauri::command]
pub fn update_app(
    app: AppHandle,
    id: String,
    default_category_id: Option<String>,
    default_project_id: Option<String>,
    excluded: Option<bool>,
) -> Result<AppRecord, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.update_app(&id, default_category_id, default_project_id, excluded, now)
}

// --- P2: AI suggestions --------------------------------------------------

#[tauri::command]
pub fn ai_status(app: AppHandle) -> AiStatus {
    app.state::<AiRuntime>().status()
}

/// "Couldn't categorize · Retry", or re-running a suggestion on demand.
#[tauri::command]
pub fn retry_classification(app: AppHandle, id: String) -> Result<(), String> {
    let now = now_epoch_ms();
    {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|e| e.to_string())?;
        store.queue_classification(&id, now)?;
    }
    entries_changed(&app);
    Ok(())
}

/// Accepts (creating a rule) or dismisses an inline rule suggestion.
#[tauri::command]
pub fn resolve_rule_suggestion(
    app: AppHandle,
    suggestion: RuleSuggestion,
    accept: bool,
) -> Result<(), String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    store.resolve_rule_suggestion(&suggestion, accept, now)
}

// --- P4: Learning loop -----------------------------------------------------

/// Settings → Categories & AI: effectiveness, calibration, the threshold
/// preview, and personal-model versions over the last `days` days.
#[tauri::command]
pub fn ai_metrics(app: AppHandle, days: u32) -> Result<AiMetrics, String> {
    let state = app.state::<AppState>();
    let reader = state.activity_reader.lock().map_err(|e| e.to_string())?;
    ai::metrics::metrics(&reader, days, now_epoch_ms())
}

/// "Retrain now": queues a personal-model retrain that skips the idle/power
/// gate. The result arrives as `ai-status-changed`.
#[tauri::command]
pub fn ai_retrain(app: AppHandle) -> AiStatus {
    ai::update_status(&app, |s| {
        s.retrain = "queued".to_string();
        s.retrain_note = None;
    });
    app.state::<AiRuntime>().request_retrain();
    app.state::<AiRuntime>().status()
}

/// "Reset learned data": forgets the personal models, calibration, and the
/// kNN store, keeping entries, categories, projects, and rules. Returns the
/// updated metrics.
#[tauri::command]
pub fn ai_reset_learned(app: AppHandle, days: u32) -> Result<AiMetrics, String> {
    let now = now_epoch_ms();
    let metrics = {
        let state = app.state::<AppState>();
        let store = state.activity.lock().map_err(|e| e.to_string())?;
        let paths = ai::store::reset_learned(store.conn(), now)?;
        for path in paths {
            if let Err(error) = std::fs::remove_dir_all(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("could not delete model {path}: {error}");
                }
            }
        }
        ai::metrics::metrics(store.conn(), days, now)?
    };
    ai::update_status(&app, |s| {
        s.outcomes = 0;
        s.calibrated = false;
        s.personal_model = false;
        s.retrain_note = None;
    });
    ai::nudge(&app);
    Ok(metrics)
}

// --- P3: Reports (Time Entries, My Timesheet, Calendar Month) -----------
//
// Read-only queries go through the reader connection so an aggregation over
// a year never holds the writer the sampler needs (decision A6).

fn with_reader<T>(
    app: &AppHandle,
    read: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let state = app.state::<AppState>();
    let conn = state.activity_reader.lock().map_err(|e| e.to_string())?;
    read(&conn)
}

#[tauri::command]
pub fn query_time_entries(
    app: AppHandle,
    filter: EntryFilter,
    limit: Option<u32>,
) -> Result<Vec<TimeEntry>, String> {
    with_reader(&app, |conn| {
        reports::query_entries(conn, &filter, limit.unwrap_or(reports::MAX_QUERY_ROWS))
    })
}

#[tauri::command]
pub fn entry_rollup(
    app: AppHandle,
    filter: EntryFilter,
    boundaries: Vec<u64>,
    group_by: String,
) -> Result<Vec<RollupCell>, String> {
    let group_by = GroupBy::parse(&group_by)?;
    with_reader(&app, |conn| {
        reports::rollup(conn, &filter, &boundaries, group_by)
    })
}

/// Writes the filtered entries to the Downloads folder as CSV or JSON.
#[tauri::command]
pub fn export_time_entries(
    app: AppHandle,
    filter: EntryFilter,
    format: String,
) -> Result<ExportResult, String> {
    let format = ExportFormat::parse(&format)?;
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| e.to_string())?;
    with_reader(&app, |conn| {
        reports::export_entries(conn, &filter, format, &dir)
    })
}

// --- P3: Projects ------------------------------------------------------

#[tauri::command]
pub fn project_stats(
    app: AppHandle,
    range_start: u64,
    range_end: u64,
    month_start: u64,
) -> Result<Vec<ProjectStats>, String> {
    with_reader(&app, |conn| {
        crate::projects::project_stats(conn, range_start, range_end, month_start)
    })
}

#[tauri::command]
pub fn project_rules(app: AppHandle, project_id: String) -> Result<Vec<ProjectRule>, String> {
    with_reader(&app, |conn| {
        crate::projects::project_rules(conn, &project_id)
    })
}

/// "Would have matched 14h in the last 30 days", per hint.
#[tauri::command]
pub fn preview_project_hints(
    app: AppHandle,
    hints: String,
    since_ms: u64,
) -> Result<HintPreview, String> {
    with_reader(&app, |conn| {
        crate::projects::preview_hints(conn, &hints, since_ms)
    })
}

#[tauri::command]
pub fn discover_projects(app: AppHandle, since_ms: u64) -> Result<Vec<ProjectSuggestion>, String> {
    with_reader(&app, |conn| crate::projects::discover(conn, since_ms))
}

#[tauri::command]
pub fn dismiss_project_suggestion(app: AppHandle, key: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let store = state.activity.lock().map_err(|e| e.to_string())?;
    crate::projects::dismiss_suggestion(store.conn(), &key)
}

#[tauri::command]
pub fn import_projects_csv(app: AppHandle, text: String) -> Result<ImportSummary, String> {
    let now = now_epoch_ms();
    let state = app.state::<AppState>();
    let mut store = state.activity.lock().map_err(|e| e.to_string())?;
    crate::projects::import_csv(&mut store, &text, now)
}

// --- Pulse panel -------------------------------------------------------

#[tauri::command]
pub fn resize_pulse_panel(app: AppHandle, height: f64) {
    crate::pulse::resize(&app, height);
}

#[tauri::command]
pub fn hide_pulse_panel(app: AppHandle) {
    crate::pulse::hide(&app);
}

/// The panel's doors into the app: closes the panel and brings the main
/// window forward, on today's review queue when `review` is set.
#[tauri::command]
pub fn open_main_window(app: AppHandle, review: bool) {
    crate::pulse::hide(&app);
    tray::show_main_window(&app);
    if review {
        let _ = app.emit_to("main", crate::EVENT_OPEN_REVIEW, ());
    }
}

// --- Preferences -------------------------------------------------------

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    app.state::<AppState>().settings_snapshot()
}

#[tauri::command]
pub fn update_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let (previous, next) = {
        let state = app.state::<AppState>();
        let mut store = state.settings.lock().map_err(|error| error.to_string())?;
        let previous = store.snapshot();
        let next = store.set(settings)?;
        (previous, next)
    };
    apply_settings_change(&app, &previous, &next);
    Ok(next)
}

/// Carries a settings change out to the parts of the app that hold a copy.
pub(crate) fn apply_settings_change(app: &AppHandle, previous: &Settings, next: &Settings) {
    if next.tray_enabled != previous.tray_enabled {
        let timers = {
            let state = app.state::<AppState>();
            state
                .store
                .lock()
                .ok()
                .and_then(|store| store.snapshot().ok())
        };
        if let Some(timers) = timers {
            if let Err(error) = tray::set_enabled(app, next.tray_enabled, &timers) {
                eprintln!("could not toggle the tray: {error}");
            }
        }
    }

    if next.extensions != previous.extensions {
        crate::agents::recheck(app);
    }

    if next.retention_days != previous.retention_days {
        let _ = sweep_retention(app);
    }

    if next.tracking_hours != previous.tracking_hours {
        let state = app.state::<AppState>();
        if let Ok(mut store) = state.activity.lock() {
            store.set_tracking_hours(next.tracking_hours.clone());
        }
        activity::emit_full(app);
    }

    let _ = app.emit(crate::EVENT_SETTINGS_CHANGED, next);
}

pub fn sweep_retention(app: &AppHandle) -> Result<u64, String> {
    let days = app.state::<AppState>().settings_snapshot().retention_days;
    let removed = {
        let state = app.state::<AppState>();
        let mut store = state.activity.lock().map_err(|error| error.to_string())?;
        let removed = store.purge_older_than(days, now_epoch_ms())?;
        let _ = crate::energy::purge_older_than(store.conn(), days, now_epoch_ms());
        removed
    };
    if removed > 0 {
        activity::emit_full(app);
    }
    Ok(removed)
}

pub use openrize_core::state::StoragePaths;

/// Whether macOS will launch OpenRize at login (see login_item.rs).
#[tauri::command]
pub fn launch_at_login() -> LoginItemState {
    login_item::state()
}

#[tauri::command]
pub fn set_launch_at_login(enabled: bool) -> Result<LoginItemState, String> {
    login_item::set(enabled)
}

#[tauri::command]
pub fn storage_paths(app: AppHandle) -> Result<StoragePaths, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    Ok(StoragePaths::new(&data_dir))
}

#[tauri::command]
pub fn get_energy_summary(
    app: AppHandle,
    days: Option<u32>,
) -> Result<crate::energy::EnergySummary, String> {
    let state = app.state::<AppState>();
    let conn = state.activity_reader.lock().map_err(|e| e.to_string())?;
    crate::energy::get_summary(
        &conn,
        days.unwrap_or(crate::energy::DEFAULT_HISTORY_DAYS),
        None,
        None,
    )
}

#[tauri::command]
pub fn query_energy_history(
    app: AppHandle,
    since_ms: Option<u64>,
    limit: Option<u32>,
) -> Result<Vec<crate::energy::EnergySample>, String> {
    let state = app.state::<AppState>();
    let conn = state.activity_reader.lock().map_err(|e| e.to_string())?;
    crate::energy::query_history(&conn, since_ms, limit)
}

#[tauri::command]
pub fn reset_energy_history(app: AppHandle) -> Result<crate::energy::EnergySummary, String> {
    let state = app.state::<AppState>();
    {
        let store = state.activity.lock().map_err(|e| e.to_string())?;
        crate::energy::reset_history(store.conn())?;
    }
    let conn = state.activity_reader.lock().map_err(|e| e.to_string())?;
    let summary =
        crate::energy::get_summary(&conn, crate::energy::DEFAULT_HISTORY_DAYS, None, None)?;
    let _ = app.emit(crate::energy::EVENT_ENERGY_CHANGED, &summary);
    Ok(summary)
}

// --- Updates -----------------------------------------------------------

#[tauri::command]
pub fn update_status(app: AppHandle) -> Result<UpdateStatus, String> {
    app.state::<UpdaterState>().snapshot()
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateStatus, String> {
    updater::check(&app, true).await;
    app.state::<UpdaterState>().snapshot()
}

/// Downloads and installs the available update, then restarts into it.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    updater::install(&app).await
}

// --- Break reminders ----------------------------------------------------

#[tauri::command]
pub fn break_state(app: AppHandle) -> Result<breaks::engine::BreakState, String> {
    breaks::current_state(&app)
}

/// Starts the pending reminder's break, or a manual one when none is pending.
#[tauri::command]
pub fn start_break(app: AppHandle) -> Result<breaks::engine::BreakState, String> {
    breaks::command(&app, breaks::engine::Command::StartBreak)
}

#[tauri::command]
pub fn end_break(app: AppHandle) -> Result<breaks::engine::BreakState, String> {
    breaks::command(&app, breaks::engine::Command::EndBreak)
}

#[tauri::command]
pub fn snooze_break(app: AppHandle, minutes: u16) -> Result<breaks::engine::BreakState, String> {
    if !crate::settings::SNOOZE_CHOICES.contains(&minutes) {
        return Err(format!("cannot snooze for {minutes} minutes"));
    }
    breaks::command(&app, breaks::engine::Command::Snooze(minutes))
}

#[tauri::command]
pub fn skip_break(app: AppHandle) -> Result<breaks::engine::BreakState, String> {
    breaks::command(&app, breaks::engine::Command::Skip)
}

#[tauri::command]
pub fn expand_break_reminder(app: AppHandle) -> Result<breaks::engine::BreakState, String> {
    breaks::command(&app, breaks::engine::Command::Expand)
}

#[tauri::command]
pub fn extend_break(app: AppHandle) -> Result<breaks::engine::BreakState, String> {
    breaks::command(
        &app,
        breaks::engine::Command::Extend(breaks::EXTEND_MINUTES),
    )
}

/// Silences reminders until `until` (epoch ms); `None` turns them back on.
#[tauri::command]
pub fn pause_break_reminders(
    app: AppHandle,
    until: Option<u64>,
) -> Result<breaks::engine::BreakState, String> {
    breaks::pause_reminders(&app, until)
}

#[tauri::command]
pub fn list_breaks(
    app: AppHandle,
    since_ms: u64,
    until_ms: u64,
) -> Result<Vec<breaks::store::BreakEntry>, String> {
    breaks::list_breaks(&app, since_ms, until_ms)
}

#[tauri::command]
pub fn resize_reminder_panel(app: AppHandle, width: f64, height: f64) {
    breaks::surface::resize(&app, width, height);
}

/// The reminder's "Reminder settings…": brings the main window forward on
/// Settings > Notifications.
#[tauri::command]
pub fn open_break_settings(app: AppHandle) {
    tray::show_main_window(&app);
    let _ = app.emit_to("main", crate::EVENT_OPEN_BREAK_SETTINGS, ());
}

/// Dev builds only: raises a sample reminder (Settings > Notifications).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn dev_sample_break_reminder(app: AppHandle) -> Result<breaks::engine::BreakState, String> {
    breaks::dev_sample(&app)
}

#[tauri::command]
pub fn preview_break_chime() {
    breaks::chime();
}
