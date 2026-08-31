//! The highlight palette: six built-ins plus unlimited custom colours.

use serde::{Deserialize, Serialize};

use crate::types::line::LineColour;
use crate::types::note::CustomColour;

/// Built-in swatches: `(key, hex, packed 0xRRGGBBAA)`.
pub const BUILTIN: &[(&str, &str, u32)] = &[
    ("yellow", "#ffe27a", 0xffe2_7aff),
    ("green", "#a8e6a1", 0xa8e6_a1ff),
    ("pink", "#ffb3d1", 0xffb3_d1ff),
    ("blue", "#a3d5ff", 0xa3d5_ffff),
    ("orange", "#ffc08a", 0xffc0_8aff),
    ("purple", "#d5b3ff", 0xd5b3_ffff),
];

/// The seven themes, in menu order.
pub const THEMES: &[&str] = &[
    "light",
    "dark",
    "glass-dark",
    "clay-light",
    "clay-dark",
    "neu-light",
    "neu-dark",
];

/// Light-family themes, used by the Ctrl+Shift+D "dark twin" toggle.
pub const LIGHT_THEMES: &[&str] = &["light", "clay-light", "neu-light"];

/// Alpha applied to the full-width highlight band behind a line.
pub const BAND_ALPHA: f32 = 0.25;

/// Alpha applied to the 3px accent bar on the left edge.
pub const ACCENT_ALPHA: f32 = 1.0;

pub fn is_known_theme(name: &str) -> bool {
    THEMES.contains(&name)
}

/// The light/dark twin of a theme, for Ctrl+Shift+D.
pub fn dark_twin(current: &str) -> &'static str {
    match current {
        "light" => "dark",
        "dark" => "light",
        "glass-dark" => "light",
        "clay-light" => "clay-dark",
        "clay-dark" => "clay-light",
        "neu-light" => "neu-dark",
        "neu-dark" => "neu-light",
        _ => "dark",
    }
}

/// `0xRRGGBBAA` -> `#rrggbb`.
pub fn hex_from_rgba(rgba: u32) -> String {
    format!("#{:06x}", (rgba >> 8) & 0x00ff_ffff)
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` -> `0xRRGGBBAA`.
pub fn rgba_from_hex(hex: &str) -> Option<u32> {
    let digits = hex.strip_prefix('#').unwrap_or(hex);
    let parse = |s: &str| u32::from_str_radix(s, 16).ok();
    match digits.len() {
        3 => {
            let r = parse(&digits[0..1])? * 17;
            let g = parse(&digits[1..2])? * 17;
            let b = parse(&digits[2..3])? * 17;
            Some((r << 24) | (g << 16) | (b << 8) | 0xff)
        }
        6 => {
            let rgb = parse(digits)?;
            Some((rgb << 8) | 0xff)
        }
        8 => parse(digits),
        _ => None,
    }
}

/// Split a packed colour into 0-255 channels.
pub fn channels(rgba: u32) -> (u8, u8, u8, u8) {
    (
        ((rgba >> 24) & 0xff) as u8,
        ((rgba >> 16) & 0xff) as u8,
        ((rgba >> 8) & 0xff) as u8,
        (rgba & 0xff) as u8,
    )
}

/// One selectable row in the colour UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteEntry {
    pub key: String,
    pub name: String,
    pub hex: String,
    pub rgba: u32,
    pub builtin: bool,
}

/// The palette: built-ins followed by the user's custom colours.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Palette {
    custom: Vec<CustomColour>,
}

impl Palette {
    pub fn new(custom: Vec<CustomColour>) -> Self {
        Self { custom }
    }

    pub fn builtin_entries() -> Vec<PaletteEntry> {
        BUILTIN
            .iter()
            .map(|(key, hex, rgba)| PaletteEntry {
                key: (*key).to_string(),
                name: capitalise(key),
                hex: (*hex).to_string(),
                rgba: *rgba,
                builtin: true,
            })
            .collect()
    }

    pub fn custom_entries(&self) -> Vec<PaletteEntry> {
        self.custom
            .iter()
            .map(|c| PaletteEntry {
                key: c.hex.clone(),
                name: c.name.clone(),
                hex: c.hex.clone(),
                rgba: c.rgba,
                builtin: false,
            })
            .collect()
    }

    pub fn entries(&self) -> Vec<PaletteEntry> {
        let mut out = Self::builtin_entries();
        out.extend(self.custom_entries());
        out
    }

    /// Resolve a [`LineColour`] to `(display name, packed rgba)`.
    pub fn resolve(&self, colour: LineColour) -> (String, u32) {
        match colour {
            LineColour::None => ("None".to_string(), 0),
            LineColour::Custom(rgba) => (hex_from_rgba(rgba), rgba),
            other => {
                let key = other.key();
                let entry = self
                    .entries()
                    .into_iter()
                    .find(|e| e.key == key)
                    .or_else(|| Self::builtin_entries().into_iter().find(|e| e.key == key));
                match entry {
                    Some(e) => (e.name, e.rgba),
                    None => (other.display_name(), other.builtin_rgba()),
                }
            }
        }
    }

    /// Look a colour up by the key used in the Slint API.
    pub fn find(&self, key: &str) -> Option<LineColour> {
        if key == "none" {
            return Some(LineColour::None);
        }
        if self.entries().iter().any(|e| e.key == key) {
            Some(LineColour::from_key(key))
        } else {
            None
        }
    }

    pub fn custom_colours(&self) -> &[CustomColour] {
        &self.custom
    }

    pub fn add_custom(&mut self, colour: CustomColour) {
        self.custom.retain(|c| c.hex != colour.hex);
        self.custom.push(colour);
    }

    /// Colours that are safe to render on a light or dark band, i.e. every
    /// swatch except `None`.
    pub fn selectable_keys(&self) -> Vec<String> {
        self.entries().into_iter().map(|e| e.key).collect()
    }
}

fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Distinct highlight colours referenced by a stored `highlights_json` blob,
/// in first-appearance order, capped at four for the sidebar chips.
pub fn chips_from_highlights_json(json: &str) -> Vec<u32> {
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(map) = parsed.as_object() else {
        return Vec::new();
    };
    // Sort by numeric line index so chip order is stable.
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by_key(|k| k.parse::<usize>().unwrap_or(usize::MAX));

    let mut chips: Vec<u32> = Vec::new();
    for key in keys {
        let Some(name) = map[key].as_str() else {
            continue;
        };
        let colour = LineColour::from_key(name);
        if !colour.is_highlighted() {
            continue;
        }
        let rgba = colour.builtin_rgba();
        if !chips.contains(&rgba) {
            chips.push(rgba);
        }
        if chips.len() == 4 {
            break;
        }
    }
    chips
}

/// Build the `highlights_json` blob from a document's lines.
pub fn highlights_json_for(lines: &[crate::types::line::EditorLine]) -> String {
    let mut map = serde_json::Map::new();
    for (index, line) in lines.iter().enumerate() {
        if line.colour.is_highlighted() {
            map.insert(index.to_string(), serde_json::Value::String(line.colour.key()));
        }
    }
    serde_json::Value::Object(map).to_string()
}

/// Build the `list_structure_json` blob from a document's lines.
pub fn list_structure_json_for(lines: &[crate::types::line::EditorLine]) -> String {
    use crate::types::line::ListType;
    let entries: Vec<serde_json::Value> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.list_type != ListType::None)
        .map(|(i, l)| {
            serde_json::json!({
                "i": i,
                "t": l.list_type.key(),
                "d": l.indent,
                "c": l.checked,
            })
        })
        .collect();
    serde_json::Value::Array(entries).to_string()
}

/// Re-apply a `highlights_json` blob to a document's lines.
pub fn apply_highlights_json(
    lines: &mut [crate::types::line::EditorLine],
    json: &str,
) -> usize {
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let Some(map) = parsed.as_object() else {
        return 0;
    };
    let mut applied = 0;
    for (key, value) in map {
        let Ok(index) = key.parse::<usize>() else {
            continue;
        };
        let Some(name) = value.as_str() else {
            continue;
        };
        let colour = LineColour::from_key(name);
        if let Some(line) = lines.get_mut(index) {
            line.colour = colour;
            applied += 1;
        }
    }
    applied
}

/// Re-apply a `list_structure_json` blob to a document's lines.
pub fn apply_list_structure_json(
    lines: &mut [crate::types::line::EditorLine],
    json: &str,
) -> usize {
    use crate::types::line::ListType;
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let Some(entries) = parsed.as_array() else {
        return 0;
    };
    let mut applied = 0;
    for entry in entries {
        let Some(index) = entry.get("i").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(kind) = entry.get("t").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(line) = lines.get_mut(index as usize) else {
            continue;
        };
        line.list_type = ListType::from_key(kind);
        if let Some(depth) = entry.get("d").and_then(|v| v.as_u64()) {
            line.indent = (depth as u8).min(crate::types::line::MAX_INDENT);
        }
        if let Some(checked) = entry.get("c").and_then(|v| v.as_bool()) {
            line.checked = checked && line.list_type == ListType::Check;
        }
        applied += 1;
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::line::{EditorLine, ListType};

    #[test]
    fn there_are_six_builtin_colours() {
        assert_eq!(BUILTIN.len(), 6);
        assert_eq!(Palette::builtin_entries().len(), 6);
    }

    #[test]
    fn every_builtin_is_opaque_and_matches_its_hex() {
        for (key, hex, rgba) in BUILTIN {
            assert_eq!(*rgba & 0xff, 0xff, "{key} must be opaque");
            assert_eq!(hex_from_rgba(*rgba), *hex);
        }
    }

    #[test]
    fn hex_parsing_accepts_three_six_and_eight_digits() {
        assert_eq!(rgba_from_hex("#fff"), Some(0xffff_ffff));
        assert_eq!(rgba_from_hex("#ff8800"), Some(0xff88_00ff));
        assert_eq!(rgba_from_hex("#ff880080"), Some(0xff88_0080));
        assert_eq!(rgba_from_hex("ff8800"), Some(0xff88_00ff), "bare hex works");
    }

    #[test]
    fn hex_parsing_rejects_garbage() {
        assert_eq!(rgba_from_hex("#zzzzzz"), None);
        assert_eq!(rgba_from_hex("#12345"), None);
        assert_eq!(rgba_from_hex(""), None);
        assert_eq!(rgba_from_hex("#"), None);
    }

    #[test]
    fn hex_roundtrips_through_rgba() {
        for (_, hex, rgba) in BUILTIN {
            assert_eq!(rgba_from_hex(hex), Some(*rgba));
        }
    }

    #[test]
    fn channels_split_a_packed_colour() {
        assert_eq!(channels(0x1122_3344), (0x11, 0x22, 0x33, 0x44));
    }

    #[test]
    fn theme_list_has_seven_entries() {
        assert_eq!(THEMES.len(), 7);
        for theme in THEMES {
            assert!(is_known_theme(theme));
        }
        assert!(!is_known_theme("solarised"));
    }

    #[test]
    fn light_and_dark_twins_are_symmetric() {
        for &light in LIGHT_THEMES {
            let dark = dark_twin(light);
            assert_ne!(light, dark);
            assert!(is_known_theme(dark));
            assert_eq!(dark_twin(dark), light, "{light} <-> {dark} must round-trip");
        }
    }

    #[test]
    fn dark_twin_of_a_dark_theme_is_its_light_partner() {
        assert_eq!(dark_twin("dark"), "light");
        assert_eq!(dark_twin("clay-dark"), "clay-light");
        assert_eq!(dark_twin("neu-dark"), "neu-light");
    }

    #[test]
    fn dark_twin_of_an_unknown_theme_is_dark() {
        assert_eq!(dark_twin("nonsense"), "dark");
    }

    #[test]
    fn glass_dark_returns_to_light() {
        assert_eq!(dark_twin("glass-dark"), "light");
    }

    #[test]
    fn palette_resolves_builtins_by_name() {
        let palette = Palette::default();
        let (name, rgba) = palette.resolve(LineColour::Yellow);
        assert_eq!(name, "Yellow");
        assert_eq!(rgba, 0xffe2_7aff);
    }

    #[test]
    fn palette_resolves_custom_colours_from_the_hex() {
        let palette = Palette::new(vec![CustomColour::new("Sunset", "#ff8800")]);
        let (name, rgba) = palette.resolve(LineColour::Custom(0xff88_00ff));
        assert_eq!(name, "#ff8800");
        assert_eq!(rgba, 0xff88_00ff);
    }

    #[test]
    fn palette_resolves_none_to_zero() {
        let (name, rgba) = Palette::default().resolve(LineColour::None);
        assert_eq!(name, "None");
        assert_eq!(rgba, 0);
    }

    #[test]
    fn palette_entries_are_builtins_then_custom() {
        let palette = Palette::new(vec![CustomColour::new("Sunset", "#ff8800")]);
        let entries = palette.entries();
        assert_eq!(entries.len(), 7);
        assert!(entries[..6].iter().all(|e| e.builtin));
        assert!(!entries[6].builtin);
        assert_eq!(entries[6].key, "#ff8800");
    }

    #[test]
    fn find_maps_keys_back_to_colours() {
        let palette = Palette::new(vec![CustomColour::new("Sunset", "#ff8800")]);
        assert_eq!(palette.find("yellow"), Some(LineColour::Yellow));
        assert_eq!(
            palette.find("#ff8800"),
            Some(LineColour::Custom(0xff88_00ff))
        );
        assert_eq!(palette.find("none"), Some(LineColour::None));
        assert_eq!(palette.find("magenta"), None);
    }

    #[test]
    fn add_custom_deduplicates_by_hex() {
        let mut palette = Palette::default();
        palette.add_custom(CustomColour::new("A", "#123456"));
        palette.add_custom(CustomColour::new("B", "#123456"));
        assert_eq!(palette.custom_colours().len(), 1);
        assert_eq!(palette.custom_colours()[0].name, "B");
    }

    #[test]
    fn selectable_keys_cover_the_whole_palette() {
        let palette = Palette::new(vec![CustomColour::new("Sunset", "#ff8800")]);
        assert_eq!(palette.selectable_keys().len(), 7);
    }

    #[test]
    fn chips_are_distinct_and_capped() {
        let json = r#"{"0":"yellow","1":"yellow","2":"green","3":"pink",
                       "4":"blue","5":"orange","6":"purple"}"#;
        let chips = chips_from_highlights_json(json);
        assert_eq!(chips.len(), 4);
        let unique: std::collections::HashSet<_> = chips.iter().collect();
        assert_eq!(unique.len(), chips.len());
    }

    #[test]
    fn chips_follow_line_order_not_key_order() {
        // Keys are sorted numerically so line 10 does not sort before line 2.
        let json = r#"{"2":"green","0":"yellow","1":"green","10":"blue"}"#;
        let chips = chips_from_highlights_json(json);
        assert_eq!(chips, vec![0xffe2_7aff, 0xa8e6_a1ff, 0xa3d5_ffff]);
    }

    #[test]
    fn chips_ignore_none_and_bad_input() {
        assert!(chips_from_highlights_json("{}").is_empty());
        assert!(chips_from_highlights_json("not json").is_empty());
        assert!(chips_from_highlights_json("[1,2,3]").is_empty());
        assert!(chips_from_highlights_json(r#"{"0":"none"}"#).is_empty());
    }

    #[test]
    fn highlights_json_roundtrips() {
        let mut lines = vec![
            EditorLine::new("a"),
            EditorLine::new("b"),
            EditorLine::new("c"),
        ];
        lines[0].colour = LineColour::Yellow;
        lines[2].colour = LineColour::Custom(0xff88_00ff);
        let json = highlights_json_for(&lines);
        assert_eq!(json, r##"{"0":"yellow","2":"#ff8800"}"##);

        let mut target = vec![EditorLine::new("a"), EditorLine::new("b"), EditorLine::new("c")];
        assert_eq!(apply_highlights_json(&mut target, &json), 2);
        assert_eq!(target[0].colour, LineColour::Yellow);
        assert_eq!(target[1].colour, LineColour::None);
        assert_eq!(target[2].colour, LineColour::Custom(0xff88_00ff));
    }

    #[test]
    fn apply_highlights_ignores_out_of_range_indices() {
        let mut lines = vec![EditorLine::new("a")];
        assert_eq!(apply_highlights_json(&mut lines, r#"{"9":"yellow"}"#), 0);
        assert_eq!(lines[0].colour, LineColour::None);
    }

    #[test]
    fn list_structure_json_roundtrips() {
        let mut lines = vec![EditorLine::new("a"), EditorLine::new("b")];
        lines[0].list_type = ListType::Bullet;
        lines[0].indent = 2;
        lines[1].list_type = ListType::Check;
        lines[1].checked = true;
        let json = list_structure_json_for(&lines);

        let mut target = vec![EditorLine::new("a"), EditorLine::new("b")];
        assert_eq!(apply_list_structure_json(&mut target, &json), 2);
        assert_eq!(target[0].list_type, ListType::Bullet);
        assert_eq!(target[0].indent, 2);
        assert_eq!(target[1].list_type, ListType::Check);
        assert!(target[1].checked);
    }

    #[test]
    fn list_structure_rejects_a_checked_flag_on_non_check_items() {
        let mut lines = vec![EditorLine::new("a")];
        apply_list_structure_json(
            &mut lines,
            r#"[{"i":0,"t":"bullet","d":1,"c":true}]"#,
        );
        assert!(!lines[0].checked);
    }

    #[test]
    fn list_structure_clamps_the_indent() {
        let mut lines = vec![EditorLine::new("a")];
        apply_list_structure_json(&mut lines, r#"[{"i":0,"t":"bullet","d":99}]"#);
        assert_eq!(lines[0].indent, crate::types::line::MAX_INDENT);
    }

    #[test]
    fn malformed_json_is_ignored_rather_than_panic() {
        let mut lines = vec![EditorLine::new("a")];
        assert_eq!(apply_list_structure_json(&mut lines, "nonsense"), 0);
        assert_eq!(apply_highlights_json(&mut lines, "nonsense"), 0);
        assert_eq!(lines[0].list_type, ListType::None);
    }

    #[test]
    fn band_alpha_is_subtle_but_visible() {
        assert!(BAND_ALPHA > 0.1 && BAND_ALPHA < 0.5);
        assert_eq!(ACCENT_ALPHA, 1.0);
    }
}
