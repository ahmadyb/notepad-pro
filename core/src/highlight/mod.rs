//! Highlighting: palette, extraction, statistics.

pub mod extractor;
pub mod palette;
pub mod stats;

pub use extractor::{extract, ExtractionOrder, ExtractionResult};
pub use palette::{Palette, PaletteEntry, BAND_ALPHA, BUILTIN, THEMES};
pub use stats::{breakdown, HighlightBreakdown};
