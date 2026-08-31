//! Translation between core types and the Slint-generated structs.
//!
//! `notepad-pro-core` deliberately knows nothing about Slint, so every
//! conversion happens here and nowhere else.

use slint::{Color, ModelRc, SharedString, VecModel};

use notepad_pro_core::config::settings::Settings;
use notepad_pro_core::editor::line_model::Document;
use notepad_pro_core::files::line_endings::LineEnding;
use notepad_pro_core::highlight::palette::{channels, Palette};
use notepad_pro_core::types::api::{HighlightStats, StatusData, TabState};
use notepad_pro_core::types::line::{EditorLine, LineColour, ListType};
use notepad_pro_core::types::note::{Note, NoteMetadata};

// Re-export the generated Slint types under stable names.
pub use crate::ui::{
    AppInfo, ColourEntry, EditorLineData, NoteData, NoteMetadataData, SessionData, SettingsData,
    StatusData as StatusView, TabData, WindowStateData,
};
pub use crate::ui::{LineColour as UiLineColour, ListType as UiListType};

/// `0xRRGGBBAA` -> Slint colour.
pub fn rgba_to_color(rgba: u32) -> Color {
    let (r, g, b, a) = channels(rgba);
    Color::from_argb_u8(a, r, g, b)
}

/// Slint colour -> `0xRRGGBBAA`.
pub fn color_to_rgba(color: Color) -> u32 {
    ((color.red() as u32) << 24)
        | ((color.green() as u32) << 16)
        | ((color.blue() as u32) << 8)
        | (color.alpha() as u32)
}

pub fn ui_colour(colour: LineColour) -> UiLineColour {
    match colour {
        LineColour::None => UiLineColour::None,
        LineColour::Yellow => UiLineColour::Yellow,
        LineColour::Green => UiLineColour::Green,
        LineColour::Pink => UiLineColour::Pink,
        LineColour::Blue => UiLineColour::Blue,
        LineColour::Orange => UiLineColour::Orange,
        LineColour::Purple => UiLineColour::Purple,
        LineColour::Custom(_) => UiLineColour::Custom,
    }
}

pub fn ui_list_type(list_type: ListType) -> UiListType {
    match list_type {
        ListType::None => UiListType::None,
        ListType::Bullet => UiListType::Bullet,
        ListType::Number => UiListType::Number,
        ListType::Check => UiListType::Check,
    }
}

/// Marker glyph / number text. Mirrors `ListEngine::marker_text`, except that
/// checkboxes are drawn as a real widget so they return no text here.
pub fn marker_text(line: &EditorLine) -> String {
    match line.list_type {
        ListType::None => String::new(),
        ListType::Bullet => notepad_pro_core::editor::list_engine::ListEngine::bullet_glyph(line.indent)
            .to_string(),
        ListType::Number => format!("{}.", line.number),
        ListType::Check => String::new(),
    }
}

pub fn line_to_ui(line: &EditorLine, palette: &Palette, selected: bool) -> EditorLineData {
    let (_, rgba) = palette.resolve(line.colour);
    EditorLineData {
        text: SharedString::from(line.text.as_str()),
        colour: ui_colour(line.colour),
        accent: rgba_to_color(rgba),
        list_type: ui_list_type(line.list_type),
        marker: SharedString::from(marker_text(line).as_str()),
        indent: line.indent as i32,
        checked: line.checked,
        selected,
    }
}

pub fn lines_to_model(
    lines: &[EditorLine],
    palette: &Palette,
    cursor_line: usize,
) -> ModelRc<EditorLineData> {
    let rows: Vec<EditorLineData> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| line_to_ui(line, palette, i == cursor_line))
        .collect();
    ModelRc::from(std::rc::Rc::new(VecModel::from(rows)))
}

pub fn tab_to_ui(tab: &TabState) -> TabData {
    TabData {
        id: SharedString::from(tab.id.as_str()),
        name: SharedString::from(tab.display_name().as_str()),
        path: SharedString::from(tab.path.as_deref().unwrap_or("")),
        dirty: tab.dirty,
    }
}

pub fn tabs_to_model(tabs: &[TabState]) -> ModelRc<TabData> {
    ModelRc::from(std::rc::Rc::new(VecModel::from(
        tabs.iter().map(tab_to_ui).collect::<Vec<_>>(),
    )))
}

pub fn note_to_ui(note: &NoteMetadata) -> NoteMetadataData {
    let chip = |i: usize| -> Color {
        note.colour_chips
            .get(i)
            .copied()
            .map(rgba_to_color)
            .unwrap_or_else(|| Color::from_argb_u8(0, 0, 0, 0))
    };
    NoteMetadataData {
        id: note.id as i32,
        title: SharedString::from(note.title.as_str()),
        snippet: SharedString::from(note.snippet.as_str()),
        modified_label: SharedString::from(note.modified_label.as_str()),
        pinned: note.pinned,
        chip_a: chip(0),
        chip_b: chip(1),
        chip_c: chip(2),
        chip_d: chip(3),
        chip_count: note.colour_chips.len() as i32,
    }
}

pub fn notes_to_model(notes: &[NoteMetadata]) -> ModelRc<NoteMetadataData> {
    ModelRc::from(std::rc::Rc::new(VecModel::from(
        notes.iter().map(note_to_ui).collect::<Vec<_>>(),
    )))
}

pub fn settings_to_ui(settings: &Settings) -> SettingsData {
    SettingsData {
        theme: SharedString::from(settings.theme.as_str()),
        font_family: SharedString::from(settings.font_family.as_str()),
        font_size: settings.font_size as i32,
        word_wrap: settings.word_wrap,
        zoom: settings.zoom,
        animations: settings.animations,
        sidebar_open: settings.sidebar_open,
        sidebar_sort: SharedString::from(settings.sidebar_sort.as_str()),
        autosave_interval_secs: settings.autosave_interval_secs as i32,
        extract_order: SharedString::from(settings.extract_order.as_str()),
        native_frame: settings.native_frame,
    }
}

/// Fold a UI settings struct back onto the stored settings, then clamp.
pub fn ui_to_settings(view: &SettingsData, target: &mut Settings) {
    target.theme = view.theme.to_string();
    target.font_family = view.font_family.to_string();
    target.font_size = view.font_size.clamp(0, 255) as u8;
    target.word_wrap = view.word_wrap;
    target.zoom = view.zoom;
    target.animations = view.animations;
    target.sidebar_open = view.sidebar_open;
    target.sidebar_sort = view.sidebar_sort.to_string();
    target.autosave_interval_secs = view.autosave_interval_secs.max(0) as u32;
    target.extract_order = view.extract_order.to_string();
    target.native_frame = view.native_frame;
    target.clamp();
}

pub fn note_to_ui_full(note: &Note) -> NoteData {
    NoteData {
        id: note.id as i32,
        title: SharedString::from(note.title.as_str()),
        content: SharedString::from(note.content.as_str()),
        highlights_json: SharedString::from(note.highlights_json.as_str()),
        list_structure_json: SharedString::from(note.list_structure_json.as_str()),
        file_path: SharedString::from(note.file_path.as_deref().unwrap_or("")),
        pinned: note.pinned,
        created_at: note.created_at as f32,
        modified_at: note.modified_at as f32,
    }
}

pub fn ui_to_note(view: &NoteData) -> Note {
    Note {
        id: view.id as i64,
        title: view.title.to_string(),
        content: view.content.to_string(),
        highlights_json: view.highlights_json.to_string(),
        list_structure_json: view.list_structure_json.to_string(),
        file_path: if view.file_path.is_empty() {
            None
        } else {
            Some(view.file_path.to_string())
        },
        pinned: view.pinned,
        created_at: view.created_at as f64,
        modified_at: view.modified_at as f64,
    }
}

pub fn stats_to_ui(stats: &HighlightStats) -> crate::ui::HighlightStats {
    crate::ui::HighlightStats {
        total_lines: stats.total_lines,
        highlighted: stats.highlighted,
        summary: SharedString::from(stats.summary.as_str()),
    }
}

pub fn status_to_ui(status: &StatusData) -> StatusView {
    StatusView {
        caret_text: SharedString::from(status.caret_text.as_str()),
        metrics_text: SharedString::from(status.metrics_text.as_str()),
        highlight_text: SharedString::from(status.highlight_text.as_str()),
        zoom_text: SharedString::from(status.zoom_text.as_str()),
        line_ending: SharedString::from(status.line_ending.as_str()),
        encoding: SharedString::from(status.encoding.as_str()),
        dirty: status.dirty,
        saved_text: SharedString::from(status.saved_text.as_str()),
        cursor_line: status.cursor_line,
        cursor_col: status.cursor_col,
        selected_chars: status.selected_chars,
        word_count: status.word_count,
        char_count: status.char_count,
        line_count: status.line_count,
        highlight_count: status.highlight_count,
        zoom: status.zoom,
    }
}

/// Build the swatch / extract-panel rows.
pub fn colour_entries(
    palette: &Palette,
    doc: &Document,
    active: &[String],
) -> ModelRc<ColourEntry> {
    let counts: Vec<(LineColour, usize)> =
        notepad_pro_core::highlight::stats::colour_counts(&doc.lines, palette);

    let rows = palette
        .entries()
        .into_iter()
        .map(|entry| {
            let colour = LineColour::from_key(&entry.key);
            let count = counts
                .iter()
                .find(|(c, _)| *c == colour)
                .map(|(_, n)| *n)
                .unwrap_or(0);
            ColourEntry {
                key: SharedString::from(entry.key.as_str()),
                name: SharedString::from(entry.name.as_str()),
                colour: rgba_to_color(entry.rgba),
                line_count: count as i32,
                count_label: SharedString::from(
                    if count == 1 {
                        "1 line".to_string()
                    } else {
                        format!("{count} lines")
                    }
                    .as_str(),
                ),
                active: active.contains(&entry.key),
            }
        })
        .collect::<Vec<_>>();

    ModelRc::from(std::rc::Rc::new(VecModel::from(rows)))
}

/// The swatch strip in the toolbar: built-ins first, then customs.
pub fn palette_model(palette: &Palette, active_key: &str) -> ModelRc<ColourEntry> {
    let rows = palette
        .entries()
        .into_iter()
        .map(|entry| ColourEntry {
            key: SharedString::from(entry.key.as_str()),
            name: SharedString::from(entry.name.as_str()),
            colour: rgba_to_color(entry.rgba),
            line_count: 0,
            count_label: SharedString::from(""),
            active: entry.key == active_key,
        })
        .collect::<Vec<_>>();
    ModelRc::from(std::rc::Rc::new(VecModel::from(rows)))
}

pub fn line_ending_label(ending: LineEnding) -> String {
    ending.label().to_string()
}

pub fn string_model(items: &[String]) -> ModelRc<SharedString> {
    ModelRc::from(std::rc::Rc::new(VecModel::from(
        items
            .iter()
            .map(|s| SharedString::from(s.as_str()))
            .collect::<Vec<_>>(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn rgba_roundtrips_through_a_slint_colour() {
        for rgba in [
            0xffe2_7aff,
            0xa8e6_a1ff,
            0x1234_56ff,
            0x0000_0080,
            0xffff_ffff,
        ] {
            assert_eq!(color_to_rgba(rgba_to_color(rgba)), rgba);
        }
    }

    #[test]
    fn every_line_colour_maps_to_a_ui_variant() {
        assert_eq!(ui_colour(LineColour::None), UiLineColour::None);
        assert_eq!(ui_colour(LineColour::Yellow), UiLineColour::Yellow);
        assert_eq!(ui_colour(LineColour::Custom(1)), UiLineColour::Custom);
    }

    #[test]
    fn marker_text_matches_the_line_type() {
        let mut line = EditorLine::new("x");
        assert_eq!(marker_text(&line), "");
        line.list_type = ListType::Bullet;
        assert_eq!(marker_text(&line), "•");
        line.list_type = ListType::Number;
        line.number = 7;
        assert_eq!(marker_text(&line), "7.");
        line.list_type = ListType::Check;
        assert_eq!(marker_text(&line), "", "the checkbox is drawn as a widget");
    }

    #[test]
    fn line_conversion_carries_the_resolved_accent() {
        let line = EditorLine {
            text: "hi".into(),
            colour: LineColour::Yellow,
            ..Default::default()
        };
        let ui = line_to_ui(&line, &Palette::default(), true);
        assert_eq!(ui.colour, UiLineColour::Yellow);
        assert_eq!(color_to_rgba(ui.accent), 0xffe2_7aff);
        assert!(ui.selected);
    }

    #[test]
    fn custom_colours_keep_their_own_accent() {
        let line = EditorLine {
            colour: LineColour::Custom(0xff88_00ff),
            ..Default::default()
        };
        let ui = line_to_ui(&line, &Palette::default(), false);
        assert_eq!(color_to_rgba(ui.accent), 0xff88_00ff);
    }

    #[test]
    fn tab_names_carry_the_dirty_marker() {
        let mut tab = TabState::new("notes.txt");
        assert_eq!(tab_to_ui(&tab).name.as_str(), "notes.txt");
        tab.dirty = true;
        assert_eq!(tab_to_ui(&tab).name.as_str(), "notes.txt *");
    }

    #[test]
    fn missing_chips_are_transparent() {
        let note = NoteMetadata {
            id: 1,
            title: "t".into(),
            snippet: "s".into(),
            pinned: false,
            modified_at: 0.0,
            modified_label: "just now".into(),
            colour_chips: vec![0xffe2_7aff],
        };
        let ui = note_to_ui(&note);
        assert_eq!(ui.chip_count, 1);
        assert_eq!(ui.chip_a.alpha(), 255);
        assert_eq!(ui.chip_b.alpha(), 0);
    }

    #[test]
    fn settings_roundtrip_through_the_ui_struct() {
        let mut settings = Settings::default();
        settings.theme = "neu-dark".into();
        settings.font_size = 18;
        settings.zoom = 1.5;
        let view = settings_to_ui(&settings);
        let mut back = Settings::default();
        ui_to_settings(&view, &mut back);
        assert_eq!(back, settings);
    }

    #[test]
    fn ui_settings_are_clamped_on_the_way_in() {
        let mut settings = Settings::default();
        let view = SettingsData {
            font_size: 900,
            zoom: 99.0,
            autosave_interval_secs: 0,
            ..settings_to_ui(&settings)
        };
        ui_to_settings(&view, &mut settings);
        assert_eq!(settings.font_size, 72);
        assert_eq!(settings.zoom, 3.0);
        assert_eq!(settings.autosave_interval_secs, 1);
    }

    #[test]
    fn note_roundtrip_preserves_the_optional_path() {
        let mut note = Note::new("T", "body");
        note.id = 3;
        // Slint floats are f32; use exactly-representable timestamps.
        note.created_at = 1_700_000_000.0;
        note.modified_at = 1_700_000_500.0;
        note.file_path = Some("/tmp/a.txt".into());
        let ui = note_to_ui_full(&note);
        assert_eq!(ui.file_path.as_str(), "/tmp/a.txt");
        assert_eq!(ui_to_note(&ui), note);
    }

    #[test]
    fn models_are_built_in_order() {
        let lines = vec![
            EditorLine::new("a"),
            EditorLine::new("b"),
            EditorLine::new("c"),
        ];
        let model = lines_to_model(&lines, &Palette::default(), 1);
        assert_eq!(model.row_count(), 3);
        assert_eq!(model.row_data(1).unwrap().text.as_str(), "b");
        assert!(model.row_data(1).unwrap().selected);
        assert!(!model.row_data(0).unwrap().selected);
    }

    #[test]
    fn colour_entries_mark_the_active_key_and_count_lines() {
        let doc = Document::from_lines(vec![
            EditorLine {
                colour: LineColour::Yellow,
                ..Default::default()
            },
            EditorLine {
                colour: LineColour::Yellow,
                ..Default::default()
            },
            EditorLine::default(),
        ]);
        let model = colour_entries(&Palette::default(), &doc, &["yellow".to_string()]);
        assert_eq!(model.row_count(), 6);
        let yellow = model.row_data(0).unwrap();
        assert_eq!(yellow.key.as_str(), "yellow");
        assert!(yellow.active);
        assert_eq!(yellow.line_count, 2);
        assert_eq!(yellow.count_label.as_str(), "2 lines");
        assert!(!model.row_data(1).unwrap().active);
    }

    #[test]
    fn singular_count_label_reads_well() {
        let doc = Document::from_lines(vec![EditorLine {
            colour: LineColour::Pink,
            ..Default::default()
        }]);
        let model = colour_entries(&Palette::default(), &doc, &[]);
        let pink = model.row_data(2).unwrap();
        assert_eq!(pink.key.as_str(), "pink");
        assert_eq!(pink.count_label.as_str(), "1 line");
    }

    #[test]
    fn string_model_preserves_order() {
        let model = string_model(&["a".into(), "b".into()]);
        assert_eq!(model.row_count(), 2);
        assert_eq!(model.row_data(0).unwrap().as_str(), "a");
    }
}
