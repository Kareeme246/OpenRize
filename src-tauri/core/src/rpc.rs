//! What `rize` asks of OpenRize (`crate::protocol`), answered from the
//! stores. Two processes answer, with the same code:
//!
//! - **The running app** serves requests on its local endpoint
//!   (`crate::ipc`) and passes itself as `Live`, so a change shows in open
//!   windows and the tray at once, and the few things only a running app can
//!   do (open a window, quit, focus sessions) work.
//! - **`rize` itself** answers when the app is closed, holding the stores'
//!   lock (`state::lock_stores`) for the one request. Nothing is tracked;
//!   the app picks up edits at its next launch, exactly as if they were made
//!   before it quit.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;
use serde_json::{json, Value};

use crate::models::UpdateTimeEntry;
use crate::protocol::{code, ApiError, Hello, Operation, Request, Response, PROTOCOL_VERSION};
use crate::reports::{self, ExportFormat, GroupBy};
use crate::settings::Settings;
use crate::state::{AppState, StoragePaths};
use crate::timers::{now_epoch_ms, Timer};
use crate::{EntryFilter, NONE};

/// What only the running app can do, and how it shows a change at once.
pub trait Live {
    fn open_window(&self, review: bool);
    /// Quits after the answer is sent, with the same shutdown as the tray's
    /// Quit.
    fn quit(&self);
    fn start_focus(&self, label: Option<String>) -> Result<(), String>;
    fn stop_focus(&self) -> Result<(), String>;
    /// The break reminder's state, as the Breaks tab shows it.
    fn breaks(&self) -> Option<Value>;
    fn settings_changed(&self, previous: &Settings, next: &Settings);
    /// Emits what a window command would after `operation` succeeded.
    fn changed(&self, state: &AppState, operation: &Operation);
}

/// Most entries one list returns; a bigger range is a report or an export.
const MAX_LIST: u32 = 5_000;
/// Most buckets one report returns (a year of days).
const MAX_BUCKETS: usize = 400;

type Outcome = Result<Value, ApiError>;

/// Answers one request. `live` is the running app, or `None` when `rize`
/// answers from the stores alone.
pub fn answer(
    state: &AppState,
    live: Option<&dyn Live>,
    data_dir: &Path,
    request: Request,
) -> Response {
    if request.protocol_version != PROTOCOL_VERSION {
        return Response::error(
            code::INCOMPATIBLE,
            format!(
                "This OpenRize ({}) speaks rize protocol {PROTOCOL_VERSION}, not {}. Restart or update the app so it matches rize.",
                env!("CARGO_PKG_VERSION"),
                request.protocol_version
            ),
        );
    }
    let context = Context {
        state,
        live,
        data_dir,
    };
    match context.dispatch(&request.operation) {
        Ok(data) => {
            if let Some(live) = live {
                live.changed(state, &request.operation);
            }
            Response::success(data)
        }
        Err(error) => Response::failure(error),
    }
}

struct Context<'a> {
    state: &'a AppState,
    live: Option<&'a dyn Live>,
    data_dir: &'a Path,
}

impl Context<'_> {
    fn dispatch(&self, operation: &Operation) -> Outcome {
        use Operation as O;
        let now = now_epoch_ms();
        match operation.clone() {
            O::Hello {} => to_value(Hello {
                app_version: env!("CARGO_PKG_VERSION").into(),
                protocol_version: PROTOCOL_VERSION,
                running: self.live.is_some(),
                data_dir: self.data_dir.display().to_string(),
            }),
            O::Status { day_start } => self.status(day_start, now),
            O::AppOpen { review } => {
                self.live()?.open_window(review);
                Ok(json!({}))
            }
            O::AppQuit {} => {
                self.live()?.quit();
                Ok(json!({}))
            }
            O::TrackSet { enabled } => {
                lock(&self.state.activity)?
                    .set_capture_enabled(enabled, now)
                    .map_err(fail)?;
                Ok(json!({ "captureEnabled": enabled }))
            }
            O::TrackIdle { minutes } => {
                if !(1..=24 * 60).contains(&minutes) {
                    return Err(invalid("the idle threshold must be 1 to 1440 minutes"));
                }
                lock(&self.state.activity)?
                    .set_idle_threshold_ms(minutes * 60_000)
                    .map_err(fail)?;
                Ok(json!({ "idleThresholdMinutes": minutes }))
            }
            O::FocusStart { label } => {
                self.live()?.start_focus(label).map_err(fail)?;
                self.status(day_start(now), now)
            }
            O::FocusStop {} => {
                self.live()?.stop_focus().map_err(fail)?;
                self.status(day_start(now), now)
            }
            O::TimersList {} => to_value(self.timers()?),
            O::TimerCreate { label } => {
                let label = label.trim();
                if label.is_empty() {
                    return Err(invalid("a timer needs a label"));
                }
                to_value(lock(&self.state.store)?.create(label).map_err(fail)?)
            }
            O::TimerStart { timer } => {
                let id = self.timer(&timer)?;
                to_value(lock(&self.state.store)?.start(&id).map_err(fail)?)
            }
            O::TimerPause { timer } => {
                let id = self.timer(&timer)?;
                to_value(lock(&self.state.store)?.pause(&id).map_err(fail)?)
            }
            O::TimerReset { timer } => {
                let id = self.timer(&timer)?;
                to_value(lock(&self.state.store)?.reset(&id).map_err(fail)?)
            }
            O::TimerRename { timer, label } => {
                let id = self.timer(&timer)?;
                to_value(
                    lock(&self.state.store)?
                        .rename(&id, label.trim())
                        .map_err(fail)?,
                )
            }
            O::TimerDelete { timer } => {
                let id = self.timer(&timer)?;
                to_value(lock(&self.state.store)?.delete(&id).map_err(fail)?)
            }
            O::EntriesList { filter, limit } => {
                if !(1..=MAX_LIST).contains(&limit) {
                    return Err(invalid(format!("--limit must be 1 to {MAX_LIST}")));
                }
                let filter = self.filter(filter)?;
                let mut entries =
                    self.read(|conn| reports::query_entries(conn, &filter, limit + 1))?;
                let truncated = entries.len() > limit as usize;
                entries.truncate(limit as usize);
                Ok(json!({
                    "entries": entries,
                    "truncated": truncated,
                    "names": self.names()?,
                }))
            }
            O::EntryShow { entry } => {
                let id = self.entry(&entry)?;
                let detail = lock(&self.state.activity)?
                    .get_entry_detail(&id)
                    .map_err(fail)?;
                Ok(json!({ "detail": detail, "names": self.names()? }))
            }
            O::EntryCreate { mut entry } => {
                if entry.ended_at <= entry.started_at {
                    return Err(invalid("--to must be after --from"));
                }
                entry.category_id = self.optional_category(entry.category_id)?;
                entry.project_id = self.optional_project(entry.project_id)?;
                let created = lock(&self.state.activity)?
                    .create_manual_entry(entry, now)
                    .map_err(fail)?;
                to_value(created)
            }
            O::EntriesEdit { entries, patch } => {
                let ids = self.entries(&entries)?;
                let patch = self.entry_patch(patch)?;
                let mut store = lock(&self.state.activity)?;
                let mut updated = Vec::with_capacity(ids.len());
                for id in &ids {
                    updated.push(
                        store
                            .update_time_entry(id, patch.clone(), now)
                            .map_err(fail)?,
                    );
                }
                to_value(updated)
            }
            O::EntriesApprove { entries } => {
                let ids = self.entries(&entries)?;
                let mut store = lock(&self.state.activity)?;
                store
                    .approve_time_entries(&ids, "user", now)
                    .map_err(fail)?;
                to_value(store.time_entries(&ids).map_err(fail)?)
            }
            O::EntriesUnapprove { entries } => {
                let ids = self.entries(&entries)?;
                let mut store = lock(&self.state.activity)?;
                store.unapprove_time_entries(&ids, now).map_err(fail)?;
                to_value(store.time_entries(&ids).map_err(fail)?)
            }
            O::EntryReject { entry } => {
                let id = self.entry(&entry)?;
                lock(&self.state.activity)?
                    .reject_time_entry(&id, now)
                    .map_err(fail)?;
                Ok(json!({ "id": id }))
            }
            O::EntrySplit { entry, at } => {
                let id = self.entry(&entry)?;
                let (first, second) = lock(&self.state.activity)?
                    .split_time_entry(&id, at, now)
                    .map_err(fail)?;
                to_value([first, second])
            }
            O::EntriesDelete { entries } => {
                let ids = self.entries(&entries)?;
                lock(&self.state.activity)?
                    .delete_time_entries(&ids, now)
                    .map_err(fail)?;
                Ok(json!({ "deleted": ids }))
            }
            O::EntriesRebuild { from, to } => {
                if to <= from {
                    return Err(invalid("--to must be after --from"));
                }
                let entries = lock(&self.state.activity)?
                    .rebuild_time_entries_in_range(from, to, now)
                    .map_err(fail)?;
                self.state.refresh_agent_days(from, to, now);
                to_value(entries)
            }
            O::EntriesExport { filter, format } => {
                let parsed = ExportFormat::parse(&format).map_err(invalid)?;
                let filter = self.filter(filter)?;
                let (content, count) =
                    self.read(|conn| reports::export_body(conn, &filter, parsed))?;
                Ok(json!({ "format": format, "count": count, "content": content }))
            }
            O::Report {
                filter,
                boundaries,
                group_by,
            } => {
                if boundaries.len() < 2
                    || boundaries.len() > MAX_BUCKETS + 1
                    || boundaries.windows(2).any(|pair| pair[1] <= pair[0])
                {
                    return Err(invalid(format!(
                        "a report needs 1 to {MAX_BUCKETS} increasing periods"
                    )));
                }
                let group = GroupBy::parse(&group_by).map_err(invalid)?;
                let filter = self.filter(filter)?;
                let cells = self.read(|conn| reports::rollup(conn, &filter, &boundaries, group))?;
                Ok(json!({ "cells": cells, "boundaries": boundaries, "names": self.names()? }))
            }
            O::ProjectsList {} => {
                let projects = lock(&self.state.activity)?.list_projects().map_err(fail)?;
                Ok(json!({ "projects": projects, "names": self.names()? }))
            }
            O::ProjectShow {
                project,
                range_start,
                range_end,
                month_start,
            } => {
                let id = self.project(&project)?;
                let project = lock(&self.state.activity)?
                    .list_projects()
                    .map_err(fail)?
                    .into_iter()
                    .find(|project| project.id == id)
                    .ok_or_else(|| not_found("project", &project))?;
                let stats = self
                    .read(|conn| {
                        crate::projects::project_stats(conn, range_start, range_end, month_start)
                    })?
                    .into_iter()
                    .find(|stats| stats.project_id == id);
                let rules = self.read(|conn| crate::projects::project_rules(conn, &id))?;
                Ok(json!({
                    "project": project,
                    "stats": stats,
                    "rules": rules,
                    "names": self.names()?,
                }))
            }
            O::ProjectCreate { mut project } => {
                project.client_id = self.optional_client(project.client_id)?;
                to_value(
                    lock(&self.state.activity)?
                        .create_project(project, now)
                        .map_err(fail)?,
                )
            }
            O::ProjectEdit { project, mut patch } => {
                let id = self.project(&project)?;
                if let Some(Some(client)) = patch.client_id {
                    patch.client_id = Some(self.optional_client(Some(client))?);
                }
                to_value(
                    lock(&self.state.activity)?
                        .update_project(&id, patch, now)
                        .map_err(fail)?,
                )
            }
            O::ProjectDelete { project } => {
                let id = self.project(&project)?;
                lock(&self.state.activity)?
                    .delete_project(&id, now)
                    .map_err(fail)?;
                Ok(json!({ "id": id }))
            }
            O::ClientsList {} => {
                to_value(lock(&self.state.activity)?.list_clients().map_err(fail)?)
            }
            O::ClientShow { client } => {
                let id = self.client(&client)?;
                let store = lock(&self.state.activity)?;
                let client = store
                    .list_clients()
                    .map_err(fail)?
                    .into_iter()
                    .find(|client| client.id == id);
                let projects: Vec<_> = store
                    .list_projects()
                    .map_err(fail)?
                    .into_iter()
                    .filter(|project| project.client_id.as_deref() == Some(id.as_str()))
                    .collect();
                drop(store);
                Ok(json!({ "client": client, "projects": projects, "names": self.names()? }))
            }
            O::ClientCreate { client } => to_value(
                lock(&self.state.activity)?
                    .create_client(client, now)
                    .map_err(fail)?,
            ),
            O::ClientEdit { client, patch } => {
                let id = self.client(&client)?;
                to_value(
                    lock(&self.state.activity)?
                        .update_client(&id, patch, now)
                        .map_err(fail)?,
                )
            }
            O::ClientDelete { client } => {
                let id = self.client(&client)?;
                lock(&self.state.activity)?
                    .delete_client(&id, now)
                    .map_err(fail)?;
                Ok(json!({ "id": id }))
            }
            O::CategoriesList {} => to_value(
                lock(&self.state.activity)?
                    .list_categories()
                    .map_err(fail)?,
            ),
            O::CategoryCreate { category } => to_value(
                lock(&self.state.activity)?
                    .create_category(category, now)
                    .map_err(fail)?,
            ),
            O::CategoryEdit { category, patch } => {
                let id = self.category(&category)?;
                to_value(
                    lock(&self.state.activity)?
                        .update_category(&id, patch, now)
                        .map_err(fail)?,
                )
            }
            O::CategoryDelete { category } => {
                let id = self.category(&category)?;
                lock(&self.state.activity)?
                    .delete_category(&id, now)
                    .map_err(fail)?;
                Ok(json!({ "id": id }))
            }
            O::SettingsGet {} => to_value(self.state.settings_snapshot()),
            O::SettingsSet { key, value } => {
                let (previous, next) = {
                    let mut store = lock(&self.state.settings)?;
                    let previous = store.snapshot();
                    (previous, store.patch(&key, value).map_err(invalid)?)
                };
                if let Some(live) = self.live {
                    live.settings_changed(&previous, &next);
                }
                to_value(next)
            }
            O::Paths {} => to_value(StoragePaths::new(self.data_dir)),
        }
    }

    fn live(&self) -> Result<&dyn Live, ApiError> {
        self.live.ok_or_else(|| {
            error(
                code::APP_NOT_RUNNING,
                "OpenRize is not running. Start it with `rize app start`.",
            )
        })
    }

    fn status(&self, day_start: u64, now: u64) -> Outcome {
        let mut activity = to_value(
            lock(&self.state.activity)?
                .snapshot(day_start, now)
                .map_err(fail)?,
        )?;
        // The segment list is the timeline's; status needs only the totals.
        if let Some(fields) = activity.as_object_mut() {
            fields.remove("segments");
        }
        let pending = self.read(|conn| {
            let filter = EntryFilter {
                start_ms: day_start,
                end_ms: now + 1,
                status: Some("pending".into()),
                ..Default::default()
            };
            Ok(reports::query_entries(conn, &filter, MAX_LIST)?.len())
        })?;
        let breaks = self.live.and_then(|live| live.breaks());
        Ok(json!({
            "running": self.live.is_some(),
            "appVersion": env!("CARGO_PKG_VERSION"),
            "activity": activity,
            "pendingEntries": pending,
            "timers": self.timers()?,
            "breaks": breaks,
        }))
    }

    fn timers(&self) -> Result<Vec<Timer>, ApiError> {
        lock(&self.state.store)?.snapshot().map_err(fail)
    }

    fn read<T>(
        &self,
        read: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
    ) -> Result<T, ApiError> {
        read(&*lock(&self.state.activity_reader)?).map_err(fail)
    }

    /// Every category, project and client id with its name, so a client can
    /// show names without a second request.
    fn names(&self) -> Result<BTreeMap<String, String>, ApiError> {
        let store = lock(&self.state.activity)?;
        let mut names = BTreeMap::new();
        for category in store.list_categories().map_err(fail)? {
            names.insert(category.id, category.name);
        }
        for project in store.list_projects().map_err(fail)? {
            names.insert(project.id, project.name);
        }
        for client in store.list_clients().map_err(fail)? {
            names.insert(client.id, client.name);
        }
        Ok(names)
    }

    fn timer(&self, reference: &str) -> Result<String, ApiError> {
        let timers = self.timers()?;
        pick(&timers, reference, "timer", |t| &t.id, |t| &t.label)
    }

    fn category(&self, reference: &str) -> Result<String, ApiError> {
        let items = lock(&self.state.activity)?
            .list_categories()
            .map_err(fail)?;
        pick(&items, reference, "category", |c| &c.id, |c| &c.name)
    }

    fn project(&self, reference: &str) -> Result<String, ApiError> {
        let items = lock(&self.state.activity)?.list_projects().map_err(fail)?;
        pick(&items, reference, "project", |p| &p.id, |p| &p.name)
    }

    fn client(&self, reference: &str) -> Result<String, ApiError> {
        let items = lock(&self.state.activity)?.list_clients().map_err(fail)?;
        pick(&items, reference, "client", |c| &c.id, |c| &c.name)
    }

    fn optional_category(&self, reference: Option<String>) -> Result<Option<String>, ApiError> {
        reference.map(|r| self.category(&r)).transpose()
    }

    fn optional_project(&self, reference: Option<String>) -> Result<Option<String>, ApiError> {
        reference.map(|r| self.project(&r)).transpose()
    }

    fn optional_client(&self, reference: Option<String>) -> Result<Option<String>, ApiError> {
        reference.map(|r| self.client(&r)).transpose()
    }

    /// `none` keeps meaning "no category / project / client" in a filter.
    fn filter(&self, mut filter: EntryFilter) -> Result<EntryFilter, ApiError> {
        let keep_none = |value: &Option<String>| value.as_deref() == Some(NONE);
        if !keep_none(&filter.category_id) {
            filter.category_id = self.optional_category(filter.category_id)?;
        }
        if !keep_none(&filter.project_id) {
            filter.project_id = self.optional_project(filter.project_id)?;
        }
        if !keep_none(&filter.client_id) {
            filter.client_id = self.optional_client(filter.client_id)?;
        }
        if filter.end_ms <= filter.start_ms {
            return Err(invalid("--to must be after --from"));
        }
        Ok(filter)
    }

    /// `none` (or an empty value) clears an entry's category or project,
    /// which the store spells as an empty string.
    fn entry_patch(&self, mut patch: UpdateTimeEntry) -> Result<UpdateTimeEntry, ApiError> {
        let clear = |value: &str| value.is_empty() || value == NONE;
        if let Some(category) = patch.category_id.take() {
            patch.category_id = Some(if clear(&category) {
                String::new()
            } else {
                self.category(&category)?
            });
        }
        if let Some(project) = patch.project_id.take() {
            patch.project_id = Some(if clear(&project) {
                String::new()
            } else {
                self.project(&project)?
            });
        }
        if let (Some(start), Some(end)) = (patch.started_at, patch.ended_at) {
            if end <= start {
                return Err(invalid("--to must be after --from"));
            }
        }
        Ok(patch)
    }

    fn entries(&self, references: &[String]) -> Result<Vec<String>, ApiError> {
        if references.is_empty() {
            return Err(invalid("name at least one entry"));
        }
        let mut ids = Vec::with_capacity(references.len());
        for reference in references {
            let id = self.entry(reference)?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    /// A full entry id, or the end of one (`rize entries list` shows the last
    /// eight characters, the random part of a v7 UUID).
    fn entry(&self, reference: &str) -> Result<String, ApiError> {
        let reference = reference.trim().to_ascii_lowercase();
        if reference.len() < 4 || !reference.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err(not_found("entry", &reference));
        }
        let matches: Vec<String> = self.read(|conn| {
            let mut statement = conn
                .prepare(
                    "SELECT id FROM time_entries
                     WHERE deleted_at IS NULL AND (id = ?1 OR id LIKE '%' || ?1)
                     LIMIT 6",
                )
                .map_err(|e| e.to_string())?;
            let ids = statement
                .query_map([&reference], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string());
            ids
        })?;
        if matches.contains(&reference) {
            return Ok(reference);
        }
        match matches.as_slice() {
            [] => Err(not_found("entry", &reference)),
            [only] => Ok(only.clone()),
            many => Err(ambiguous("entry", &reference, many.to_vec())),
        }
    }
}

/// Resolves what someone typed: an id, a whole name (any case), or the start
/// of exactly one name.
fn pick<T>(
    items: &[T],
    reference: &str,
    kind: &str,
    id: impl Fn(&T) -> &String,
    name: impl Fn(&T) -> &String,
) -> Result<String, ApiError> {
    let wanted = reference.trim().to_lowercase();
    if wanted.is_empty() {
        return Err(invalid(format!("name a {kind}")));
    }
    if let Some(item) = items.iter().find(|item| *id(item) == reference.trim()) {
        return Ok(id(item).clone());
    }
    let candidates = |test: &dyn Fn(&str) -> bool| -> Vec<&T> {
        items
            .iter()
            .filter(|item| test(&name(item).to_lowercase()))
            .collect()
    };
    let exact = candidates(&|name| name == wanted);
    let matches = if exact.is_empty() {
        candidates(&|name| name.starts_with(&wanted))
    } else {
        exact
    };
    match matches.as_slice() {
        [] => Err(not_found(kind, reference)),
        [only] => Ok(id(only).clone()),
        many => Err(ambiguous(
            kind,
            reference,
            many.iter()
                .map(|item| format!("{} ({})", name(item), id(item)))
                .collect(),
        )),
    }
}

fn day_start(now: u64) -> u64 {
    crate::agents::ledger::calendar_day(now).0
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, ApiError> {
    mutex.lock().map_err(|_| fail("a store lock was poisoned"))
}

fn to_value(value: impl Serialize) -> Outcome {
    serde_json::to_value(value).map_err(|e| fail(e.to_string()))
}

fn error(code: &str, message: impl Into<String>) -> ApiError {
    ApiError {
        code: code.into(),
        message: message.into(),
        candidates: Vec::new(),
    }
}

fn fail(message: impl Into<String>) -> ApiError {
    error(code::FAILED, message)
}

fn invalid(message: impl Into<String>) -> ApiError {
    error(code::INVALID_ARGUMENT, message)
}

fn not_found(kind: &str, reference: &str) -> ApiError {
    error(
        code::NOT_FOUND,
        format!("no {kind} matches \"{reference}\""),
    )
}

fn ambiguous(kind: &str, reference: &str, candidates: Vec<String>) -> ApiError {
    ApiError {
        code: code::AMBIGUOUS.into(),
        message: format!("\"{reference}\" matches more than one {kind}; be more specific"),
        candidates,
    }
}
