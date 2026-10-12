//! The session's undo history. Every user change to time entries is one
//! `Command`, applied inside a savepoint so it lands whole or not at all, and
//! undone or redone the same way:
//!
//! - `Edit`: field changes (accept, unapprove, reject, category, project,
//!   billable, description, times) and new entries, captured generically as
//!   each entry's row before and after. Undo writes back only the columns the
//!   change touched, after checking nothing else changed them since.
//! - `Split` and `Merge`: structural changes. Their inverse depends on time,
//!   not on a snapshot: a live half keeps recording after a split, so on undo
//!   every segment it gathered in the meantime goes home with it.
//! - `Deletion`: entries and catalog items together (see `deletions.rs`).
//!
//! Structural removals never soft-delete. `deleted_at` means the user deleted
//! that time, and the entry builder keeps a deleted entry's span empty, so a
//! split half or merged-away entry marked deleted would blank tracked time.
//! An entry a command removes is stashed instead (its row and every row that
//! hangs off it) and put back verbatim on redo.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, params_from_iter, types::Value, Connection};

use super::{deletions::Deletion, segment_from_row, ActivityStore};
use crate::entry_builder::{describe, SegmentInput};
use crate::models::TimeEntry;

const CONFLICT: &str = "These entries have changed since; undo/redo cannot safely overwrite them.";

/// How many commands the undo stack keeps.
const DEPTH: usize = 100;

/// The `time_entries` columns undo tracks, in `EntryRow` order. `updated_at`
/// is only ever bumped.
const COLUMNS: [&str; 14] = [
    "id",
    "started_at",
    "ended_at",
    "description",
    "description_origin",
    "category_id",
    "project_id",
    "status",
    "approved_by",
    "source",
    "billable",
    "invoice_id",
    "created_at",
    "deleted_at",
];
const STARTED: usize = 1;
const ENDED: usize = 2;
const DESCRIPTION: usize = 3;
const STATUS: usize = 7;
const INVOICE: usize = 11;
const DELETED: usize = 13;

/// One `time_entries` row, as `COLUMNS`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct EntryRow(Vec<Value>);

impl EntryRow {
    fn read(conn: &Connection, id: &str) -> Result<Option<Self>, String> {
        let sql = format!(
            "SELECT {} FROM time_entries WHERE id = ?1;",
            COLUMNS.join(", ")
        );
        let mut statement = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = statement
            .query_map([id], |row| {
                (0..COLUMNS.len())
                    .map(|i| row.get::<_, Value>(i))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|e| e.to_string())?;
        rows.next()
            .transpose()
            .map(|row| row.map(Self))
            .map_err(|e| e.to_string())
    }

    /// The row, when it exists, is not deleted and is not invoiced: the only
    /// rows a command may change.
    fn editable(conn: &Connection, id: &str) -> Result<Self, String> {
        Self::read(conn, id)?
            .filter(|row| row.0[DELETED] == Value::Null && row.0[INVOICE] == Value::Null)
            .ok_or_else(|| CONFLICT.to_string())
    }

    fn int(&self, column: usize) -> u64 {
        match self.0[column] {
            Value::Integer(value) => value as u64,
            _ => 0,
        }
    }

    fn text(&self, column: usize) -> &str {
        match &self.0[column] {
            Value::Text(value) => value,
            _ => "",
        }
    }

    fn building(&self) -> bool {
        self.text(STATUS) == "building"
    }
}

/// The tables whose rows belong to one entry, keyed by `entry_id`.
const DEPENDENTS: [&str; 4] = [
    "suggestions",
    "classify_jobs",
    "entry_embeddings",
    "entry_events",
];

/// A removed entry: its `time_entries` row and every dependent row, as they
/// were, ready to go back.
#[derive(Clone)]
pub(super) struct Stash {
    tables: Vec<Rows>,
}

/// Rows of one table, every column.
#[derive(Clone)]
struct Rows {
    table: &'static str,
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

impl Stash {
    /// Removes an entry and its dependent rows, returning them. Its segments
    /// are the caller's: relink them first.
    pub(super) fn take(conn: &Connection, id: &str) -> Result<Self, String> {
        let mut tables = Vec::new();
        for (table, key) in std::iter::once(("time_entries", "id"))
            .chain(DEPENDENTS.iter().map(|table| (*table, "entry_id")))
        {
            let (columns, rows) = select_all(conn, table, key, id)?;
            conn.execute(&format!("DELETE FROM {table} WHERE {key} = ?1;"), [id])
                .map_err(|e| e.to_string())?;
            tables.push(Rows {
                table,
                columns,
                rows,
            });
        }
        conn.execute(
            "UPDATE segments SET entry_id = NULL WHERE entry_id = ?1;",
            [id],
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { tables })
    }

    fn restore(&self, conn: &Connection) -> Result<(), String> {
        for Rows {
            table,
            columns,
            rows,
        } in &self.tables
        {
            for row in rows {
                insert(conn, table, columns, row)?;
            }
        }
        Ok(())
    }
}

/// Every column of every `table` row whose `key` is `id`.
fn select_all(
    conn: &Connection,
    table: &str,
    key: &str,
    id: &str,
) -> Result<(Vec<String>, Vec<Vec<Value>>), String> {
    let mut statement = conn
        .prepare(&format!("SELECT * FROM {table} WHERE {key} = ?1;"))
        .map_err(|e| e.to_string())?;
    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    let rows = statement
        .query_map([id], |row| {
            (0..columns.len())
                .map(|i| row.get::<_, Value>(i))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok((columns, rows))
}

fn insert(conn: &Connection, table: &str, columns: &[String], row: &[Value]) -> Result<(), String> {
    let marks = vec!["?"; columns.len()].join(", ");
    conn.execute(
        &format!(
            "INSERT INTO {table} ({}) VALUES ({marks});",
            columns.join(", ")
        ),
        params_from_iter(row),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// One undoable user action.
pub(super) enum Command {
    Deletion(Deletion),
    Edit(Edit),
    Split(Split),
    Merge(Merge),
}

impl Command {
    /// What the undo toast calls it.
    fn label(&self) -> &'static str {
        match self {
            Self::Deletion(_) => "Delete",
            Self::Edit(edit) => edit.label,
            Self::Split(_) => "Split",
            Self::Merge(_) => "Merge",
        }
    }
}

/// Field changes and new entries, as before/after rows.
pub(super) struct Edit {
    label: &'static str,
    entries: Vec<Change>,
    /// Suggestion outcomes the change recorded (accepted, rejected,
    /// changed): (suggestion id, before, after).
    outcomes: Vec<(String, Option<String>, Option<String>)>,
}

struct Change {
    id: String,
    /// `None`: the change created this entry.
    before: Option<EntryRow>,
    after: EntryRow,
    /// A created entry while it is undone.
    stash: Option<Stash>,
}

impl Edit {
    fn apply(&mut self, conn: &Connection, undo: bool, now: u64) -> Result<(), String> {
        for change in &mut self.entries {
            let Some(before) = &change.before else {
                if undo {
                    EntryRow::editable(conn, &change.id)?;
                    change.stash = Some(Stash::take(conn, &change.id)?);
                } else {
                    change.stash.take().ok_or(CONFLICT)?.restore(conn)?;
                }
                continue;
            };
            let (from, to) = if undo {
                (&change.after, before)
            } else {
                (before, &change.after)
            };
            let current = EntryRow::editable(conn, &change.id)?;
            let touched: Vec<usize> = (1..COLUMNS.len())
                .filter(|&i| from.0[i] != to.0[i])
                .collect();
            for &column in &touched {
                // The builder owns a live entry's times and template text.
                let live = matches!(column, STARTED | ENDED | DESCRIPTION)
                    && current.building()
                    && from.building();
                if !live && current.0[column] != from.0[column] {
                    return Err(CONFLICT.to_string());
                }
            }
            let sets: Vec<String> = touched
                .iter()
                .map(|&i| format!("{} = ?", COLUMNS[i]))
                .chain(["updated_at = ?".to_string()])
                .collect();
            let mut values: Vec<Value> = touched.iter().map(|&i| to.0[i].clone()).collect();
            values.push(Value::Integer(now as i64));
            values.push(Value::Text(change.id.clone()));
            conn.execute(
                &format!("UPDATE time_entries SET {} WHERE id = ?;", sets.join(", ")),
                params_from_iter(values),
            )
            .map_err(|e| e.to_string())?;
        }
        for (id, before, after) in &self.outcomes {
            let (from, to) = if undo {
                (after, before)
            } else {
                (before, after)
            };
            // Learning feedback only: a suggestion the AI has since replaced
            // keeps its newer outcome.
            conn.execute(
                "UPDATE suggestions SET outcome = ?1, updated_at = ?2 WHERE id = ?3 AND outcome IS ?4;",
                params![to, now as i64, id, from],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// The current outcome of every suggestion on these entries.
fn outcomes(
    conn: &Connection,
    ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, Option<String>>, String> {
    let mut statement = conn
        .prepare("SELECT id, outcome FROM suggestions WHERE entry_id = ?1;")
        .map_err(|e| e.to_string())?;
    let mut map = BTreeMap::new();
    for id in ids {
        let rows = statement
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, outcome) = row.map_err(|e| e.to_string())?;
            map.insert(id, outcome);
        }
    }
    Ok(map)
}

/// A split: `original` keeps the time before `at`, `second` takes the rest.
pub(super) struct Split {
    original: String,
    second: String,
    at: u64,
    before: EntryRow,
    /// The second half while the split is undone.
    stash: Option<Stash>,
}

/// A merge: `primary` absorbed `secondary`, which met it at `boundary`.
pub(super) struct Merge {
    primary: String,
    secondary: String,
    boundary: u64,
    secondary_later: bool,
    before: EntryRow,
    after: EntryRow,
    /// The secondary while the merge stands.
    stash: Option<Stash>,
}

impl ActivityStore {
    pub(super) fn record_undo(&mut self, command: Command) {
        self.undo_stack.push(command);
        self.redo_stack.clear();
        if self.undo_stack.len() > DEPTH {
            self.undo_stack.remove(0);
        }
    }

    /// Runs `op` in a savepoint: on error every write it made is rolled back,
    /// and so is the capture state it may have moved (`current`).
    pub(super) fn atomically<T>(
        &mut self,
        op: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        self.conn
            .execute_batch("SAVEPOINT history;")
            .map_err(|e| e.to_string())?;
        let current = self.current.clone();
        let pending = self.pending_switch.clone();
        match op(self) {
            Ok(value) => {
                self.conn
                    .execute_batch("RELEASE history;")
                    .map_err(|e| e.to_string())?;
                Ok(value)
            }
            Err(error) => {
                let _ = self
                    .conn
                    .execute_batch("ROLLBACK TO history; RELEASE history;");
                self.current = current;
                self.pending_switch = pending;
                Err(error)
            }
        }
    }

    /// Runs a change to these entries as one undoable `Edit` called `label`.
    /// `ids` must name every entry `op` touches, including one it creates.
    pub(super) fn edit<T>(
        &mut self,
        label: &'static str,
        ids: &[String],
        op: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let ids: BTreeSet<String> = ids.iter().cloned().collect();
        let (value, command) = self.atomically(|store| {
            let before = ids
                .iter()
                .map(|id| EntryRow::read(&store.conn, id))
                .collect::<Result<Vec<_>, _>>()?;
            let outcomes_before = outcomes(&store.conn, &ids)?;
            let value = op(store)?;
            let mut entries = Vec::new();
            for (id, before) in ids.iter().zip(before) {
                let Some(after) = EntryRow::read(&store.conn, id)? else {
                    continue;
                };
                if before.as_ref() != Some(&after) {
                    entries.push(Change {
                        id: id.clone(),
                        before,
                        after,
                        stash: None,
                    });
                }
            }
            let outcomes = outcomes(&store.conn, &ids)?
                .into_iter()
                .filter_map(|(id, after)| {
                    let before = outcomes_before.get(&id)?.clone();
                    (before != after).then_some((id, before, after))
                })
                .collect::<Vec<_>>();
            let command = (!entries.is_empty() || !outcomes.is_empty()).then_some(Edit {
                label,
                entries,
                outcomes,
            });
            Ok((value, command))
        })?;
        if let Some(edit) = command {
            self.record_undo(Command::Edit(edit));
        }
        Ok(value)
    }

    /// Undoes the last change; returns what it was, or `None` when there is
    /// nothing to undo. A change that conflicts with later edits stays put.
    pub fn undo(&mut self, now: u64) -> Result<Option<&'static str>, String> {
        self.step(true, now)
    }

    pub fn redo(&mut self, now: u64) -> Result<Option<&'static str>, String> {
        self.step(false, now)
    }

    fn step(&mut self, undo: bool, now: u64) -> Result<Option<&'static str>, String> {
        let stack = if undo {
            &mut self.undo_stack
        } else {
            &mut self.redo_stack
        };
        let Some(mut command) = stack.pop() else {
            return Ok(None);
        };
        let result = self.atomically(|store| store.apply(&mut command, undo, now));
        let label = command.label();
        match result {
            Ok(()) => {
                if undo {
                    self.redo_stack.push(command);
                } else {
                    self.undo_stack.push(command);
                }
                Ok(Some(label))
            }
            Err(error) => {
                if undo {
                    self.undo_stack.push(command);
                } else {
                    self.redo_stack.push(command);
                }
                Err(error)
            }
        }
    }

    fn apply(&mut self, command: &mut Command, undo: bool, now: u64) -> Result<(), String> {
        let label = command.label();
        let touched: Vec<String> = match command {
            Command::Deletion(deletion) => {
                deletion.apply(&self.conn, undo, now)?;
                if !undo {
                    self.forget_deleted_segment(deletion);
                }
                Vec::new()
            }
            Command::Edit(edit) => {
                edit.apply(&self.conn, undo, now)?;
                edit.entries
                    .iter()
                    .filter(|change| change.before.is_some())
                    .map(|change| change.id.clone())
                    .collect()
            }
            Command::Split(split) => {
                if undo {
                    self.unsplit(split, now)?;
                } else {
                    let stash = split.stash.clone();
                    self.split_at(&split.original, &split.second, split.at, stash, now)?;
                    split.stash = None;
                }
                vec![split.original.clone()]
            }
            Command::Merge(merge) => {
                if undo {
                    self.unmerge(merge, now)?;
                } else {
                    merge.stash = Some(self.merge_into(&merge.primary, &merge.secondary, now)?.1);
                    merge.after = EntryRow::read(&self.conn, &merge.primary)?.ok_or(CONFLICT)?;
                }
                vec![merge.primary.clone()]
            }
        };
        let kind = if undo { "undone" } else { "redone" };
        let payload = serde_json::json!({ "action": label }).to_string();
        for id in touched {
            self.log_event(&id, kind, "user", Some(&payload), now);
        }
        Ok(())
    }

    // --- Split ---------------------------------------------------------------

    /// Splits an entry at `at`: the entry keeps the time before it, a new
    /// entry the time after. Splitting the live entry closes its first half
    /// and keeps recording into the second.
    pub fn split_time_entry(
        &mut self,
        id: &str,
        at_ms: u64,
        now: u64,
    ) -> Result<(TimeEntry, TimeEntry), String> {
        let before = EntryRow::read(&self.conn, id)?.ok_or("That entry no longer exists.")?;
        let second = uuid::Uuid::now_v7().to_string();
        let halves = self.atomically(|store| store.split_at(id, &second, at_ms, None, now))?;
        self.record_undo(Command::Split(Split {
            original: id.to_string(),
            second,
            at: at_ms,
            before,
            stash: None,
        }));
        Ok(halves)
    }

    /// The split itself, shared by the first run and redo (which puts the
    /// stashed second half back instead of minting a new one).
    fn split_at(
        &mut self,
        id: &str,
        second: &str,
        at: u64,
        restore: Option<Stash>,
        now: u64,
    ) -> Result<(TimeEntry, TimeEntry), String> {
        let entry = self.time_entry(id)?;
        if entry.deleted_at.is_some() {
            return Err("That entry was deleted.".to_string());
        }
        if entry.invoice_id.is_some() {
            return Err("Invoiced entries cannot be split.".to_string());
        }
        let live = entry.status == "building";
        // A live entry's stored end trails the clock by up to a rebuild.
        let end = if live {
            entry.ended_at.max(now)
        } else {
            entry.ended_at
        };
        if at <= entry.started_at || at >= end {
            return Err("Split point must be strictly inside the entry duration".to_string());
        }

        if live {
            // Activity captured since the last rebuild is this session's too.
            self.conn
                .execute(
                    "UPDATE segments SET entry_id = ?1
                     WHERE entry_id IS NULL AND kind != 'break' AND started_at >= ?2;",
                    params![id, entry.started_at as i64],
                )
                .map_err(|e| e.to_string())?;
        }
        // Each half owns exactly its own activity: the segment running across
        // the cut is cut too, and a live one keeps recording into the second.
        self.cut_segments(id, at, Some(second))?;
        self.conn
            .execute(
                "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2 AND started_at >= ?3;",
                params![second, id, at as i64],
            )
            .map_err(|e| e.to_string())?;

        match restore {
            Some(stash) => stash.restore(&self.conn)?,
            None => {
                self.conn
                    .execute(
                        "INSERT INTO time_entries (id, started_at, ended_at, description, description_origin, category_id, project_id, status, approved_by, source, billable, created_at, updated_at)
                         SELECT ?1, started_at, ended_at, description, description_origin, category_id, project_id, status, approved_by, source, billable, ?2, ?2
                         FROM time_entries WHERE id = ?3;",
                        params![second, now as i64, id],
                    )
                    .map_err(|e| e.to_string())?;
                // Both halves are the same work: the second inherits the
                // suggestions, so it is not classified all over again.
                let (columns, rows) = select_all(&self.conn, "suggestions", "entry_id", id)?;
                for mut row in rows {
                    for (column, value) in columns.iter().zip(row.iter_mut()) {
                        match column.as_str() {
                            "id" => *value = Value::Text(uuid::Uuid::now_v7().to_string()),
                            "entry_id" => *value = Value::Text(second.to_string()),
                            _ => {}
                        }
                    }
                    insert(&self.conn, "suggestions", &columns, &row)?;
                }
            }
        }
        self.conn
            .execute(
                "UPDATE time_entries SET started_at = ?1, ended_at = ?2, updated_at = ?3 WHERE id = ?4;",
                params![at as i64, end as i64, now as i64, second],
            )
            .map_err(|e| e.to_string())?;
        // The first half of a live session is a finished session now.
        self.conn
            .execute(
                "UPDATE time_entries SET ended_at = ?1, updated_at = ?2,
                   status = CASE WHEN status = 'building' THEN 'pending' ELSE status END
                 WHERE id = ?3;",
                params![at as i64, now as i64, id],
            )
            .map_err(|e| e.to_string())?;
        self.describe_closed(id)?;
        self.describe_closed(second)?;

        self.log_event(id, "split", "user", None, now);
        self.log_event(second, "split", "user", None, now);
        Ok((self.time_entry(id)?, self.time_entry(second)?))
    }

    fn unsplit(&mut self, split: &mut Split, now: u64) -> Result<(), String> {
        let first = EntryRow::editable(&self.conn, &split.original)?;
        let second = EntryRow::editable(&self.conn, &split.second)?;
        // The second half may start later than the cut when the cut fell in
        // a gap; anything else means the halves were resized since.
        if first.int(ENDED) != split.at || second.int(STARTED) < split.at {
            return Err(CONFLICT.to_string());
        }
        self.conn
            .execute(
                "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2;",
                params![split.original, split.second],
            )
            .map_err(|e| e.to_string())?;
        // A live second half has kept recording: the rejoined entry keeps
        // that time.
        let ended = split.before.int(ENDED).max(second.int(ENDED));
        self.conn
            .execute(
                "UPDATE time_entries SET ended_at = ?1, status = ?2, description = ?3,
                   description_origin = ?4, updated_at = ?5
                 WHERE id = ?6;",
                params![
                    ended as i64,
                    split.before.text(STATUS),
                    split.before.text(DESCRIPTION),
                    split.before.text(DESCRIPTION + 1),
                    now as i64,
                    split.original,
                ],
            )
            .map_err(|e| e.to_string())?;
        split.stash = Some(Stash::take(&self.conn, &split.second)?);
        Ok(())
    }

    // --- Merge ---------------------------------------------------------------

    /// Merges `secondary_id` into the adjacent `primary_id`, which keeps its
    /// id and fields. Merging into the live session keeps it recording when
    /// both are captured sessions; otherwise the live one closes now.
    pub fn merge_time_entries(
        &mut self,
        primary_id: &str,
        secondary_id: &str,
        now: u64,
    ) -> Result<TimeEntry, String> {
        let before = EntryRow::read(&self.conn, primary_id)?.ok_or("Primary entry not found")?;
        let secondary = self.time_entry(secondary_id)?;
        let (merged, stash) =
            self.atomically(|store| store.merge_into(primary_id, secondary_id, now))?;
        let secondary_later = secondary.started_at >= before.int(ENDED);
        let after = EntryRow::read(&self.conn, primary_id)?.ok_or(CONFLICT)?;
        self.record_undo(Command::Merge(Merge {
            primary: primary_id.to_string(),
            secondary: secondary_id.to_string(),
            boundary: if secondary_later {
                secondary.started_at
            } else {
                secondary.ended_at
            },
            secondary_later,
            before,
            after,
            stash: Some(stash),
        }));
        Ok(merged)
    }

    fn merge_into(
        &mut self,
        primary_id: &str,
        secondary_id: &str,
        now: u64,
    ) -> Result<(TimeEntry, Stash), String> {
        if primary_id == secondary_id {
            return Err("An entry cannot be merged with itself".to_string());
        }
        let primary = self
            .time_entry(primary_id)
            .ok()
            .filter(|entry| entry.deleted_at.is_none())
            .ok_or("Primary entry not found")?;
        let secondary = self
            .time_entry(secondary_id)
            .ok()
            .filter(|entry| entry.deleted_at.is_none())
            .ok_or("Secondary entry not found")?;
        if primary.invoice_id.is_some() || secondary.invoice_id.is_some() {
            return Err("Invoiced entries cannot be merged.".to_string());
        }
        let adjacent =
            primary.ended_at == secondary.started_at || secondary.ended_at == primary.started_at;
        if !adjacent {
            return Err(
                "Time entries must start and stop at the same border to be mergeable".to_string(),
            );
        }

        let live = primary.status == "building" || secondary.status == "building";
        // A captured session can take in its neighbour and keep recording; an
        // approved or hand-made entry cannot be rebuilt, so the live session
        // closes now instead and recording carries on in a new one.
        let keep_live = live
            && primary.source == "auto"
            && secondary.source == "auto"
            && primary.status != "approved";
        let started_at = primary.started_at.min(secondary.started_at);
        let mut ended_at = primary.ended_at.max(secondary.ended_at);
        if live {
            ended_at = ended_at.max(now);
        }
        let status = if keep_live {
            "building"
        } else if primary.status == "building" {
            "pending"
        } else {
            primary.status.as_str()
        };

        self.conn
            .execute(
                "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2;",
                params![primary_id, secondary_id],
            )
            .map_err(|e| e.to_string())?;
        let stash = Stash::take(&self.conn, secondary_id)?;
        self.conn
            .execute(
                "UPDATE time_entries SET started_at = ?1, ended_at = ?2, status = ?3, updated_at = ?4 WHERE id = ?5;",
                params![started_at as i64, ended_at as i64, status, now as i64, primary_id],
            )
            .map_err(|e| e.to_string())?;
        self.describe_closed(primary_id)?;
        self.log_event(primary_id, "merged", "user", None, now);
        Ok((self.time_entry(primary_id)?, stash))
    }

    fn unmerge(&mut self, merge: &mut Merge, now: u64) -> Result<(), String> {
        let current = EntryRow::editable(&self.conn, &merge.primary)?;
        if !current.building()
            && (current.int(STARTED) != merge.after.int(STARTED)
                || current.int(ENDED) != merge.after.int(ENDED))
        {
            return Err(CONFLICT.to_string());
        }
        merge.stash.take().ok_or(CONFLICT)?.restore(&self.conn)?;
        let side = if merge.secondary_later {
            "started_at >= ?3"
        } else {
            "started_at < ?3"
        };
        self.conn
            .execute(
                &format!("UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2 AND {side};"),
                params![merge.secondary, merge.primary, merge.boundary as i64],
            )
            .map_err(|e| e.to_string())?;
        // Whichever half was live gets the time recorded since the merge.
        let recorded = current.int(ENDED);
        let primary_end = if merge.before.building() {
            merge.before.int(ENDED).max(recorded)
        } else {
            merge.before.int(ENDED)
        };
        self.conn
            .execute(
                "UPDATE time_entries SET started_at = ?1, ended_at = ?2, status = ?3, approved_by = ?4,
                   description = ?5, description_origin = ?6, updated_at = ?7
                 WHERE id = ?8;",
                params![
                    merge.before.int(STARTED) as i64,
                    primary_end as i64,
                    merge.before.text(STATUS),
                    merge.before.0[STATUS + 1],
                    merge.before.text(DESCRIPTION),
                    merge.before.text(DESCRIPTION + 1),
                    now as i64,
                    merge.primary,
                ],
            )
            .map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "UPDATE time_entries SET ended_at = MAX(ended_at, ?1), updated_at = ?2
                 WHERE id = ?3 AND status = 'building';",
                params![recorded as i64, now as i64, merge.secondary],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // --- Shared --------------------------------------------------------------

    /// Cuts every segment of `entry_id` that runs across `at` in two, giving
    /// the later piece to `owner` (or to no entry). When the piece is the one
    /// capture is still writing, capture carries on in the piece.
    pub(super) fn cut_segments(
        &mut self,
        entry_id: &str,
        at: u64,
        owner: Option<&str>,
    ) -> Result<(), String> {
        let crossing: Vec<i64> = {
            let mut statement = self
                .conn
                .prepare(
                    "SELECT id FROM segments
                     WHERE entry_id = ?1 AND started_at < ?2 AND (ended_at IS NULL OR ended_at > ?2);",
                )
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map(params![entry_id, at as i64], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?
        };
        for id in crossing {
            self.cut_segment(id, entry_id, at, owner)?;
        }
        Ok(())
    }

    /// Cuts one segment of `entry_id` at `at`; see `cut_segments`.
    pub(super) fn cut_segment(
        &mut self,
        id: i64,
        entry_id: &str,
        at: u64,
        owner: Option<&str>,
    ) -> Result<(), String> {
        let segment = self
            .conn
            .query_row(
                "SELECT id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id
                 FROM segments WHERE id = ?1 AND entry_id = ?2;",
                params![id, entry_id],
                segment_from_row,
            )
            .map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "UPDATE segments SET ended_at = ?1 WHERE id = ?2;",
                params![at as i64, segment.id],
            )
            .map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "INSERT INTO segments (app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12);",
                params![
                    segment.app,
                    segment.title,
                    segment.kind,
                    segment.label,
                    at as i64,
                    segment.ended_at.map(|end| end as i64),
                    segment.reviewed,
                    segment.app_id,
                    segment.bundle_id,
                    segment.url,
                    segment.domain,
                    owner,
                ],
            )
            .map_err(|e| e.to_string())?;
        let piece = self.conn.last_insert_rowid();
        if let Some(current) = self.current.as_mut().filter(|c| c.id == segment.id) {
            current.id = piece;
            current.started_at = at;
        }
        Ok(())
    }

    /// Gives a closed entry with a template description the one the builder
    /// would have: a live session is only "Untitled session" until it closes.
    pub(super) fn describe_closed(&self, id: &str) -> Result<(), String> {
        let entry = self.time_entry(id)?;
        if entry.status == "building" || entry.description_origin != "template" {
            return Ok(());
        }
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, app, title, kind, label, started_at, ended_at, entry_id
                 FROM segments WHERE entry_id = ?1 ORDER BY started_at;",
            )
            .map_err(|e| e.to_string())?;
        let segments = statement
            .query_map([id], |row| {
                Ok(SegmentInput {
                    id: row.get(0)?,
                    app: row.get(1)?,
                    title: row.get(2)?,
                    kind: row.get(3)?,
                    label: row.get(4)?,
                    started_at: row.get::<_, i64>(5)? as u64,
                    ended_at: Some(
                        row.get::<_, Option<i64>>(6)?
                            .map_or(entry.ended_at, |end| end as u64),
                    ),
                    entry_id: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        if segments.iter().all(|segment| segment.kind == "break") {
            return Ok(());
        }
        self.conn
            .execute(
                "UPDATE time_entries SET description = ?1 WHERE id = ?2;",
                params![describe(&segments), id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
