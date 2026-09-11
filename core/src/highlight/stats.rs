//! Highlight statistics for the status bar and extract panel.

use crate::highlight::palette::Palette;
use crate::types::api::HighlightStats;
use crate::types::line::{EditorLine, LineColour};

/// Per-colour tally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColourCount {
    pub colour: LineColour,
    pub name: String,
    pub rgba: u32,
    pub count: usize,
}

/// Full breakdown of a document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HighlightBreakdown {
    pub total_lines: usize,
    pub highlighted_lines: usize,
    /// Counts in first-appearance order.
    pub counts: Vec<ColourCount>,
}

impl HighlightBreakdown {
    pub fn is_empty(&self) -> bool {
        self.highlighted_lines == 0
    }

    /// "Yellow 3 · Green 1", or an empty string when nothing is highlighted.
    pub fn summary(&self) -> String {
        self.counts
            .iter()
            .map(|c| format!("{} {}", c.name, c.count))
            .collect::<Vec<_>>()
            .join(" \u{00b7} ")
    }

    pub fn to_api(&self) -> HighlightStats {
        HighlightStats {
            total_lines: self.total_lines as i32,
            highlighted: self.highlighted_lines as i32,
            summary: self.summary(),
        }
    }
}

/// Count highlighted lines per colour, in first-appearance order.
pub fn breakdown(lines: &[EditorLine], palette: &Palette) -> HighlightBreakdown {
    let mut counts: Vec<ColourCount> = Vec::new();
    let mut highlighted = 0usize;

    for line in lines {
        if !line.colour.is_highlighted() {
            continue;
        }
        highlighted += 1;
        if let Some(slot) = counts.iter_mut().find(|c| c.colour == line.colour) {
            slot.count += 1;
        } else {
            let (name, rgba) = palette.resolve(line.colour);
            counts.push(ColourCount {
                colour: line.colour,
                name,
                rgba,
                count: 1,
            });
        }
    }

    HighlightBreakdown {
        total_lines: lines.len(),
        highlighted_lines: highlighted,
        counts,
    }
}

/// Shortcut for the status bar.
pub fn compute(lines: &[EditorLine], palette: &Palette) -> HighlightStats {
    breakdown(lines, palette).to_api()
}

/// Distinct highlight colours present, with their counts. Used to pre-tick
/// the extract panel checkboxes.
pub fn colour_counts(lines: &[EditorLine], palette: &Palette) -> Vec<(LineColour, usize)> {
    breakdown(lines, palette)
        .counts
        .into_iter()
        .map(|c| (c.colour, c.count))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::note::CustomColour;

    fn line(colour: LineColour) -> EditorLine {
        EditorLine {
            text: "x".into(),
            colour,
            ..Default::default()
        }
    }

    #[test]
    fn counts_each_colour_separately() {
        let lines = vec![
            line(LineColour::Yellow),
            line(LineColour::None),
            line(LineColour::Yellow),
            line(LineColour::Green),
        ];
        let b = breakdown(&lines, &Palette::default());
        assert_eq!(b.total_lines, 4);
        assert_eq!(b.highlighted_lines, 3);
        assert_eq!(b.counts.len(), 2);
        assert_eq!(b.counts[0].colour, LineColour::Yellow);
        assert_eq!(b.counts[0].count, 2);
        assert_eq!(b.counts[1].colour, LineColour::Green);
        assert_eq!(b.counts[1].count, 1);
    }

    #[test]
    fn counts_appear_in_first_appearance_order() {
        let lines = vec![
            line(LineColour::Pink),
            line(LineColour::Yellow),
            line(LineColour::Pink),
        ];
        let b = breakdown(&lines, &Palette::default());
        assert_eq!(b.counts[0].colour, LineColour::Pink);
        assert_eq!(b.counts[1].colour, LineColour::Yellow);
    }

    #[test]
    fn an_unhighlighted_document_is_empty() {
        let b = breakdown(&[line(LineColour::None)], &Palette::default());
        assert!(b.is_empty());
        assert_eq!(b.total_lines, 1);
        assert_eq!(b.summary(), "");
    }

    #[test]
    fn summary_is_human_readable() {
        let lines = vec![
            line(LineColour::Yellow),
            line(LineColour::Yellow),
            line(LineColour::Green),
        ];
        let b = breakdown(&lines, &Palette::default());
        assert_eq!(b.summary(), "Yellow 2 \u{00b7} Green 1");
    }

    #[test]
    fn api_struct_carries_the_same_numbers() {
        let lines = vec![line(LineColour::Yellow), line(LineColour::None)];
        let api = compute(&lines, &Palette::default());
        assert_eq!(api.total_lines, 2);
        assert_eq!(api.highlighted, 1);
        assert_eq!(api.summary, "Yellow 1");
    }

    #[test]
    fn custom_colours_are_named_by_hex() {
        let palette = Palette::new(vec![CustomColour::new("Sunset", "#ff8800")]);
        let b = breakdown(&[line(LineColour::Custom(0xff88_00ff))], &palette);
        assert_eq!(b.counts[0].name, "#ff8800");
        assert_eq!(b.counts[0].rgba, 0xff88_00ff);
    }

    #[test]
    fn colour_counts_pairs_colours_with_totals() {
        let lines = vec![line(LineColour::Blue), line(LineColour::Blue)];
        let counts = colour_counts(&lines, &Palette::default());
        assert_eq!(counts, vec![(LineColour::Blue, 2)]);
    }

    #[test]
    fn an_empty_document_reports_zero_lines() {
        let b = breakdown(&[], &Palette::default());
        assert_eq!(b.total_lines, 0);
        assert_eq!(b.highlighted_lines, 0);
        assert!(b.is_empty());
    }
}
