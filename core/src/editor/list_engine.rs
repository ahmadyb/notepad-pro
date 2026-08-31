//! List behaviour: Enter, Tab/Shift+Tab, renumbering, markdown shortcuts.

use crate::types::line::{EditorLine, ListType, MAX_INDENT};

/// What the caller should do after [`ListEngine::handle_enter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterOutcome {
    /// Not a list line — the caller inserts a blank line normally.
    InsertBlank,
    /// The Enter key exited the list or outdented; no new line was created.
    Consumed,
    /// Move the caret to this line index.
    MoveTo(usize),
}

pub struct ListEngine;

impl ListEngine {
    /// Handle Enter pressed at character offset `col` on line `index`.
    pub fn handle_enter(lines: &mut Vec<EditorLine>, index: usize, col: usize) -> EnterOutcome {
        let Some(current) = lines.get(index) else {
            return EnterOutcome::InsertBlank;
        };
        if current.list_type == ListType::None {
            return EnterOutcome::InsertBlank;
        }

        // Empty top-level item exits the list.
        if current.text.is_empty() && current.indent == 0 {
            lines[index].list_type = ListType::None;
            lines[index].checked = false;
            return EnterOutcome::Consumed;
        }

        // Empty nested item outdents instead of exiting.
        if current.text.is_empty() && current.indent > 0 {
            lines[index].indent -= 1;
            return EnterOutcome::Consumed;
        }

        // Split the text at the caret. `col` is a character offset.
        let split_at = col.min(current.text.chars().count());
        let (head, tail) = split_chars(&current.text, split_at);

        let template = lines[index].clone();
        lines[index].text = head;

        let mut new_line = EditorLine {
            text: tail,
            colour: template.colour,
            list_type: template.list_type,
            indent: template.indent,
            checked: false,
            number: 0,
            inline_spans: Vec::new(),
        };
        // A checked item continues as an unchecked one.
        new_line.checked = false;
        lines.insert(index + 1, new_line);
        Self::renumber(lines);
        EnterOutcome::MoveTo(index + 1)
    }

    /// Handle Tab (indent) / Shift+Tab (outdent). No-op for plain lines.
    pub fn handle_tab(lines: &mut Vec<EditorLine>, index: usize, indent: bool) -> bool {
        let Some(line) = lines.get_mut(index) else {
            return false;
        };
        if line.list_type == ListType::None {
            return false;
        }
        let before = line.indent;
        line.indent = if indent {
            (line.indent + 1).min(MAX_INDENT)
        } else {
            line.indent.saturating_sub(1)
        };
        let changed = line.indent != before;
        if changed {
            Self::renumber(lines);
        }
        changed
    }

    /// Recompute display numbers. Each indent depth has its own counter and a
    /// non-number line interrupts the run (matching the original behaviour,
    /// and fixing the "numbers collided after a paragraph" bug).
    pub fn renumber(lines: &mut [EditorLine]) {
        let mut counters = [0u32; (MAX_INDENT as usize) + 1];
        for line in lines.iter_mut() {
            if line.list_type == ListType::Number {
                let depth = (line.indent as usize).min(MAX_INDENT as usize);
                counters[depth] += 1;
                for deeper in counters.iter_mut().skip(depth + 1) {
                    *deeper = 0;
                }
                line.number = counters[depth];
            } else {
                line.number = 0;
                counters = [0; (MAX_INDENT as usize) + 1];
            }
        }
    }

    /// Convert a markdown-style prefix typed by the user. Returns `true` when
    /// the line was converted.
    pub fn try_markdown_shortcut(line: &mut EditorLine) -> bool {
        if line.list_type != ListType::None {
            return false;
        }
        if let Some(rest) = strip_prefix(&line.text, &["- ", "* ", "+ "]) {
            line.text = rest;
            line.list_type = ListType::Bullet;
            return true;
        }
        if let Some(rest) = strip_prefix(&line.text, &["[] ", "[ ] "]) {
            line.text = rest;
            line.list_type = ListType::Check;
            line.checked = false;
            return true;
        }
        if let Some(rest) = strip_numbered_prefix(&line.text) {
            line.text = rest;
            line.list_type = ListType::Number;
            return true;
        }
        false
    }

    /// Toggle a checkbox line; returns the new state.
    pub fn toggle_checked(line: &mut EditorLine) -> bool {
        if line.list_type == ListType::Check {
            line.checked = !line.checked;
        }
        line.checked
    }

    /// Glyph for a bullet at a given depth. Mirrors `ui/components/list_marker.slint`.
    pub fn bullet_glyph(indent: u8) -> &'static str {
        match indent {
            0 => "•",
            1 => "◦",
            2 => "▪",
            _ => "‣",
        }
    }

    /// Marker text for a line, or `None` when no marker is drawn.
    pub fn marker_text(line: &EditorLine) -> Option<String> {
        match line.list_type {
            ListType::None => None,
            ListType::Bullet => Some(Self::bullet_glyph(line.indent).to_string()),
            ListType::Number => Some(format!("{}.", line.number)),
            ListType::Check => Some(if line.checked { "☑" } else { "☐" }.to_string()),
        }
    }
}

/// Split `s` at a character offset, never at a byte boundary.
fn split_chars(s: &str, char_index: usize) -> (String, String) {
    let byte_index = s
        .char_indices()
        .nth(char_index)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    (s[..byte_index].to_string(), s[byte_index..].to_string())
}

fn strip_prefix(text: &str, prefixes: &[&str]) -> Option<String> {
    prefixes
        .iter()
        .find(|p| text.starts_with(**p))
        .map(|p| text[p.len()..].to_string())
}

/// Recognise `1. `, `12. `, `123. ` — up to three digits.
fn strip_numbered_prefix(text: &str) -> Option<String> {
    let digits = text.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = &text[digits..];
    if let Some(tail) = rest.strip_prefix(". ") {
        Some(tail.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bullet(text: &str, indent: u8) -> EditorLine {
        EditorLine {
            text: text.into(),
            list_type: ListType::Bullet,
            indent,
            ..Default::default()
        }
    }

    fn numbered(text: &str, indent: u8) -> EditorLine {
        EditorLine {
            text: text.into(),
            list_type: ListType::Number,
            indent,
            ..Default::default()
        }
    }

    #[test]
    fn enter_on_a_plain_line_asks_the_caller_to_insert_blank() {
        let mut lines = vec![EditorLine::new("hello")];
        assert_eq!(ListEngine::handle_enter(&mut lines, 0, 5), EnterOutcome::InsertBlank);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn enter_continues_a_bullet_and_keeps_the_marker() {
        let mut lines = vec![bullet("First item", 0)];
        let outcome = ListEngine::handle_enter(&mut lines, 0, 10);
        assert_eq!(outcome, EnterOutcome::MoveTo(1));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].list_type, ListType::Bullet);
        assert_eq!(lines[1].indent, 0);
    }

    #[test]
    fn enter_splits_the_line_at_the_caret() {
        let mut lines = vec![bullet("onetwo", 0)];
        ListEngine::handle_enter(&mut lines, 0, 3);
        assert_eq!(lines[0].text, "one");
        assert_eq!(lines[1].text, "two");
    }

    #[test]
    fn enter_splits_on_a_multibyte_boundary_safely() {
        let mut lines = vec![bullet("café au lait", 0)];
        ListEngine::handle_enter(&mut lines, 0, 4);
        assert_eq!(lines[0].text, "café");
        assert_eq!(lines[1].text, " au lait");
    }

    #[test]
    fn caret_beyond_end_of_line_is_clamped() {
        let mut lines = vec![bullet("ab", 0)];
        ListEngine::handle_enter(&mut lines, 0, 99);
        assert_eq!(lines[0].text, "ab");
        assert_eq!(lines[1].text, "");
    }

    #[test]
    fn enter_on_empty_top_level_item_exits_the_list() {
        let mut lines = vec![bullet("", 0)];
        assert_eq!(ListEngine::handle_enter(&mut lines, 0, 0), EnterOutcome::Consumed);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].list_type, ListType::None);
    }

    #[test]
    fn enter_on_empty_nested_item_outdents() {
        let mut lines = vec![bullet("", 2)];
        assert_eq!(ListEngine::handle_enter(&mut lines, 0, 0), EnterOutcome::Consumed);
        assert_eq!(lines[0].indent, 1);
        assert_eq!(lines[0].list_type, ListType::Bullet);
    }

    #[test]
    fn checked_state_does_not_propagate_to_the_new_item() {
        let mut lines = vec![EditorLine {
            text: "done".into(),
            list_type: ListType::Check,
            checked: true,
            ..Default::default()
        }];
        ListEngine::handle_enter(&mut lines, 0, 4);
        assert!(lines[0].checked);
        assert!(!lines[1].checked);
        assert_eq!(lines[1].list_type, ListType::Check);
    }

    #[test]
    fn enter_on_an_out_of_range_index_is_harmless() {
        let mut lines = vec![EditorLine::new("a")];
        assert_eq!(ListEngine::handle_enter(&mut lines, 42, 0), EnterOutcome::InsertBlank);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn tab_indents_and_shift_tab_outdents() {
        let mut lines = vec![bullet("a", 0)];
        assert!(ListEngine::handle_tab(&mut lines, 0, true));
        assert_eq!(lines[0].indent, 1);
        assert!(ListEngine::handle_tab(&mut lines, 0, false));
        assert_eq!(lines[0].indent, 0);
    }

    #[test]
    fn tab_is_clamped_to_the_maximum_depth() {
        let mut lines = vec![bullet("a", MAX_INDENT)];
        assert!(!ListEngine::handle_tab(&mut lines, 0, true));
        assert_eq!(lines[0].indent, MAX_INDENT);
    }

    #[test]
    fn outdent_at_zero_is_a_noop() {
        let mut lines = vec![bullet("a", 0)];
        assert!(!ListEngine::handle_tab(&mut lines, 0, false));
        assert_eq!(lines[0].indent, 0);
    }

    #[test]
    fn tab_does_nothing_on_plain_lines() {
        let mut lines = vec![EditorLine::new("plain")];
        assert!(!ListEngine::handle_tab(&mut lines, 0, true));
        assert_eq!(lines[0].indent, 0);
    }

    #[test]
    fn renumber_numbers_a_flat_list_from_one() {
        let mut lines = vec![numbered("a", 0), numbered("b", 0), numbered("c", 0)];
        ListEngine::renumber(&mut lines);
        let numbers: Vec<u32> = lines.iter().map(|l| l.number).collect();
        assert_eq!(numbers, vec![1, 2, 3]);
    }

    #[test]
    fn renumber_gives_each_depth_its_own_counter() {
        let mut lines = vec![
            numbered("a", 0),
            numbered("a1", 1),
            numbered("a2", 1),
            numbered("b", 0),
            numbered("b1", 1),
        ];
        ListEngine::renumber(&mut lines);
        let numbers: Vec<u32> = lines.iter().map(|l| l.number).collect();
        assert_eq!(numbers, vec![1, 1, 2, 2, 1]);
    }

    #[test]
    fn renumber_resets_after_a_plain_paragraph() {
        let mut lines = vec![
            numbered("a", 0),
            numbered("b", 0),
            EditorLine::new("paragraph"),
            numbered("c", 0),
        ];
        ListEngine::renumber(&mut lines);
        let numbers: Vec<u32> = lines.iter().map(|l| l.number).collect();
        assert_eq!(numbers, vec![1, 2, 0, 1]);
    }

    #[test]
    fn renumber_clears_stale_numbers_on_non_number_lines() {
        let mut lines = vec![EditorLine {
            list_type: ListType::Bullet,
            number: 99,
            text: "x".into(),
            ..Default::default()
        }];
        ListEngine::renumber(&mut lines);
        assert_eq!(lines[0].number, 0);
    }

    #[test]
    fn renumber_tolerates_out_of_range_indent() {
        let mut lines = vec![EditorLine {
            list_type: ListType::Number,
            indent: 200,
            ..Default::default()
        }];
        ListEngine::renumber(&mut lines);
        assert_eq!(lines[0].number, 1);
    }

    #[test]
    fn markdown_dash_becomes_a_bullet() {
        let mut line = EditorLine::new("- buy milk");
        assert!(ListEngine::try_markdown_shortcut(&mut line));
        assert_eq!(line.list_type, ListType::Bullet);
        assert_eq!(line.text, "buy milk");
    }

    #[test]
    fn markdown_asterisk_and_plus_become_bullets() {
        for prefix in ["* ", "+ "] {
            let mut line = EditorLine::new(format!("{prefix}item"));
            assert!(ListEngine::try_markdown_shortcut(&mut line));
            assert_eq!(line.list_type, ListType::Bullet);
            assert_eq!(line.text, "item");
        }
    }

    #[test]
    fn markdown_brackets_become_a_checkbox() {
        let mut line = EditorLine::new("[] ship it");
        assert!(ListEngine::try_markdown_shortcut(&mut line));
        assert_eq!(line.list_type, ListType::Check);
        assert!(!line.checked);
        assert_eq!(line.text, "ship it");
    }

    #[test]
    fn markdown_digits_become_a_numbered_item() {
        let mut line = EditorLine::new("1. first");
        assert!(ListEngine::try_markdown_shortcut(&mut line));
        assert_eq!(line.list_type, ListType::Number);
        assert_eq!(line.text, "first");
    }

    #[test]
    fn markdown_shortcut_rejects_loose_matches() {
        for text in ["-no space", "1.no space", "1234. too many digits", "plain text", "-"] {
            let mut line = EditorLine::new(text);
            assert!(!ListEngine::try_markdown_shortcut(&mut line), "{text:?}");
            assert_eq!(line.list_type, ListType::None);
        }
    }

    #[test]
    fn markdown_shortcut_does_not_reconvert_list_lines() {
        let mut line = EditorLine {
            text: "- already a bullet".into(),
            list_type: ListType::Bullet,
            ..Default::default()
        };
        assert!(!ListEngine::try_markdown_shortcut(&mut line));
        assert_eq!(line.text, "- already a bullet");
    }

    #[test]
    fn toggle_checked_only_applies_to_check_items() {
        let mut check = EditorLine {
            list_type: ListType::Check,
            ..Default::default()
        };
        assert!(ListEngine::toggle_checked(&mut check));
        assert!(!ListEngine::toggle_checked(&mut check));

        let mut bullet = bullet("x", 0);
        assert!(!ListEngine::toggle_checked(&mut bullet));
    }

    #[test]
    fn bullet_glyph_varies_with_depth() {
        assert_eq!(ListEngine::bullet_glyph(0), "•");
        assert_eq!(ListEngine::bullet_glyph(1), "◦");
        assert_eq!(ListEngine::bullet_glyph(2), "▪");
        assert_eq!(ListEngine::bullet_glyph(5), "‣");
    }

    #[test]
    fn marker_text_matches_the_line_type() {
        assert_eq!(ListEngine::marker_text(&EditorLine::default()), None);
        assert_eq!(ListEngine::marker_text(&bullet("x", 0)).as_deref(), Some("•"));
        let mut n = numbered("x", 0);
        n.number = 3;
        assert_eq!(ListEngine::marker_text(&n).as_deref(), Some("3."));
        let c = EditorLine {
            list_type: ListType::Check,
            checked: true,
            ..Default::default()
        };
        assert_eq!(ListEngine::marker_text(&c).as_deref(), Some("☑"));
    }
}
