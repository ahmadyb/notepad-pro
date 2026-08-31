//! Typed payloads exchanged between the Slint layer and the core.
//!
//! Each struct here has a one-to-one `struct` declaration in `ui/model.slint`.
//! `tools/check_consistency.py` asserts the two lists agree field by field.

use serde::{Deserialize, Serialize};

use crate::files::line_endings::LineEnding;
use crate::types::note::{CustomColour, NoteMetadata};

/// Reported by the `app-info()` API method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub data_dir: String,
    pub slint_backend: String,
}

/// Mirror of [`crate::config::settings::Settings`] minus the collections that
/// Slint passes as separate model properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsData {
    pub theme: String,
    pub font_family: String,
    pub font_size: i32,
    pub word_wrap: bool,
    pub zoom: f32,
    pub animations: bool,
    pub sidebar_open: bool,
    pub sidebar_sort: String,
    pub autosave_interval_secs: i32,
    pub extract_order: String,
    pub native_frame: bool,
}

/// Per-tab state, mirrored in `ui/model.slint` as `TabData`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TabState {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub dirty: bool,
    pub note_id: Option<i64>,
    pub line_ending: String,
    pub encoding: String,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub scroll_top: f64,
}

impl TabState {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
            path: None,
            dirty: false,
            note_id: None,
            line_ending: LineEnding::Lf.label().to_string(),
            encoding: "utf-8".to_string(),
            cursor_line: 0,
            cursor_col: 0,
            scroll_top: 0.0,
        }
    }

    /// Display name with the unsaved-changes marker.
    pub fn display_name(&self) -> String {
        if self.dirty {
            format!("{} *", self.name)
        } else {
            self.name.clone()
        }
    }
}

/// Result of `load-file()`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadedFileData {
    pub ok: bool,
    pub error: String,
    pub path: String,
    pub content: String,
    pub encoding: String,
    pub line_ending: String,
}

impl LoadedFileData {
    pub fn failed(path: &str, error: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: error.into(),
            path: path.to_string(),
            content: String::new(),
            encoding: String::new(),
            line_ending: String::new(),
        }
    }
}

/// Summary returned by `highlight-stats()`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HighlightStats {
    pub total_lines: i32,
    pub highlighted: i32,
    /// "Yellow 3 · Green 1" — pre-formatted so Slint does no string maths.
    pub summary: String,
}

/// One row of the extract panel's colour list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColourEntry {
    /// Lookup key understood by [`crate::types::line::LineColour::from_key`].
    pub key: String,
    pub name: String,
    /// Packed `0xRRGGBBAA`.
    pub rgba: u32,
    pub line_count: i32,
    /// "3 lines" — pre-formatted.
    pub count_label: String,
    pub active: bool,
}

/// Result of `load-session()`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionData {
    pub ok: bool,
    pub tab_count: i32,
    pub active_tab: i32,
}

/// Result of `window-state()`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowStateData {
    pub maximised: bool,
    pub minimised: bool,
    pub fullscreen: bool,
}

/// Status bar payload. Every field is pre-formatted where a string is needed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StatusData {
    pub caret_text: String,
    pub metrics_text: String,
    pub highlight_text: String,
    pub zoom_text: String,
    pub line_ending: String,
    pub encoding: String,
    pub dirty: bool,
    pub saved_text: String,
    pub cursor_line: i32,
    pub cursor_col: i32,
    pub selected_chars: i32,
    pub word_count: i32,
    pub char_count: i32,
    pub line_count: i32,
    pub highlight_count: i32,
    pub zoom: f32,
}

/// A note as it crosses the API boundary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteData {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub highlights_json: String,
    pub list_structure_json: String,
    /// Empty string means "not linked"; Slint has no `Option`.
    pub file_path: String,
    pub pinned: bool,
    pub created_at: f64,
    pub modified_at: f64,
}

impl NoteData {
    pub fn from_note(note: &crate::types::note::Note) -> Self {
        Self {
            id: note.id,
            title: note.title.clone(),
            content: note.content.clone(),
            highlights_json: note.highlights_json.clone(),
            list_structure_json: note.list_structure_json.clone(),
            file_path: note.file_path.clone().unwrap_or_default(),
            pinned: note.pinned,
            created_at: note.created_at,
            modified_at: note.modified_at,
        }
    }

    pub fn to_note(&self) -> crate::types::note::Note {
        crate::types::note::Note {
            id: self.id,
            title: self.title.clone(),
            content: self.content.clone(),
            highlights_json: if self.highlights_json.is_empty() {
                "{}".to_string()
            } else {
                self.highlights_json.clone()
            },
            list_structure_json: if self.list_structure_json.is_empty() {
                "[]".to_string()
            } else {
                self.list_structure_json.clone()
            },
            file_path: if self.file_path.is_empty() {
                None
            } else {
                Some(self.file_path.clone())
            },
            pinned: self.pinned,
            created_at: self.created_at,
            modified_at: self.modified_at,
        }
    }
}

/// Everything needed to rebuild the session on next launch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    pub version: u32,
    pub active_tab: usize,
    pub tabs: Vec<TabState>,
    /// Plain text of each tab, index-aligned with `tabs`.
    pub documents: Vec<String>,
    /// Highlight maps, index-aligned with `tabs`.
    pub highlights: Vec<String>,
    /// List structure, index-aligned with `tabs`.
    pub list_structures: Vec<String>,
    pub window: WindowGeometry,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowGeometry {
    pub width: u32,
    pub height: u32,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            width: 1200,
            height: 800,
        }
    }
}

/// A recently opened file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentFile {
    pub path: String,
    pub opened_at: f64,
}

/// Convenience bundle so callers do not have to import three modules.
pub type PaletteVec = Vec<CustomColour>;
pub type NoteList = Vec<NoteMetadata>;

/// Generate a short random-ish tab id without pulling in `uuid`.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("tab-{:x}-{:x}", nanos, seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_display_name_marks_dirty_tabs() {
        let mut tab = TabState::new("notes.txt");
        assert_eq!(tab.display_name(), "notes.txt");
        tab.dirty = true;
        assert_eq!(tab.display_name(), "notes.txt *");
    }

    #[test]
    fn tab_ids_are_unique() {
        let a = TabState::new("a");
        let b = TabState::new("b");
        assert_ne!(a.id, b.id);
        assert!(a.id.starts_with("tab-"));
    }

    #[test]
    fn loaded_file_failure_carries_the_message() {
        let f = LoadedFileData::failed("/tmp/x", "boom");
        assert!(!f.ok);
        assert_eq!(f.error, "boom");
        assert_eq!(f.path, "/tmp/x");
    }

    #[test]
    fn note_data_roundtrip_preserves_optional_path() {
        let mut note = crate::types::note::Note::new("t", "c");
        note.id = 7;
        note.file_path = Some("/tmp/a.txt".into());
        note.pinned = true;
        let data = NoteData::from_note(&note);
        assert_eq!(data.file_path, "/tmp/a.txt");
        let back = data.to_note();
        assert_eq!(back, note);
    }

    #[test]
    fn note_data_defaults_empty_json_payloads() {
        let data = NoteData {
            id: 0,
            title: "t".into(),
            content: String::new(),
            highlights_json: String::new(),
            list_structure_json: String::new(),
            file_path: String::new(),
            pinned: false,
            created_at: 0.0,
            modified_at: 0.0,
        };
        let note = data.to_note();
        assert_eq!(note.highlights_json, "{}");
        assert_eq!(note.list_structure_json, "[]");
        assert_eq!(note.file_path, None);
    }

    #[test]
    fn session_defaults_to_a_single_tab_geometry() {
        let s = Session::default();
        assert_eq!(s.tabs.len(), 0);
        assert_eq!(s.window.width, 1200);
        assert_eq!(s.version, 0);
    }
}
