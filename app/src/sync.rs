//! Pushes [`AppState`] into the window's Slint properties.
//!
//! Split into small functions so a callback only re-syncs what it changed.
//! `sync_editor` is the expensive one (it rebuilds the line model and so
//! recreates every row), so the typing path deliberately avoids it.

use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::convert;
use crate::state::AppState;
use crate::ui::{AppWindow, EditorLineData};

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
    update_lines(
        window,
        convert::lines_vec(&doc.lines, &state.palette, state.cursor.line, &state.find),
    );
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

/// Reconciles `fresh` against the live row model instead of replacing it.
///
/// Replacing the model (`set_lines`) makes the repeater destroy and recreate
/// every row: the focused `TextInput` dies, Slint 1.6 cannot re-focus it
/// (`has-focus` is output-only, no `focus()`), and the user has to click
/// again before typing continues. So when the line count is unchanged we
/// mutate the existing `VecModel` in place and keep unchanged rows — with
/// their focus and caret — alive:
/// * identical rows are skipped entirely;
/// * rows whose colour/marker/flags changed get `set_row_data` (bindings
///   stay alive, text and caret untouched);
/// * rows whose **text** changed are removed + re-inserted, because native
///   editing kills the one-way `text:` binding and a recreated row is the
///   only way to display the new text.
///
/// When the line count *does* change (Enter split, backspace join, paste,
/// file load) the row structure has shifted and every `TextInput` after the
/// edit holds stale text, so there is nothing to reconcile — we replace the
/// whole model, which is the correct (if focus-dropping) rebuild.
fn update_lines(window: &AppWindow, fresh: Vec<EditorLineData>) {
    let install = |window: &AppWindow, fresh: Vec<EditorLineData>| {
        window.set_lines(ModelRc::from(std::rc::Rc::new(VecModel::from(fresh))));
    };

    let current = window.get_lines();
    let Some(vm) = current.as_any().downcast_ref::<VecModel<EditorLineData>>() else {
        // First sync (or a model we did not build): install it wholesale.
        return install(window, fresh);
    };

    if vm.row_count() != fresh.len() {
        // Line count changed — rebuild rather than reconcile (see above).
        return install(window, fresh);
    }

    // Same count: reconcile in place.
    for (i, row) in fresh.iter().enumerate() {
        let Some(cur) = vm.row_data(i) else { continue };
        if cur == *row {
            continue;
        }
        if cur.text == row.text {
            vm.set_row_data(i, row.clone());
        } else {
            vm.remove(i);
            vm.insert(i, row.clone());
        }
    }
}

/// Pushes a single row into the live model. The typing callback uses this to
/// keep the focused row's model data equal to the document between full
/// syncs, so a later reconcile sees no difference and does not recreate the
/// row (which would drop focus mid-typing).
pub fn sync_editor_row(window: &AppWindow, state: &AppState, index: usize) {
    let Some(row) = convert::line_row(
        &state.doc().lines,
        &state.palette,
        state.cursor.line,
        &state.find,
        index,
    ) else {
        return;
    };
    if let Some(vm) = window
        .get_lines()
        .as_any()
        .downcast_ref::<VecModel<EditorLineData>>()
    {
        if index < vm.row_count() {
            vm.set_row_data(index, row);
        }
    }
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

/// Give keyboard focus to one editor row. The token bump makes the
/// `has-focus` binding on the target row re-evaluate even when the same
/// row is requested twice in a row.
pub fn focus_line(window: &AppWindow, index: usize) {
    window.set_editor_focus_line(index as i32);
    let token = window.get_editor_focus_token();
    window.set_editor_focus_token(token + 1);
}
