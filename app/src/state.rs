//! Application state: tabs, documents, cursor, find engine, extraction.
//!
//! Everything here is plain Rust so it can be driven from tests without a
//! window. [`crate::sync`] is the only module that pushes it to Slint.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use notepad_pro_core::config::settings::Settings;
use notepad_pro_core::db::notes::{NotesDb, SortOrder};
use notepad_pro_core::editor::find_replace::FindEngine;
use notepad_pro_core::editor::line_model::Document;
use notepad_pro_core::editor::list_engine::{EnterOutcome, ListEngine};
use notepad_pro_core::files::line_endings::LineEnding;
use notepad_pro_core::files::manager;
use notepad_pro_core::highlight::extractor::{self, ExtractionOrder};
use notepad_pro_core::highlight::palette::{
    chips_from_highlights_json, highlights_json_for, list_structure_json_for, Palette,
};
use notepad_pro_core::highlight::stats;
use notepad_pro_core::types::api::{Session, StatusData, TabState};
use notepad_pro_core::types::line::{LineColour, ListType};
use notepad_pro_core::types::note::{CustomColour, Note, NoteMetadata};

/// An action waiting behind a confirm dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    CloseTab(usize),
    CloseApp,
    DeleteNote(i64),
    /// Re-save a file that changed on disk since it was opened.
    OverwriteFile(PathBuf),
    /// Discard edits and reload from the note store.
    RevertTab(usize),
}

/// One open document plus the metadata needed to write it back.
#[derive(Debug, Clone)]
pub struct Tab {
    pub state: TabState,
    pub doc: Document,
    pub encoding: String,
    pub line_ending: LineEnding,
    pub had_bom: bool,
}

impl Tab {
    pub fn untitled() -> Self {
        Self {
            state: TabState::new("Untitled"),
            doc: Document::new(),
            encoding: "utf-8".to_string(),
            line_ending: LineEnding::Lf,
            had_bom: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.doc.dirty
    }
}

/// Caret position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
}

/// The whole application state.
pub struct AppState {
    pub settings: Settings,
    pub db: NotesDb,

    pub tabs: Vec<Tab>,
    pub active: usize,
    pub cursor: Cursor,
    /// Set while Shift+Up/Down extends a line selection.
    pub anchor: Option<usize>,

    pub palette: Palette,
    /// Colour key currently armed in the toolbar ("" when none).
    pub armed_colour: String,

    pub find: FindEngine,
    pub find_open: bool,
    pub replace_open: bool,

    pub extract_open: bool,
    pub extract_selected: Vec<String>,

    pub note_query: String,
    pub selected_note_id: i64,

    pub startup_files: Vec<String>,
    pub pending: Option<PendingAction>,

    pub picker_hex: String,
    pub picker_name: String,

    /// Mirror of the window geometry. Only the setters on `slint::Window` are
    /// portable, so the app tracks the state it asked for.
    pub window_minimised: bool,
    pub window_maximised: bool,
    pub window_fullscreen: bool,

    /// Monotonic counter used to keep toast messages distinct.
    pub toast_seq: u64,
}

impl AppState {
    pub fn new(settings: Settings, db: NotesDb) -> Self {
        let palette = Palette::new(settings.custom_palette.clone());
        Self {
            settings,
            db,
            tabs: vec![Tab::untitled()],
            active: 0,
            cursor: Cursor::default(),
            anchor: None,
            palette,
            armed_colour: "yellow".to_string(),
            find: FindEngine::new(),
            find_open: false,
            replace_open: false,
            extract_open: false,
            extract_selected: vec!["yellow".into()],
            note_query: String::new(),
            selected_note_id: -1,
            startup_files: Vec::new(),
            pending: None,
            picker_hex: "#ffe27a".to_string(),
            picker_name: "Custom".to_string(),
            window_minimised: false,
            window_maximised: false,
            window_fullscreen: false,
            toast_seq: 0,
        }
    }

    // ── Tabs ──────────────────────────────────────────────────────────────

    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn doc(&self) -> &Document {
        &self.tab().doc
    }

    pub fn doc_mut(&mut self) -> &mut Document {
        &mut self.tab_mut().doc
    }

    pub fn any_dirty(&self) -> bool {
        self.tabs.iter().any(|t| t.is_dirty())
    }

    pub fn tab_states(&self) -> Vec<TabState> {
        self.tabs.iter().map(|t| t.state.clone()).collect()
    }

    /// Create a new empty tab and make it active.
    pub fn new_tab(&mut self) -> usize {
        let index = self.tabs.len();
        self.tabs.push(Tab::untitled());
        self.active = index;
        self.cursor = Cursor::default();
        self.anchor = None;
        self.invalidate_find();
        index
    }

    /// Close a tab. Never closes the last one — it is cleared instead.
    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            self.tabs[0] = Tab::untitled();
            self.active = 0;
        } else {
            self.tabs.remove(index);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len() - 1;
            } else if self.active > index {
                self.active -= 1;
            }
        }
        self.cursor = Cursor::default();
        self.anchor = None;
        self.invalidate_find();
    }

    pub fn select_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() || index == self.active {
            return false;
        }
        self.active = index;
        let (line, col) = (
            self.tabs[self.active].state.cursor_line,
            self.tabs[self.active].state.cursor_col,
        );
        self.cursor = Cursor { line, col };
        self.anchor = None;
        self.invalidate_find();
        true
    }

    pub fn cycle_tab(&mut self, forward: bool) {
        if self.tabs.len() < 2 {
            return;
        }
        let next = if forward {
            (self.active + 1) % self.tabs.len()
        } else {
            (self.active + self.tabs.len() - 1) % self.tabs.len()
        };
        self.select_tab(next);
    }

    /// Remember the caret so it survives a tab switch.
    pub fn stash_cursor(&mut self) {
        let (line, col) = (self.cursor.line, self.cursor.col);
        let tab = &mut self.tabs[self.active];
        tab.state.cursor_line = line;
        tab.state.cursor_col = col;
        tab.state.dirty = tab.doc.dirty;
    }

    // ── Caret & selection ─────────────────────────────────────────────────

    pub fn clamp_cursor(&mut self) {
        let last = self.doc().line_count().saturating_sub(1);
        self.cursor.line = self.cursor.line.min(last);
        let cols = self.doc().lines[self.cursor.line].char_len();
        self.cursor.col = self.cursor.col.min(cols);
    }

    /// Inclusive line range covered by the current selection.
    pub fn selection_range(&self) -> (usize, usize) {
        let last = self.doc().line_count().saturating_sub(1);
        let line = self.cursor.line.min(last);
        match self.anchor {
            Some(anchor) => {
                let a = anchor.min(last);
                (line.min(a), line.max(a))
            }
            None => (line, line),
        }
    }

    /// Character count of a multi-line selection, 0 for a plain caret.
    pub fn selected_chars(&self) -> usize {
        let (start, end) = self.selection_range();
        if start == end {
            return 0;
        }
        self.doc().lines[start..=end]
            .iter()
            .map(|l| l.char_len())
            .sum::<usize>()
            + (end - start)
    }

    // ── Editing ───────────────────────────────────────────────────────────

    pub fn set_line_text(&mut self, index: usize, text: &str) -> bool {
        if text.contains('\n') {
            return self.split_line_text(index, text);
        }
        let changed = self.tabs[self.active].doc.set_text(index, text);
        if changed {
            self.mark_dirty();
            self.invalidate_find();
        }
        changed
    }

    /// The row `TextInput`s are multi-line, so pressing Enter inserts a `"\n"`
    /// into the row text and `edited` hands it here. Split it, continuing the
    /// list (type + indent) onto the new row, and park the caret after the
    /// last fragment.
    fn split_line_text(&mut self, index: usize, text: &str) -> bool {
        let (orig_type, orig_indent) = match self.doc().lines.get(index) {
            Some(l) => (l.list_type, l.indent),
            None => (ListType::None, 0),
        };
        let mut parts = text.split('\n');
        let first = parts.next().unwrap_or_default().to_string();
        let rest: Vec<String> = parts.map(|p| p.to_string()).collect();
        self.tabs[self.active].doc.set_text(index, &first);
        let mut at = index;
        for part in &rest {
            let new = self.tabs[self.active].doc.insert_line_after(at);
            self.tabs[self.active].doc.set_text(new, part);
            if orig_type != ListType::None {
                self.tabs[self.active].doc.lines[new].list_type = orig_type;
                self.tabs[self.active].doc.lines[new].indent = orig_indent;
            }
            at = new;
        }
        self.tabs[self.active].doc.commit();
        self.cursor = Cursor {
            line: at,
            col: rest.last().map(|p| p.chars().count()).unwrap_or(0),
        };
        self.mark_dirty();
        self.invalidate_find();
        true
    }

    pub fn press_enter(&mut self) -> EnterOutcome {
        let (line, col) = (self.cursor.line, self.cursor.col);
        let outcome = ListEngine::handle_enter(&mut self.tabs[self.active].doc.lines, line, col);
        self.tabs[self.active].doc.commit();
        if let EnterOutcome::MoveTo(index) = outcome {
            self.cursor = Cursor { line: index, col: 0 };
        }
        self.invalidate_find();
        outcome
    }

    pub fn insert_blank_line(&mut self) {
        let line = self.cursor.line;
        let index = self.tabs[self.active].doc.insert_line_after(line);
        self.cursor = Cursor { line: index, col: 0 };
        self.invalidate_find();
    }

    pub fn set_list_type(&mut self, list_type: ListType) {
        let (start, end) = self.selection_range();
        self.doc_mut().set_list_type(start, end, list_type);
        self.mark_dirty();
    }

    pub fn indent(&mut self, deeper: bool) {
        let (start, end) = self.selection_range();
        self.doc_mut().change_indent(start, end, if deeper { 1 } else { -1 });
        self.mark_dirty();
    }

    pub fn toggle_checked(&mut self, index: usize) -> bool {
        let result = self.doc_mut().toggle_checked(index).unwrap_or(false);
        self.mark_dirty();
        result
    }

    pub fn undo(&mut self) -> bool {
        if self.doc_mut().undo() {
            self.clamp_cursor();
            self.mark_dirty();
            self.invalidate_find();
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if self.doc_mut().redo() {
            self.clamp_cursor();
            self.mark_dirty();
            self.invalidate_find();
            true
        } else {
            false
        }
    }

    fn mark_dirty(&mut self) {
        let tab = &mut self.tabs[self.active];
        tab.state.dirty = tab.doc.dirty;
    }

    // ── Feature 1 & 2: highlighting ───────────────────────────────────────

    /// Resolve a palette key, adding unknown `#rrggbb` keys on the fly.
    pub fn colour_for_key(&mut self, key: &str) -> Option<LineColour> {
        if let Some(found) = self.palette.find(key) {
            return Some(found);
        }
        // A `#rgb`/`#rrggbb`/`#rrggbbaa` key creates a custom colour on the
        // fly (feature 2 — unlimited custom colours).
        let hex = key.strip_prefix('#')?;
        if !matches!(hex.len(), 3 | 6 | 8) {
            return None;
        }
        let expanded = if hex.len() == 3 {
            hex.chars().map(|c| format!("{c}{c}")).collect::<String>()
        } else {
            hex.to_string()
        };
        let rgba = u32::from_str_radix(&expanded, 16).ok()?;
        let rgba = if expanded.len() == 6 { (rgba << 8) | 0xff } else { rgba };
        if !self.palette.custom_colours().iter().any(|c| c.rgba == rgba) {
            self.palette.add_custom(CustomColour {
                name: format!("#{expanded}"),
                hex: format!("#{}", &expanded[..6.min(expanded.len())]),
                rgba,
            });
        }
        Some(LineColour::Custom(rgba))
    }

    /// Feature 1 — toggle the colour on the current selection.
    /// Returns `true` when the colour was applied, `false` when removed.
    pub fn toggle_highlight_key(&mut self, key: &str) -> Option<bool> {
        let colour = self.colour_for_key(key)?;
        if colour == LineColour::None {
            let (start, end) = self.selection_range();
            self.doc_mut().remove_highlight(start, end);
            self.armed_colour.clear();
            self.mark_dirty();
            return Some(false);
        }
        self.armed_colour = key.to_string();
        let (start, end) = self.selection_range();
        let applied = self.doc_mut().toggle_highlight(start, end, colour);
        self.mark_dirty();
        Some(applied)
    }

    /// Force a colour onto the selection (no toggle).
    pub fn apply_highlight_key(&mut self, key: &str) -> bool {
        let Some(colour) = self.colour_for_key(key) else {
            return false;
        };
        let (start, end) = self.selection_range();
        self.doc_mut().highlight_lines(start, end, colour);
        self.armed_colour = key.to_string();
        self.mark_dirty();
        true
    }

    pub fn clear_highlight(&mut self) {
        let (start, end) = self.selection_range();
        self.doc_mut().remove_highlight(start, end);
        self.mark_dirty();
    }

    pub fn add_custom_colour(&mut self, name: &str, hex: &str) -> bool {
        let Some(rgba) = notepad_pro_core::highlight::palette::rgba_from_hex(hex) else {
            return false;
        };
        let colour = CustomColour::new(name, notepad_pro_core::highlight::palette::hex_from_rgba(rgba));
        self.palette.add_custom(colour.clone());
        self.settings.add_custom_colour(colour);
        true
    }

    // ── Feature 3: extraction ─────────────────────────────────────────────

    pub fn selected_extract_colours(&self) -> Vec<LineColour> {
        self.extract_selected
            .iter()
            .map(|k| LineColour::from_key(k))
            .filter(|c| c.is_highlighted())
            .collect()
    }

    pub fn extract_order(&self) -> ExtractionOrder {
        if self.settings.extract_order == "grouped" {
            ExtractionOrder::GroupByColour
        } else {
            ExtractionOrder::Document
        }
    }

    pub fn extract(&self) -> extractor::ExtractionResult {
        extractor::extract(
            &self.doc().lines,
            &self.selected_extract_colours(),
            self.extract_order(),
        )
    }

    pub fn toggle_extract_colour(&mut self, key: &str) -> bool {
        if let Some(pos) = self.extract_selected.iter().position(|k| k == key) {
            self.extract_selected.remove(pos);
            false
        } else {
            self.extract_selected.push(key.to_string());
            true
        }
    }

    // ── Find & replace ────────────────────────────────────────────────────

    pub fn invalidate_find(&mut self) {
        let lines = &self.tabs[self.active].doc.lines;
        self.find.search(lines);
    }

    pub fn set_find_query(&mut self, query: &str) {
        self.find.set_query(query);
        self.invalidate_find();
    }

    pub fn set_find_replacement(&mut self, replacement: &str) {
        self.find.set_replacement(replacement);
    }

    pub fn find_next(&mut self) -> Option<notepad_pro_core::editor::find_replace::Match> {
        let m = self.find.next()?;
        self.cursor.line = m.line;
        self.anchor = None;
        Some(m)
    }

    pub fn find_prev(&mut self) -> Option<notepad_pro_core::editor::find_replace::Match> {
        let m = self.find.prev()?;
        self.cursor.line = m.line;
        self.anchor = None;
        Some(m)
    }

    pub fn replace_one(&mut self) -> bool {
        let applied = self
            .find
            .replace_current(&mut self.tabs[self.active].doc.lines);
        if applied {
            self.tabs[self.active].doc.commit();
            self.invalidate_find();
        }
        applied
    }

    pub fn replace_all(&mut self) -> usize {
        let count = self.find.replace_all(&mut self.tabs[self.active].doc.lines);
        if count > 0 {
            self.tabs[self.active].doc.commit();
        }
        count
    }

    // ── Files ─────────────────────────────────────────────────────────────

    /// Load a path into a new tab (or reuse the tab already showing it).
    pub fn open_path(&mut self, path: &Path) -> Result<usize> {
        let path_string = path.to_string_lossy().into_owned();
        if let Some(index) = self
            .tabs
            .iter()
            .position(|t| t.state.path.as_deref() == Some(path_string.as_str()))
        {
            self.active = index;
            self.cursor = Cursor::default();
            self.invalidate_find();
            return Ok(index);
        }

        let loaded = manager::load_file(path)
            .with_context(|| format!("cannot open {}", path.display()))?;

        // Reuse a pristine "Untitled" tab rather than piling up empties.
        let reuse = self.tabs.len() == 1 && !self.tabs[0].is_dirty() && self.tabs[0].doc.is_empty();
        if !reuse {
            self.tabs.push(Tab::untitled());
            self.active = self.tabs.len() - 1;
        }

        let tab = self.tab_mut();
        tab.doc = Document::from_plain_text(&loaded.content);
        tab.state.name = manager::file_name(&path_string);
        tab.state.path = Some(path_string.clone());
        tab.state.dirty = false;
        tab.state.encoding = loaded.encoding.clone();
        tab.state.line_ending = loaded.line_ending.label().to_string();
        tab.encoding = loaded.encoding;
        tab.line_ending = loaded.line_ending;
        tab.had_bom = loaded.had_bom;
        tab.doc.mark_saved();

        self.cursor = Cursor::default();
        self.anchor = None;
        self.invalidate_find();
        self.settings.remember_file(&path_string);
        Ok(self.active)
    }

    /// Load raw text into the active tab (used by session restore and by
    /// "Open extracted in new tab").
    pub fn load_text(&mut self, name: &str, content: &str, path: Option<String>) {
        let tab = self.tab_mut();
        tab.doc = Document::from_plain_text(content);
        tab.state.name = name.to_string();
        tab.state.path = path;
        tab.state.dirty = false;
        tab.doc.mark_saved();
        self.cursor = Cursor::default();
        self.anchor = None;
        self.invalidate_find();
    }

    /// Save the active tab. `path` overrides the tab's stored path.
    pub fn save_to(&mut self, path: &Path) -> Result<()> {
        let text = self.doc().plain_text();
        let encoding = self.tab().encoding.clone();
        let ending = self.tab().line_ending;
        let bom = self.tab().had_bom;
        manager::save_file_with_bom(path, &text, &encoding, ending, bom)?;

        let path_string = path.to_string_lossy().into_owned();
        let tab = self.tab_mut();
        tab.state.path = Some(path_string.clone());
        tab.state.name = manager::file_name(&path_string);
        tab.state.dirty = false;
        tab.doc.mark_saved();
        self.settings.remember_file(&path_string);
        Ok(())
    }

    pub fn save_active(&mut self) -> Result<Option<PathBuf>> {
        let Some(path) = self.tab().state.path.clone() else {
            return Ok(None);
        };
        let path_buf = PathBuf::from(&path);
        self.save_to(&path_buf)?;
        Ok(Some(path_buf))
    }

    // ── Notes (Feature 5) ─────────────────────────────────────────────────

    pub fn note_list(&self) -> Result<Vec<NoteMetadata>> {
        self.db
            .list(&self.note_query, SortOrder::from_key(&self.settings.sidebar_sort))
    }

    pub fn note_count_label(&self, shown: usize) -> String {
        let total = self.db.count().unwrap_or(0);
        if shown == total {
            format!("{total} {}", if total == 1 { "note" } else { "notes" })
        } else {
            format!(
                "{shown} of {total} {}",
                if total == 1 { "note" } else { "notes" }
            )
        }
    }

    /// Store the active tab as a note. Reuses the tab's linked note so that
    /// re-saving does not discard edits (bug #5).
    pub fn save_active_as_note(&mut self) -> Result<i64> {
        let title = if self.tab().state.name == "Untitled" {
            first_line_or_untitled(&self.doc().plain_text())
        } else {
            self.tab().state.name.clone()
        };
        let existing = self
            .tab()
            .state
            .note_id
            .and_then(|id| self.db.get(id).ok().flatten());

        let mut note = existing.unwrap_or_else(|| Note::new(title.clone(), String::new()));
        note.title = title;
        note.content = self.doc().plain_text();
        note.highlights_json = highlights_json_for(&self.doc().lines);
        note.list_structure_json = list_structure_json_for(&self.doc().lines);
        note.file_path = self.tab().state.path.clone();
        note.modified_at = notepad_pro_core::db::notes::now();

        let id = self.db.save(&note)?;
        self.tab_mut().state.note_id = Some(id);
        self.selected_note_id = id;
        Ok(id)
    }

    /// Load a note into the active tab.
    pub fn open_note(&mut self, id: i64) -> Result<()> {
        let Some(note) = self.db.get(id)? else {
            bail!("note {id} no longer exists");
        };
        let mut doc = Document::from_plain_text(&note.content);
        notepad_pro_core::highlight::palette::apply_highlights_json(
            &mut doc.lines,
            &note.highlights_json,
        );
        notepad_pro_core::highlight::palette::apply_list_structure_json(
            &mut doc.lines,
            &note.list_structure_json,
        );
        ListEngine::renumber(&mut doc.lines);

        let title = if note.title.is_empty() {
            "Untitled".to_string()
        } else {
            note.title.clone()
        };
        let tab = self.tab_mut();
        tab.doc = doc;
        tab.doc.mark_saved();
        tab.state.name = title;
        tab.state.note_id = Some(note.id);
        tab.state.path = note.file_path.clone();
        tab.state.dirty = false;
        self.selected_note_id = note.id;
        self.cursor = Cursor::default();
        self.invalidate_find();
        Ok(())
    }

    pub fn new_note(&mut self) -> Result<i64> {
        let note = Note::new("Untitled", String::new());
        let id = self.db.save(&note)?;
        self.selected_note_id = id;
        Ok(id)
    }

    pub fn delete_note(&mut self, id: i64) -> Result<bool> {
        let removed = self.db.delete(id)?;
        if self.selected_note_id == id {
            self.selected_note_id = -1;
        }
        for tab in self.tabs.iter_mut() {
            if tab.state.note_id == Some(id) {
                tab.state.note_id = None;
            }
        }
        Ok(removed)
    }

    pub fn toggle_pin(&mut self, id: i64) -> Result<bool> {
        let Some(note) = self.db.get(id)? else {
            bail!("note {id} no longer exists");
        };
        self.db.set_pinned(id, !note.pinned)
    }

    // ── Zoom / view ───────────────────────────────────────────────────────

    pub fn zoom_step(&mut self, delta: f32) {
        if delta == 0.0 {
            self.settings.zoom = 1.0;
        } else {
            self.settings.zoom = (self.settings.zoom + delta * 0.1).clamp(0.5, 3.0);
        }
        self.settings.clamp();
    }

    pub fn set_theme(&mut self, name: &str) -> bool {
        if !notepad_pro_core::highlight::palette::is_known_theme(name) {
            return false;
        }
        self.settings.theme = name.to_string();
        true
    }

    pub fn toggle_dark_twin(&mut self) -> String {
        let next = notepad_pro_core::highlight::palette::dark_twin(&self.settings.theme).to_string();
        self.settings.theme = next.clone();
        next
    }

    // ── Status ────────────────────────────────────────────────────────────

    pub fn compute_status(&self) -> StatusData {
        let doc = self.doc();
        let breakdown = stats::breakdown(&doc.lines, &self.palette);
        let selected = self.selected_chars();

        let caret_text = if selected > 0 {
            format!(
                "Ln {}, Col {}    {} selected",
                self.cursor.line + 1,
                self.cursor.col + 1,
                selected
            )
        } else {
            format!("Ln {}, Col {}", self.cursor.line + 1, self.cursor.col + 1)
        };

        let metrics_text = format!(
            "{} words \u{00b7} {} chars \u{00b7} {} lines",
            doc.word_count(),
            doc.char_count(),
            doc.line_count()
        );

        let highlight_text = if breakdown.highlighted_lines == 0 {
            String::new()
        } else if breakdown.highlighted_lines == 1 {
            "1 highlight".to_string()
        } else {
            format!("{} highlights", breakdown.highlighted_lines)
        };

        StatusData {
            caret_text,
            metrics_text,
            highlight_text,
            zoom_text: format!("{}%", (self.settings.zoom * 100.0).round() as i32),
            line_ending: self.tab().line_ending.label().to_string(),
            encoding: self.tab().encoding.clone(),
            dirty: doc.dirty,
            saved_text: if doc.dirty {
                "Unsaved changes".to_string()
            } else {
                "Saved".to_string()
            },
            cursor_line: (self.cursor.line + 1) as i32,
            cursor_col: (self.cursor.col + 1) as i32,
            selected_chars: selected as i32,
            word_count: doc.word_count() as i32,
            char_count: doc.char_count() as i32,
            line_count: doc.line_count() as i32,
            highlight_count: breakdown.highlighted_lines as i32,
            zoom: self.settings.zoom,
        }
    }

    pub fn window_title(&self) -> String {
        let name = self.tabs[self.active].state.name.clone();
        if self.tabs[self.active].is_dirty() {
            format!("{name} \u{25cf} \u{2014} {}", notepad_pro_core::APP_NAME)
        } else {
            format!("{name} \u{2014} {}", notepad_pro_core::APP_NAME)
        }
    }

    // ── Session ───────────────────────────────────────────────────────────

    pub fn build_session(&self) -> Session {
        Session {
            version: notepad_pro_core::config::session::SESSION_VERSION,
            active_tab: self.active,
            tabs: self.tab_states(),
            documents: self
                .tabs
                .iter()
                .map(|t| t.doc.plain_text())
                .collect(),
            highlights: self
                .tabs
                .iter()
                .map(|t| highlights_json_for(&t.doc.lines))
                .collect(),
            list_structures: self
                .tabs
                .iter()
                .map(|t| list_structure_json_for(&t.doc.lines))
                .collect(),
            window: Default::default(),
        }
    }

    pub fn restore_session(&mut self, session: &Session) {
        if session.tabs.is_empty() {
            return;
        }
        self.tabs.clear();
        for (i, tab_state) in session.tabs.iter().enumerate() {
            let mut doc = Document::from_plain_text(
                session.documents.get(i).map(|s| s.as_str()).unwrap_or(""),
            );
            if let Some(highlights) = session.highlights.get(i) {
                notepad_pro_core::highlight::palette::apply_highlights_json(
                    &mut doc.lines,
                    highlights,
                );
            }
            if let Some(structure) = session.list_structures.get(i) {
                notepad_pro_core::highlight::palette::apply_list_structure_json(
                    &mut doc.lines,
                    structure,
                );
            }
            ListEngine::renumber(&mut doc.lines);
            doc.mark_saved();

            let mut state = tab_state.clone();
            state.dirty = false;
            self.tabs.push(Tab {
                encoding: state.encoding.clone(),
                line_ending: LineEnding::from_label(&state.line_ending),
                had_bom: false,
                state,
                doc,
            });
        }
        self.active = session.active_tab.min(self.tabs.len() - 1);
        let (line, col) = (
            self.tabs[self.active].state.cursor_line,
            self.tabs[self.active].state.cursor_col,
        );
        self.cursor = Cursor { line, col };
        self.clamp_cursor();
        self.invalidate_find();
    }

    /// Colour chips for the sidebar cards come from the stored highlight blob.
    pub fn chips_for(&self, highlights_json: &str) -> Vec<u32> {
        chips_from_highlights_json(highlights_json)
    }

    pub fn next_toast_seq(&mut self) -> u64 {
        self.toast_seq += 1;
        self.toast_seq
    }
}

/// First non-empty line, or "Untitled".
fn first_line_or_untitled(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| {
            if l.chars().count() > 48 {
                let truncated: String = l.chars().take(45).collect();
                format!("{truncated}...")
            } else {
                l.to_string()
            }
        })
        .unwrap_or_else(|| "Untitled".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use notepad_pro_core::types::line::EditorLine;

    fn state() -> AppState {
        AppState::new(Settings::default(), NotesDb::in_memory().unwrap())
    }

    fn state_with(text: &str) -> AppState {
        let mut s = state();
        s.load_text("test.txt", text, None);
        s
    }

    // ── tabs ──────────────────────────────────────────────────────────────

    #[test]
    fn starts_with_one_clean_untitled_tab() {
        let s = state();
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(s.tab().state.name, "Untitled");
        assert!(!s.any_dirty());
    }

    #[test]
    fn new_tab_becomes_active() {
        let mut s = state();
        let index = s.new_tab();
        assert_eq!(index, 1);
        assert_eq!(s.active, 1);
        assert_eq!(s.tabs.len(), 2);
    }

    #[test]
    fn closing_a_tab_moves_the_active_index_back() {
        let mut s = state();
        s.new_tab();
        s.new_tab();
        assert_eq!(s.active, 2);
        s.close_tab(1);
        assert_eq!(s.active, 1);
        assert_eq!(s.tabs.len(), 2);
    }

    #[test]
    fn closing_the_active_last_tab_shifts_left() {
        let mut s = state();
        s.new_tab();
        s.close_tab(1);
        assert_eq!(s.active, 0);
        assert_eq!(s.tabs.len(), 1);
    }

    #[test]
    fn the_last_tab_is_never_removed() {
        let mut s = state();
        s.load_text("a.txt", "content", None);
        s.close_tab(0);
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(s.tab().state.name, "Untitled");
        assert_eq!(s.doc().plain_text(), "");
    }

    #[test]
    fn closing_an_out_of_range_index_is_harmless() {
        let mut s = state();
        s.close_tab(42);
        assert_eq!(s.tabs.len(), 1);
    }

    #[test]
    fn cycle_tab_wraps_both_ways() {
        let mut s = state();
        s.new_tab();
        s.new_tab();
        assert_eq!(s.active, 2);
        s.cycle_tab(true);
        assert_eq!(s.active, 0);
        s.cycle_tab(false);
        assert_eq!(s.active, 2);
    }

    #[test]
    fn select_tab_restores_the_caret() {
        let mut s = state();
        s.load_text("a.txt", "one\ntwo\nthree", None);
        s.cursor = Cursor { line: 2, col: 3 };
        s.stash_cursor();
        s.new_tab();
        assert_eq!(s.cursor.line, 0);
        s.select_tab(0);
        assert_eq!(s.cursor.line, 2);
        assert_eq!(s.cursor.col, 3);
    }

    #[test]
    fn select_tab_rejects_the_same_index() {
        let mut s = state();
        assert!(!s.select_tab(0));
    }

    #[test]
    fn any_dirty_detects_a_modified_tab() {
        let mut s = state();
        assert!(!s.any_dirty());
        s.set_line_text(0, "hello");
        assert!(s.any_dirty());
    }

    // ── caret & selection ─────────────────────────────────────────────────

    #[test]
    fn cursor_is_clamped_into_the_document() {
        let mut s = state_with("a\nb");
        s.cursor = Cursor { line: 99, col: 99 };
        s.clamp_cursor();
        assert_eq!(s.cursor.line, 1);
        assert_eq!(s.cursor.col, 1);
    }

    #[test]
    fn selection_defaults_to_a_single_line() {
        let mut s = state_with("a\nb\nc");
        s.cursor.line = 1;
        assert_eq!(s.selection_range(), (1, 1));
    }

    #[test]
    fn anchor_extends_the_selection_in_both_directions() {
        let mut s = state_with("a\nb\nc\nd");
        s.anchor = Some(3);
        s.cursor.line = 1;
        assert_eq!(s.selection_range(), (1, 3));
        s.anchor = Some(0);
        s.cursor.line = 2;
        assert_eq!(s.selection_range(), (0, 2));
    }

    #[test]
    fn selected_chars_counts_text_plus_newlines() {
        let mut s = state_with("abc\nde\nf");
        s.anchor = Some(0);
        s.cursor.line = 2;
        // "abc" + "de" + "f" + two line breaks
        assert_eq!(s.selected_chars(), 8);
    }

    #[test]
    fn a_plain_caret_selects_nothing() {
        let s = state_with("abc");
        assert_eq!(s.selected_chars(), 0);
    }

    // ── editing ───────────────────────────────────────────────────────────

    #[test]
    fn editing_marks_the_tab_dirty() {
        let mut s = state();
        assert!(s.set_line_text(0, "hello"));
        assert!(s.tab().is_dirty());
        assert!(s.tab().state.dirty);
    }

    #[test]
    fn enter_continues_a_list_and_moves_the_caret() {
        let mut s = state();
        s.set_line_text(0, "- first");
        assert_eq!(s.doc().lines[0].list_type, ListType::Bullet);
        s.cursor.col = s.doc().lines[0].char_len();
        assert_eq!(s.press_enter(), EnterOutcome::MoveTo(1));
        assert_eq!(s.cursor.line, 1);
        assert_eq!(s.doc().line_count(), 2);
    }

    #[test]
    fn indent_and_outdent_apply_to_the_selection() {
        let mut s = state_with("a\nb");
        s.set_list_type(ListType::Bullet);
        s.anchor = Some(0);
        s.cursor.line = 1;
        s.indent(true);
        assert_eq!(s.doc().lines[0].indent, 1);
        assert_eq!(s.doc().lines[1].indent, 1);
        s.indent(false);
        assert_eq!(s.doc().lines[0].indent, 0);
    }

    #[test]
    fn checkbox_toggling_only_works_on_check_lines() {
        let mut s = state_with("task");
        assert!(!s.toggle_checked(0));
        s.set_list_type(ListType::Check);
        assert!(s.toggle_checked(0));
    }

    #[test]
    fn undo_and_redo_move_the_document() {
        let mut s = state_with("a");
        s.toggle_highlight_key("yellow");
        assert_eq!(s.doc().highlighted_count(), 1);
        assert!(s.undo());
        assert_eq!(s.doc().highlighted_count(), 0);
        assert!(s.redo());
        assert_eq!(s.doc().highlighted_count(), 1);
    }

    #[test]
    fn undo_clamps_a_caret_left_outside_the_document() {
        let mut s = state_with("a\nb\nc");
        s.cursor.line = 2;
        s.doc_mut().mutate(|lines| {
            lines.clear();
            lines.push(EditorLine::new("only"));
        });
        s.undo();
        assert!(s.cursor.line < s.doc().line_count());
    }

    // ── highlighting ──────────────────────────────────────────────────────

    #[test]
    fn highlight_toggle_applies_then_removes() {
        let mut s = state_with("a\nb");
        s.anchor = Some(0);
        s.cursor.line = 1;
        assert_eq!(s.toggle_highlight_key("yellow"), Some(true));
        assert_eq!(s.doc().highlighted_count(), 2);
        assert_eq!(s.toggle_highlight_key("yellow"), Some(false));
        assert_eq!(s.doc().highlighted_count(), 0);
    }

    #[test]
    fn highlighting_the_none_key_clears_the_range() {
        let mut s = state_with("a");
        s.apply_highlight_key("blue");
        assert_eq!(s.toggle_highlight_key("none"), Some(false));
        assert_eq!(s.doc().highlighted_count(), 0);
    }

    #[test]
    fn an_unknown_colour_key_is_rejected() {
        let mut s = state_with("a");
        assert_eq!(s.toggle_highlight_key("magenta"), None);
        assert!(!s.apply_highlight_key("magenta"));
    }

    #[test]
    fn a_hex_key_creates_a_custom_colour_on_the_fly() {
        let mut s = state_with("a");
        assert_eq!(s.toggle_highlight_key("#ff8800"), Some(true));
        assert_eq!(
            s.doc().lines[0].colour,
            LineColour::Custom(0xff88_00ff)
        );
    }

    #[test]
    fn custom_colours_are_persisted_into_settings() {
        let mut s = state();
        assert!(s.add_custom_colour("Sunset", "#ff8800"));
        assert_eq!(s.settings.custom_palette.len(), 1);
        assert_eq!(s.palette.entries().len(), 7);
        assert!(!s.add_custom_colour("Bad", "not-a-colour"));
    }

    #[test]
    fn armed_colour_tracks_the_last_choice() {
        let mut s = state_with("a");
        assert_eq!(s.armed_colour, "yellow");
        s.apply_highlight_key("green");
        assert_eq!(s.armed_colour, "green");
    }

    #[test]
    fn clear_highlight_removes_everything_in_range() {
        let mut s = state_with("a\nb");
        s.anchor = Some(0);
        s.cursor.line = 1;
        s.apply_highlight_key("pink");
        s.clear_highlight();
        assert_eq!(s.doc().highlighted_count(), 0);
    }

    // ── extraction ────────────────────────────────────────────────────────

    #[test]
    fn extraction_uses_the_ticked_colours() {
        let mut s = state_with("a\nb\nc");
        s.doc_mut().highlight_lines(0, 0, LineColour::Yellow);
        s.doc_mut().highlight_lines(2, 2, LineColour::Pink);
        s.extract_selected = vec!["yellow".into(), "pink".into()];
        let result = s.extract();
        assert_eq!(result.text, "a\nc");
    }

    #[test]
    fn toggling_an_extract_colour_adds_and_removes_it() {
        let mut s = state();
        assert_eq!(s.extract_selected, vec!["yellow".to_string()]);
        assert!(!s.toggle_extract_colour("yellow"));
        assert!(s.extract_selected.is_empty());
        assert!(s.toggle_extract_colour("green"));
        assert_eq!(s.extract_selected, vec!["green".to_string()]);
    }

    #[test]
    fn grouped_extraction_follows_the_setting() {
        let mut s = state_with("a\nb");
        s.doc_mut().highlight_lines(0, 1, LineColour::Blue);
        s.extract_selected = vec!["blue".into()];
        assert_eq!(s.extract().text, "a\nb");
        s.settings.extract_order = "grouped".to_string();
        assert!(s.extract().text.starts_with("# Blue"));
    }

    // ── find & replace ────────────────────────────────────────────────────

    #[test]
    fn find_counts_matches_and_moves_the_caret() {
        let mut s = state_with("cat cat\ncat");
        s.set_find_query("cat");
        assert_eq!(s.find.match_count(), 3);
        let m = s.find_next().unwrap();
        assert_eq!((m.line, m.start), (0, 4), "next advances to the second match");
        assert_eq!(s.cursor.line, 0);
    }

    #[test]
    fn replace_all_updates_the_document() {
        let mut s = state_with("cat cat");
        s.set_find_query("cat");
        s.set_find_replacement("dog");
        assert_eq!(s.replace_all(), 2);
        assert_eq!(s.doc().plain_text(), "dog dog");
        assert!(s.tab().is_dirty());
    }

    #[test]
    fn replace_one_reports_failure_when_there_is_no_match() {
        let mut s = state_with("cat");
        s.set_find_query("zebra");
        assert!(!s.replace_one());
    }

    // ── files ─────────────────────────────────────────────────────────────

    #[test]
    fn open_path_loads_the_file_and_remembers_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, "one\r\ntwo").unwrap();

        let mut s = state();
        let index = s.open_path(&path).unwrap();
        assert_eq!(index, 0, "the pristine Untitled tab is reused");
        assert_eq!(s.doc().plain_text(), "one\ntwo");
        assert_eq!(s.tab().state.name, "notes.txt");
        assert_eq!(s.tab().line_ending, LineEnding::Crlf);
        assert!(!s.tab().is_dirty());
        assert_eq!(s.settings.recent_files[0], path.to_string_lossy());
    }

    #[test]
    fn opening_the_same_file_twice_focuses_the_existing_tab() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "x").unwrap();
        let mut s = state();
        s.open_path(&path).unwrap();
        s.new_tab();
        let index = s.open_path(&path).unwrap();
        assert_eq!(index, 0);
        assert_eq!(s.tabs.len(), 2);
    }

    #[test]
    fn opening_a_second_file_adds_a_tab() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let mut s = state();
        s.open_path(&a).unwrap();
        s.set_line_text(0, "edited");
        s.open_path(&b).unwrap();
        assert_eq!(s.tabs.len(), 2);
        assert_eq!(s.active, 1);
    }

    #[test]
    fn open_path_reports_missing_files() {
        let mut s = state();
        let err = s.open_path(Path::new("/nope/missing.txt")).unwrap_err();
        assert!(err.to_string().contains("missing.txt"));
    }

    #[test]
    fn save_roundtrips_content_and_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let mut s = state();
        s.load_text("out.txt", "a\nb", None);
        s.tab_mut().line_ending = LineEnding::Crlf;
        s.save_to(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"a\r\nb");
        assert!(!s.tab().is_dirty());
        assert_eq!(s.tab().state.path.as_deref(), Some(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn save_active_without_a_path_asks_for_one() {
        let mut s = state();
        assert!(s.save_active().unwrap().is_none());
    }

    #[test]
    fn save_active_uses_the_stored_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.txt");
        std::fs::write(&path, "old").unwrap();
        let mut s = state();
        s.open_path(&path).unwrap();
        s.set_line_text(0, "new");
        let saved = s.save_active().unwrap();
        assert_eq!(saved, Some(path.clone()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    // ── notes ─────────────────────────────────────────────────────────────

    #[test]
    fn saving_a_note_twice_updates_instead_of_duplicating() {
        let mut s = state_with("body");
        let id = s.save_active_as_note().unwrap();
        s.set_line_text(0, "body v2");
        let again = s.save_active_as_note().unwrap();
        assert_eq!(id, again);
        assert_eq!(s.db.count().unwrap(), 1);
        assert_eq!(s.db.get(id).unwrap().unwrap().content, "body v2");
    }

    #[test]
    fn a_saved_note_keeps_its_highlights() {
        let mut s = state_with("a\nb");
        s.doc_mut().highlight_lines(1, 1, LineColour::Green);
        let id = s.save_active_as_note().unwrap();
        let note = s.db.get(id).unwrap().unwrap();
        assert_eq!(note.highlights_json, r#"{"1":"green"}"#);
        assert_eq!(s.chips_for(&note.highlights_json), vec![0xa8e6_a1ff]);
    }

    #[test]
    fn opening_a_note_restores_highlights_and_lists() {
        let mut s = state_with("- task");
        s.set_list_type(ListType::Bullet);
        s.doc_mut().highlight_lines(0, 0, LineColour::Purple);
        let id = s.save_active_as_note().unwrap();

        // A second window over the SAME database (simulates a restart).
        let mut other = AppState::new(Settings::default(), s.db.clone());
        other.open_note(id).unwrap();
        assert_eq!(other.doc().lines[0].colour, LineColour::Purple);
        assert_eq!(other.doc().lines[0].list_type, ListType::Bullet);
        assert_eq!(other.selected_note_id, id);
    }

    #[test]
    fn opening_a_deleted_note_is_an_error() {
        let mut s = state();
        assert!(s.open_note(4242).is_err());
    }

    #[test]
    fn deleting_a_note_unlinks_tabs() {
        let mut s = state_with("body");
        let id = s.save_active_as_note().unwrap();
        assert!(s.delete_note(id).unwrap());
        assert_eq!(s.tab().state.note_id, None);
        assert_eq!(s.selected_note_id, -1);
    }

    #[test]
    fn pinning_flips_and_reports() {
        let mut s = state_with("body");
        let id = s.save_active_as_note().unwrap();
        assert!(s.toggle_pin(id).unwrap());
        assert!(!s.toggle_pin(id).unwrap());
        assert!(s.toggle_pin(9999).is_err());
    }

    #[test]
    fn note_count_label_shows_a_filtered_total() {
        let s = state();
        s.db.save(&Note::new("alpha", "")).unwrap();
        s.db.save(&Note::new("beta", "")).unwrap();
        assert_eq!(s.note_count_label(2), "2 notes");
        assert_eq!(s.note_count_label(1), "1 of 2 notes");
    }

    #[test]
    fn note_list_respects_the_search_query() {
        let mut s = state();
        s.db.save(&Note::new("alpha", "")).unwrap();
        s.db.save(&Note::new("beta", "")).unwrap();
        s.note_query = "alp".into();
        let list = s.note_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "alpha");
    }

    #[test]
    fn new_note_creates_a_placeholder_row() {
        let mut s = state();
        let id = s.new_note().unwrap();
        assert_eq!(s.db.count().unwrap(), 1);
        assert_eq!(s.selected_note_id, id);
    }

    // ── view / theme ──────────────────────────────────────────────────────

    #[test]
    fn zoom_steps_are_clamped() {
        let mut s = state();
        for _ in 0..50 {
            s.zoom_step(1.0);
        }
        assert_eq!(s.settings.zoom, 3.0);
        for _ in 0..80 {
            s.zoom_step(-1.0);
        }
        assert_eq!(s.settings.zoom, 0.5);
        s.zoom_step(0.0);
        assert_eq!(s.settings.zoom, 1.0);
    }

    #[test]
    fn unknown_themes_are_rejected() {
        let mut s = state();
        assert!(s.set_theme("dark"));
        assert!(!s.set_theme("chartreuse"));
        assert_eq!(s.settings.theme, "dark");
    }

    #[test]
    fn the_dark_twin_toggle_swaps_themes() {
        let mut s = state();
        assert_eq!(s.toggle_dark_twin(), "dark");
        assert_eq!(s.toggle_dark_twin(), "light");
    }

    // ── status ────────────────────────────────────────────────────────────

    #[test]
    fn status_reports_caret_metrics_and_encoding() {
        let mut s = state_with("hello world\nsecond");
        s.cursor = Cursor { line: 1, col: 3 };
        let status = s.compute_status();
        assert_eq!(status.caret_text, "Ln 2, Col 4");
        assert_eq!(status.metrics_text, "3 words \u{00b7} 17 chars \u{00b7} 2 lines");
        assert_eq!(status.line_count, 2);
        assert_eq!(status.encoding, "utf-8");
        assert_eq!(status.line_ending, "LF");
        assert_eq!(status.zoom_text, "100%");
        assert_eq!(status.saved_text, "Saved");
    }

    #[test]
    fn status_shows_the_selection_and_dirty_flag() {
        let mut s = state_with("abc\nde");
        s.anchor = Some(0);
        s.cursor.line = 1;
        s.set_line_text(0, "abcd");
        let status = s.compute_status();
        assert!(status.caret_text.contains("selected"));
        assert_eq!(status.selected_chars, 7);
        assert!(status.dirty);
        assert_eq!(status.saved_text, "Unsaved changes");
    }

    #[test]
    fn status_counts_highlights() {
        let mut s = state_with("a\nb\nc");
        s.doc_mut().highlight_lines(0, 1, LineColour::Yellow);
        let status = s.compute_status();
        assert_eq!(status.highlight_count, 2);
        assert_eq!(status.highlight_text, "2 highlights");
    }

    #[test]
    fn window_title_shows_the_tab_name_and_app() {
        let mut s = state_with("x");
        assert_eq!(s.window_title(), "test.txt \u{2014} NotePad Pro");
        s.set_line_text(0, "changed");
        assert!(s.window_title().starts_with("test.txt \u{25cf}"));
    }

    // ── session ───────────────────────────────────────────────────────────

    #[test]
    fn session_roundtrips_tabs_highlights_and_caret() {
        let mut s = state_with("one\ntwo");
        s.doc_mut().highlight_lines(0, 0, LineColour::Yellow);
        s.cursor = Cursor { line: 1, col: 2 };
        s.stash_cursor();
        s.new_tab();
        s.load_text("second.txt", "other", None);
        let session = s.build_session();

        let mut restored = state();
        restored.restore_session(&session);
        assert_eq!(restored.tabs.len(), 2);
        assert_eq!(restored.active, 1);
        assert_eq!(restored.tabs[0].doc.plain_text(), "one\ntwo");
        assert_eq!(
            restored.tabs[0].doc.lines[0].colour,
            LineColour::Yellow
        );
        restored.select_tab(0);
        assert_eq!(restored.cursor.line, 1);
        assert_eq!(restored.cursor.col, 2);
    }

    #[test]
    fn restoring_an_empty_session_keeps_the_current_tabs() {
        let mut s = state_with("keep me");
        s.restore_session(&Session::default());
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(s.doc().plain_text(), "keep me");
    }

    #[test]
    fn restored_tabs_start_clean() {
        let mut s = state_with("x");
        s.set_line_text(0, "dirty");
        let session = s.build_session();
        let mut restored = state();
        restored.restore_session(&session);
        assert!(!restored.any_dirty());
    }

    #[test]
    fn first_line_or_untitled_prefers_real_text() {
        assert_eq!(first_line_or_untitled("\n\nhello"), "hello");
        assert_eq!(first_line_or_untitled(""), "Untitled");
        let long = "x".repeat(100);
        assert_eq!(first_line_or_untitled(&long).chars().count(), 48);
    }
}
