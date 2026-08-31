//! NotePad Pro — the Slint front end.
//!
//! The crate is split so that the whole controller layer is testable without
//! a display:
//!
//! * [`state`] — pure Rust application state (tabs, documents, cursor, find,
//!   extraction). No Slint types appear in it beyond what [`convert`] needs.
//! * [`convert`] — the only place that touches the generated Slint structs.
//! * [`sync`] — pushes state into window properties.
//! * [`callbacks`] — the thin Slint closures that glue the two together.

/// The Slint-generated module. Produced by `build.rs` at compile time.
pub mod ui {
    slint::include_modules!();
}

pub mod callbacks;
pub mod convert;
pub mod dialogs;
pub mod state;
pub mod sync;

pub use state::AppState;
pub use ui::AppWindow;
