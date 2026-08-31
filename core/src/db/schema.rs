//! Database schema.

/// Table + index DDL applied on every open (idempotent).
///
/// `STRICT` mode rejects malformed writes at the SQL layer, which is worth
/// having for a database the user may inspect by hand.
pub const INIT_SQL: &str = "
CREATE TABLE IF NOT EXISTS notes (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    title                TEXT    NOT NULL DEFAULT '',
    content              TEXT    NOT NULL DEFAULT '',
    html                 TEXT    NOT NULL DEFAULT '',
    highlights_json      TEXT    NOT NULL DEFAULT '{}',
    list_structure_json  TEXT    NOT NULL DEFAULT '[]',
    file_path            TEXT,
    pinned               INTEGER NOT NULL DEFAULT 0,
    created_at           REAL    NOT NULL,
    modified_at          REAL    NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_modified
    ON notes (modified_at DESC);

CREATE INDEX IF NOT EXISTS idx_pinned
    ON notes (pinned DESC, modified_at DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
    title, content, content='notes', content_rowid='id'
);
";

/// Fallback DDL used when the FTS5 module is not compiled into SQLite.
pub const INIT_SQL_WITHOUT_FTS: &str = "
CREATE TABLE IF NOT EXISTS notes (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    title                TEXT    NOT NULL DEFAULT '',
    content              TEXT    NOT NULL DEFAULT '',
    html                 TEXT    NOT NULL DEFAULT '{}',
    highlights_json      TEXT    NOT NULL DEFAULT '{}',
    list_structure_json  TEXT    NOT NULL DEFAULT '[]',
    file_path            TEXT,
    pinned               INTEGER NOT NULL DEFAULT 0,
    created_at           REAL    NOT NULL,
    modified_at          REAL    NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_modified
    ON notes (modified_at DESC);

CREATE INDEX IF NOT EXISTS idx_pinned
    ON notes (pinned DESC, modified_at DESC);
";

/// Current schema version, stamped into `meta`.
pub const SCHEMA_VERSION: i64 = 1;

pub const META_SQL: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddl_creates_every_required_column() {
        for column in [
            "id",
            "title",
            "content",
            "html",
            "highlights_json",
            "list_structure_json",
            "file_path",
            "pinned",
            "created_at",
            "modified_at",
        ] {
            assert!(INIT_SQL.contains(column), "missing column {column}");
            assert!(INIT_SQL_WITHOUT_FTS.contains(column), "missing column {column}");
        }
    }

    #[test]
    fn both_ddl_variants_create_the_indexes() {
        for ddl in [INIT_SQL, INIT_SQL_WITHOUT_FTS] {
            assert!(ddl.contains("idx_modified"));
            assert!(ddl.contains("idx_pinned"));
        }
    }

    #[test]
    fn ddl_is_idempotent() {
        assert!(INIT_SQL.contains("IF NOT EXISTS"));
        assert!(META_SQL.contains("IF NOT EXISTS"));
    }
}
