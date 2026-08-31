//! End-to-end behaviour exercised headlessly through `AppState`.
//!
//! These tests never open a window; they drive the same state layer that the
//! Slint callbacks call, so they verify the editor semantics that the UI
//! surfaces. 83 checks.

use notepad_pro::state::AppState;
use notepad_pro_core::config::settings::Settings;
use notepad_pro_core::db::notes::NotesDb;
use notepad_pro_core::editor::list_engine::{self, EnterOutcome};
use notepad_pro_core::files::{encoding, line_endings::LineEnding};
use notepad_pro_core::highlight::{extractor, palette::Palette, stats};
use notepad_pro_core::types::line::{LineColour, ListType};

fn app() -> AppState {
    AppState::new(Settings::default(), NotesDb::in_memory().unwrap())
}

// ── Tabs ──────────────────────────────────────────────────────────────────

#[test]
fn starts_with_one_untitled_tab() {
    let s = app();
    assert_eq!(s.tabs.len(), 1);
    assert_eq!(s.active, 0);
}

#[test]
fn new_tab_is_added_and_selected() {
    let mut s = app();
    let idx = s.new_tab();
    assert_eq!(idx, 1);
    assert_eq!(s.active, 1);
    assert_eq!(s.tabs.len(), 2);
}

#[test]
fn select_tab_switches_active() {
    let mut s = app();
    s.new_tab();
    assert!(s.select_tab(0));
    assert_eq!(s.active, 0);
}

#[test]
fn close_tab_never_leaves_zero_tabs() {
    let mut s = app();
    s.close_tab(0);
    assert!(!s.tabs.is_empty());
}

#[test]
fn close_last_dirty_tab_replaces_with_fresh() {
    let mut s = app();
    s.load_text("t.txt", "x", None);
    s.set_line_text(0, "dirty");
    s.close_tab(0);
    assert_eq!(s.tabs.len(), 1);
    assert!(!s.tab().is_dirty());
}

#[test]
fn cycle_tab_wraps_forward() {
    let mut s = app();
    s.new_tab();
    s.cycle_tab(true);
    assert_eq!(s.active, 0);
}

#[test]
fn cycle_tab_wraps_backward() {
    let mut s = app();
    s.new_tab();
    s.select_tab(1);
    s.cycle_tab(false);
    assert_eq!(s.active, 0);
}

#[test]
fn any_dirty_reports_unsaved_tabs() {
    let mut s = app();
    assert!(!s.any_dirty());
    s.load_text("t.txt", "a", None);
    s.set_line_text(0, "changed");
    assert!(s.any_dirty());
}

// ── Editing ───────────────────────────────────────────────────────────────

#[test]
fn load_text_splits_lines() {
    let mut s = app();
    s.load_text("t.txt", "a\nb\nc", None);
    assert_eq!(s.doc().line_count(), 3);
}

#[test]
fn set_line_text_marks_dirty() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    assert!(s.set_line_text(0, "b"));
    assert!(s.is_dirty());
}

#[test]
fn set_line_text_ignores_out_of_range() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    assert!(!s.set_line_text(5, "x"));
}

#[test]
fn press_enter_on_plain_line_is_insert_blank() {
    let mut s = app();
    s.load_text("t.txt", "one", None);
    assert_eq!(s.press_enter(), EnterOutcome::InsertBlank);
}

#[test]
fn press_enter_on_bullet_continues_list() {
    let mut s = app();
    s.load_text("t.txt", "- item", None);
    let out = s.press_enter();
    // Continuing a list is not InsertBlank.
    assert!(!matches!(out, EnterOutcome::InsertBlank));
}

#[test]
fn insert_blank_line_grows_document() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.insert_blank_line();
    assert_eq!(s.doc().line_count(), 2);
}

// ── Lists ─────────────────────────────────────────────────────────────────

#[test]
fn set_list_type_bullet() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.set_list_type(ListType::Bullet);
    assert_eq!(s.doc().lines[0].list_type, ListType::Bullet);
}

#[test]
fn set_list_type_number_assigns_counter() {
    let mut s = app();
    s.load_text("t.txt", "a\nb", None);
    s.set_list_type(ListType::Number);
    assert_eq!(s.doc().lines[0].number, 1);
    assert_eq!(s.doc().lines[1].number, 2);
}

#[test]
fn set_list_type_check() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.set_list_type(ListType::Check);
    assert_eq!(s.doc().lines[0].list_type, ListType::Check);
}

#[test]
fn toggle_checked_flips() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.set_list_type(ListType::Check);
    assert!(s.toggle_checked(0));
    assert!(s.doc().lines[0].checked);
}

#[test]
fn indent_clamps_at_max() {
    let mut s = app();
    s.load_text("t.txt", "- a", None);
    for _ in 0..10 {
        s.indent(true);
    }
    assert!(s.doc().lines[0].indent <= 5);
}

#[test]
fn outdent_clamps_at_zero() {
    let mut s = app();
    s.load_text("t.txt", "- a", None);
    s.indent(false);
    assert_eq!(s.doc().lines[0].indent, 0);
}

#[test]
fn markdown_bullet_shortcut() {
    let mut s = app();
    s.load_text("t.txt", "", None);
    s.set_line_text(0, "- ");
    s.set_line_text(0, "- first");
    // After typing "- text" the line should be a bullet.
    assert_eq!(s.doc().lines[0].list_type, ListType::Bullet);
}

// ── Undo / redo ───────────────────────────────────────────────────────────

#[test]
fn undo_reverts_edit() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.set_line_text(0, "b");
    assert!(s.undo());
    assert_eq!(s.doc().lines[0].text, "a");
}

#[test]
fn redo_reapplies_edit() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.set_line_text(0, "b");
    s.undo();
    assert!(s.redo());
    assert_eq!(s.doc().lines[0].text, "b");
}

#[test]
fn undo_on_empty_history_is_false() {
    let mut s = app();
    assert!(!s.undo());
}

#[test]
fn undo_history_is_capped() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    for i in 0..250 {
        s.set_line_text(0, &format!("v{i}"));
    }
    // Only the last 200 states are retained.
    let mut reverted = 0;
    while s.undo() {
        reverted += 1;
    }
    assert!(reverted <= 200);
}

// ── Highlights ────────────────────────────────────────────────────────────

#[test]
fn toggle_highlight_yellow() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    assert_eq!(s.toggle_highlight_key("yellow"), Some(true));
    assert_eq!(s.doc().lines[0].colour, LineColour::Yellow);
}

#[test]
fn toggle_highlight_twice_clears() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.toggle_highlight_key("yellow");
    assert_eq!(s.toggle_highlight_key("yellow"), Some(false));
    assert_eq!(s.doc().lines[0].colour, LineColour::None);
}

#[test]
fn toggle_unknown_key_is_none() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    assert_eq!(s.toggle_highlight_key("nope"), None);
}

#[test]
fn apply_highlight_overwrites() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.apply_highlight_key("green");
    assert_eq!(s.doc().lines[0].colour, LineColour::Green);
}

#[test]
fn clear_highlight_removes_all() {
    let mut s = app();
    s.load_text("t.txt", "a\nb", None);
    s.apply_highlight_key("pink");
    s.clear_highlight();
    assert_eq!(s.doc().lines[0].colour, LineColour::None);
}

#[test]
fn add_custom_colour_valid() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    assert!(s.add_custom_colour("mint", "#a1e6c1"));
}

#[test]
fn add_custom_colour_invalid_hex() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    assert!(!s.add_custom_colour("bad", "notahex"));
}

#[test]
fn colour_for_key_resolves_builtin() {
    let s = app();
    assert_eq!(s.colour_for_key("blue"), Some(LineColour::Blue));
}

// ── Extract ───────────────────────────────────────────────────────────────

#[test]
fn extract_empty_selection_is_empty() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.apply_highlight_key("yellow");
    s.extract_selected.clear();
    assert_eq!(s.extract().text, "");
}

#[test]
fn extract_document_order() {
    let mut s = app();
    s.load_text("t.txt", "a\nb\nc", None);
    s.doc_mut().highlight_lines(0, 0, LineColour::Yellow);
    s.doc_mut().highlight_lines(2, 2, LineColour::Green);
    s.extract_selected = vec!["yellow".into(), "green".into()];
    let r = s.extract();
    assert_eq!(r.line_count, 2);
    assert!(r.text.contains('a'));
}

#[test]
fn extract_counts_lines_and_chars() {
    let mut s = app();
    s.load_text("t.txt", "hello", None);
    s.doc_mut().highlight_lines(0, 0, LineColour::Yellow);
    s.extract_selected = vec!["yellow".into()];
    let r = s.extract();
    assert_eq!(r.line_count, 1);
    assert_eq!(r.char_count, 5);
}

#[test]
fn toggle_extract_colour() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    assert!(s.toggle_extract_colour("yellow"));
    assert!(s.extract_selected.contains(&"yellow".to_string()));
}

#[test]
fn extract_order_switches() {
    let mut s = app();
    s.settings.extract_order = "grouped".into();
    assert_eq!(
        s.extract_order(),
        notepad_pro_core::highlight::extractor::ExtractionOrder::GroupByColour
    );
}

// ── Find & replace ────────────────────────────────────────────────────────

#[test]
fn find_next_locates_match() {
    let mut s = app();
    s.load_text("t.txt", "foo bar foo", None);
    s.set_find_query("foo");
    assert!(s.find_next().is_some());
}

#[test]
fn find_next_respects_case() {
    let mut s = app();
    s.load_text("t.txt", "Foo", None);
    s.set_find_query("foo");
    s.find.case_sensitive = true;
    s.invalidate_find();
    assert!(s.find_next().is_none());
}

#[test]
fn find_next_respects_word() {
    let mut s = app();
    s.load_text("t.txt", "foobar", None);
    s.set_find_query("foo");
    s.find.whole_word = true;
    s.invalidate_find();
    assert!(s.find_next().is_none());
}

#[test]
fn find_prev_wraps() {
    let mut s = app();
    s.load_text("t.txt", "foo foo", None);
    s.set_find_query("foo");
    assert!(s.find_prev().is_some());
}

#[test]
fn replace_one_replaces_single() {
    let mut s = app();
    s.load_text("t.txt", "foo foo", None);
    s.set_find_query("foo");
    s.set_find_replacement("bar");
    assert!(s.replace_one());
    assert!(s.doc().plain_text().contains("bar"));
}

#[test]
fn replace_all_replaces_every_match() {
    let mut s = app();
    s.load_text("t.txt", "foo foo foo", None);
    s.set_find_query("foo");
    s.set_find_replacement("x");
    let n = s.replace_all();
    assert_eq!(n, 3);
    assert!(!s.doc().plain_text().contains("foo"));
}

// ── Notes ─────────────────────────────────────────────────────────────────

#[test]
fn save_active_as_note_and_reopen() {
    let mut s = app();
    s.load_text("t.txt", "note body", None);
    let id = s.save_active_as_note().unwrap();
    assert!(id > 0);
    let mut t = app();
    t.open_note(id).unwrap();
    assert!(t.doc().plain_text().contains("note body"));
}

#[test]
fn new_note_creates_entry() {
    let mut s = app();
    let id = s.new_note().unwrap();
    assert!(id > 0);
}

#[test]
fn delete_note_removes_it() {
    let mut s = app();
    let id = s.new_note().unwrap();
    assert!(s.delete_note(id).unwrap());
}

#[test]
fn toggle_pin_flips() {
    let mut s = app();
    let id = s.new_note().unwrap();
    let pinned = s.toggle_pin(id).unwrap();
    assert!(pinned);
}

#[test]
fn note_list_returns_metadata() {
    let mut s = app();
    s.new_note().unwrap();
    assert!(!s.note_list().unwrap().is_empty());
}

// ── Files ─────────────────────────────────────────────────────────────────

#[test]
fn save_and_reload_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.txt");
    let mut s = app();
    s.load_text("x.txt", "line one\nline two", None);
    s.save_to(&path).unwrap();

    let mut t = app();
    t.open_path(&path).unwrap();
    assert_eq!(t.doc().line_count(), 2);
}

#[test]
fn save_active_returns_path_once_saved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("y.txt");
    let mut s = app();
    s.load_text("y.txt", "a", Some(path.to_string_lossy().into_owned()));
    assert!(s.save_active().unwrap().is_some());
}

// ── Settings / theme ──────────────────────────────────────────────────────

#[test]
fn zoom_is_clamped() {
    let mut s = app();
    for _ in 0..50 {
        s.zoom_step(1.0);
    }
    assert_eq!(s.settings.zoom, 3.0);
}

#[test]
fn set_theme_known() {
    let mut s = app();
    assert!(s.set_theme("dark"));
    assert_eq!(s.settings.theme, "dark");
}

#[test]
fn set_theme_unknown_is_false() {
    let mut s = app();
    assert!(!s.set_theme("nope"));
}

#[test]
fn toggle_dark_twin_switches_pairs() {
    let mut s = app();
    s.settings.theme = "light".into();
    let twin = s.toggle_dark_twin();
    assert_eq!(twin, "dark");
}

// ── Status / title / session ──────────────────────────────────────────────

#[test]
fn status_counts_words() {
    let mut s = app();
    s.load_text("t.txt", "hello world", None);
    let st = s.compute_status();
    assert!(st.word_count >= 2);
    assert!(st.metrics_text.contains('2') || st.word_count == 2);
}

#[test]
fn window_title_shows_dirty_dot() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    s.set_line_text(0, "dirty");
    assert!(s.window_title().contains('\u{25cf}'));
}

#[test]
fn window_title_clean_has_no_dot() {
    let mut s = app();
    s.load_text("t.txt", "a", None);
    assert!(!s.window_title().contains('\u{25cf}'));
}

#[test]
fn session_roundtrips_tabs() {
    let mut s = app();
    s.load_text("a.txt", "alpha", None);
    s.new_tab();
    s.load_text("b.txt", "beta", None);
    let session = s.build_session();
    let mut t = app();
    t.restore_session(&session);
    assert_eq!(t.tabs.len(), 2);
}

#[test]
fn session_preserves_highlights() {
    let mut s = app();
    s.load_text("a.txt", "alpha", None);
    s.doc_mut().highlight_lines(0, 0, LineColour::Yellow);
    let session = s.build_session();
    let mut t = app();
    t.restore_session(&session);
    assert_eq!(t.doc().lines[0].colour, LineColour::Yellow);
}

#[test]
fn chips_for_returns_up_to_four() {
    let s = app();
    let json = "[{\"line\":0,\"colour\":\"yellow\"},{\"line\":1,\"colour\":\"green\"}]";
    let chips = s.chips_for(json);
    assert!(chips.len() <= 4);
}

#[test]
fn toast_seq_is_monotonic() {
    let mut s = app();
    let a = s.next_toast_seq();
    let b = s.next_toast_seq();
    assert!(b > a);
}

// ── Core engines (no window needed) ───────────────────────────────────────

fn line(text: &str, colour: LineColour) -> notepad_pro_core::types::line::EditorLine {
    let mut l = notepad_pro_core::types::line::EditorLine::new(text);
    l.colour = colour;
    l
}

#[test]
fn extract_grouped_adds_colour_headings() {
    let lines = vec![line("a", LineColour::Yellow), line("b", LineColour::Green)];
    let out = extractor::grouped(&lines, &[LineColour::Yellow, LineColour::Green]);
    assert!(out.contains('#'), "grouped output should carry headings: {out}");
}

#[test]
fn extract_grouped_skips_empty_sections() {
    let lines = vec![line("a", LineColour::Yellow)];
    let out = extractor::grouped(&lines, &[LineColour::Yellow, LineColour::Green]);
    assert!(!out.contains("Green"), "empty sections must be skipped: {out}");
}

#[test]
fn stats_summary_lists_counts_in_first_appearance_order() {
    let lines = vec![line("a", LineColour::Green), line("b", LineColour::Yellow), line("c", LineColour::Green)];
    let b = stats::breakdown(&lines, &Palette::default());
    let summary = b.summary();
    let green_at = summary.find("Green").unwrap();
    let yellow_at = summary.find("Yellow").unwrap();
    assert!(green_at < yellow_at, "first-appearance order, got: {summary}");
}

#[test]
fn stats_count_highlights() {
    let lines = vec![line("a", LineColour::Yellow), line("b", LineColour::None)];
    let b = stats::breakdown(&lines, &Palette::default());
    assert_eq!(b.highlighted_lines, 1);
    assert_eq!(b.total_lines, 2);
}

#[test]
fn bullet_glyphs_follow_depth() {
    assert_eq!(list_engine::bullet_glyph(0), "•");
    assert_eq!(list_engine::bullet_glyph(1), "◦");
    assert_eq!(list_engine::bullet_glyph(2), "▪");
    assert_eq!(list_engine::bullet_glyph(3), "‣");
}

#[test]
fn bullet_glyph_clamps_past_max_depth() {
    assert_eq!(list_engine::bullet_glyph(9), "‣");
}

#[test]
fn renumber_resets_on_non_number_line() {
    let mut lines = vec![
        {
            let mut l = line("one", LineColour::None);
            l.list_type = ListType::Number;
            l
        },
        line("break", LineColour::None),
        {
            let mut l = line("again", LineColour::None);
            l.list_type = ListType::Number;
            l
        },
    ];
    list_engine::renumber(&mut lines);
    assert_eq!(lines[0].number, 1);
    assert_eq!(lines[2].number, 1, "counter resets after a plain line");
}

#[test]
fn markdown_check_shortcut() {
    let mut l = line("[] ", LineColour::None);
    l.text = "[] todo".into();
    assert!(list_engine::try_markdown_shortcut(&mut l));
    assert_eq!(l.list_type, ListType::Check);
}

#[test]
fn line_ending_detect_crlf() {
    assert_eq!(LineEnding::detect("a\r\nb"), LineEnding::Crlf);
}

#[test]
fn line_ending_detect_cr() {
    assert_eq!(LineEnding::detect("a\rb"), LineEnding::Cr);
}

#[test]
fn line_ending_apply_crlf() {
    assert_eq!(LineEnding::Crlf.apply("a\nb"), "a\r\nb");
}

#[test]
fn line_ending_labels_roundtrip() {
    for ending in [LineEnding::Lf, LineEnding::Crlf, LineEnding::Cr] {
        assert_eq!(LineEnding::from_label(ending.label()), ending);
    }
}

#[test]
fn encode_utf8_without_bom_by_default_choice() {
    let out = encoding::encode("hi", "UTF-8", false);
    assert_eq!(out, b"hi");
}

#[test]
fn encode_utf8_with_bom_writes_one_bom() {
    let out = encoding::encode("hi", "UTF-8", true);
    assert_eq!(&out[..3], &[0xef, 0xbb, 0xbf]);
    assert_eq!(&out[3..], b"hi");
}

#[test]
fn encode_utf16le_writes_single_bom() {
    let out = encoding::encode("A", "UTF-16LE", true);
    assert_eq!(&out[..2], &[0xff, 0xfe]);
    // Exactly one BOM, then one UTF-16 code unit.
    assert_eq!(out.len(), 4);
}

#[test]
fn detect_encoding_finds_utf16le() {
    let raw = encoding::encode("hello", "UTF-16LE", true);
    let info = encoding::detect_encoding(&raw);
    assert!(!info.is_utf8());
}

#[test]
fn decode_roundtrips_utf16() {
    let raw = encoding::encode("hello", "UTF-16LE", true);
    let info = encoding::detect_encoding(&raw);
    let text = encoding::decode(&raw, &info);
    assert_eq!(text, "hello");
}

#[test]
fn selection_range_orders_anchor_and_caret() {
    let mut s = app();
    s.load_text("t.txt", "abcdef", None);
    s.cursor.line = 0;
    s.cursor.col = 4;
    s.anchor = Some(1);
    let (lo, hi) = s.selection_range();
    assert_eq!((lo, hi), (1, 4));
}

#[test]
fn selected_chars_counts_selection() {
    let mut s = app();
    s.load_text("t.txt", "abcdef", None);
    s.cursor.col = 4;
    s.anchor = Some(1);
    assert_eq!(s.selected_chars(), 3);
}

#[test]
fn note_count_label_formats() {
    let s = app();
    let label = s.note_count_label(3);
    assert!(label.contains('3'), "label should mention the shown count: {label}");
}

#[test]
fn toggle_dark_twin_from_dark_returns_light() {
    let mut s = app();
    s.settings.theme = "clay-dark".into();
    assert_eq!(s.toggle_dark_twin(), "clay-light");
}
