//! The document: an ordered list of [`EditorLine`]s plus undo history.

use crate::editor::list_engine::ListEngine;
use crate::editor::undo::UndoStack;
use crate::types::line::{EditorLine, LineColour, ListType, MAX_INDENT};

/// A single open document.
#[derive(Debug, Clone)]
pub struct Document {
    pub lines: Vec<EditorLine>,
    pub dirty: bool,
    undo: UndoStack,
    /// Set while the user is typing in one line so keystrokes coalesce into a
    /// single undo step. Cleared by any structural edit.
    typing_in: Option<usize>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// An empty document containing one blank line.
    pub fn new() -> Self {
        let lines = vec![EditorLine::default()];
        Self {
            undo: UndoStack::new(lines.clone()),
            lines,
            dirty: false,
            typing_in: None,
        }
    }

    /// Build from an existing line vector.
    pub fn from_lines(mut lines: Vec<EditorLine>) -> Self {
        if lines.is_empty() {
            lines.push(EditorLine::default());
        }
        for line in lines.iter_mut() {
            line.normalise();
        }
        ListEngine::renumber(&mut lines);
        let snapshot = lines.clone();
        Self {
            lines,
            dirty: false,
            undo: UndoStack::new(snapshot),
            typing_in: None,
        }
    }

    /// Parse plain text. Never produces a zero-line document.
    pub fn from_plain_text(text: &str) -> Self {
        let lines: Vec<EditorLine> = if text.is_empty() {
            vec![EditorLine::default()]
        } else {
            text.split('\n')
                .map(|t| EditorLine::new(t.trim_end_matches('\r')))
                .collect()
        };
        Self::from_lines(lines)
    }

    /// Serialise to plain text, dropping all metadata.
    pub fn plain_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ── Undo ──────────────────────────────────────────────────────────────

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo.undo() {
            self.lines = prev;
            self.dirty = true;
            self.typing_in = None;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.undo.redo() {
            self.lines = next;
            self.dirty = true;
            self.typing_in = None;
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    /// Drop undo history and mark the document clean. Called after save.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
        self.typing_in = None;
    }

    pub fn clear_history(&mut self) {
        self.undo.reset(self.lines.clone());
        self.typing_in = None;
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.undo_depth()
    }

    // ── Metrics ───────────────────────────────────────────────────────────

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn char_count(&self) -> usize {
        self.lines.iter().map(|l| l.text.chars().count()).sum()
    }

    pub fn word_count(&self) -> usize {
        self.lines.iter().map(|l| l.text.split_whitespace().count()).sum()
    }

    pub fn highlighted_count(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| l.colour.is_highlighted())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.text.is_empty())
    }

    // ── Editing primitives ────────────────────────────────────────────────

    /// Record the current buffer as a new undo step and mark the document
    /// dirty.
    ///
    /// Call this after mutating `lines` directly — the list engine and the
    /// find/replace engine both do that. Going through [`Document::mutate`]
    /// instead is preferred when the change can be expressed as a closure.
    pub fn commit(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(EditorLine::default());
        }
        ListEngine::renumber(&mut self.lines);
        self.typing_in = None;
        self.undo.push(self.lines.clone());
        self.dirty = true;
    }

    /// Apply `f` to the line buffer, then renumber and record history.
    ///
    /// Every structural mutation funnels through here so undo can never miss
    /// a change.
    pub fn mutate<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Vec<EditorLine>),
    {
        f(&mut self.lines);
        self.commit();
    }

    /// Replace a line's text. Keystrokes in the same line coalesce into one
    /// undo step; the first keystroke of a burst creates the step.
    pub fn set_text(&mut self, index: usize, text: &str) -> bool {
        let Some(line) = self.lines.get_mut(index) else {
            return false;
        };
        if line.text == text {
            return false;
        }
        let coalesce = self.typing_in == Some(index) && self.undo.can_replace_top();
        line.text = text.to_string();
        // Markdown shortcut: "- " typed at the start of a plain line.
        ListEngine::try_markdown_shortcut(line);
        ListEngine::renumber(&mut self.lines);
        if coalesce {
            self.undo.replace_top(self.lines.clone());
        } else {
            self.undo.push(self.lines.clone());
        }
        self.typing_in = Some(index);
        self.dirty = true;
        true
    }

    /// Insert a blank line after `index` and return its index.
    pub fn insert_line_after(&mut self, index: usize) -> usize {
        let at = index.min(self.lines.len().saturating_sub(1));
        self.mutate(|lines| lines.insert(at + 1, EditorLine::default()));
        at + 1
    }

    /// Remove line `index`. The document always keeps at least one line.
    pub fn remove_line(&mut self, index: usize) -> Option<EditorLine> {
        if index >= self.lines.len() {
            return None;
        }
        let mut removed = None;
        self.mutate(|lines| {
            removed = Some(lines.remove(index));
        });
        removed
    }

    /// Delete a line and append its text to the previous one (backspace at
    /// column 0). Returns `(line, char_column)` for the caret.
    pub fn join_with_previous(&mut self, index: usize) -> Option<(usize, usize)> {
        if index == 0 || index >= self.lines.len() {
            return None;
        }
        let mut caret_col = 0usize;
        self.mutate(|lines| {
            let text = lines[index].text.clone();
            caret_col = lines[index - 1].text.chars().count();
            let joined = format!("{}{}", lines[index - 1].text, text);
            lines[index - 1].text = joined;
            lines.remove(index);
        });
        Some((index - 1, caret_col))
    }

    // ── Feature 1 & 2: highlighting ───────────────────────────────────────

    /// Apply a colour to an inclusive range of lines. Out-of-range indices
    /// are clamped, never panicking.
    pub fn highlight_lines(&mut self, start: usize, end: usize, colour: LineColour) {
        let (start, end) = self.clamp_range(start, end);
        self.mutate(|lines| {
            for line in lines.iter_mut().take(end + 1).skip(start) {
                line.colour = colour;
            }
        });
    }

    pub fn remove_highlight(&mut self, start: usize, end: usize) {
        self.highlight_lines(start, end, LineColour::None);
    }

    /// Feature 1: toggle. Removes the colour when *every* line in the range
    /// already has it, otherwise applies it to the whole range.
    ///
    /// Returns `true` when the colour was applied, `false` when it was removed.
    pub fn toggle_highlight(&mut self, start: usize, end: usize, colour: LineColour) -> bool {
        let (start, end) = self.clamp_range(start, end);
        let all_highlighted = self.lines[start..=end]
            .iter()
            .all(|l| l.colour == colour);
        if all_highlighted {
            self.remove_highlight(start, end);
            false
        } else {
            self.highlight_lines(start, end, colour);
            true
        }
    }

    // ── Feature 4: lists ──────────────────────────────────────────────────

    pub fn set_list_type(&mut self, start: usize, end: usize, list_type: ListType) {
        let (start, end) = self.clamp_range(start, end);
        self.mutate(|lines| {
            for line in lines.iter_mut().take(end + 1).skip(start) {
                line.list_type = list_type;
                if list_type == ListType::None {
                    line.indent = 0;
                    line.checked = false;
                }
                if list_type != ListType::Check {
                    line.checked = false;
                }
                line.normalise();
            }
        });
    }

    /// Indent (`delta > 0`) or outdent a range.
    pub fn change_indent(&mut self, start: usize, end: usize, delta: i8) {
        let (start, end) = self.clamp_range(start, end);
        self.mutate(|lines| {
            for line in lines.iter_mut().take(end + 1).skip(start) {
                let next = (line.indent as i16 + delta as i16).clamp(0, MAX_INDENT as i16);
                line.indent = next as u8;
            }
        });
    }

    pub fn toggle_checked(&mut self, index: usize) -> Option<bool> {
        if index >= self.lines.len() {
            return None;
        }
        let mut state = None;
        self.mutate(|lines| {
            state = Some(ListEngine::toggle_checked(&mut lines[index]));
        });
        state
    }

    // ── Find & replace support ────────────────────────────────────────────

    /// Replace one match given byte offsets, clamping to char boundaries.
    pub fn replace_in_line(&mut self, line: usize, start: usize, end: usize, replacement: &str) -> bool {
        if line >= self.lines.len() {
            return false;
        }
        let text = self.lines[line].text.clone();
        let start = clamp_to_boundary(&text, start);
        let end = clamp_to_boundary(&text, end.max(start));
        let replaced = format!("{}{}{}", &text[..start], replacement, &text[end..]);
        self.mutate(|lines| lines[line].text = replaced);
        true
    }

    fn clamp_range(&self, start: usize, end: usize) -> (usize, usize) {
        let last = self.lines.len().saturating_sub(1);
        let start = start.min(last);
        let end = end.min(last).max(start);
        (start, end)
    }
}

/// Snap a byte offset onto the nearest valid UTF-8 character boundary.
fn clamp_to_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut i = index;
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(texts: &[&str]) -> Document {
        Document::from_lines(texts.iter().map(|t| EditorLine::new(*t)).collect())
    }

    #[test]
    fn new_document_has_one_blank_line() {
        let d = Document::new();
        assert_eq!(d.line_count(), 1);
        assert_eq!(d.plain_text(), "");
        assert!(!d.dirty);
    }

    #[test]
    fn from_plain_text_preserves_every_line_including_trailing_blank() {
        let d = Document::from_plain_text("a\nb\n");
        assert_eq!(d.line_count(), 3);
        assert_eq!(d.lines[0].text, "a");
        assert_eq!(d.lines[2].text, "");
    }

    #[test]
    fn plain_text_roundtrips() {
        let original = "one\ntwo\nthree";
        assert_eq!(Document::from_plain_text(original).plain_text(), original);
    }

    #[test]
    fn from_plain_text_normalises_crlf() {
        let d = Document::from_plain_text("a\r\nb\r\n");
        assert_eq!(d.lines[0].text, "a");
        assert_eq!(d.lines[1].text, "b");
    }

    #[test]
    fn empty_input_yields_one_line_not_zero() {
        assert_eq!(Document::from_plain_text("").line_count(), 1);
        assert_eq!(Document::from_lines(vec![]).line_count(), 1);
    }

    #[test]
    fn metrics_count_words_chars_and_lines() {
        let d = doc(&["hello world", "again", ""]);
        assert_eq!(d.line_count(), 3);
        assert_eq!(d.word_count(), 3);
        assert_eq!(d.char_count(), 11 + 5 + 0);
    }

    #[test]
    fn highlight_applies_to_the_whole_range() {
        let mut d = doc(&["a", "b", "c", "d"]);
        d.highlight_lines(1, 2, LineColour::Yellow);
        let colours: Vec<LineColour> = d.lines.iter().map(|l| l.colour).collect();
        assert_eq!(
            colours,
            vec![
                LineColour::None,
                LineColour::Yellow,
                LineColour::Yellow,
                LineColour::None
            ]
        );
        assert_eq!(d.highlighted_count(), 2);
        assert!(d.dirty);
    }

    #[test]
    fn highlight_range_is_clamped_not_panicked() {
        let mut d = doc(&["a", "b"]);
        d.highlight_lines(0, 99, LineColour::Pink);
        assert_eq!(d.highlighted_count(), 2);
        d.highlight_lines(50, 60, LineColour::Blue);
        assert_eq!(d.lines[1].colour, LineColour::Blue);
    }

    #[test]
    fn remove_highlight_clears_the_range() {
        let mut d = doc(&["a", "b"]);
        d.highlight_lines(0, 1, LineColour::Green);
        d.remove_highlight(0, 1);
        assert_eq!(d.highlighted_count(), 0);
    }

    #[test]
    fn toggle_highlight_applies_then_removes() {
        let mut d = doc(&["a", "b"]);
        assert!(d.toggle_highlight(0, 1, LineColour::Blue));
        assert_eq!(d.highlighted_count(), 2);
        assert!(!d.toggle_highlight(0, 1, LineColour::Blue));
        assert_eq!(d.highlighted_count(), 0);
    }

    #[test]
    fn toggle_highlight_over_a_mixed_range_applies() {
        let mut d = doc(&["a", "b"]);
        d.highlight_lines(0, 0, LineColour::Blue);
        assert!(d.toggle_highlight(0, 1, LineColour::Blue));
        assert_eq!(d.highlighted_count(), 2);
    }

    #[test]
    fn toggle_with_a_different_colour_replaces_it() {
        let mut d = doc(&["a"]);
        d.highlight_lines(0, 0, LineColour::Blue);
        assert!(d.toggle_highlight(0, 0, LineColour::Yellow));
        assert_eq!(d.lines[0].colour, LineColour::Yellow);
    }

    #[test]
    fn set_list_type_marks_the_range() {
        let mut d = doc(&["a", "b", "c"]);
        d.set_list_type(0, 1, ListType::Bullet);
        assert_eq!(d.lines[0].list_type, ListType::Bullet);
        assert_eq!(d.lines[2].list_type, ListType::None);
    }

    #[test]
    fn clearing_the_list_type_resets_indent_and_checked() {
        let mut d = doc(&["a"]);
        d.set_list_type(0, 0, ListType::Check);
        d.change_indent(0, 0, 3);
        d.toggle_checked(0);
        assert!(d.lines[0].checked);
        d.set_list_type(0, 0, ListType::None);
        assert_eq!(d.lines[0].indent, 0);
        assert!(!d.lines[0].checked);
    }

    #[test]
    fn indent_is_clamped_in_both_directions() {
        let mut d = doc(&["a"]);
        d.set_list_type(0, 0, ListType::Bullet);
        d.change_indent(0, 0, 100);
        assert_eq!(d.lines[0].indent, MAX_INDENT);
        d.change_indent(0, 0, -100);
        assert_eq!(d.lines[0].indent, 0);
    }

    #[test]
    fn toggle_checked_only_works_on_check_lines() {
        let mut d = doc(&["a"]);
        assert_eq!(d.toggle_checked(0), Some(false), "plain line stays unchecked");
        d.set_list_type(0, 0, ListType::Check);
        assert_eq!(d.toggle_checked(0), Some(true));
        assert_eq!(d.toggle_checked(99), None);
    }

    #[test]
    fn set_text_updates_and_dirties_the_document() {
        let mut d = doc(&["a"]);
        assert!(d.set_text(0, "hello"));
        assert_eq!(d.plain_text(), "hello");
        assert!(d.dirty);
        assert!(!d.set_text(0, "hello"), "no-op edit returns false");
        assert!(!d.set_text(99, "x"), "out of range returns false");
    }

    #[test]
    fn typing_a_markdown_prefix_converts_the_line() {
        let mut d = doc(&[""]);
        d.set_text(0, "- task");
        assert_eq!(d.lines[0].list_type, ListType::Bullet);
        assert_eq!(d.lines[0].text, "task");
    }

    #[test]
    fn a_typing_burst_is_one_undo_step() {
        let mut d = doc(&[""]);
        for text in ["h", "he", "hel", "hell", "hello"] {
            d.set_text(0, text);
        }
        assert_eq!(d.undo_depth(), 1);
        assert!(d.undo());
        assert_eq!(d.plain_text(), "");
        assert!(!d.can_undo());
    }

    #[test]
    fn switching_lines_starts_a_new_undo_step() {
        let mut d = doc(&["", ""]);
        d.set_text(0, "a");
        d.set_text(1, "b");
        assert_eq!(d.undo_depth(), 2);
    }

    #[test]
    fn undo_and_redo_restore_highlight_state() {
        let mut d = doc(&["a", "b"]);
        d.toggle_highlight(0, 1, LineColour::Green);
        assert!(d.undo());
        assert_eq!(d.highlighted_count(), 0);
        assert!(d.redo());
        assert_eq!(d.highlighted_count(), 2);
    }

    #[test]
    fn undo_on_a_fresh_document_returns_false() {
        let mut d = doc(&["a"]);
        assert!(!d.undo());
        assert!(!d.redo());
    }

    #[test]
    fn structural_edit_breaks_the_typing_coalescing() {
        let mut d = doc(&["a"]);
        d.set_text(0, "ab");
        d.set_text(0, "abc");
        d.insert_line_after(0);
        d.set_text(0, "abcd");
        assert_eq!(d.undo_depth(), 3);
    }

    #[test]
    fn insert_line_after_returns_the_new_index() {
        let mut d = doc(&["a", "b"]);
        assert_eq!(d.insert_line_after(0), 1);
        assert_eq!(d.line_count(), 3);
        assert_eq!(d.lines[1].text, "");
    }

    #[test]
    fn insert_line_after_clamps_the_index() {
        let mut d = doc(&["a"]);
        assert_eq!(d.insert_line_after(99), 1);
        assert_eq!(d.line_count(), 2);
    }

    #[test]
    fn remove_line_returns_the_removed_line() {
        let mut d = doc(&["a", "b"]);
        let removed = d.remove_line(0).unwrap();
        assert_eq!(removed.text, "a");
        assert_eq!(d.plain_text(), "b");
        assert!(d.remove_line(42).is_none());
    }

    #[test]
    fn removing_the_last_line_leaves_one_blank() {
        let mut d = doc(&["a"]);
        d.remove_line(0);
        assert_eq!(d.line_count(), 1);
        assert_eq!(d.plain_text(), "");
    }

    #[test]
    fn join_with_previous_merges_text() {
        let mut d = doc(&["hello", "world"]);
        assert_eq!(d.join_with_previous(1), Some((0, 5)));
        assert_eq!(d.plain_text(), "helloworld");
        assert!(d.join_with_previous(0).is_none());
    }

    #[test]
    fn replace_in_line_swaps_the_matched_range() {
        let mut d = doc(&["the cat sat"]);
        assert!(d.replace_in_line(0, 4, 7, "dog"));
        assert_eq!(d.plain_text(), "the dog sat");
    }

    #[test]
    fn replace_in_line_clamps_invalid_offsets() {
        let mut d = doc(&["café"]);
        // Byte 4 is inside 'é' when counted naively; must not panic.
        assert!(d.replace_in_line(0, 4, 5, "!"));
        assert!(!d.replace_in_line(9, 0, 1, "x"));
    }

    #[test]
    fn mark_saved_clears_the_dirty_flag() {
        let mut d = doc(&["a"]);
        d.set_text(0, "b");
        assert!(d.dirty);
        d.mark_saved();
        assert!(!d.dirty);
    }

    #[test]
    fn clear_history_resets_the_undo_baseline() {
        let mut d = doc(&["a"]);
        d.set_text(0, "b");
        d.clear_history();
        assert!(!d.can_undo());
    }

    #[test]
    fn commit_records_direct_buffer_edits() {
        let mut d = doc(&["a"]);
        d.lines.push(EditorLine::new("b"));
        d.commit();
        assert!(d.dirty);
        assert!(d.undo_depth() == 1);
        assert!(d.undo());
        assert_eq!(d.line_count(), 1);
    }

    #[test]
    fn commit_guarantees_at_least_one_line() {
        let mut d = doc(&["a"]);
        d.lines.clear();
        d.commit();
        assert_eq!(d.line_count(), 1);
        assert_eq!(d.plain_text(), "");
    }

    #[test]
    fn from_lines_normalises_untrusted_indent() {
        let d = Document::from_lines(vec![EditorLine {
            indent: 99,
            list_type: ListType::Bullet,
            text: "x".into(),
            ..Default::default()
        }]);
        assert_eq!(d.lines[0].indent, MAX_INDENT);
    }

    #[test]
    fn is_empty_ignores_blank_lines() {
        let mut d = doc(&["", "  "]);
        // "  " is not empty text, so the document is not "empty".
        assert!(!d.is_empty());
        d.set_text(1, "");
        assert!(d.is_empty());
    }
}
