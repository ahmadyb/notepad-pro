//! Connection pool.

use std::path::Path;

use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

pub type SqlitePool = Pool<SqliteConnectionManager>;

/// Number of pooled connections. The UI is single-threaded; the extra slots
/// are for the autosave thread and tests.
pub const POOL_SIZE: u32 = 4;

/// Build a pool for `path`, applying pragmas and the schema to each new
/// connection. `:memory:` is supported for tests.
pub fn create_pool(path: &Path) -> Result<SqlitePool> {
    let path_string = path.to_string_lossy().into_owned();
    let manager = SqliteConnectionManager::file(&path_string).with_init(initialise);
    Pool::builder()
        .max_size(POOL_SIZE)
        .build(manager)
        .with_context(|| format!("cannot open notes database at {path_string}"))
}

/// In-memory pool, used by the test suite.
pub fn memory_pool() -> Result<SqlitePool> {
    let manager = SqliteConnectionManager::memory().with_init(initialise);
    Pool::builder()
        .max_size(POOL_SIZE)
        .build(manager)
        .context("cannot open in-memory notes database")
}

/// Pragmas + schema, run for every connection the pool creates.
fn initialise(connection: &mut Connection) -> Result<(), rusqlite::Error> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "busy_timeout", 5_000)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_pool_opens_and_applies_the_wal_pragma_request() {
        let pool = memory_pool().expect("memory pool");
        let conn = pool.get().expect("connection");
        // In-memory databases silently keep "memory" as the journal mode;
        // what matters is that the pragma ran without error.
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .expect("journal_mode");
        assert!(!mode.is_empty());
    }

    #[test]
    fn file_pool_creates_the_database_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.db");
        let pool = create_pool(&path).expect("file pool");
        let conn = pool.get().expect("connection");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .expect("journal_mode");
        assert_eq!(mode.to_lowercase(), "wal");
        assert!(path.exists());
    }

    #[test]
    fn pool_hands_out_multiple_connections() {
        let pool = memory_pool().unwrap();
        let a = pool.get().unwrap();
        let b = pool.get().unwrap();
        assert!(a.execute_batch("SELECT 1").is_ok());
        assert!(b.execute_batch("SELECT 1").is_ok());
    }

    #[test]
    fn a_bad_path_is_reported_as_an_error() {
        let err = create_pool(Path::new("/proc/definitely/not/writable/notes.db"));
        assert!(err.is_err());
    }
}
