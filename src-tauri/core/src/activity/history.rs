//! The session's undo history. Every user change to time entries is one
//! `Command`, applied inside a savepoint so it lands whole or not at all, and
//! undone or redone the same way:
//!
//! - `Edit`: field changes (accept, unapprove, reject, category, project,
//!   billable, description, times) and new entries, captured generically as
//!   each entry's row before and after. Undo writes back only the columns the
//!   change touched, after checking nothing else changed them since. A change
//!   that claims minutes from other entries (see `claim.rs`) records those
//!   entries, and the segments that changed hands, in the same `Edit`, so one
//!   Undo gives every minute back.
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

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};

use super::{deletions::Deletion, merge_gap, segment_from_row, ActivityStore, LOCKED};
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

    fn approved(&self) -> bool {
        self.text(STATUS) == "approved"
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
#[derive(Clone)]
pub(super) struct Edit {
    label: &'static str,
    entries: Vec<Change>,
    /// Segments that moved between entries along the way.
    handovers: Vec<Handover>,
    /// Suggestion outcomes the change recorded (accepted, rejected,
    /// changed): (suggestion id, before, after).
    outcomes: Vec<(String, Option<String>, Option<String>)>,
    /// What the last undo or redo of this change took from other entries to
    /// give its own minutes back: a freed stretch may have been filled since
    /// (capture rebuilds it into a new entry). The next step gives it back first.
    taken: Option<Box<Edit>>,
}

#[derive(Clone)]
struct Change {
    id: String,
    /// `None`: the change created this entry.
    before: Option<EntryRow>,
    /// `None`: the change removed this entry.
    after: Option<EntryRow>,
    /// A created or removed entry while it is absent.
    stash: Option<Stash>,
}

/// A segment that changed owner: another entry, or none.
#[derive(Clone)]
struct Handover {
    segment: i64,
    before: Option<String>,
    after: Option<String>,
}

/// What a change touched, collected while it runs and turned into an `Edit`
/// once it has finished.
#[derive(Default)]
pub(super) struct Journal {
    /// Every entry the change touched, as it was before; `None` if it did not
    /// exist.
    entries: BTreeMap<String, Option<EntryRow>>,
    /// The entries the change removed.
    removed: BTreeMap<String, Stash>,
    /// Who owned each segment before the change moved it.
    segments: BTreeMap<i64, Option<String>>,
}

impl Journal {
    fn into_edit(
        self,
        label: &'static str,
        conn: &Connection,
        outcomes: Vec<(String, Option<String>, Option<String>)>,
    ) -> Result<Option<Edit>, String> {
        let Self {
            entries,
            mut removed,
            segments,
        } = self;
        let mut changes = Vec::new();
        for (id, before) in entries {
            let after = EntryRow::read(conn, &id)?;
            if before != after {
                changes.push(Change {
                    stash: removed.remove(&id),
                    id,
                    before,
                    after,
                });
            }
        }
        let mut handovers = Vec::new();
        for (segment, before) in segments {
            let after: Option<String> = conn
                .query_row(
                    "SELECT entry_id FROM segments WHERE id = ?1;",
                    [segment],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if before != after {
                handovers.push(Handover {
                    segment,
                    before,
                    after,
                });
            }
        }
        Ok(
            (!changes.is_empty() || !outcomes.is_empty()).then_some(Edit {
                label,
                entries: changes,
                handovers,
                outcomes,
                taken: None,
            }),
        )
    }
}

/// `id` has just become live again. If another live session has been
/// recording since, they are one session when nothing sits between them: the
/// newer one is folded into `id` (its activity moves over and it goes, as a
/// rebuild would have done). Otherwise `id` is closed. Returns whether `id`
/// is still live.
fn settle_live(conn: &Connection, id: &str, now: u64) -> Result<bool, String> {
    let newer: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT id, started_at, ended_at FROM time_entries
             WHERE status = 'building' AND deleted_at IS NULL AND id != ?1;",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((other, other_start, other_end)) = newer else {
        return Ok(true);
    };
    let (start, end): (i64, i64) = conn
        .query_row(
            "SELECT started_at, ended_at FROM time_entries WHERE id = ?1;",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let between: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM time_entries
             WHERE deleted_at IS NULL AND id NOT IN (?1, ?2)
               AND started_at < ?3 AND ended_at > ?4);",
            params![id, other, other_start, end],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if other_start >= start && !between {
        conn.execute(
            "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2;",
            params![id, other],
        )
        .map_err(|e| e.to_string())?;
        Stash::take(conn, &other)?;
        conn.execute(
            "UPDATE time_entries SET ended_at = MAX(ended_at, ?1), updated_at = ?2 WHERE id = ?3;",
            params![other_end, now as i64, id],
        )
        .map_err(|e| e.to_string())?;
        return Ok(true);
    }
    conn.execute(
        "UPDATE time_entries SET status = 'pending', updated_at = ?1 WHERE id = ?2;",
        params![now as i64, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(false)
}

impl Edit {
    fn apply(&mut self, conn: &Connection, undo: bool, now: u64) -> Result<(), String> {
        // Entries come back first, so everything below finds them; the ones
        // that go wait until their segments have been handed on.
        for change in &mut self.entries {
            let (from, to) = Self::direction(change, undo);
            if from.is_none() && to.is_some() {
                change.stash.as_ref().ok_or(CONFLICT)?.restore(conn)?;
                let restored = EntryRow::editable(conn, &change.id)?;
                super::ensure_no_approved_overlap(
                    conn,
                    restored.int(STARTED),
                    restored.int(ENDED),
                    &[],
                )?;
                change.stash = None;
            }
        }
        for handover in &self.handovers {
            let (from, to) = if undo {
                (&handover.after, &handover.before)
            } else {
                (&handover.before, &handover.after)
            };
            // A segment capture has since linked elsewhere stays where it is.
            conn.execute(
                "UPDATE segments SET entry_id = ?1 WHERE id = ?2 AND entry_id IS ?3;",
                params![to, handover.segment, from],
            )
            .map_err(|e| e.to_string())?;
        }
        for change in &mut self.entries {
            let (from, to) = Self::direction(change, undo);
            let (Some(from), Some(to)) = (from, to) else {
                continue;
            };
            let current = EntryRow::editable(conn, &change.id)?;
            let touched: Vec<usize> = (1..COLUMNS.len())
                .filter(|&i| from.0[i] != to.0[i])
                .collect();
            // An approved entry may only leave approval (undoing an Accept);
            // any other change to it waits for an unapprove.
            if current.approved() && !(touched.contains(&STATUS) && !to.approved()) {
                return Err(LOCKED.to_string());
            }
            for &column in &touched {
                // The builder owns a live entry's times and template text.
                let live = matches!(column, STARTED | ENDED | DESCRIPTION)
                    && current.building()
                    && from.building();
                // Queued for classification since: still the same pending entry.
                let classifying = column == STATUS
                    && from.text(STATUS) == "pending"
                    && current.text(STATUS) == "processing";
                if !live && !classifying && current.0[column] != from.0[column] {
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
        for change in &mut self.entries {
            let (from, to) = Self::direction(change, undo);
            if from.is_some() && to.is_none() {
                if EntryRow::editable(conn, &change.id)?.approved() {
                    return Err(LOCKED.to_string());
                }
                change.stash = Some(Stash::take(conn, &change.id)?);
            }
        }
        // Capture may have carried on in a newer live session since: an entry
        // that comes back live joins it, or, when something sits between them,
        // stays closed. Never two live sessions.
        for change in &mut self.entries {
            let (from, to) = Self::direction(change, undo);
            let (Some(from), Some(to)) = (from, to) else {
                continue;
            };
            if to.building() && !from.building() && !settle_live(conn, &change.id, now)? {
                let mut row = to.clone();
                row.0[STATUS] = Value::Text("pending".to_string());
                // The change now reads as what it did, so the way back is exact.
                if undo {
                    change.before = Some(row);
                } else {
                    change.after = Some(row);
                }
            }
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

    /// The entry's row before and after this step.
    fn direction(change: &Change, undo: bool) -> (Option<&EntryRow>, Option<&EntryRow>) {
        if undo {
            (change.after.as_ref(), change.before.as_ref())
        } else {
            (change.before.as_ref(), change.after.as_ref())
        }
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

/// What a merge hands back: the merged entry, the secondary's stash, the
/// secondary's and the primary's segments, and what it took from others.
type MergeOutcome = (TimeEntry, Stash, Vec<i64>, Vec<i64>, Option<Edit>);

/// A merge: `primary` absorbed `secondary`. The secondary's segments are
/// remembered by id rather than by where they sit in time, because overlapping
/// entries interleave in time.
pub(super) struct Merge {
    primary: String,
    secondary: String,
    /// The secondary's segments when it was absorbed.
    segments: Vec<i64>,
    /// The primary's own segments when the merge happened.
    kept: Vec<i64>,
    /// Whether the secondary was the live entry, whose later segments return
    /// to it on undo.
    secondary_live: bool,
    /// When the merge happened; segments recorded after it belong to the live half.
    merged_at: u64,
    before: EntryRow,
    after: EntryRow,
    /// The secondary while the merge stands.
    stash: Option<Stash>,
    /// What the merge took from other entries: the gap it spans.
    claimed: Option<Edit>,
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
    /// `ids` must name every entry `op` touches itself; entries it takes
    /// minutes from (`claim`) are recorded as it goes.
    pub(super) fn edit<T>(
        &mut self,
        label: &'static str,
        ids: &[String],
        op: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let ids: BTreeSet<String> = ids.iter().cloned().collect();
        let result = self.atomically(|store| {
            store.journal = Some(Journal::default());
            for id in &ids {
                store.note_entry(id)?;
            }
            let outcomes_before = outcomes(&store.conn, &ids)?;
            let value = op(store)?;
            let journal = store.journal.take().unwrap_or_default();
            let outcomes = outcomes(&store.conn, &ids)?
                .into_iter()
                .filter_map(|(id, after)| {
                    let before = outcomes_before.get(&id)?.clone();
                    (before != after).then_some((id, before, after))
                })
                .collect::<Vec<_>>();
            Ok((value, journal.into_edit(label, &store.conn, outcomes)?))
        });
        self.journal = None;
        let (value, command) = result?;
        if let Some(edit) = command {
            self.record_undo(Command::Edit(edit));
        }
        Ok(value)
    }

    /// Remembers an entry as it is now, for the `Edit` being journaled. Call
    /// it before the first change to the entry.
    pub(super) fn note_entry(&mut self, id: &str) -> Result<(), String> {
        if self
            .journal
            .as_ref()
            .is_none_or(|journal| journal.entries.contains_key(id))
        {
            return Ok(());
        }
        let row = EntryRow::read(&self.conn, id)?;
        if let Some(journal) = self.journal.as_mut() {
            journal.entries.insert(id.to_string(), row);
        }
        Ok(())
    }

    /// Remembers who owns a segment now, before the journaled change moves it.
    pub(super) fn note_segment(&mut self, segment: i64, owner: Option<&str>) {
        if let Some(journal) = self.journal.as_mut() {
            journal
                .segments
                .entry(segment)
                .or_insert_with(|| owner.map(str::to_string));
        }
    }

    /// Remembers every segment an entry owns now.
    pub(super) fn note_segments_of(&mut self, entry_id: &str) -> Result<(), String> {
        if self.journal.is_none() {
            return Ok(());
        }
        let mut statement = self
            .conn
            .prepare("SELECT id FROM segments WHERE entry_id = ?1;")
            .map_err(|e| e.to_string())?;
        let ids = statement
            .query_map([entry_id], |row| row.get::<_, i64>(0))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        drop(statement);
        for id in ids {
            self.note_segment(id, Some(entry_id));
        }
        Ok(())
    }

    /// Removes an entry for good and keeps it for the journal, so the change
    /// that removed it can bring it back. Its segments are left unowned.
    pub(super) fn remove_entry(&mut self, id: &str) -> Result<(), String> {
        self.note_entry(id)?;
        self.note_segments_of(id)?;
        let stash = Stash::take(&self.conn, id)?;
        if let Some(journal) = self.journal.as_mut() {
            journal.removed.insert(id.to_string(), stash);
        }
        Ok(())
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
        let overlapping = self.overlapping_pairs()?;
        let touched: Vec<String> = match command {
            Command::Deletion(deletion) => {
                deletion.apply(&self.conn, undo, now)?;
                if !undo {
                    self.forget_deleted_segment(deletion);
                }
                Vec::new()
            }
            Command::Edit(edit) => {
                // Applied to a copy, so a step that fails leaves the command
                // as it was along with the database.
                let mut stepped = edit.clone();
                if let Some(taken) = stepped.taken.as_mut() {
                    taken.apply(&self.conn, true, now)?;
                }
                stepped.taken = None;
                stepped.apply(&self.conn, undo, now)?;
                stepped.taken = self
                    .reclaim(&stepped, undo, &overlapping, now)?
                    .map(Box::new);
                *edit = stepped;
                edit.entries
                    .iter()
                    .filter(|change| change.before.is_some() && change.after.is_some())
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
                    if let Some(claimed) = merge.claimed.as_mut() {
                        claimed.apply(&self.conn, true, now)?;
                    }
                } else {
                    let (_, stash, segments, kept, claimed) =
                        self.merge_claiming(&merge.primary, &merge.secondary, now)?;
                    merge.stash = Some(stash);
                    merge.segments = segments;
                    merge.kept = kept;
                    merge.claimed = claimed;
                    merge.merged_at = now;
                    merge.after = EntryRow::read(&self.conn, &merge.primary)?.ok_or(CONFLICT)?;
                }
                vec![merge.primary.clone()]
            }
        };
        if !matches!(command, Command::Edit(_)) {
            // (Edits settle theirs while recording them, above.)
            let winners = match command {
                Command::Split(split) => vec![split.original.clone(), split.second.clone()],
                Command::Merge(merge) => vec![merge.primary.clone(), merge.secondary.clone()],
                _ => Vec::new(),
            };
            self.journal = Some(Journal::default());
            let settled = self.settle_overlaps(&overlapping, &winners, now);
            self.journal = None;
            settled?;
        }
        let kind = if undo { "undone" } else { "redone" };
        let payload = serde_json::json!({ "action": label }).to_string();
        for id in touched {
            self.log_event(&id, kind, "user", Some(&payload), now);
        }
        Ok(())
    }

    /// After a step, claims the minutes it gave entries back from anyone who
    /// holds them now, so undo and redo keep one owner per minute too.
    fn reclaim(
        &mut self,
        edit: &Edit,
        undo: bool,
        overlapping: &BTreeSet<(String, String)>,
        now: u64,
    ) -> Result<Option<Edit>, String> {
        let mut claims: Vec<(&str, Vec<(u64, u64)>)> = Vec::new();
        for change in &edit.entries {
            let (from, to) = Edit::direction(change, undo);
            let Some(to) = to else {
                continue;
            };
            let held: Vec<(u64, u64)> = from
                .map(|row| (row.int(STARTED), row.int(ENDED)))
                .into_iter()
                .collect();
            let gained = super::claim::gained(&held, to.int(STARTED), to.int(ENDED));
            if !gained.is_empty() {
                claims.push((&change.id, gained));
            }
        }
        // Each entry claims for itself, so an entry the same edit trimmed that
        // the live session has since grown back into gives that up too.
        self.journal = Some(Journal::default());
        let mut claimed = Ok(());
        for (id, ranges) in &claims {
            claimed = self.claim(&[id], ranges, now);
            if claimed.is_err() {
                break;
            }
        }
        let winners: Vec<String> = edit
            .entries
            .iter()
            .filter(|change| Edit::direction(change, undo).1.is_some())
            .map(|change| change.id.clone())
            .collect();
        if claimed.is_ok() {
            claimed = self.settle_overlaps(overlapping, &winners, now);
        }
        let journal = self.journal.take().unwrap_or_default();
        claimed?;
        journal.into_edit(edit.label, &self.conn, Vec::new())
    }

    /// Every pair of entries that share time.
    fn overlapping_pairs(&self) -> Result<BTreeSet<(String, String)>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT a.id, b.id FROM time_entries a JOIN time_entries b
                   ON a.id < b.id AND a.started_at < b.ended_at AND b.started_at < a.ended_at
                  AND a.deleted_at IS NULL AND b.deleted_at IS NULL;",
            )
            .map_err(|e| e.to_string())?;
        let pairs = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;
        Ok(pairs)
    }

    /// The last word of undo and redo: whatever it did, no two entries share
    /// time that did not share it before. Where the step brought back an
    /// entry (`winners`) over time another holds, the restored entry keeps it
    /// and the other gives it up (or the step is refused when that is
    /// approved). Time two other entries share is the newer entry's to give.
    fn settle_overlaps(
        &mut self,
        before: &BTreeSet<(String, String)>,
        winners: &[String],
        now: u64,
    ) -> Result<(), String> {
        for (a, b) in self.overlapping_pairs()?.difference(before) {
            let (Ok(first), Ok(second)) = (self.time_entry(a), self.time_entry(b)) else {
                continue;
            };
            // Settled by an earlier claim in this loop.
            if first.deleted_at.is_some()
                || second.deleted_at.is_some()
                || first.started_at >= second.ended_at
                || second.started_at >= first.ended_at
            {
                continue;
            }
            let restored = |id: &String| winners.contains(id);
            let winner = match (restored(a), restored(b)) {
                (true, false) => a,
                (false, true) => b,
                (true, true) => continue,
                // Neither came back: the one that is recording yields.
                (false, false) if first.status == "building" => b,
                (false, false) => a,
            };
            let (from, to) = (
                first.started_at.max(second.started_at),
                first.ended_at.min(second.ended_at),
            );
            self.claim(&[winner], &[(from, to)], now)?;
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
        if entry.status == "approved" {
            return Err(LOCKED.to_string());
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
                self.copy_suggestions(id, second)?;
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

    /// Gives `to` its own copy of the suggestions `from` has.
    pub(super) fn copy_suggestions(&self, from: &str, to: &str) -> Result<(), String> {
        let (columns, rows) = select_all(&self.conn, "suggestions", "entry_id", from)?;
        for mut row in rows {
            for (column, value) in columns.iter().zip(row.iter_mut()) {
                match column.as_str() {
                    "id" => *value = Value::Text(uuid::Uuid::now_v7().to_string()),
                    "entry_id" => *value = Value::Text(to.to_string()),
                    _ => {}
                }
            }
            insert(&self.conn, "suggestions", &columns, &row)?;
        }
        Ok(())
    }

    fn unsplit(&mut self, split: &mut Split, now: u64) -> Result<(), String> {
        let first = EntryRow::editable(&self.conn, &split.original)?;
        let second = EntryRow::editable(&self.conn, &split.second)?;
        if first.approved() || second.approved() {
            return Err(LOCKED.to_string());
        }
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
        let (merged, stash, segments, kept, claimed) =
            self.atomically(|store| store.merge_claiming(primary_id, secondary_id, now))?;
        let after = EntryRow::read(&self.conn, primary_id)?.ok_or(CONFLICT)?;
        self.record_undo(Command::Merge(Merge {
            primary: primary_id.to_string(),
            secondary: secondary_id.to_string(),
            segments,
            kept,
            secondary_live: secondary.status == "building",
            merged_at: now,
            before,
            after,
            stash: Some(stash),
            claimed,
        }));
        Ok(merged)
    }

    /// The merge, journaling the minutes it takes from other entries.
    fn merge_claiming(
        &mut self,
        primary_id: &str,
        secondary_id: &str,
        now: u64,
    ) -> Result<MergeOutcome, String> {
        self.journal = Some(Journal::default());
        let merged = self.merge_into(primary_id, secondary_id, now);
        let journal = self.journal.take().unwrap_or_default();
        let (merged, stash, segments, kept) = merged?;
        Ok((
            merged,
            stash,
            segments,
            kept,
            journal.into_edit("Merge", &self.conn, Vec::new())?,
        ))
    }

    fn merge_into(
        &mut self,
        primary_id: &str,
        secondary_id: &str,
        now: u64,
    ) -> Result<(TimeEntry, Stash, Vec<i64>, Vec<i64>), String> {
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
        if primary.status == "approved" || secondary.status == "approved" {
            return Err(LOCKED.to_string());
        }
        // Only a neighbour merges: one that overlaps, touches, or misses by
        // at most MERGE_GAP_MS, on one local day. The merge spans both
        // entries, so that gap becomes tracked time and an overlap is absorbed.
        if merge_gap(&primary, &secondary).is_none() {
            return Err(
                "Time entries can only merge with a neighbouring entry on the same day: they must overlap, touch, or sit no more than five minutes apart".to_string(),
            );
        }

        let live = primary.status == "building" || secondary.status == "building";
        // A captured session can take in its neighbour and keep recording; a
        // hand-made entry cannot be rebuilt, so the live session closes now instead and recording carries on in a new one.
        let keep_live = live && primary.source == "auto" && secondary.source == "auto";
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

        // The merge spans both entries, so it owns the gap between them; any
        // other entry in that gap gives the minutes up.
        let own = |entry: &TimeEntry| (entry.started_at, entry.ended_at);
        let gained = super::claim::gained(&[own(&primary), own(&secondary)], started_at, ended_at);
        self.claim(&[primary_id, secondary_id], &gained, now)?;

        // Activity an earlier edit left linked to an entry from outside the
        // time it now covers is not part of the merged entry: left linked,
        // rebuilding the live merged entry would stretch it back over it.
        for id in [primary_id, secondary_id] {
            self.note_segments_of(id)?;
        }
        self.conn
            .execute(
                "UPDATE segments SET entry_id = NULL
                 WHERE entry_id IN (?1, ?2) AND started_at < ?3;",
                params![primary_id, secondary_id, started_at as i64],
            )
            .map_err(|e| e.to_string())?;

        let owned = |id: &str| -> Result<Vec<i64>, String> {
            let mut statement = self
                .conn
                .prepare("SELECT id FROM segments WHERE entry_id = ?1;")
                .map_err(|e| e.to_string())?;
            let rows = statement
                .query_map([id], |row| row.get(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())
        };
        let kept = owned(primary_id)?;
        let segments = owned(secondary_id)?;
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
        Ok((self.time_entry(primary_id)?, stash, segments, kept))
    }

    fn unmerge(&mut self, merge: &mut Merge, now: u64) -> Result<(), String> {
        let current = EntryRow::editable(&self.conn, &merge.primary)?;
        if current.approved()
            || EntryRow::editable(&self.conn, &merge.secondary).is_ok_and(|row| row.approved())
        {
            return Err(LOCKED.to_string());
        }
        if !current.building()
            && (current.int(STARTED) != merge.after.int(STARTED)
                || current.int(ENDED) != merge.after.int(ENDED))
        {
            return Err(CONFLICT.to_string());
        }
        merge.stash.take().ok_or(CONFLICT)?.restore(&self.conn)?;
        for segment in &merge.segments {
            self.conn
                .execute(
                    "UPDATE segments SET entry_id = ?1 WHERE id = ?2 AND entry_id = ?3;",
                    params![merge.secondary, segment, merge.primary],
                )
                .map_err(|e| e.to_string())?;
        }
        // Activity nobody owned before the merge, which the rebuilt merged
        // entry took in since (it covered the time), is nobody's again.
        let kept: Vec<String> = merge.kept.iter().map(i64::to_string).collect();
        self.conn
            .execute(
                &format!(
                    "UPDATE segments SET entry_id = NULL
                     WHERE entry_id = ?1 AND started_at < ?2 AND id NOT IN ({});",
                    if kept.is_empty() {
                        "0".to_string()
                    } else {
                        kept.join(",")
                    }
                ),
                params![merge.primary, merge.merged_at as i64],
            )
            .map_err(|e| e.to_string())?;
        if merge.secondary_live {
            // Capture kept writing into the merged entry; the live half takes
            // back what it recorded since.
            self.conn
                .execute(
                    "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2 AND started_at >= ?3;",
                    params![merge.secondary, merge.primary, merge.merged_at as i64],
                )
                .map_err(|e| e.to_string())?;
        }
        // Whichever half was live gets the time recorded since the merge, if
        // the merged entry kept recording. One the merge closed has nothing
        // recorded into it, and its end is the other half's.
        let recorded = current.int(ENDED);
        let primary_end = if merge.before.building() && current.building() {
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
        // Capture may have carried on in a newer live session since the merge
        // closed a half that was live: it joins that session, or stays closed.
        for id in [&merge.primary, &merge.secondary] {
            let live: bool = self
                .conn
                .query_row(
                    "SELECT status = 'building' FROM time_entries WHERE id = ?1;",
                    [id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if live {
                settle_live(&self.conn, id, now)?;
            }
        }
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
            self.cut_segment(id, Some(entry_id), at, owner)?;
        }
        Ok(())
    }

    /// Cuts one segment of `entry_id` (or of no entry) at `at`; see
    /// `cut_segments`. Returns the later piece.
    pub(super) fn cut_segment(
        &mut self,
        id: i64,
        entry_id: Option<&str>,
        at: u64,
        owner: Option<&str>,
    ) -> Result<i64, String> {
        let segment = self
            .conn
            .query_row(
                "SELECT id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id
                 FROM segments WHERE id = ?1 AND entry_id IS ?2;",
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
        // Until the change says otherwise, the later piece belongs where the
        // segment did.
        self.note_segment(piece, entry_id);
        if let Some(current) = self.current.as_mut().filter(|c| c.id == segment.id) {
            current.id = piece;
            current.started_at = at;
        }
        Ok(piece)
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
