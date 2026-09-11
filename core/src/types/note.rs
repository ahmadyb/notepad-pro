//! Notes persisted in SQLite.

use serde::{Deserialize, Serialize};

/// A full note row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// `0` means "not stored yet"; [`crate::db::notes::NotesDb::save`] fills it in.
    pub id: i64,
    pub title: String,
    /// The document as plain text.
    pub content: String,
    /// Per-line highlight colours, serialised as a JSON object
    /// `{"0": "yellow", "4": "#ff8800"}`.
    pub highlights_json: String,
    /// Per-line list structure, serialised as a JSON array.
    pub list_structure_json: String,
    /// Set when the note is linked to a file on disk.
    pub file_path: Option<String>,
    pub pinned: bool,
    /// Unix epoch seconds, fractional.
    pub created_at: f64,
    pub modified_at: f64,
}

impl Note {
    pub fn new(title: impl Into<String>, content: impl Into<String>) -> Self {
        let now = crate::db::notes::now();
        Self {
            id: 0,
            title: title.into(),
            content: content.into(),
            highlights_json: "{}".to_string(),
            list_structure_json: "[]".to_string(),
            file_path: None,
            pinned: false,
            created_at: now,
            modified_at: now,
        }
    }

    pub fn is_transient(&self) -> bool {
        self.id == 0
    }
}

impl Default for Note {
    fn default() -> Self {
        Self::new(String::new(), String::new())
    }
}

/// Lightweight row used by the sidebar list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteMetadata {
    pub id: i64,
    pub title: String,
    /// First non-empty line of the body, truncated.
    pub snippet: String,
    pub pinned: bool,
    pub modified_at: f64,
    /// Human readable age, e.g. "2h ago" — formatted in Rust so the Slint
    /// layer never has to do string arithmetic.
    pub modified_label: String,
    /// Up to four distinct highlight colours present in the note, as packed
    /// `0xRRGGBBAA`. The sidebar renders them as chips.
    pub colour_chips: Vec<u32>,
}

impl NoteMetadata {
    /// Build a snippet + chip list from note content.
    pub fn summarise(note: &Note) -> Self {
        Self {
            id: note.id,
            title: if note.title.is_empty() {
                "Untitled".to_string()
            } else {
                note.title.clone()
            },
            snippet: snippet_of(&note.content),
            pinned: note.pinned,
            modified_at: note.modified_at,
            modified_label: relative_time(note.modified_at),
            colour_chips: crate::highlight::palette::chips_from_highlights_json(&note.highlights_json),
        }
    }
}

/// First non-empty line, trimmed and clamped to 120 characters.
pub fn snippet_of(content: &str) -> String {
    let first = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();
    if first.chars().count() <= 120 {
        first
    } else {
        let truncated: String = first.chars().take(117).collect();
        format!("{truncated}...")
    }
}

/// "just now" / "12m ago" / "3h ago" / "5d ago" / ISO date.
pub fn relative_time(epoch_seconds: f64) -> String {
    let now = crate::db::notes::now();
    let delta = (now - epoch_seconds).max(0.0);
    if delta < 45.0 {
        "just now".to_string()
    } else if delta < 3_600.0 {
        format!("{}m ago", (delta / 60.0).round() as u64)
    } else if delta < 86_400.0 {
        format!("{}h ago", (delta / 3_600.0).round() as u64)
    } else if delta < 7.0 * 86_400.0 {
        format!("{}d ago", (delta / 86_400.0).round() as u64)
    } else {
        // No chrono dependency: fall back to a day count.
        format!("{}d ago", (delta / 86_400.0).round() as u64)
    }
}

/// A user defined swatch colour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomColour {
    pub name: String,
    /// `#rrggbb`
    pub hex: String,
    /// Packed `0xRRGGBBAA`.
    pub rgba: u32,
}

impl CustomColour {
    pub fn new(name: impl Into<String>, hex: impl Into<String>) -> Self {
        let hex = hex.into();
        let rgba = crate::highlight::palette::rgba_from_hex(&hex).unwrap_or(0x0000_00ff);
        Self {
            name: name.into(),
            hex,
            rgba,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_takes_first_non_empty_line() {
        let content = "\n\n   \nMeeting notes\nsecond line";
        assert_eq!(snippet_of(content), "Meeting notes");
    }

    #[test]
    fn snippet_is_clamped_to_120_chars() {
        let content = "x".repeat(400);
        let snippet = snippet_of(&content);
        assert_eq!(snippet.chars().count(), 120);
        assert!(snippet.ends_with("..."));
    }

    #[test]
    fn snippet_of_empty_content_is_empty() {
        assert_eq!(snippet_of(""), "");
        assert_eq!(snippet_of("\n\n"), "");
    }

    #[test]
    fn new_note_is_transient_and_untitled() {
        let note = Note::new("", "body");
        assert!(note.is_transient());
        let meta = NoteMetadata::summarise(&note);
        assert_eq!(meta.title, "Untitled");
        assert_eq!(meta.snippet, "body");
    }

    #[test]
    fn relative_time_buckets_are_ordered() {
        let now = crate::db::notes::now();
        assert_eq!(relative_time(now - 5.0), "just now");
        assert_eq!(relative_time(now - 600.0), "10m ago");
        assert_eq!(relative_time(now - 3.0 * 3600.0), "3h ago");
        assert_eq!(relative_time(now - 4.0 * 86_400.0), "4d ago");
    }

    #[test]
    fn relative_time_never_goes_negative() {
        let future = crate::db::notes::now() + 10_000.0;
        assert_eq!(relative_time(future), "just now");
    }

    #[test]
    fn custom_colour_packs_hex_into_rgba() {
        let c = CustomColour::new("Sunset", "#ff8800");
        assert_eq!(c.rgba, 0xff88_00ff);
        assert_eq!(c.hex, "#ff8800");
    }
}
