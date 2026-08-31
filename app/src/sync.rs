//! Pushes [`AppState`] into the window's Slint properties.
//!
//! Split into small functions so a callback only re-syncs what it changed.
//! `sync_editor` is the expensive one (it rebuilds the line model and so
//! recreates every row), so the typing path deliberately avoids it.

use slint::ComponentHandle;

use crate::convert;
use crate::state::AppState;
use crate::ui::AppWindow;

/// Everything except the line model. Cheap enough to call after any change.
pub fn sync_light(window: &AppWindow, state: &AppState) {
    sync_tabs(window, state);
    sync_status(window, state);
    sync_flags(window, state);
    sync_palette(window, state);
    sync_find(window, state);
    sync_extract(window, state);
}

/// Full refresh, including the line model.
pub fn sync_all(window: &AppWindow, state: &AppState) {
    sync_light(window, state);
    sync_editor(window, state);
    sync_notes(window, state);
}

pub fn sync_tabs(window: &AppWindow, state: &AppState) {
    window.set_tabs(convert::tabs_to_model(&state.tab_states()));
    window.set_active_tab(state.active as i32);
    window.set_window_title(state.window_title().as_str().into());
}

pub fn sync_editor(window: &AppWindow, state: &AppState) {
    let doc = state.doc();
    window.set_lines(convert::lines_to_model(
        &doc.lines,
        &state.palette,
        state.cursor.line,
    ));
    window.set_cursor_line(state.cursor.line as i32);
    window.set_cursor_col(state.cursor.col as i32);
    window.set_document_empty(doc.is_empty());
    window.set_can_undo(doc.can_undo());
    window.set_can_redo(doc.can_redo());
    window.set_zoom(state.settings.zoom);
    window.set_word_wrap(state.settings.word_wrap);
    window.set_font_family(state.settings.font_family.as_str().into());
    window.set_base_font_size(state.settings.font_size as f32);
    window.set_animations(state.settings.animations);
    window.set_theme(state.settings.theme.as_str().into());
    window.set_native_frame(state.settings.native_frame);
}

pub fn sync_flags(window: &AppWindow, state: &AppState) {
    window.set_sidebar_open(state.settings.sidebar_open);
    window.set_find_open(state.find_open);
    window.set_replace_open(state.replace_open);
    window.set_extract_open(state.extract_open);
    window.set_selected_note_id(state.selected_note_id as i32);
    window.set_note_query(state.note_query.as_str().into());
    window.set_note_sort(state.settings.sidebar_sort.as_str().into());
    window.set_extract_grouped(state.settings.extract_order == "grouped");
    window.set_show_confirm(state.pending.is_some());
    window.set_picker_hex(state.picker_hex.as_str().into());
    window.set_picker_name(state.picker_name.as_str().into());
}

pub fn sync_status(window: &AppWindow, state: &AppState) {
    window.set_status(convert::status_to_ui(&state.compute_status()));
}

pub fn sync_palette(window: &AppWindow, state: &AppState) {
    window.set_palette(convert::palette_model(&state.palette, &state.armed_colour));
}

pub fn sync_find(window: &AppWindow, state: &AppState) {
    let total = state.find.match_count();
    let counter = if total == 0 {
        if state.find.query.is_empty() {
            "No matches".to_string()
        } else {
            "No matches".to_string()
        }
    } else {
        format!("{} / {}", state.find.position(), total)
    };
    window.set_find_counter(counter.as_str().into());
    window.set_find_total(total as i32);
    window.set_find_query(state.find.query.as_str().into());
    window.set_find_replacement(state.find.replacement.as_str().into());
    window.set_find_case(state.find.case_sensitive);
    window.set_find_word(state.find.whole_word);
}

pub fn sync_extract(window: &AppWindow, state: &AppState) {
    let result = state.extract();
    window.set_extract_entries(convert::colour_entries(
        &state.palette,
        state.doc(),
        &state.extract_selected,
    ));
    window.set_extract_preview(result.text.as_str().into());
    window.set_extract_label(
        format!("{} lines \u{00b7} {} chars", result.line_count, result.char_count)
            .as_str()
            .into(),
    );
}

pub fn sync_notes(window: &AppWindow, state: &AppState) {
    let notes = state.note_list().unwrap_or_else(|err| {
        tracing::warn!(%err, "cannot list notes");
        Vec::new()
    });
    let count_label = state.note_count_label(notes.len());
    window.set_notes(convert::notes_to_model(&notes));
    window.set_note_count_label(count_label.as_str().into());
}

/// Move focus to a line and scroll it into view.
pub fn reveal_line(window: &AppWindow, index: usize) {
    window.invoke_reveal_line(index as i32);
    window.invoke_focus_line(index as i32);
}
