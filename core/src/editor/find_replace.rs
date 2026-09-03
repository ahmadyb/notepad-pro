//! Find & replace.
//!
//! Offsets are byte offsets into `EditorLine::text`, but they are always
//! produced from a `char_indices` walk, so they land on character boundaries
//! even when case folding changes byte lengths (e.g. `İ` → `i̇`).

use crate::types::line::EditorLine;

/// One occurrence of the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    /// Byte offset of the first matched byte.
    pub start: usize,
    /// Byte offset just past the last matched byte.
    pub end: usize,
}

impl Match {
    /// Extract the matched substring, guarding against stale offsets.
    pub fn slice<'a>(&self, text: &'a str) -> Option<&'a str> {
        if self.start <= self.end && text.is_char_boundary(self.start) && text.is_char_boundary(self.end)
        {
            Some(&text[self.start..self.end])
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct FindEngine {
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub matches: Vec<Match>,
    /// Index into `matches` of the current match.
    pub current: usize,
}

impl Default for FindEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FindEngine {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            replacement: String::new(),
            case_sensitive: false,
            whole_word: false,
            matches: Vec::new(),
            current: 0,
        }
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
    }

    pub fn set_replacement(&mut self, replacement: impl Into<String>) {
        self.replacement = replacement.into();
    }

    /// Recompute all matches for the document. Resets the cursor to 0.
    pub fn search(&mut self, lines: &[EditorLine]) {
        self.matches.clear();
        self.current = 0;
        if self.query.is_empty() {
            return;
        }
        for (line_index, line) in lines.iter().enumerate() {
            for (start, end) in
                find_all(&line.text, &self.query, self.case_sensitive, self.whole_word)
            {
                self.matches.push(Match {
                    line: line_index,
                    start,
                    end,
                });
            }
        }
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    pub fn has_matches(&self) -> bool {
        !self.matches.is_empty()
    }

    pub fn current_match(&self) -> Option<&Match> {
        self.matches.get(self.current)
    }

    /// Advance to the next match, wrapping around. Returns the new position.
    pub fn next(&mut self) -> Option<Match> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + 1) % self.matches.len();
        Some(self.matches[self.current])
    }

    /// Step back one match, wrapping around.
    pub fn prev(&mut self) -> Option<Match> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = if self.current == 0 {
            self.matches.len() - 1
        } else {
            self.current - 1
        };
        Some(self.matches[self.current])
    }

    /// 1-based position for the "3/17" indicator, or 0 when there are none.
    pub fn position(&self) -> usize {
        if self.matches.is_empty() {
            0
        } else {
            self.current + 1
        }
    }

    /// Move the cursor to the first match at or after `line`.
    pub fn focus_line(&mut self, line: usize) -> Option<Match> {
        if self.matches.is_empty() {
            return None;
        }
        let idx = self
            .matches
            .iter()
            .position(|m| m.line >= line)
            .unwrap_or(0);
        self.current = idx;
        Some(self.matches[idx])
    }

    /// Replace the current match. Returns `true` when a replacement happened.
    pub fn replace_current(&mut self, lines: &mut Vec<EditorLine>) -> bool {
        let Some(m) = self.current_match().copied() else {
            return false;
        };
        if m.line >= lines.len() {
            return false;
        }
        let replacement = self.replacement.clone();
        let applied = replace_once(&mut lines[m.line].text, m.start, m.end, &replacement);
        if !applied {
            return false;
        }
        // Offsets after this point on the same line, and every later line's
        // match, are now stale — recompute and stay on the same slot.
        let keep = self.current;
        self.search(lines);
        if !self.matches.is_empty() {
            self.current = keep.min(self.matches.len() - 1);
        }
        true
    }

    /// Replace every match. Returns how many replacements were made.
    pub fn replace_all(&mut self, lines: &mut Vec<EditorLine>) -> usize {
        if self.matches.is_empty() || self.query.is_empty() {
            return 0;
        }
        let replacement = self.replacement.clone();
        let mut count = 0usize;
        // Reverse order keeps earlier offsets valid within each line.
        for m in self.matches.iter().rev() {
            if m.line < lines.len()
                && replace_once(&mut lines[m.line].text, m.start, m.end, &replacement)
            {
                count += 1;
            }
        }
        self.search(lines);
        count
    }

    pub fn clear(&mut self) {
        self.matches.clear();
        self.current = 0;
        self.query.clear();
    }
}

fn replace_once(text: &mut String, start: usize, end: usize, replacement: &str) -> bool {
    if start > end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return false;
    }
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..start]);
    out.push_str(replacement);
    out.push_str(&text[end..]);
    *text = out;
    true
}

/// All non-overlapping occurrences of `needle` in `haystack`.
///
/// Returns byte offsets. Case-insensitive matching compares folded characters
/// one at a time, so offsets always refer to the *original* string.
pub fn find_all(
    haystack: &str,
    needle: &str,
    case_sensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }

    if case_sensitive {
        let mut from = 0usize;
        while let Some(rel) = haystack[from..].find(needle) {
            let start = from + rel;
            let end = start + needle.len();
            if whole_word && !is_word_boundary(haystack, start, end) {
                from = advance_by_one_char(haystack, start);
                continue;
            }
            out.push((start, end));
            from = end.max(advance_by_one_char(haystack, start));
        }
        return out;
    }

    // Folded view: (byte offset of the original char, folded char).
    let hay: Vec<(usize, char)> = haystack
        .char_indices()
        .map(|(i, c)| (i, fold(c)))
        .collect();
    let ndl: Vec<char> = needle.chars().map(fold).collect();
    if ndl.is_empty() || hay.len() < ndl.len() {
        return out;
    }

    let mut i = 0usize;
    while i + ndl.len() <= hay.len() {
        let hit = hay[i..i + ndl.len()]
            .iter()
            .zip(ndl.iter())
            .all(|((_, hc), nc)| hc == nc);
        if hit {
            let start = hay[i].0;
            let last_char_index = i + ndl.len() - 1;
            let end = end_of_char(haystack, hay[last_char_index].0);
            if whole_word && !is_word_boundary(haystack, start, end) {
                i += 1;
                continue;
            }
            out.push((start, end));
            i += ndl.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Simple per-character case fold. `to_lowercase` can emit multiple chars
/// (e.g. `İ`); the first one is enough for matching purposes.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn end_of_char(text: &str, index: usize) -> usize {
    text[index..]
        .chars()
        .next()
        .map(|c| index + c.len_utf8())
        .unwrap_or(index)
}

fn advance_by_one_char(text: &str, index: usize) -> usize {
    end_of_char(text, index)
}

/// Neither neighbour may be alphanumeric or `_`.
pub fn is_word_boundary(text: &str, start: usize, end: usize) -> bool {
    let before_ok = text[..start]
        .chars()
        .next_back()
        .map(|c| !is_word_char(c))
        .unwrap_or(true);
    let after_ok = text[end..]
        .chars()
        .next()
        .map(|c| !is_word_char(c))
        .unwrap_or(true);
    before_ok && after_ok
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::line::EditorLine;

    fn engine(query: &str) -> FindEngine {
        let mut e = FindEngine::new();
        e.set_query(query);
        e
    }

    fn lines_of(texts: &[&str]) -> Vec<EditorLine> {
        texts.iter().map(|t| EditorLine::new(*t)).collect()
    }

    #[test]
    fn empty_query_produces_no_matches() {
        let mut e = engine("");
        e.search(&lines_of(&["anything"]));
        assert_eq!(e.match_count(), 0);
        assert!(!e.has_matches());
        assert_eq!(e.position(), 0);
    }

    #[test]
    fn finds_every_occurrence_across_lines() {
        let mut e = engine("cat");
        e.search(&lines_of(&["cat and cat", "no match", "a cat"]));
        assert_eq!(e.match_count(), 3);
        assert_eq!(e.matches[0].line, 0);
        assert_eq!(e.matches[1].line, 0);
        assert_eq!(e.matches[2].line, 2);
    }

    #[test]
    fn search_is_case_insensitive_by_default() {
        let mut e = engine("cat");
        e.search(&lines_of(&["Cat cAt CAT"]));
        assert_eq!(e.match_count(), 3);
    }

    #[test]
    fn case_sensitive_mode_respects_case() {
        let mut e = engine("cat");
        e.case_sensitive = true;
        e.search(&lines_of(&["Cat cat CAT"]));
        assert_eq!(e.match_count(), 1);
    }

    #[test]
    fn whole_word_mode_skips_substrings() {
        let mut e = engine("cat");
        e.whole_word = true;
        e.search(&lines_of(&["cat catalogue a cat."]));
        assert_eq!(e.match_count(), 2);
    }

    #[test]
    fn whole_word_treats_underscore_as_part_of_the_word() {
        let mut e = engine("cat");
        e.whole_word = true;
        e.search(&lines_of(&["cat _cat cat_"]));
        assert_eq!(e.match_count(), 1);
    }

    #[test]
    fn overlapping_candidates_do_not_double_count() {
        let mut e = engine("aa");
        e.search(&lines_of(&["aaaa"]));
        assert_eq!(e.match_count(), 2);
    }

    #[test]
    fn multibyte_offsets_land_on_char_boundaries() {
        let mut e = engine("café");
        e.search(&lines_of(&["le café est bon"]));
        assert_eq!(e.match_count(), 1);
        let m = e.current_match().unwrap();
        assert_eq!(m.slice("le café est bon"), Some("café"));
    }

    #[test]
    fn case_insensitive_match_on_a_multibyte_char_is_safe() {
        let mut e = engine("É");
        e.search(&lines_of(&["café"]));
        assert_eq!(e.match_count(), 1);
        let m = e.current_match().unwrap();
        assert!(m.slice("café").is_some());
    }

    #[test]
    fn next_wraps_around() {
        let mut e = engine("a");
        e.search(&lines_of(&["aaa"]));
        assert_eq!(e.position(), 1);
        assert_eq!(e.next().unwrap().start, 1);
        assert_eq!(e.position(), 2);
        e.next();
        assert_eq!(e.position(), 3);
        e.next();
        assert_eq!(e.position(), 1, "wrapped to the first match");
    }

    #[test]
    fn prev_wraps_backwards() {
        let mut e = engine("a");
        e.search(&lines_of(&["aaa"]));
        let m = e.prev().unwrap();
        assert_eq!(m.start, 2);
        assert_eq!(e.position(), 3);
    }

    #[test]
    fn next_and_prev_on_no_matches_return_none() {
        let mut e = engine("zzz");
        e.search(&lines_of(&["abc"]));
        assert!(e.next().is_none());
        assert!(e.prev().is_none());
        assert!(e.current_match().is_none());
    }

    #[test]
    fn focus_line_jumps_to_the_first_later_match() {
        let mut e = engine("x");
        e.search(&lines_of(&["x", "x", "x"]));
        let m = e.focus_line(2).unwrap();
        assert_eq!(m.line, 2);
        // Asking for a line past the end falls back to the first match.
        let m = e.focus_line(99).unwrap();
        assert_eq!(m.line, 0);
    }

    #[test]
    fn replace_current_swaps_one_match() {
        let mut e = engine("cat");
        e.set_replacement("dog");
        let mut lines = lines_of(&["the cat sat"]);
        e.search(&lines);
        assert!(e.replace_current(&mut lines));
        assert_eq!(lines[0].text, "the dog sat");
    }

    #[test]
    fn replace_current_keeps_the_cursor_in_range() {
        let mut e = engine("a");
        e.set_replacement("b");
        let mut lines = lines_of(&["aaa"]);
        e.search(&lines);
        e.next();
        e.next();
        assert!(e.replace_current(&mut lines));
        assert!(e.current < e.match_count());
    }

    #[test]
    fn replace_current_on_no_matches_is_false() {
        let mut e = engine("zzz");
        e.set_replacement("b");
        let mut lines = lines_of(&["abc"]);
        e.search(&lines);
        assert!(!e.replace_current(&mut lines));
    }

    #[test]
    fn replace_all_counts_every_substitution() {
        let mut e = engine("cat");
        e.set_replacement("dog");
        let mut lines = lines_of(&["cat cat", "cat"]);
        e.search(&lines);
        assert_eq!(e.replace_all(&mut lines), 3);
        assert_eq!(lines[0].text, "dog dog");
        assert_eq!(lines[1].text, "dog");
        assert_eq!(e.match_count(), 0);
    }

    #[test]
    fn replace_all_handles_multibyte_text() {
        let mut e = engine("café");
        e.set_replacement("tea");
        let mut lines = lines_of(&["café café"]);
        e.search(&lines);
        assert_eq!(e.replace_all(&mut lines), 2);
        assert_eq!(lines[0].text, "tea tea");
    }

    #[test]
    fn replace_all_with_an_empty_needle_is_a_noop() {
        let mut e = engine("");
        e.set_replacement("x");
        let mut lines = lines_of(&["abc"]);
        e.search(&lines);
        assert_eq!(e.replace_all(&mut lines), 0);
        assert_eq!(lines[0].text, "abc");
    }

    #[test]
    fn replace_with_empty_string_deletes_the_match() {
        let mut e = engine("na");
        e.set_replacement("");
        let mut lines = lines_of(&["banana"]);
        e.search(&lines);
        assert_eq!(e.replace_all(&mut lines), 2);
        assert_eq!(lines[0].text, "ba");
    }

    #[test]
    fn replacing_with_a_longer_string_keeps_later_offsets_valid() {
        let mut e = engine("a");
        e.set_replacement("xxxx");
        let mut lines = lines_of(&["a a a"]);
        e.search(&lines);
        assert_eq!(e.replace_all(&mut lines), 3);
        assert_eq!(lines[0].text, "xxxx xxxx xxxx");
    }

    #[test]
    fn clear_resets_the_engine() {
        let mut e = engine("cat");
        e.search(&lines_of(&["cat"]));
        e.clear();
        assert_eq!(e.match_count(), 0);
        assert_eq!(e.query, "");
    }

    #[test]
    fn stale_offsets_are_rejected_instead_of_panicking() {
        assert!(!replace_once(&mut "café".to_string(), 4, 5, "x"));
        assert!(!replace_once(&mut "abc".to_string(), 5, 2, "x"));
        let m = Match {
            line: 0,
            start: 3,
            end: 4,
        };
        assert!(m.slice("café").is_none());
    }

    #[test]
    fn find_all_returns_empty_for_an_empty_needle() {
        assert!(find_all("abc", "", false, false).is_empty());
    }
}
