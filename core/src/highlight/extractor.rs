//! Feature 3 — extract lines by highlight colour.

use serde::{Deserialize, Serialize};

use crate::types::line::{EditorLine, LineColour};

/// Ordering of the extracted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtractionOrder {
    /// Keep the original document order.
    Document,
    /// Group under a `# Colour` heading per colour.
    GroupByColour,
}

impl Default for ExtractionOrder {
    fn default() -> Self {
        ExtractionOrder::Document
    }
}

impl ExtractionOrder {
    pub fn key(self) -> &'static str {
        match self {
            ExtractionOrder::Document => "document",
            ExtractionOrder::GroupByColour => "grouped",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "grouped" | "group" | "colour" | "color" => ExtractionOrder::GroupByColour,
            _ => ExtractionOrder::Document,
        }
    }

    pub fn is_grouped(self) -> bool {
        matches!(self, ExtractionOrder::GroupByColour)
    }
}

/// Result of an extraction.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtractionResult {
    pub text: String,
    pub line_count: usize,
    pub char_count: usize,
    /// How many lines each colour contributed, in the order the colours were
    /// requested.
    pub per_colour: Vec<(LineColour, usize)>,
}

/// Extract every line whose colour is in `colours`.
///
/// Colours are matched in the order given; lines are emitted in document
/// order unless `order` is [`ExtractionOrder::GroupByColour`]. This is the
/// fix for the original bug where extraction came back ordered by frequency.
pub fn extract(
    lines: &[EditorLine],
    colours: &[LineColour],
    order: ExtractionOrder,
) -> ExtractionResult {
    let selected: Vec<LineColour> = colours
        .iter()
        .filter(|c| c.is_highlighted())
        .copied()
        .collect();

    let mut per_colour: Vec<(LineColour, usize)> =
        selected.iter().map(|c| (*c, 0usize)).collect();
    for line in lines {
        if let Some(slot) = per_colour.iter_mut().find(|(c, _)| *c == line.colour) {
            slot.1 += 1;
        }
    }

    let text = if selected.is_empty() {
        String::new()
    } else {
        match order {
            ExtractionOrder::Document => document_order(lines, &selected),
            ExtractionOrder::GroupByColour => grouped(lines, &selected),
        }
    };

    let line_count = if text.is_empty() {
        0
    } else {
        text.lines().count()
    };
    let char_count = text.chars().count();

    ExtractionResult {
        text,
        line_count,
        char_count,
        per_colour,
    }
}

/// Plain concatenation preserving document order.
pub fn document_order(lines: &[EditorLine], colours: &[LineColour]) -> String {
    lines
        .iter()
        .filter(|l| colours.contains(&l.colour))
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// One `# Colour` section per requested colour, in the requested order.
/// Empty sections are skipped.
pub fn grouped(lines: &[EditorLine], colours: &[LineColour]) -> String {
    let mut out = String::new();
    for colour in colours {
        let members: Vec<&EditorLine> = lines
            .iter()
            .filter(|l| l.colour == *colour)
            .collect();
        if members.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("# {}\n", colour.display_name()));
        for line in members {
            out.push_str(&line.text);
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

/// Convenience: extract into a plain `String`.
pub fn extract_text(lines: &[EditorLine], colours: &[LineColour], grouped: bool) -> String {
    let order = if grouped {
        ExtractionOrder::GroupByColour
    } else {
        ExtractionOrder::Document
    };
    extract(lines, colours, order).text
}

/// Colours actually present in the document, in first-appearance order.
pub fn colours_present(lines: &[EditorLine]) -> Vec<LineColour> {
    let mut seen: Vec<LineColour> = Vec::new();
    for line in lines {
        if line.colour.is_highlighted() && !seen.contains(&line.colour) {
            seen.push(line.colour);
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, colour: LineColour) -> EditorLine {
        EditorLine {
            text: text.into(),
            colour,
            ..Default::default()
        }
    }

    fn sample() -> Vec<EditorLine> {
        vec![
            line("Yellow line 1", LineColour::Yellow),
            line("plain line", LineColour::None),
            line("Pink line 3", LineColour::Pink),
            line("Yellow line 4", LineColour::Yellow),
        ]
    }

    #[test]
    fn document_order_preserves_the_original_sequence() {
        let result = extract(
            &sample(),
            &[LineColour::Pink, LineColour::Yellow],
            ExtractionOrder::Document,
        );
        assert_eq!(
            result.text,
            "Yellow line 1\nPink line 3\nYellow line 4"
        );
    }

    #[test]
    fn document_order_is_not_ordered_by_frequency() {
        // Regression guard: yellow appears twice, pink once. Asking for both
        // must still interleave them as they appear.
        let result = extract(
            &sample(),
            &[LineColour::Pink, LineColour::Yellow],
            ExtractionOrder::Document,
        );
        assert!(result.text.starts_with("Yellow line 1"));
        assert!(result.text.contains("Pink line 3"));
    }

    #[test]
    fn unhighlighted_lines_are_never_extracted() {
        let result = extract(
            &sample(),
            &[LineColour::None],
            ExtractionOrder::Document,
        );
        assert_eq!(result.text, "");
        assert_eq!(result.line_count, 0);
    }

    #[test]
    fn counts_are_reported() {
        let result = extract(
            &sample(),
            &[LineColour::Yellow],
            ExtractionOrder::Document,
        );
        assert_eq!(result.line_count, 2);
        assert_eq!(result.char_count, "Yellow line 1\nYellow line 4".chars().count());
        assert_eq!(result.per_colour, vec![(LineColour::Yellow, 2)]);
    }

    #[test]
    fn grouped_output_has_a_heading_per_colour() {
        let result = extract(
            &sample(),
            &[LineColour::Yellow, LineColour::Pink],
            ExtractionOrder::GroupByColour,
        );
        assert_eq!(
            result.text,
            "# Yellow\nYellow line 1\nYellow line 4\n\n# Pink\nPink line 3"
        );
    }

    #[test]
    fn grouped_output_follows_the_requested_colour_order() {
        let result = extract(
            &sample(),
            &[LineColour::Pink, LineColour::Yellow],
            ExtractionOrder::GroupByColour,
        );
        assert!(result.text.starts_with("# Pink"));
    }

    #[test]
    fn grouped_output_skips_empty_sections() {
        let result = extract(
            &sample(),
            &[LineColour::Green, LineColour::Yellow],
            ExtractionOrder::GroupByColour,
        );
        assert!(!result.text.contains("# Green"));
        assert!(result.text.contains("# Yellow"));
    }

    #[test]
    fn empty_colour_list_yields_empty_text() {
        let result = extract(&sample(), &[], ExtractionOrder::Document);
        assert_eq!(result.text, "");
        assert_eq!(result.line_count, 0);
        assert_eq!(result.char_count, 0);
    }

    #[test]
    fn extracting_from_an_empty_document_is_safe() {
        let result = extract(&[], &[LineColour::Yellow], ExtractionOrder::Document);
        assert_eq!(result.text, "");
        assert_eq!(result.per_colour, vec![(LineColour::Yellow, 0)]);
    }

    #[test]
    fn custom_colours_can_be_extracted() {
        let lines = vec![
            line("custom", LineColour::Custom(0xff88_00ff)),
            line("yellow", LineColour::Yellow),
        ];
        let result = extract(
            &lines,
            &[LineColour::Custom(0xff88_00ff)],
            ExtractionOrder::Document,
        );
        assert_eq!(result.text, "custom");
    }

    #[test]
    fn blank_highlighted_lines_are_kept() {
        let lines = vec![line("", LineColour::Yellow), line("x", LineColour::Yellow)];
        let result = extract(&lines, &[LineColour::Yellow], ExtractionOrder::Document);
        assert_eq!(result.text, "\nx");
        assert_eq!(result.line_count, 2);
    }

    #[test]
    fn duplicate_colours_in_the_request_do_not_duplicate_output() {
        let result = extract(
            &sample(),
            &[LineColour::Yellow, LineColour::Yellow],
            ExtractionOrder::Document,
        );
        assert_eq!(result.text.matches("Yellow line 1").count(), 1);
    }

    #[test]
    fn extract_text_helper_matches_the_struct_api() {
        let plain = extract_text(&sample(), &[LineColour::Yellow], false);
        let via_struct = extract(
            &sample(),
            &[LineColour::Yellow],
            ExtractionOrder::Document,
        );
        assert_eq!(plain, via_struct.text);
    }

    #[test]
    fn colours_present_is_in_first_appearance_order() {
        assert_eq!(
            colours_present(&sample()),
            vec![LineColour::Yellow, LineColour::Pink]
        );
    }

    #[test]
    fn colours_present_ignores_none() {
        let lines = vec![line("a", LineColour::None)];
        assert!(colours_present(&lines).is_empty());
    }

    #[test]
    fn extraction_order_keys_roundtrip() {
        assert_eq!(
            ExtractionOrder::from_key(ExtractionOrder::Document.key()),
            ExtractionOrder::Document
        );
        assert_eq!(
            ExtractionOrder::from_key(ExtractionOrder::GroupByColour.key()),
            ExtractionOrder::GroupByColour
        );
        assert_eq!(ExtractionOrder::from_key("bogus"), ExtractionOrder::Document);
        assert!(ExtractionOrder::GroupByColour.is_grouped());
        assert!(!ExtractionOrder::Document.is_grouped());
    }

    #[test]
    fn unicode_is_counted_in_chars_not_bytes() {
        let lines = vec![line("café", LineColour::Yellow)];
        let result = extract(&lines, &[LineColour::Yellow], ExtractionOrder::Document);
        assert_eq!(result.char_count, 4);
    }
}
