//! One owner per minute: an entry that gains minutes claims them, and others give them up
//! (approved entries refuse the change instead). Only ownership of captured activity
//! changes, recorded in the change's `Edit` so Undo restores it.

use rusqlite::{named_params, params};

use super::{ensure_no_approved_overlap, ActivityStore};

pub(super) fn gained(held: &[(u64, u64)], start: u64, end: u64) -> Vec<(u64, u64)> {
    let mut held: Vec<(u64, u64)> = held.iter().copied().filter(|(s, e)| s < e).collect();
    held.sort_unstable();
    let mut gained = Vec::new();
    let mut cursor = start;
    for (held_start, held_end) in held {
        if cursor >= end || held_start >= end {
            break;
        }
        if held_end <= cursor {
            continue;
        }
        if held_start > cursor {
            gained.push((cursor, held_start));
        }
        cursor = held_end;
    }
    if cursor < end {
        gained.push((cursor, end));
    }
    gained
}

const HELD_END: &str = "(CASE WHEN status = 'building' THEN MAX(ended_at, COALESCE((SELECT MAX(COALESCE(s.ended_at, :now)) FROM segments s WHERE s.entry_id = time_entries.id), 0)) ELSE ended_at END)";

impl ActivityStore {
    /// Takes the minutes in `ranges` from every entry but `except`; fails, changing nothing, when
    /// an approved one holds them.
    pub(super) fn claim(
        &mut self,
        except: &[&str],
        ranges: &[(u64, u64)],
        now: u64,
    ) -> Result<(), String> {
        for &(start, end) in ranges {
            ensure_no_approved_overlap(&self.conn, start, end, except)?;
        }
        for &(start, end) in ranges {
            for id in self.holders(except, start, end, now)? {
                self.displace(&id, start, end, now)?;
            }
            self.close_capture_before(start, end, now)?;
        }
        Ok(())
    }

    /// Entries holding any of `start..end`, earliest first. A live entry holds through the end of
    /// its activity (to now if open) plus the moment it is about to record, so a claim past now
    /// closes it where the claim begins; `rebuild_range` fences growth after that.
    fn holders(
        &self,
        except: &[&str],
        start: u64,
        end: u64,
        now: u64,
    ) -> Result<Vec<String>, String> {
        let mut statement = self
            .conn
            .prepare(&format!(
                "SELECT id FROM time_entries
                 WHERE deleted_at IS NULL AND started_at < :end
                   AND ({HELD_END} > :start
                        OR (status = 'building' AND {HELD_END} >= :now AND :start <= :now AND :now < :end))
                 ORDER BY started_at, id;"
            ))
            .map_err(|e| e.to_string())?;
        let ids = statement
            .query_map(
                named_params! {":start": start as i64, ":end": end as i64, ":now": now as i64},
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        Ok(ids
            .into_iter()
            .filter(|id| !except.contains(&id.as_str()))
            .collect())
    }

    fn held_end(&self, id: &str, now: u64) -> Result<u64, String> {
        self.conn
            .query_row(
                &format!("SELECT {HELD_END} FROM time_entries WHERE id = :id;"),
                named_params! {":id": id, ":now": now as i64},
                |row| row.get::<_, i64>(0),
            )
            .map(|end| end as u64)
            .map_err(|e| e.to_string())
    }

    /// Makes `id` give up `start..end`: removed if that is all it held, else cut back either side.
    fn displace(&mut self, id: &str, start: u64, end: u64, now: u64) -> Result<(), String> {
        let entry = self.time_entry(id)?;
        let held_end = self.held_end(id, now)?;
        let keeps_head = entry.started_at < start;
        let keeps_tail = held_end > end;
        if !keeps_head && !keeps_tail {
            return self.remove_entry(id);
        }
        self.note_entry(id)?;
        self.note_segments_of(id)?;

        // The tail, if any, becomes its own entry; the live session's
        let tail = if keeps_head && keeps_tail {
            let tail = uuid::Uuid::now_v7().to_string();
            self.note_entry(&tail)?;
            self.conn
                .execute(
                    "INSERT INTO time_entries (id, started_at, ended_at, description, description_origin, category_id, project_id, status, approved_by, source, billable, created_at, updated_at)
                     SELECT ?1, ?2, ?3, description, description_origin, category_id, project_id, status, approved_by, source, billable, ?4, ?4
                     FROM time_entries WHERE id = ?5;",
                    params![tail, end as i64, held_end as i64, now as i64, id],
                )
                .map_err(|e| e.to_string())?;
            self.copy_suggestions(id, &tail)?;
            Some(tail)
        } else {
            None
        };

        // Each piece owns exactly its own activity: segments running across
        if keeps_tail && end <= now {
            let owner = tail.as_deref().unwrap_or(id);
            self.cut_segments(id, end, Some(owner))?;
        }
        if keeps_head && start <= now {
            self.cut_segments(id, start, None)?;
        }
        // What the entry keeps lies in its pieces: a head the claim cut off
        let released_from = if keeps_head { start } else { 0 };
        self.conn
            .execute(
                "UPDATE segments SET entry_id = NULL
                 WHERE entry_id = ?1 AND started_at >= ?2 AND started_at < ?3;",
                params![id, released_from as i64, end as i64],
            )
            .map_err(|e| e.to_string())?;
        if let Some(tail) = &tail {
            self.conn
                .execute(
                    "UPDATE segments SET entry_id = ?1 WHERE entry_id = ?2 AND started_at >= ?3;",
                    params![tail, id, end as i64],
                )
                .map_err(|e| e.to_string())?;
        }

        if keeps_head {
            self.conn
                .execute(
                    "UPDATE time_entries SET ended_at = ?1, updated_at = ?2,
                       status = CASE WHEN status = 'building' THEN 'pending' ELSE status END
                     WHERE id = ?3;",
                    params![start as i64, now as i64, id],
                )
                .map_err(|e| e.to_string())?;
        } else {
            self.conn
                .execute(
                    "UPDATE time_entries SET started_at = ?1, ended_at = ?2, updated_at = ?3 WHERE id = ?4;",
                    params![end as i64, held_end as i64, now as i64, id],
                )
                .map_err(|e| e.to_string())?;
        }
        for piece in std::iter::once(id).chain(tail.as_deref()) {
            self.describe_closed(piece)?;
            self.log_event(piece, "trimmed", "user", None, now);
        }
        Ok(())
    }

    /// A segment capture is writing may begin inside the claim and run past it, so it is cut where
    /// the claim ends.
    fn close_capture_before(&mut self, start: u64, end: u64, now: u64) -> Result<(), String> {
        let Some(current) = self.current.as_ref() else {
            return Ok(());
        };
        if end > now || current.started_at < start || current.started_at >= end {
            return Ok(());
        }
        let id = current.id;
        let owner: Option<String> = self
            .conn
            .query_row(
                "SELECT entry_id FROM segments WHERE id = ?1;",
                [id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        self.note_segment(id, owner.as_deref());
        self.cut_segment(id, owner.as_deref(), end, None)
            .map(|_| ())
    }
}
