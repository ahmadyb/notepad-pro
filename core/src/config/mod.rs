//! Settings and session persistence (JSON files next to the database).

pub mod session;
pub mod settings;

pub use session::SessionStore;
pub use settings::Settings;
