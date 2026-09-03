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
    let mut fresh =
        convert::lines_vec(&doc.lines, &state.palette, state.cursor.line, &state.find);
    // Longest line (in chars) drives the wrap-off horizontal viewport.
    let max_len = doc
        .lines
        .iter()
        .map(|l| l.text.chars().count())
        .max()
        .unwrap_or(0);
    window.set_max_line_len(max_len as i32);

    // Overlay geometry from renderer-measured metrics, then the row model.
    let geom = compute_geom(window, state);
    for (row, g) in fresh.iter_mut().zip(geom.iter()) {
        row.y_pos = g.0;
        row.band_h = g.1;
    }
    update_lines(window, fresh);

    // Cursor-line wash.
    if let Some(g) = geom.get(state.cursor.line) {
        window.set_cursor_y(g.0);
        window.set_cursor_h(g.1);
    }

    // Document surface (two-way binding): push only when Rust changed the
    // text (file open, undo, replace-all) so the native caret survives
    // ordinary typing.
    let doc_text = doc
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if window.get_doc_text().to_string() != doc_text {
        window.set_doc_text(doc_text.as_str().into());
    }
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

/// Per-line overlay geometry: `(y, height)` in logical px. The line pitch is
/// renderer-measured (two-line ruler in the editor); with wrapping on, each
/// line's visual-line count comes from a greedy word-wrap simulation that
/// mirrors the renderer: tokens pack onto a visual line until the next token
/// (plus its space) no longer fits, and overlong tokens break mid-word. The
/// available width is exactly the TextInput's: the content width minus the
/// input's x inset (`gutter + zoom * 16px` in editor.slint).
pub fn compute_geom(window: &AppWindow, state: &AppState) -> Vec<(f32, f32)> {
    let pitch = window.get_line_pitch().max(1.0);
    let char_w = window.get_editor_char_w();
    let view_w = window.get_editor_view_w();
    let zoom = state.settings.zoom;
    let avail_chars = if state.settings.word_wrap && view_w > 1.0 && char_w > 0.1 {
        ((view_w - input_x(zoom)) / char_w).max(1.0)
    } else {
        0.0
    };
    let mut out = Vec::with_capacity(state.doc().lines.len());
    let mut y = 0.0f32;
    for line in &state.doc().lines {
        let vis = if avail_chars > 1.0 {
            wrapped_visual_lines(&line.text, avail_chars)
        } else {
            1.0
        };
        let h = vis * pitch;
        out.push((y, h));
        y += h;
    }
    out
}

/// The TextInput's x inset in logical px; must mirror `editor.slint`.
pub fn input_x(zoom: f32) -> f32 {
    10.0 + zoom * 16.0
}

/// Greedy word-wrap visual-line count with a uniform monospace advance.
///
/// Mirrors the renderer's `word-wrap`: a token moves to the next visual line
/// when it no longer fits (spaces are break opportunities and consume one
/// cell); a token longer than a whole line breaks mid-word.
pub fn wrapped_visual_lines(text: &str, avail: f32) -> f32 {
    if avail <= 1.0 {
        return 1.0;
    }
    let mut lines = 1.0f32;
    let mut used = 0.0f32;
    for (i, token) in text.split(' ').enumerate() {
        let mut w = token.chars().count() as f32;
        if i > 0 && w > 0.0 {
            // The space is a break opportunity: if space+token do not fit,
            // the token starts a fresh visual line (without the space).
            if used > 0.0 && used + 1.0 + w > avail {
                lines += 1.0;
                used = 0.0;
            } else {
                used += 1.0;
            }
        }
        loop {
            let rem = avail - used;
            if w <= rem + 1e-3 {
                used += w;
                break;
            }
            if used > 1e-3 {
                // Continuation of an overlong token wraps to a new line.
                lines += 1.0;
                used = 0.0;
            } else {
                // Token longer than a full line: break mid-word.
                w -= rem;
                lines += 1.0;
                used = 0.0;
            }
        }
    }
    lines
}

/// Character column at pixel `x` inside visual row `row_within` of `text`,
/// using the same greedy packing as [`wrapped_visual_lines`]. Returns the
/// UTF-8 *character* index (not byte index) into the line.
pub fn col_at_point(text: &str, avail: f32, row_within: usize, x_chars: f32) -> usize {
    let mut row = 0usize;
    let mut row_start = 0usize;
    let mut prev_row_start = 0usize;
    let mut used = 0.0f32;
    let mut chars = 0usize;
    for (i, token) in text.split(' ').enumerate() {
        let tw = token.chars().count();
        if i > 0 && tw > 0 {
            if used > 0.0 && used + 1.0 + tw as f32 > avail {
                // The space stays at the (invisible) end of the previous row;
                // the new visual row starts at the token itself.
                prev_row_start = row_start;
                row += 1;
                row_start = chars + 1;
                chars += 1;
                used = 0.0;
            } else {
                used += 1.0;
                chars += 1; // the space itself
            }
        }
        let mut placed = 0usize;
        while placed < tw {
            let rem = (avail - used) as usize;
            if tw - placed <= rem {
                used += (tw - placed) as f32;
                chars += tw - placed;
                placed = tw;
            } else if used > 1e-3 {
                prev_row_start = row_start;
                row += 1;
                row_start = chars;
                used = 0.0;
            } else {
                placed += rem;
                chars += rem;
                prev_row_start = row_start;
                row += 1;
                row_start = chars;
                used = 0.0;
            }
        }
        if row > row_within {
            break;
        }
    }
    let base = if row == row_within {
        row_start
    } else if row > row_within {
        // Walked past the target row: it started where the next one took over.
        prev_row_start
    } else {
        // x past the last visual row: clamp to the line end.
        return text.chars().count();
    };
    let in_row = x_chars.round().max(0.0) as usize;
    (base + in_row).min(text.chars().count())
}

/// Reconciles `fresh` against the live row model strictly in place.
///
/// The editor rows are READ-ONLY `TextInput`s: their `text:` bindings never
/// die, so the model is the single source of truth and every change —
/// including line splits and joins — can be applied with `set_row_data` /
/// `push` / `remove` without ever recreating a row. Repeated-row components
/// survive, and with them the focused row's keyboard focus: typing and
/// Enter continue uninterrupted, exactly like a textarea. (Replacing the
/// whole model would make the repeater destroy and recreate every row,
/// dropping focus — Slint 1.6 cannot re-focus programmatically.)
fn update_lines(window: &AppWindow, fresh: Vec<EditorLineData>) {
    let current = window.get_lines();
    let Some(vm) = current.as_any().downcast_ref::<VecModel<EditorLineData>>() else {
        // First sync (or a model we did not build): install it wholesale.
        window.set_lines(ModelRc::from(std::rc::Rc::new(VecModel::from(fresh))));
        return;
    };

    let shared = vm.row_count().min(fresh.len());
    for (i, row) in fresh.iter().take(shared).enumerate() {
        let Some(cur) = vm.row_data(i) else { continue };
        if cur != *row {
            vm.set_row_data(i, row.clone());
        }
    }
    for row in fresh.iter().skip(shared) {
        vm.push(row.clone());
    }
    while vm.row_count() > fresh.len() {
        vm.remove(vm.row_count() - 1);
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

#[cfg(test)]
mod geom_tests {
    use super::*;

    #[test]
    fn short_line_is_one_visual_line() {
        assert_eq!(wrapped_visual_lines("hello world", 80.0), 1.0);
    }

    #[test]
    fn words_that_do_not_fit_wrap_to_new_rows() {
        // avail 10: "abcde abcde abcde" packs one token per visual row.
        assert_eq!(wrapped_visual_lines("abcde abcde abcde", 10.0), 3.0);
    }

    #[test]
    fn exact_fit_stays_on_one_line() {
        assert_eq!(wrapped_visual_lines("abcde abcde", 11.0), 1.0);
    }

    #[test]
    fn overlong_token_breaks_mid_word() {
        // 10 chars over avail 4 -> rows of 4 + 4 + 2.
        assert_eq!(wrapped_visual_lines("abcdefghij", 4.0), 3.0);
    }

    #[test]
    fn empty_line_is_one_row() {
        assert_eq!(wrapped_visual_lines("", 80.0), 1.0);
    }

    #[test]
    fn col_at_point_maps_second_visual_row_past_the_space() {
        let text = "abcde abcde abcde";
        assert_eq!(col_at_point(text, 10.0, 1, 0.0), 6);
        assert_eq!(col_at_point(text, 10.0, 0, 3.0), 3);
        // Beyond the last row: clamp to the line end.
        assert_eq!(col_at_point(text, 10.0, 7, 0.0), text.chars().count());
    }
}
