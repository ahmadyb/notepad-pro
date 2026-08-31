//! The line model — the single most important data structure in the app.
//!
//! A document is a `Vec<EditorLine>`. Every line owns its own text, highlight
//! colour, list marker and indent depth, which is what makes per-line colour
//! bands, list markers and colour extraction trivial.

use serde::{Deserialize, Serialize};

/// Maximum nesting depth for list items (0..=5).
pub const MAX_INDENT: u8 = 5;

/// The highlight colour of a single line.
///
/// `None` is the default and must stay the first variant: the Slint code
/// generator derives `Default` for enums from their first value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LineColour {
    None,
    Yellow,
    Green,
    Pink,
    Blue,
    Orange,
    Purple,
    /// A user defined colour, stored as packed `0xRRGGBBAA`.
    Custom(u32),
}

impl Default for LineColour {
    fn default() -> Self {
        LineColour::None
    }
}

impl LineColour {
    /// `true` when the line carries a visible highlight band.
    pub fn is_highlighted(self) -> bool {
        !matches!(self, LineColour::None)
    }

    /// Stable, lower-case identifier used in settings and the Slint API.
    ///
    /// Custom colours are identified by their hex value, e.g. `#ff8800`.
    pub fn key(self) -> String {
        match self {
            LineColour::None => "none".to_string(),
            LineColour::Yellow => "yellow".to_string(),
            LineColour::Green => "green".to_string(),
            LineColour::Pink => "pink".to_string(),
            LineColour::Blue => "blue".to_string(),
            LineColour::Orange => "orange".to_string(),
            LineColour::Purple => "purple".to_string(),
            LineColour::Custom(rgba) => crate::highlight::palette::hex_from_rgba(rgba),
        }
    }

    /// Parse the inverse of [`LineColour::key`]. Unknown keys map to `None`.
    pub fn from_key(key: &str) -> Self {
        match key {
            "yellow" => LineColour::Yellow,
            "green" => LineColour::Green,
            "pink" => LineColour::Pink,
            "blue" => LineColour::Blue,
            "orange" => LineColour::Orange,
            "purple" => LineColour::Purple,
            other if other.starts_with('#') => {
                crate::highlight::palette::rgba_from_hex(other)
                    .map(LineColour::Custom)
                    .unwrap_or(LineColour::None)
            }
            _ => LineColour::None,
        }
    }

    /// Built-in swatch colours as packed `0xRRGGBBAA`. Custom colours are
    /// returned unchanged so callers never need a palette lookup.
    pub fn builtin_rgba(self) -> u32 {
        match self {
            LineColour::None => 0x0000_0000,
            LineColour::Yellow => 0xffe2_7aff,
            LineColour::Green => 0xa8e6_a1ff,
            LineColour::Pink => 0xffb3_d1ff,
            LineColour::Blue => 0xa3d5_ffff,
            LineColour::Orange => 0xffc0_8aff,
            LineColour::Purple => 0xd5b3_ffff,
            LineColour::Custom(rgba) => rgba,
        }
    }

    /// Display name used in the extract panel and status summaries.
    pub fn display_name(self) -> String {
        match self {
            LineColour::None => "None".to_string(),
            LineColour::Yellow => "Yellow".to_string(),
            LineColour::Green => "Green".to_string(),
            LineColour::Pink => "Pink".to_string(),
            LineColour::Blue => "Blue".to_string(),
            LineColour::Orange => "Orange".to_string(),
            LineColour::Purple => "Purple".to_string(),
            LineColour::Custom(rgba) => crate::highlight::palette::hex_from_rgba(rgba),
        }
    }
}

/// The list marker drawn in front of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ListType {
    None,
    Bullet,
    Number,
    Check,
}

impl Default for ListType {
    fn default() -> Self {
        ListType::None
    }
}

impl ListType {
    pub fn key(self) -> &'static str {
        match self {
            ListType::None => "none",
            ListType::Bullet => "bullet",
            ListType::Number => "number",
            ListType::Check => "check",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "bullet" => ListType::Bullet,
            "number" => ListType::Number,
            "check" => ListType::Check,
            _ => ListType::None,
        }
    }
}

/// A coloured span *inside* a line.
///
/// Slint's `TextInput` cannot render mixed inline formatting, so spans are
/// persisted and round-tripped but rendered as the full-line colour band.
/// See DEVIATIONS.md ("Rich text / inline highlights").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineSpan {
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset just past the last character.
    pub end: usize,
    pub colour: LineColour,
}

/// One line of a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorLine {
    pub text: String,
    pub colour: LineColour,
    pub list_type: ListType,
    /// Nesting depth, clamped to `0..=MAX_INDENT`.
    pub indent: u8,
    /// Only meaningful for [`ListType::Check`].
    pub checked: bool,
    /// Resolved display number for [`ListType::Number`], recomputed by
    /// [`crate::editor::list_engine::ListEngine::renumber`].
    pub number: u32,
    pub inline_spans: Vec<InlineSpan>,
}

impl Default for EditorLine {
    fn default() -> Self {
        Self {
            text: String::new(),
            colour: LineColour::None,
            list_type: ListType::None,
            indent: 0,
            checked: false,
            number: 0,
            inline_spans: Vec::new(),
        }
    }
}

impl EditorLine {
    /// A plain line with the given text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Character length (not byte length) — used for the status bar column.
    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    /// Word count using the same rule as the status bar.
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }

    /// Clamp the indent into range. Called after deserialising untrusted
    /// `.npro` files so a hand-edited document cannot panic the renderer.
    pub fn normalise(&mut self) {
        if self.indent > MAX_INDENT {
            self.indent = MAX_INDENT;
        }
        if self.list_type == ListType::None {
            self.indent = 0;
            self.checked = false;
        }
        if self.list_type != ListType::Check {
            self.checked = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_line_is_plain_and_unhighlighted() {
        let line = EditorLine::default();
        assert_eq!(line.text, "");
        assert_eq!(line.colour, LineColour::None);
        assert_eq!(line.list_type, ListType::None);
        assert_eq!(line.indent, 0);
        assert!(!line.checked);
        assert!(!line.colour.is_highlighted());
    }

    #[test]
    fn colour_key_roundtrips_for_every_builtin() {
        for colour in [
            LineColour::None,
            LineColour::Yellow,
            LineColour::Green,
            LineColour::Pink,
            LineColour::Blue,
            LineColour::Orange,
            LineColour::Purple,
        ] {
            assert_eq!(LineColour::from_key(&colour.key()), colour);
        }
    }

    #[test]
    fn custom_colour_key_roundtrips_through_hex() {
        let colour = LineColour::Custom(0x1234_56ff);
        assert_eq!(colour.key(), "#123456");
        assert_eq!(LineColour::from_key(&colour.key()), colour);
    }

    #[test]
    fn unknown_key_maps_to_none() {
        assert_eq!(LineColour::from_key("chartreuse"), LineColour::None);
        assert_eq!(LineColour::from_key(""), LineColour::None);
    }

    #[test]
    fn every_builtin_has_an_opaque_swatch() {
        for colour in [
            LineColour::Yellow,
            LineColour::Green,
            LineColour::Pink,
            LineColour::Blue,
            LineColour::Orange,
            LineColour::Purple,
        ] {
            assert_eq!(colour.builtin_rgba() & 0xff, 0xff, "{:?}", colour);
        }
    }

    #[test]
    fn normalise_clamps_indent_and_clears_orphan_flags() {
        let mut line = EditorLine {
            indent: 200,
            list_type: ListType::None,
            checked: true,
            ..Default::default()
        };
        line.normalise();
        assert_eq!(line.indent, 0, "indent resets when the marker is removed");
        assert!(!line.checked);
    }

    #[test]
    fn counts_are_measured_in_chars_not_bytes() {
        let line = EditorLine::new("café résumé");
        assert_eq!(line.char_len(), 11);
        assert_eq!(line.word_count(), 2);
        assert!(line.text.len() > line.char_len());
    }
}
