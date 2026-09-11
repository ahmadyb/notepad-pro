//! Line ending detection and normalisation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEnding {
    /// `\n` — Unix, macOS (modern), Linux.
    Lf,
    /// `\r\n` — Windows.
    Crlf,
    /// `\r` — classic Mac OS.
    Cr,
}

impl Default for LineEnding {
    fn default() -> Self {
        LineEnding::Lf
    }
}

impl LineEnding {
    /// Infer the dominant line ending. Ties resolve towards the platform
    /// convention: CRLF first, then LF, then CR.
    pub fn detect(text: &str) -> Self {
        let crlf = text.matches("\r\n").count();
        let cr_total = text.matches('\r').count();
        let lf_total = text.matches('\n').count();
        let cr = cr_total - crlf;
        let lf = lf_total - crlf;

        if crlf >= lf && crlf >= cr && crlf > 0 {
            LineEnding::Crlf
        } else if cr > lf && cr > 0 {
            LineEnding::Cr
        } else {
            LineEnding::Lf
        }
    }

    /// Rewrite every line ending in `text` to this style.
    pub fn apply(self, text: &str) -> String {
        let lf = text.replace("\r\n", "\n").replace('\r', "\n");
        match self {
            LineEnding::Lf => lf,
            LineEnding::Crlf => lf.replace('\n', "\r\n"),
            LineEnding::Cr => lf.replace('\n', "\r"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
            LineEnding::Cr => "CR",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label.to_ascii_uppercase().as_str() {
            "CRLF" => LineEnding::Crlf,
            "CR" => LineEnding::Cr,
            _ => LineEnding::Lf,
        }
    }

    /// The literal sequence written to disk.
    pub fn sequence(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_style() {
        assert_eq!(LineEnding::detect("a\r\nb\r\nc"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("a\nb\nc"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\rb\rc"), LineEnding::Cr);
    }

    #[test]
    fn single_line_text_defaults_to_lf() {
        assert_eq!(LineEnding::detect("no breaks here"), LineEnding::Lf);
        assert_eq!(LineEnding::detect(""), LineEnding::Lf);
    }

    #[test]
    fn mixed_endings_pick_the_majority() {
        assert_eq!(LineEnding::detect("a\r\nb\r\nc\nd"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("a\nb\nc\r\nd"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\rb\rc\nd"), LineEnding::Cr);
    }

    #[test]
    fn crlf_is_not_double_counted_as_cr_and_lf() {
        // "a\r\nb" contains one CR and one LF; both belong to the same CRLF.
        assert_eq!(LineEnding::detect("a\r\nb"), LineEnding::Crlf);
    }

    #[test]
    fn apply_normalises_to_the_target_style() {
        let mixed = "a\r\nb\nc\rd";
        assert_eq!(LineEnding::Lf.apply(mixed), "a\nb\nc\nd");
        assert_eq!(LineEnding::Crlf.apply(mixed), "a\r\nb\r\nc\r\nd");
        assert_eq!(LineEnding::Cr.apply(mixed), "a\rb\rc\rd");
    }

    #[test]
    fn apply_is_idempotent() {
        let text = "a\r\nb\r\nc";
        assert_eq!(LineEnding::Crlf.apply(text), text);
        assert_eq!(
            LineEnding::Crlf.apply(&LineEnding::Crlf.apply(text)),
            text
        );
    }

    #[test]
    fn labels_roundtrip() {
        for ending in [LineEnding::Lf, LineEnding::Crlf, LineEnding::Cr] {
            assert_eq!(LineEnding::from_label(ending.label()), ending);
        }
        assert_eq!(LineEnding::from_label("crlf"), LineEnding::Crlf);
        assert_eq!(LineEnding::from_label("nonsense"), LineEnding::Lf);
    }

    #[test]
    fn sequences_match_the_labels() {
        assert_eq!(LineEnding::Lf.sequence(), "\n");
        assert_eq!(LineEnding::Crlf.sequence(), "\r\n");
        assert_eq!(LineEnding::Cr.sequence(), "\r");
    }
}
