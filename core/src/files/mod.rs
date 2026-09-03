//! File I/O: loading, encoding detection, line endings, atomic saves.

pub mod encoding;
pub mod line_endings;
pub mod manager;

pub use encoding::{detect_encoding, EncodingInfo};
pub use line_endings::LineEnding;
pub use manager::{load_file, save_file, LoadedFile};
