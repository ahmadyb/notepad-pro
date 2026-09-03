//! Editing engine: document model, undo, lists, find & replace.

pub mod find_replace;
pub mod line_model;
pub mod list_engine;
pub mod undo;

pub use find_replace::{FindEngine, Match};
pub use line_model::Document;
pub use list_engine::{EnterOutcome, ListEngine};
pub use undo::UndoStack;
