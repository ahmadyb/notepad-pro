//! Character-set detection.
//!
//! Order of precedence: UTF-8 BOM, UTF-16 BOM, strict UTF-8 validation, then
//! a statistical guess via `chardet`. The returned label is always something
//! `encoding_rs` understands, so the caller never has to guess twice.

/// Result of sniffing a byte buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodingInfo {
    /// `encoding_rs` label, e.g. `utf-8`, `utf-16le`, `windows-1252`.
    pub label: String,
    /// `true` when a byte-order mark was found and must be skipped.
    pub has_bom: bool,
    /// Length of the BOM in bytes.
    pub bom_len: usize,
    /// Detection confidence, 0.0..=1.0. `1.0` for BOM/UTF-8 hits.
    pub confidence: f32,
}

impl EncodingInfo {
    pub fn is_utf8(&self) -> bool {
        self.label == "utf-8"
    }
}

/// Guess the encoding of `raw`.
pub fn detect_encoding(raw: &[u8]) -> EncodingInfo {
    if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return EncodingInfo {
            label: "utf-8".into(),
            has_bom: true,
            bom_len: 3,
            confidence: 1.0,
        };
    }
    if raw.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return EncodingInfo {
            label: "utf-32le".into(),
            has_bom: true,
            bom_len: 4,
            confidence: 1.0,
        };
    }
    if raw.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return EncodingInfo {
            label: "utf-32be".into(),
            has_bom: true,
            bom_len: 4,
            confidence: 1.0,
        };
    }
    if raw.starts_with(&[0xFF, 0xFE]) {
        return EncodingInfo {
            label: "utf-16le".into(),
            has_bom: true,
            bom_len: 2,
            confidence: 1.0,
        };
    }
    if raw.starts_with(&[0xFE, 0xFF]) {
        return EncodingInfo {
            label: "utf-16be".into(),
            has_bom: true,
            bom_len: 2,
            confidence: 1.0,
        };
    }
    if raw.is_empty() {
        return EncodingInfo {
            label: "utf-8".into(),
            has_bom: false,
            bom_len: 0,
            confidence: 1.0,
        };
    }
    if std::str::from_utf8(raw).is_ok() {
        return EncodingInfo {
            label: "utf-8".into(),
            has_bom: false,
            bom_len: 0,
            confidence: 1.0,
        };
    }

    let detected = chardet::detect(raw);
    let mapped = chardet::charset2encoding(&detected.0);
    let label = if mapped.is_empty() {
        "windows-1252".to_string()
    } else {
        mapped.to_string()
    };
    EncodingInfo {
        label,
        has_bom: false,
        bom_len: 0,
        confidence: detected.1 as f32,
    }
}

/// Resolve an `encoding_rs` label, falling back to UTF-8 for unknown names.
pub fn encoding_for_label(label: &str) -> &'static encoding_rs::Encoding {
    encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8)
}

/// Decode bytes to a `String`, skipping any BOM.
pub fn decode(raw: &[u8], info: &EncodingInfo) -> String {
    let encoding = encoding_for_label(&info.label);
    let offset = info.bom_len.min(raw.len());
    let (text, _, had_errors) = encoding.decode(&raw[offset..]);
    if had_errors {
        tracing::warn!(
            encoding = %info.label,
            "file contained bytes that are invalid in this encoding; replaced with U+FFFD"
        );
    }
    text.into_owned()
}

/// Encode a `String`.
///
/// `encoding_rs` always emits a BOM from its UTF-16/UTF-32 encoders, so a
/// manual BOM is only prepended for UTF-8, and only when `with_bom` is set.
pub fn encode(text: &str, label: &str, with_bom: bool) -> Vec<u8> {
    let encoding = encoding_for_label(label);
    let (bytes, _, _) = encoding.encode(text);
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + 4);
    if with_bom && encoding == encoding_rs::UTF_8 {
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    out.extend_from_slice(bytes.as_ref());
    out
}

/// `true` when `encoding_rs` emits a BOM automatically for this label.
pub fn encoder_emits_bom(label: &str) -> bool {
    // `encoding_rs` prepends a BOM when encoding UTF-16/UTF-32 (and never
    // for UTF-8 or single-byte encodings).
    matches!(
        encoding_for_label(label).name(),
        "UTF-16LE" | "UTF-16BE" | "UTF-32LE" | "UTF-32BE"
    )
}

/// The list offered in the Save / Reopen-with dialogs.
pub fn offered_encodings() -> Vec<&'static str> {
    vec![
        "utf-8",
        "utf-16le",
        "utf-16be",
        "windows-1252",
        "iso-8859-1",
        "shift_jis",
        "gbk",
        "big5",
        "euc-kr",
        "koi8-r",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_utf8_bom() {
        let info = detect_encoding(b"\xEF\xBB\xBFhello");
        assert_eq!(info.label, "utf-8");
        assert!(info.has_bom);
        assert_eq!(info.bom_len, 3);
        assert!(info.is_utf8());
    }

    #[test]
    fn detects_utf16_boms_in_both_orders() {
        let le = detect_encoding(&[0xFF, 0xFE, b'a', 0x00]);
        assert_eq!(le.label, "utf-16le");
        assert_eq!(le.bom_len, 2);
        let be = detect_encoding(&[0xFE, 0xFF, 0x00, b'a']);
        assert_eq!(be.label, "utf-16be");
    }

    #[test]
    fn utf32_bom_wins_over_the_utf16_prefix() {
        // FF FE 00 00 starts with the UTF-16LE BOM but is really UTF-32LE.
        let info = detect_encoding(&[0xFF, 0xFE, 0x00, 0x00, b'a', 0, 0, 0]);
        assert_eq!(info.label, "utf-32le");
        assert_eq!(info.bom_len, 4);
    }

    #[test]
    fn plain_ascii_is_utf8_without_a_bom() {
        let info = detect_encoding(b"hello world");
        assert_eq!(info.label, "utf-8");
        assert!(!info.has_bom);
        assert_eq!(info.bom_len, 0);
        assert_eq!(info.confidence, 1.0);
    }

    #[test]
    fn multibyte_utf8_is_recognised() {
        let info = detect_encoding("héllo wörld".as_bytes());
        assert_eq!(info.label, "utf-8");
    }

    #[test]
    fn empty_input_is_utf8() {
        let info = detect_encoding(b"");
        assert_eq!(info.label, "utf-8");
        assert!(!info.has_bom);
    }

    #[test]
    fn latin1_bytes_fall_back_to_a_single_byte_encoding() {
        // 0xE9 alone is invalid UTF-8; chardet must propose something usable.
        let raw = b"Caf\xe9 au lait, tr\xe8s bien";
        let info = detect_encoding(raw);
        assert_ne!(info.label, "utf-8");
        assert!(encoding_rs::Encoding::for_label(info.label.as_bytes()).is_some());
    }

    #[test]
    fn decode_skips_the_bom() {
        let raw = b"\xEF\xBB\xBFhello";
        let info = detect_encoding(raw);
        assert_eq!(decode(raw, &info), "hello");
    }

    #[test]
    fn decode_handles_utf16le() {
        let text = "hi";
        let (bytes, _, _) = encoding_rs::UTF_16LE.encode(text);
        let raw = [&[0xFF, 0xFE][..], bytes.as_ref()].concat();
        let info = detect_encoding(&raw);
        assert_eq!(decode(&raw, &info), "hi");
    }

    #[test]
    fn decode_replaces_invalid_bytes_instead_of_failing() {
        let info = EncodingInfo {
            label: "utf-8".into(),
            has_bom: false,
            bom_len: 0,
            confidence: 1.0,
        };
        let out = decode(&[0xFF, 0xFE, 0xFD], &info);
        assert!(out.contains('\u{FFFD}'));
    }

    #[test]
    fn encode_roundtrips_utf8() {
        let bytes = encode("hello", "utf-8", false);
        assert_eq!(bytes, b"hello");
        assert_eq!(decode(&bytes, &detect_encoding(&bytes)), "hello");
    }

    #[test]
    fn encode_can_prepend_a_utf8_bom() {
        let bytes = encode("hello", "utf-8", true);
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        assert_eq!(decode(&bytes, &detect_encoding(&bytes)), "hello");
    }

    #[test]
    fn encode_utf16_roundtrips() {
        let bytes = encode("héllo", "utf-16le", false);
        let info = detect_encoding(&bytes);
        assert_eq!(decode(&bytes, &info), "héllo");
    }

    #[test]
    fn unknown_labels_fall_back_to_utf8() {
        assert_eq!(encoding_for_label("definitely-not-real"), encoding_rs::UTF_8);
    }

    #[test]
    fn every_offered_encoding_resolves() {
        for label in offered_encodings() {
            assert!(
                encoding_rs::Encoding::for_label(label.as_bytes()).is_some(),
                "{label} did not resolve"
            );
        }
    }

    #[test]
    fn utf16_encoders_emit_their_own_bom() {
        assert!(encoder_emits_bom("utf-16le"));
        assert!(encoder_emits_bom("utf-16be"));
        assert!(!encoder_emits_bom("utf-8"));
        assert!(!encoder_emits_bom("windows-1252"));
    }

    #[test]
    fn utf16_is_never_double_prefixed_with_a_bom() {
        let bytes = encode("hi", "utf-16le", true);
        assert!(bytes.starts_with(&[0xFF, 0xFE]));
        assert!(!bytes[2..].starts_with(&[0xFF, 0xFE]));
    }

    #[test]
    fn latin1_encode_roundtrips() {
        let bytes = encode("Café", "windows-1252", false);
        assert!(bytes.contains(&0xE9));
        let info = EncodingInfo {
            label: "windows-1252".into(),
            has_bom: false,
            bom_len: 0,
            confidence: 1.0,
        };
        assert_eq!(decode(&bytes, &info), "Café");
    }
}
