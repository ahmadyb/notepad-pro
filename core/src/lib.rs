//! NotePad Pro — core logic.
//!
//! This crate is deliberately UI-framework agnostic: it knows nothing about
//! Slint. The `app` crate owns the Slint layer and translates between
//! [`types::line::EditorLine`] (rich, serialisable) and the generated
//! `EditorLineData` struct (flat, model-friendly).
//!
//! Keeping the split means the entire editing engine, find/replace engine,
//! colour extractor, SQLite notes store and settings/session persistence can
//! be unit tested headlessly with plain `cargo test`.

pub mod config;
pub mod db;
pub mod editor;
pub mod files;
pub mod highlight;
pub mod types;

pub use config::{session, settings};
pub use db::notes::NotesDb;
pub use editor::{find_replace, line_model, list_engine, undo};
pub use files::{encoding, line_endings, manager};
pub use highlight::{extractor, palette, stats};
pub use types::{api, line, note};

/// Human readable application name shown in the UI and About dialog.
pub const APP_NAME: &str = "NotePad Pro";

/// SemVer of this build. Reported through the `app-info()` API method.
pub const APP_VERSION: &str = "1.0.2-slint";

/// Extension used by "Save to NotePad Pro" documents.
pub const APP_FILE_EXTENSION: &str = "txt";
