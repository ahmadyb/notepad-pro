//! SQLite persistence for the notes sidebar.

pub mod notes;
pub mod pool;
pub mod schema;

pub use notes::{NotesDb, SortOrder};
pub use pool::create_pool;
