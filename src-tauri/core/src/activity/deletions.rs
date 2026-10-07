//! Session-local undo for deletions. Only the fields deletion changes are
//! captured, so restoring a catalog link never overwrites later descriptions.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection};

use super::{segment_from_row, ActivityStore};
use crate::models::ActivitySegment;

#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq)]
struct Links {
    category: Option<String>,
    project: Option<String>,
    deleted: Option<i64>,
}

struct EntryChange {
    id: String,
    before: Links,
    after: Links,
}

struct CategoryChange {
    id: String,
    archived: bool,
}

#[derive(Default)]
pub(super) struct Deletion {
    at: i64,
    entries: Vec<EntryChange>,
    categories: Vec<CategoryChange>,
    projects: Vec<String>,
    rules: Vec<String>,
    apps: Vec<(String, Option<String>, Option<String>)>,
    segments: Vec<ActivitySegment>,
}

impl Deletion {
    fn capture(
        conn: &Connection,
        ids: &[String],
        category: Option<&str>,
        project: Option<&str>,
        now: u64,
    ) -> Result<Self, String> {
        let mut deletion = Self {
            at: now as i64,
            ..Self::default()
        };
        let mut categories = BTreeSet::new();
        let mut projects = BTreeSet::new();
        if let Some(id) = category {
            categories.insert(id.to_owned());
        }
        if let Some(id) = project {
            projects.insert(id.to_owned());
        }
        for id in ids.iter().collect::<BTreeSet<_>>() {
            let links: Links = conn.query_row(
                "SELECT category_id, project_id, deleted_at FROM time_entries WHERE id = ?1 AND deleted_at IS NULL;",
                [id], |row| Ok(Links { category: row.get(0)?, project: row.get(1)?, deleted: row.get(2)? }),
            ).map_err(|error| error.to_string())?;
            if let Some(id) = links.category {
                categories.insert(id);
            }
            if let Some(id) = links.project {
                projects.insert(id);
            }
            let mut statement = conn.prepare(
                "SELECT id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id FROM segments WHERE entry_id = ?1;"
            ).map_err(|error| error.to_string())?;
            let segments = statement
                .query_map([id], segment_from_row)
                .map_err(|error| error.to_string())?;
            for segment in segments {
                let mut segment = segment.map_err(|error| error.to_string())?;
                // A live entry can be deleted, but undo must not revive an
                // orphaned open segment after capture has moved on.
                segment.ended_at.get_or_insert(now);
                deletion.segments.push(segment);
            }
        }
        for id in &categories {
            let archived = conn.query_row(
                "SELECT archived FROM categories WHERE id = ?1 AND deleted_at IS NULL;",
                [id],
                |row| row.get::<_, bool>(0),
            );
            match archived {
                Ok(archived) => deletion.categories.push(CategoryChange {
                    id: id.clone(),
                    archived,
                }),
                Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        for id in &projects {
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1 AND deleted_at IS NULL);",
                    [id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            if exists {
                deletion.projects.push(id.clone());
            }
        }
        // A catalog can be shared: unlink other entries, do not delete their time.
        let mut affected = BTreeMap::new();
        for (column, values) in [
            ("id", ids.to_vec()),
            ("category_id", categories.into_iter().collect()),
            ("project_id", projects.into_iter().collect()),
        ] {
            for value in values {
                let mut statement = conn.prepare(&format!(
                    "SELECT id, category_id, project_id, deleted_at, invoice_id FROM time_entries WHERE {column} = ?1 AND deleted_at IS NULL;"
                )).map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map([value], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            Links {
                                category: row.get(1)?,
                                project: row.get(2)?,
                                deleted: row.get(3)?,
                            },
                            row.get::<_, Option<String>>(4)?,
                        ))
                    })
                    .map_err(|error| error.to_string())?;
                for row in rows {
                    let (id, links, invoice) = row.map_err(|error| error.to_string())?;
                    if invoice.is_some() {
                        return Err("This deletion affects invoiced time. Remove its draft invoice first; finalized time cannot be deleted.".to_string());
                    }
                    affected.insert(id, links);
                }
            }
        }
        for (id, before) in affected {
            let mut after = before.clone();
            if ids.contains(&id) {
                after.deleted = Some(deletion.at);
            }
            if deletion
                .categories
                .iter()
                .any(|item| Some(&item.id) == before.category.as_ref())
            {
                after.category = None;
            }
            if deletion
                .projects
                .iter()
                .any(|item| Some(item) == before.project.as_ref())
            {
                after.project = None;
            }
            deletion.entries.push(EntryChange { id, before, after });
        }
        for (column, values) in [
            (
                "category_id",
                deletion
                    .categories
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<Vec<_>>(),
            ),
            ("project_id", deletion.projects.clone()),
        ] {
            for value in values {
                let mut statement = conn
                    .prepare(&format!(
                        "SELECT id FROM rules WHERE {column} = ?1 AND deleted_at IS NULL;"
                    ))
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map([&value], |row| row.get::<_, String>(0))
                    .map_err(|error| error.to_string())?;
                for row in rows {
                    let id = row.map_err(|error| error.to_string())?;
                    if !deletion.rules.contains(&id) {
                        deletion.rules.push(id);
                    }
                }
                let mut statement = conn.prepare(&format!("SELECT id, default_category_id, default_project_id FROM apps WHERE default_{column} = ?1;"))
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map([&value], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    })
                    .map_err(|error| error.to_string())?;
                for row in rows {
                    let item = row.map_err(|error| error.to_string())?;
                    if !deletion.apps.iter().any(|app| app.0 == item.0) {
                        deletion.apps.push(item);
                    }
                }
            }
        }
        Ok(deletion)
    }

    fn apply(&self, conn: &Connection, undo: bool, now: u64) -> Result<(), String> {
        let conflict = "The deleted items have changed since this operation; undo/redo cannot safely overwrite them.";
        if !undo {
            // Recording may have resumed into a restored building entry. Do
            // not redo against new or edited activity that undo cannot restore.
            for entry in self
                .entries
                .iter()
                .filter(|entry| entry.after.deleted.is_some())
            {
                let mut statement = conn.prepare("SELECT id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id FROM segments WHERE entry_id = ?1;")
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map([&entry.id], segment_from_row)
                    .map_err(|error| error.to_string())?;
                let current = rows
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(|error| error.to_string())?;
                let expected: Vec<_> = self
                    .segments
                    .iter()
                    .filter(|segment| segment.entry_id.as_ref() == Some(&entry.id))
                    .collect();
                if current.len() != expected.len() {
                    return Err(conflict.to_string());
                }
                for mut segment in current {
                    // Initial deletion closes a live segment in its snapshot.
                    segment.ended_at.get_or_insert(self.at as u64);
                    if !expected.iter().any(|expected| **expected == segment) {
                        return Err(conflict.to_string());
                    }
                }
            }
        }
        for entry in &self.entries {
            let (from, to) = if undo {
                (&entry.after, &entry.before)
            } else {
                (&entry.before, &entry.after)
            };
            let count = conn.execute(
                "UPDATE time_entries SET category_id = ?1, project_id = ?2, deleted_at = ?3, updated_at = ?4 WHERE id = ?5 AND category_id IS ?6 AND project_id IS ?7 AND deleted_at IS ?8 AND invoice_id IS NULL;",
                params![to.category, to.project, to.deleted, now as i64, entry.id, from.category, from.project, from.deleted],
            ).map_err(|error| error.to_string())?;
            if count != 1 {
                return Err(conflict.to_string());
            }
        }
        let (from, to) = if undo {
            (Some(self.at), None)
        } else {
            (None, Some(self.at))
        };
        for category in &self.categories {
            let count = conn.execute(
                "UPDATE categories SET deleted_at = ?1, archived = ?2, updated_at = ?3 WHERE id = ?4 AND deleted_at IS ?5;",
                params![to, if undo { category.archived } else { true }, now as i64, category.id, from],
            ).map_err(|error| error.to_string())?;
            if count != 1 {
                return Err(conflict.to_string());
            }
        }
        for (table, ids) in [("projects", &self.projects), ("rules", &self.rules)] {
            for id in ids {
                let count = conn.execute(&format!("UPDATE {table} SET deleted_at = ?1, updated_at = ?2 WHERE id = ?3 AND deleted_at IS ?4;"), params![to, now as i64, id, from])
                    .map_err(|error| error.to_string())?;
                if count != 1 {
                    return Err(conflict.to_string());
                }
            }
        }
        for (id, category, project) in &self.apps {
            let after_category = category
                .as_ref()
                .filter(|id| !self.categories.iter().any(|item| &item.id == *id));
            let after_project = project.as_ref().filter(|id| !self.projects.contains(id));
            let (from_category, from_project, to_category, to_project) = if undo {
                (
                    after_category,
                    after_project,
                    category.as_ref(),
                    project.as_ref(),
                )
            } else {
                (
                    category.as_ref(),
                    project.as_ref(),
                    after_category,
                    after_project,
                )
            };
            let count = conn.execute("UPDATE apps SET default_category_id = ?1, default_project_id = ?2, updated_at = ?3 WHERE id = ?4 AND default_category_id IS ?5 AND default_project_id IS ?6;", params![to_category, to_project, now as i64, id, from_category, from_project])
                .map_err(|error| error.to_string())?;
            if count != 1 {
                return Err(conflict.to_string());
            }
        }
        for segment in &self.segments {
            if undo {
                conn.execute("INSERT INTO segments (id, app, title, kind, label, started_at, ended_at, reviewed, app_id, bundle_id, url, domain, entry_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13);",
                    params![segment.id, segment.app, segment.title, segment.kind, segment.label, segment.started_at as i64, segment.ended_at.map(|value| value as i64), segment.reviewed, segment.app_id, segment.bundle_id, segment.url, segment.domain, segment.entry_id])
                    .map_err(|error| error.to_string())?;
            } else {
                conn.execute(
                    "DELETE FROM segments WHERE id = ?1 AND entry_id IS ?2;",
                    params![segment.id, segment.entry_id],
                )
                .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
}

pub(super) enum UndoItem {
    Deletion(Deletion),
    Split {
        original_id: String,
        original_ended_at: u64,
        new_id: String,
        split_at: u64,
    },
    Add {
        id: String,
    },
    Merge {
        primary_id: String,
        secondary_id: String,
        primary_original_started_at: u64,
        primary_original_ended_at: u64,
        secondary_original_started_at: u64,
        secondary_original_ended_at: u64,
        secondary_segment_ids: Vec<String>,
    },
}

impl ActivityStore {
    pub(super) fn delete_undoable(
        &mut self,
        ids: &[String],
        category: Option<&str>,
        project: Option<&str>,
        now: u64,
    ) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|error| error.to_string())?;
        let deletion = Deletion::capture(&tx, ids, category, project, now)?;
        if deletion.entries.is_empty()
            && deletion.categories.is_empty()
            && deletion.projects.is_empty()
        {
            return Err("Those items no longer exist.".to_string());
        }
        deletion.apply(&tx, false, now)?;
        tx.commit().map_err(|error| error.to_string())?;
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.entry_id.as_ref().is_some_and(|id| ids.contains(id)))
        {
            self.current = None;
            self.pending_switch = None;
        }
        self.record_undo(UndoItem::Deletion(deletion));
        Ok(())
    }

    pub fn undo_deletion(&mut self, now: u64) -> Result<bool, String> {
        let Some(item) = self.undo_stack.last() else {
            return Ok(false);
        };
        let tx = self.conn.transaction().map_err(|error| error.to_string())?;
        match item {
            UndoItem::Deletion(deletion) => {
                deletion.apply(&tx, true, now)?;
            }
            UndoItem::Split {
                original_id,
                original_ended_at,
                new_id,
                ..
            } => {
                tx.execute(
                    "UPDATE time_entries SET ended_at = ?1, updated_at = ?2 WHERE id = ?3;",
                    params![*original_ended_at as i64, now as i64, original_id],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2;",
                    params![original_id, new_id],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "UPDATE time_entries SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2;",
                    params![now as i64, new_id],
                )
                .map_err(|e| e.to_string())?;
            }
            UndoItem::Add { id } => {
                tx.execute(
                    "UPDATE time_entries SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2;",
                    params![now as i64, id],
                )
                .map_err(|e| e.to_string())?;
            }
            UndoItem::Merge {
                primary_id,
                secondary_id,
                primary_original_started_at,
                primary_original_ended_at,
                secondary_original_started_at,
                secondary_original_ended_at,
                secondary_segment_ids,
            } => {
                tx.execute(
                    "UPDATE time_entries SET started_at = ?1, ended_at = ?2, updated_at = ?3 WHERE id = ?4;",
                    params![
                        *primary_original_started_at as i64,
                        *primary_original_ended_at as i64,
                        now as i64,
                        primary_id
                    ],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "UPDATE time_entries SET deleted_at = NULL, started_at = ?1, ended_at = ?2, updated_at = ?3 WHERE id = ?4;",
                    params![
                        *secondary_original_started_at as i64,
                        *secondary_original_ended_at as i64,
                        now as i64,
                        secondary_id
                    ],
                )
                .map_err(|e| e.to_string())?;
                for seg_id in secondary_segment_ids {
                    tx.execute(
                        "UPDATE segments SET entry_id = ?1 WHERE id = ?2;",
                        params![secondary_id, seg_id],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        tx.commit().map_err(|error| error.to_string())?;

        let item = self.undo_stack.pop().unwrap();
        match &item {
            UndoItem::Split {
                original_id,
                new_id,
                ..
            } => {
                if self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.entry_id.as_deref() == Some(new_id))
                {
                    if let Some(ref mut c) = self.current {
                        c.entry_id = Some(original_id.clone());
                    }
                }
                self.log_event(original_id, "merged", "user", None, now);
            }
            UndoItem::Add { id } => {
                if self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.entry_id.as_deref() == Some(id))
                {
                    self.current = None;
                    self.pending_switch = None;
                }
            }
            UndoItem::Merge { primary_id, .. } => {
                self.log_event(primary_id, "split", "user", None, now);
            }
            UndoItem::Deletion(_) => {}
        }

        self.redo_stack.push(item);
        Ok(true)
    }

    pub fn redo_deletion(&mut self, now: u64) -> Result<bool, String> {
        let Some(item) = self.redo_stack.last() else {
            return Ok(false);
        };
        let tx = self.conn.transaction().map_err(|error| error.to_string())?;
        match item {
            UndoItem::Deletion(deletion) => {
                deletion.apply(&tx, false, now)?;
            }
            UndoItem::Split {
                original_id,
                split_at,
                new_id,
                ..
            } => {
                tx.execute(
                    "UPDATE time_entries SET ended_at = ?1, updated_at = ?2 WHERE id = ?3;",
                    params![*split_at as i64, now as i64, original_id],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "UPDATE time_entries SET deleted_at = NULL, updated_at = ?1 WHERE id = ?2;",
                    params![now as i64, new_id],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2 AND started_at >= ?3;",
                    params![new_id, original_id, *split_at as i64],
                )
                .map_err(|e| e.to_string())?;
            }
            UndoItem::Add { id } => {
                tx.execute(
                    "UPDATE time_entries SET deleted_at = NULL, updated_at = ?1 WHERE id = ?2;",
                    params![now as i64, id],
                )
                .map_err(|e| e.to_string())?;
            }
            UndoItem::Merge {
                primary_id,
                secondary_id,
                primary_original_started_at,
                primary_original_ended_at,
                secondary_original_started_at,
                secondary_original_ended_at,
                ..
            } => {
                let merged_started_at =
                    primary_original_started_at.min(secondary_original_started_at);
                let merged_ended_at = primary_original_ended_at.max(secondary_original_ended_at);
                tx.execute(
                    "UPDATE time_entries SET started_at = ?1, ended_at = ?2, updated_at = ?3 WHERE id = ?4;",
                    params![
                        *merged_started_at as i64,
                        *merged_ended_at as i64,
                        now as i64,
                        primary_id
                    ],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2;",
                    params![primary_id, secondary_id],
                )
                .map_err(|e| e.to_string())?;
                tx.execute(
                    "UPDATE time_entries SET deleted_at = ?1, updated_at = ?2 WHERE id = ?3;",
                    params![now as i64, now as i64, secondary_id],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|error| error.to_string())?;

        let item = self.redo_stack.pop().unwrap();
        match &item {
            UndoItem::Split { original_id, .. } => {
                self.log_event(original_id, "split", "user", None, now);
            }
            UndoItem::Merge { primary_id, .. } => {
                self.log_event(primary_id, "merged", "user", None, now);
            }
            UndoItem::Add { .. } | UndoItem::Deletion(_) => {}
        }

        self.undo_stack.push(item);
        Ok(true)
    }
}
