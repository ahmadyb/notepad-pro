//! CRUD for the notes table.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::db::pool::{memory_pool, SqlitePool};
use crate::db::schema;
use crate::types::note::{Note, NoteMetadata};

/// Sidebar ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    /// Most recently modified first, pins always on top.
    Modified,
    /// Alphabetical by title, pins always on top.
    Title,
    /// Oldest first.
    Created,
}

impl SortOrder {
    pub fn key(self) -> &'static str {
        match self {
            SortOrder::Modified => "modified",
            SortOrder::Title => "title",
            SortOrder::Created => "created",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "title" => SortOrder::Title,
            "created" => SortOrder::Created,
            _ => SortOrder::Modified,
        }
    }

    fn order_by(self) -> &'static str {
        match self {
            SortOrder::Modified => "ORDER BY pinned DESC, modified_at DESC",
            SortOrder::Title => "ORDER BY pinned DESC, title COLLATE NOCASE ASC",
            SortOrder::Created => "ORDER BY pinned DESC, created_at ASC",
        }
    }
}

/// Current unix time in fractional seconds.
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Handle on the notes database.
#[derive(Clone)]
pub struct NotesDb {
    pool: SqlitePool,
    /// `false` when the FTS5 module was unavailable and we fell back to LIKE.
    fts: bool,
}

impl NotesDb {
    /// Open (and create/migrate) the database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let pool = crate::db::pool::create_pool(path)?;
        Self::from_pool(pool)
    }

    /// Wrap an existing pool. Falls back to the non-FTS schema when needed.
    pub fn from_pool(pool: SqlitePool) -> Result<Self> {
        let mut fts = true;
        {
            let conn = pool.get().context("cannot get a database connection")?;
            if conn.execute_batch(schema::INIT_SQL).is_err() {
                fts = false;
                conn.execute_batch(schema::INIT_SQL_WITHOUT_FTS)
                    .context("cannot initialise the notes schema")?;
            }
            conn.execute_batch(schema::META_SQL)
                .context("cannot initialise the meta table")?;
            let version: i64 = conn
                .query_row(
                    "SELECT value FROM meta WHERE key = 'schema_version'",
                    [],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if version != schema::SCHEMA_VERSION {
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![schema::SCHEMA_VERSION.to_string()],
                )?;
            }
        }
        Ok(Self { pool, fts })
    }

    /// In-memory store for tests.
    pub fn in_memory() -> Result<Self> {
        Self::from_pool(memory_pool()?)
    }

    /// `true` when full-text search is active.
    pub fn has_fts(&self) -> bool {
        self.fts
    }

    fn conn(&self) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>> {
        self.pool.get().context("cannot get a database connection")
    }

    /// List notes, optionally filtered by a search query.
    pub fn list(&self, query: &str, sort: SortOrder) -> Result<Vec<NoteMetadata>> {
        let conn = self.conn()?;
        let trimmed = query.trim();
        let mut notes = if trimmed.is_empty() {
            let sql = format!("SELECT * FROM notes {}", sort.order_by());
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], row_to_note)?;
            collect(rows)?
        } else {
            let like = format!("%{}%", trimmed.replace('%', "\\%").replace('_', "\\_"));
            let sql = format!(
                "SELECT * FROM notes WHERE title LIKE ?1 ESCAPE '\\' OR content LIKE ?1 ESCAPE '\\' {}",
                sort.order_by()
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![like], row_to_note)?;
            collect(rows)?
        };

        // Case-insensitive ranking: exact title hits float to the top.
        if !trimmed.is_empty() {
            let needle = trimmed.to_lowercase();
            notes.sort_by_key(|n| {
                let title = n.title.to_lowercase();
                if title == needle {
                    0
                } else if title.starts_with(&needle) {
                    1
                } else {
                    2
                }
            });
            // Keep pinned notes on top even after re-ranking.
            notes.sort_by_key(|n| !n.pinned);
        }
        Ok(notes)
    }

    pub fn get(&self, id: i64) -> Result<Option<Note>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT * FROM notes WHERE id = ?1")?;
        let note = stmt.query_row(params![id], row_to_note).optional()?;
        Ok(note)
    }

    /// Insert or update. Returns the row id.
    pub fn save(&self, note: &Note) -> Result<i64> {
        let conn = self.conn()?;
        let modified_at = if note.modified_at > 0.0 { note.modified_at } else { now() };
        let created_at = if note.created_at > 0.0 { note.created_at } else { modified_at };

        if note.id == 0 {
            conn.execute(
                "INSERT INTO notes
                    (title, content, html, highlights_json, list_structure_json,
                     file_path, pinned, created_at, modified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    note.title,
                    note.content,
                    "",
                    note.highlights_json,
                    note.list_structure_json,
                    note.file_path,
                    note.pinned as i64,
                    created_at,
                    modified_at,
                ],
            )?;
            let id = conn.last_insert_rowid();
            if self.fts {
                self.sync_fts(&conn, id, &note.title, &note.content)?;
            }
            Ok(id)
        } else {
            let updated = conn.execute(
                "UPDATE notes SET
                    title = ?1, content = ?2, highlights_json = ?3,
                    list_structure_json = ?4, file_path = ?5, pinned = ?6,
                    modified_at = ?7
                 WHERE id = ?8",
                params![
                    note.title,
                    note.content,
                    note.highlights_json,
                    note.list_structure_json,
                    note.file_path,
                    note.pinned as i64,
                    modified_at,
                    note.id,
                ],
            )?;
            if updated == 0 {
                anyhow::bail!("note {} no longer exists", note.id);
            }
            if self.fts {
                self.sync_fts(&conn, note.id, &note.title, &note.content)?;
            }
            Ok(note.id)
        }
    }

    pub fn delete(&self, id: i64) -> Result<bool> {
        let conn = self.conn()?;
        if self.fts {
            conn.execute("DELETE FROM notes_fts WHERE rowid = ?1", params![id])?;
        }
        let removed = conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
        Ok(removed > 0)
    }

    /// Set the pin flag; returns the new value.
    pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<bool> {
        let conn = self.conn()?;
        let updated = conn.execute(
            "UPDATE notes SET pinned = ?1, modified_at = ?2 WHERE id = ?3",
            params![pinned as i64, now(), id],
        )?;
        if updated == 0 {
            anyhow::bail!("note {id} no longer exists");
        }
        Ok(pinned)
    }

    /// Touch `modified_at` without changing content.
    pub fn touch(&self, id: i64) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE notes SET modified_at = ?1 WHERE id = ?2",
            params![now(), id],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<usize> {
        let conn = self.conn()?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn pinned_count(&self) -> Result<usize> {
        let conn = self.conn()?;
        let n: i64 =
            conn.query_row("SELECT COUNT(*) FROM notes WHERE pinned = 1", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Remove every note. Used by "Clear all notes" and by tests.
    pub fn clear(&self) -> Result<usize> {
        let conn = self.conn()?;
        if self.fts {
            conn.execute("DELETE FROM notes_fts", [])?;
        }
        let removed = conn.execute("DELETE FROM notes", [])?;
        Ok(removed)
    }

    fn sync_fts(&self, conn: &Connection, id: i64, title: &str, content: &str) -> Result<()> {
        conn.execute("DELETE FROM notes_fts WHERE rowid = ?1", params![id])?;
        conn.execute(
            "INSERT INTO notes_fts (rowid, title, content) VALUES (?1, ?2, ?3)",
            params![id, title, content],
        )?;
        Ok(())
    }
}

fn row_to_note(row: &Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get("id")?,
        title: row.get("title")?,
        content: row.get("content")?,
        highlights_json: row.get("highlights_json")?,
        list_structure_json: row.get("list_structure_json")?,
        file_path: row.get("file_path")?,
        pinned: row.get::<_, i64>("pinned")? != 0,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}

fn collect<I>(rows: I) -> Result<Vec<NoteMetadata>>
where
    I: Iterator<Item = rusqlite::Result<Note>>,
{
    let mut out = Vec::new();
    for row in rows {
        out.push(NoteMetadata::summarise(&row?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> NotesDb {
        NotesDb::in_memory().expect("in-memory notes db")
    }

    #[test]
    fn opens_an_empty_database() {
        let db = db();
        assert_eq!(db.count().unwrap(), 0);
        assert!(db.list("", SortOrder::Modified).unwrap().is_empty());
    }

    #[test]
    fn schema_is_idempotent_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.db");
        {
            let db = NotesDb::open(&path).unwrap();
            db.save(&Note::new("a", "b")).unwrap();
        }
        let db = NotesDb::open(&path).unwrap();
        assert_eq!(db.count().unwrap(), 1);
    }

    #[test]
    fn save_assigns_an_id_on_insert() {
        let db = db();
        let id = db.save(&Note::new("Title", "Body")).unwrap();
        assert!(id > 0);
        assert_eq!(db.count().unwrap(), 1);
    }

    #[test]
    fn save_updates_in_place_when_the_id_is_known() {
        let db = db();
        let id = db.save(&Note::new("v1", "body")).unwrap();
        let mut note = db.get(id).unwrap().unwrap();
        note.title = "v2".into();
        note.id = id;
        let again = db.save(&note).unwrap();
        assert_eq!(again, id);
        assert_eq!(db.count().unwrap(), 1);
        assert_eq!(db.get(id).unwrap().unwrap().title, "v2");
    }

    #[test]
    fn saving_a_missing_id_is_an_error() {
        let db = db();
        let mut note = Note::new("ghost", "");
        note.id = 4242;
        assert!(db.save(&note).is_err());
    }

    #[test]
    fn get_returns_none_for_unknown_ids() {
        let db = db();
        assert!(db.get(999).unwrap().is_none());
    }

    #[test]
    fn roundtrip_preserves_every_field() {
        let db = db();
        let mut note = Note::new("Title", "line one\nline two");
        note.highlights_json = r#"{"0":"yellow"}"#.into();
        note.list_structure_json = r#"[{"i":0,"t":"bullet"}]"#.into();
        note.file_path = Some("/tmp/a.txt".into());
        note.pinned = true;
        let id = db.save(&note).unwrap();
        let loaded = db.get(id).unwrap().unwrap();
        assert_eq!(loaded.title, note.title);
        assert_eq!(loaded.content, note.content);
        assert_eq!(loaded.highlights_json, note.highlights_json);
        assert_eq!(loaded.list_structure_json, note.list_structure_json);
        assert_eq!(loaded.file_path, note.file_path);
        assert!(loaded.pinned);
        assert!(loaded.created_at > 0.0);
        assert!(loaded.modified_at > 0.0);
    }

    #[test]
    fn delete_removes_the_row() {
        let db = db();
        let id = db.save(&Note::new("x", "")).unwrap();
        assert!(db.delete(id).unwrap());
        assert!(!db.delete(id).unwrap(), "second delete is a no-op");
        assert_eq!(db.count().unwrap(), 0);
    }

    #[test]
    fn pinning_is_reported_and_persisted() {
        let db = db();
        let id = db.save(&Note::new("x", "")).unwrap();
        assert!(db.set_pinned(id, true).unwrap());
        assert!(db.get(id).unwrap().unwrap().pinned);
        assert_eq!(db.pinned_count().unwrap(), 1);
        assert!(!db.set_pinned(id, false).unwrap());
        assert_eq!(db.pinned_count().unwrap(), 0);
    }

    #[test]
    fn pinning_a_missing_note_is_an_error() {
        let db = db();
        assert!(db.set_pinned(1234, true).is_err());
    }

    #[test]
    fn list_sorts_most_recent_first_by_default() {
        let db = db();
        let first = db.save(&Note::new("first", "")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = db.save(&Note::new("second", "")).unwrap();
        db.touch(first).unwrap();
        let list = db.list("", SortOrder::Modified).unwrap();
        assert_eq!(list[0].id, first);
        assert_eq!(list[1].id, second);
    }

    #[test]
    fn list_can_sort_by_title() {
        let db = db();
        db.save(&Note::new("banana", "")).unwrap();
        db.save(&Note::new("Apple", "")).unwrap();
        db.save(&Note::new("cherry", "")).unwrap();
        let titles: Vec<String> = db
            .list("", SortOrder::Title)
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(titles, vec!["Apple", "banana", "cherry"]);
    }

    #[test]
    fn pinned_notes_always_come_first() {
        let db = db();
        let a = db.save(&Note::new("aaa", "")).unwrap();
        db.save(&Note::new("zzz", "")).unwrap();
        db.set_pinned(a, true).unwrap();
        let list = db.list("", SortOrder::Title).unwrap();
        assert_eq!(list[0].id, a);
    }

    #[test]
    fn search_matches_title_and_body() {
        let db = db();
        db.save(&Note::new("Shopping", "milk and eggs")).unwrap();
        db.save(&Note::new("Work", "quarterly report")).unwrap();
        assert_eq!(db.list("milk", SortOrder::Modified).unwrap().len(), 1);
        assert_eq!(db.list("report", SortOrder::Modified).unwrap().len(), 1);
        assert_eq!(db.list("quarterly", SortOrder::Modified).unwrap().len(), 1);
        assert_eq!(db.list("nothing", SortOrder::Modified).unwrap().len(), 0);
    }

    #[test]
    fn search_is_case_insensitive() {
        let db = db();
        db.save(&Note::new("Milk", "")).unwrap();
        assert_eq!(db.list("milk", SortOrder::Modified).unwrap().len(), 1);
        assert_eq!(db.list("MILK", SortOrder::Modified).unwrap().len(), 1);
    }

    #[test]
    fn search_escapes_like_wildcards() {
        let db = db();
        db.save(&Note::new("100%", "")).unwrap();
        db.save(&Note::new("other", "")).unwrap();
        // A literal '%' must not match everything.
        assert_eq!(db.list("%", SortOrder::Modified).unwrap().len(), 1);
        assert_eq!(db.list("_", SortOrder::Modified).unwrap().len(), 0);
    }

    #[test]
    fn search_ranks_exact_title_hits_first() {
        let db = db();
        db.save(&Note::new("report about milk", "")).unwrap();
        db.save(&Note::new("milk", "")).unwrap();
        let list = db.list("milk", SortOrder::Modified).unwrap();
        assert_eq!(list[0].title, "milk");
    }

    #[test]
    fn blank_query_returns_everything() {
        let db = db();
        db.save(&Note::new("a", "")).unwrap();
        db.save(&Note::new("b", "")).unwrap();
        assert_eq!(db.list("   ", SortOrder::Modified).unwrap().len(), 2);
    }

    #[test]
    fn metadata_carries_a_snippet_and_chips() {
        let db = db();
        let mut note = Note::new("T", "\n\nfirst real line\nsecond");
        note.highlights_json = r#"{"0":"yellow","1":"yellow","2":"green"}"#.into();
        let id = db.save(&note).unwrap();
        let meta = &db.list("", SortOrder::Modified).unwrap()[0];
        assert_eq!(meta.id, id);
        assert_eq!(meta.snippet, "first real line");
        assert_eq!(meta.colour_chips.len(), 2, "distinct colours only");
        assert!(!meta.modified_label.is_empty());
    }

    #[test]
    fn clear_removes_every_note() {
        let db = db();
        db.save(&Note::new("a", "")).unwrap();
        db.save(&Note::new("b", "")).unwrap();
        assert_eq!(db.clear().unwrap(), 2);
        assert_eq!(db.count().unwrap(), 0);
    }

    #[test]
    fn touch_updates_the_timestamp() {
        let db = db();
        let id = db.save(&Note::new("a", "")).unwrap();
        let before = db.get(id).unwrap().unwrap().modified_at;
        std::thread::sleep(std::time::Duration::from_millis(5));
        db.touch(id).unwrap();
        let after = db.get(id).unwrap().unwrap().modified_at;
        assert!(after > before);
    }

    #[test]
    fn sort_order_keys_roundtrip() {
        for order in [SortOrder::Modified, SortOrder::Title, SortOrder::Created] {
            assert_eq!(SortOrder::from_key(order.key()), order);
        }
        assert_eq!(SortOrder::from_key("bogus"), SortOrder::Modified);
    }

    #[test]
    fn unicode_survives_the_roundtrip() {
        let db = db();
        let id = db.save(&Note::new("Ünïcødé 日本", "emoji 🎉 ok")).unwrap();
        let note = db.get(id).unwrap().unwrap();
        assert_eq!(note.title, "Ünïcødé 日本");
        assert_eq!(note.content, "emoji 🎉 ok");
        assert_eq!(db.list("日本", SortOrder::Modified).unwrap().len(), 1);
    }
}
