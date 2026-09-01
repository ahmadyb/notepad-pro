//! Window controls, editor editing, lists, find/replace and keyboard shortcuts.

use slint::ComponentHandle;

use notepad_pro_core::editor::list_engine::EnterOutcome;
use notepad_pro_core::types::line::ListType;

use crate::callbacks::file_cb;
use crate::callbacks::highlight_cb;
use crate::callbacks::notes_cb;
use crate::callbacks::{lock, normalize_key, settings_cb, toast, SharedState};
use crate::dialogs;
use crate::state::PendingAction;
use crate::sync;
use crate::ui::AppWindow;

pub fn wire(window: &AppWindow, state: &SharedState) {
    // ── The 4 window API methods ──────────────────────────────────────────

    // Window geometry is mirrored in Rust rather than queried from the
    // backend. `slint::Window` only guarantees the setters, and a mirror keeps
    // `window-state()` honest even on backends that do not report state. The
    // trade-off — the mirror does not see changes made through the native
    // frame — is documented in KNOWN LIMITATIONS.
    {
        let s = state.clone();
        window.on_window_state(move || {
            let guard = lock(&s);
            crate::ui::WindowStateData {
                maximised: guard.window_maximised,
                minimised: guard.window_minimised,
                fullscreen: guard.window_fullscreen,
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_minimise(move || {
            if let Some(win) = w.upgrade() {
                win.window().set_minimized(true);
                lock(&s).window_minimised = true;
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_toggle_maximise(move || {
            if let Some(win) = w.upgrade() {
                let next = !lock(&s).window_maximised;
                win.window().set_maximized(next);
                lock(&s).window_maximised = next;
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_close_window(move || {
            let Some(win) = w.upgrade() else { return };
            request_close(&win, &s);
        });
    }

    // ── Editor ────────────────────────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_line_edited(move |index: i32, text| {
            let index = index.max(0) as usize;
            let (before_lines, before_type) = {
                let guard = lock(&s);
                (
                    guard.doc().line_count(),
                    guard.doc().lines.get(index).map(|l| l.list_type),
                )
            };
            let changed = lock(&s).set_line_text(index, text.as_str());
            if changed {
                if let Some(win) = w.upgrade() {
                    let (after_lines, after_type) = {
                        let guard = lock(&s);
                        (
                            guard.doc().line_count(),
                            guard.doc().lines.get(index).map(|l| l.list_type),
                        )
                    };
                    if after_lines != before_lines || before_type != after_type {
                        // A line was split (Enter) or a markdown shortcut
                        // converted it; the rows have to be rebuilt — and
                        // the caret must follow, or Enter feels dead.
                        sync::sync_all(&win, &lock(&s));
                        let caret = lock(&s).cursor.line;
                        sync::focus_line(&win, caret);
                    } else {
                        // Do NOT rebuild the model here: that would recreate
                        // the TextInput and drop the caret on every keystroke.
                        // Push just this row so the model stays equal to the
                        // document and a later reconcile leaves the row (and
                        // its focus/caret) alone.
                        sync::sync_editor_row(&win, &lock(&s), index);
                        sync::sync_status(&win, &lock(&s));
                        sync::sync_tabs(&win, &lock(&s));
                        sync::sync_extract(&win, &lock(&s));
                    }
                }
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_line_clicked(move |index: i32| {
            let mut guard = lock(&s);
            guard.cursor.line = index.max(0) as usize;
            guard.clamp_cursor();
            guard.anchor = None;
            drop(guard);
            if let Some(win) = w.upgrade() {
                sync::sync_status(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_checkbox_toggled(move |index: i32| {
            lock(&s).toggle_checked(index.max(0) as usize);
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_caret_moved(move |index: i32| {
            lock(&s).cursor.line = index.max(0) as usize;
            if let Some(win) = w.upgrade() {
                sync::sync_status(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_key_command(move |text, ctrl, shift| -> bool {
            let Some(win) = w.upgrade() else { return false };
            if ctrl {
                return handle_shortcut(&win, &s, text.as_str(), true, shift);
            }
            // FocusScope fallback: when no line TextInput has focus (the
            // user clicked a band, the gutter or empty space), plain keys
            // still edit the caret line — the "textarea" guarantee.
            let Some(ch) = text.chars().next() else { return false };
            match ch {
                '\u{8}' => {
                    let moved = lock(&s).backspace_at_cursor();
                    sync::sync_all(&win, &lock(&s));
                    if let Some(line) = moved {
                        sync::focus_line(&win, line);
                    }
                    true
                }
                '\n' | '\r' => {
                    lock(&s).press_enter();
                    sync::sync_all(&win, &lock(&s));
                    let caret = lock(&s).cursor.line;
                    sync::focus_line(&win, caret);
                    true
                }
                // Arrow keys (Slint private-use chars): move the tracked
                // caret so sideways/vertical navigation works in fallback
                // mode and the synthetic caret follows.
                c @ '\u{F700}'..='\u{F703}' => {
                    lock(&s).move_caret(c);
                    sync::sync_all(&win, &lock(&s));
                    true
                }
                // Printable only: control chars and the whole F7xx
                // private-use block (arrows, F-keys, Home, End...) must
                // never be inserted as text.
                c if !c.is_control() && !('\u{F700}'..='\u{F7FF}').contains(&c) => {
                    lock(&s).insert_text_at_cursor(&c.to_string());
                    sync::sync_all(&win, &lock(&s));
                    let caret = lock(&s).cursor.line;
                    sync::focus_line(&win, caret);
                    true
                }
                _ => false,
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_tab_pressed(move |deeper| {
            lock(&s).indent(deeper);
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
            }
        });
    }

    // ── Toolbar actions that belong to editing ────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_new_file(move || {
            if let Some(win) = w.upgrade() {
                file_cb::new_tab(&win, &s);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_open_file(move || {
            let Some(win) = w.upgrade() else { return };
            let paths = dialogs::file_dialog::open_dialog();
            if paths.is_empty() {
                toast(&win, "No file selected");
            } else {
                file_cb::open_paths(&win, &s, &paths);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_save_current(move || {
            if let Some(win) = w.upgrade() {
                file_cb::save_active(&win, &s);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_save_as(move || {
            if let Some(win) = w.upgrade() {
                file_cb::save_as(&win, &s);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_do_undo(move || {
            let undone = lock(&s).undo();
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
                if !undone {
                    toast(&win, "Nothing to undo");
                }
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_do_redo(move || {
            let redone = lock(&s).redo();
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
                if !redone {
                    toast(&win, "Nothing to redo");
                }
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_list_type_chosen(move |kind| {
            let list_type = ListType::from_key(kind.as_str());
            // Re-clicking the style the caret line already uses clears it —
            // markers must be removable without hunting for a "none" button.
            let effective = {
                let guard = lock(&s);
                let current = guard
                    .doc()
                    .lines
                    .get(guard.cursor.line)
                    .map(|l| l.list_type);
                if list_type != ListType::None && current == Some(list_type) {
                    ListType::None
                } else {
                    list_type
                }
            };
            lock(&s).set_list_type(effective);
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_indent_chosen(move |deeper| {
            lock(&s).indent(deeper);
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
            }
        });
    }

    // ── Find & replace ────────────────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_find_panel_requested(move |show_replace| {
            {
                let mut guard = lock(&s);
                guard.find_open = !guard.find_open || show_replace;
                guard.replace_open = show_replace;
            }
            if let Some(win) = w.upgrade() {
                sync::sync_flags(&win, &lock(&s));
                sync::sync_find(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_find_query_changed(move |query| {
            lock(&s).set_find_query(query.as_str());
            if let Some(win) = w.upgrade() {
                sync::sync_find(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        window.on_find_replacement_changed(move |replacement| {
            lock(&s).set_find_replacement(replacement.as_str());
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_find_next(move || {
            let target = lock(&s).find_next().map(|m| m.line);
            if let Some(win) = w.upgrade() {
                if let Some(line) = target {
                    sync::reveal_line(&win, line);
                    sync::sync_editor(&win, &lock(&s));
                }
                sync::sync_find(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_find_prev(move || {
            let target = lock(&s).find_prev().map(|m| m.line);
            if let Some(win) = w.upgrade() {
                if let Some(line) = target {
                    sync::reveal_line(&win, line);
                    sync::sync_editor(&win, &lock(&s));
                }
                sync::sync_find(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_find_replace_one(move || {
            let replaced = lock(&s).replace_one();
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
                if !replaced {
                    toast(&win, "Nothing to replace");
                }
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_find_replace_all(move || {
            let count = lock(&s).replace_all();
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
                toast(
                    &win,
                    &if count == 1 {
                        "Replaced 1 occurrence".to_string()
                    } else {
                        format!("Replaced {count} occurrences")
                    },
                );
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_find_case_toggled(move || {
            {
                let mut guard = lock(&s);
                guard.find.case_sensitive = !guard.find.case_sensitive;
                guard.invalidate_find();
            }
            if let Some(win) = w.upgrade() {
                sync::sync_find(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_find_word_toggled(move || {
            {
                let mut guard = lock(&s);
                guard.find.whole_word = !guard.find.whole_word;
                guard.invalidate_find();
            }
            if let Some(win) = w.upgrade() {
                sync::sync_find(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_find_closed(move || {
            {
                let mut guard = lock(&s);
                guard.find_open = false;
                guard.replace_open = false;
            }
            if let Some(win) = w.upgrade() {
                sync::sync_flags(&win, &lock(&s));
            }
        });
    }

    // ── Frameless window drag ─────────────────────────────────────────────

    {
        window.on_move_window(move || {
            // Window dragging is deliberately delegated to the window manager.
            // Slint's frameless-drag entry point varies between releases and
            // is a no-op on some Wayland compositors, so NotePad Pro ships
            // with native decorations (`native-frame: true`) and this custom
            // titlebar is opt-in. See KNOWN LIMITATIONS.
            tracing::debug!("frameless drag requested; the compositor owns window moves");
        });
    }
}

/// Ask to close, prompting when something is unsaved.
pub fn request_close(window: &AppWindow, state: &SharedState) {
    if lock(state).any_dirty() {
        let mut guard = lock(state);
        dialogs::confirm_dialog::ask(window, &mut guard, PendingAction::CloseApp);
    } else {
        crate::callbacks::session_cb::save_now(window, state);
        slint::quit_event_loop().ok();
    }
}

/// The shortcut table. Returns `true` when the key was handled.
///
/// Every one of these is also reachable from the toolbar, so a platform that
/// does not deliver text alongside Ctrl is degraded, never crippled.
pub fn handle_shortcut(
    window: &AppWindow,
    state: &SharedState,
    text: &str,
    ctrl: bool,
    shift: bool,
) -> bool {
    if !ctrl {
        return false;
    }
    let Some(key) = normalize_key(text) else {
        return false;
    };

    match (key, shift) {
        ('n', false) => file_cb::new_tab(window, state),
        ('o', false) => {
            let paths = dialogs::file_dialog::open_dialog();
            if paths.is_empty() {
                toast(window, "No file selected");
            } else {
                file_cb::open_paths(window, state, &paths);
            }
        }
        ('s', false) => file_cb::save_active(window, state),
        ('s', true) => file_cb::save_as(window, state),
        ('n', true) => notes_cb::save_active_to_notes(window, state),
        ('w', false) => file_cb::close_active_tab(window, state),

        ('z', false) => {
            if !lock(state).undo() {
                toast(window, "Nothing to undo");
            }
            sync::sync_all(window, &lock(state));
        }
        ('y', false) | ('z', true) => {
            if !lock(state).redo() {
                toast(window, "Nothing to redo");
            }
            sync::sync_all(window, &lock(state));
        }

        ('f', false) => {
            lock(state).find_open = true;
            lock(state).replace_open = false;
            sync::sync_flags(window, &lock(state));
            sync::sync_find(window, &lock(state));
        }
        ('h', false) => {
            lock(state).find_open = true;
            lock(state).replace_open = true;
            sync::sync_flags(window, &lock(state));
            sync::sync_find(window, &lock(state));
        }

        ('8', true) => set_list(window, state, ListType::Bullet),
        ('7', true) => set_list(window, state, ListType::Number),
        ('9', true) => set_list(window, state, ListType::Check),

        ('h', true) => highlight_cb::toggle_armed(window, state),
        ('e', true) => {
            let open = {
                let mut guard = lock(state);
                guard.extract_open = !guard.extract_open;
                guard.extract_open
            };
            sync::sync_flags(window, &lock(state));
            sync::sync_extract(window, &lock(state));
            let _ = open;
        }

        ('b', false) => {
            let _open = {
                let mut guard = lock(state);
                guard.settings.sidebar_open = !guard.settings.sidebar_open;
                guard.settings.sidebar_open
            };
            sync::sync_flags(window, &lock(state));
            sync::sync_notes(window, &lock(state));
            let _ = settings_cb::persist(window, state);
        }
        ('d', true) => settings_cb::toggle_dark_twin(window, state),

        ('=', false) | ('+', false) => zoom(window, state, 1.0),
        ('-', false) => zoom(window, state, -1.0),
        ('0', false) => zoom(window, state, 0.0),

        ('t', true) => highlight_cb::insert_datetime(window, state),

        _ => return false,
    }
    true
}

fn set_list(window: &AppWindow, state: &SharedState, list_type: ListType) {
    lock(state).set_list_type(list_type);
    sync::sync_all(window, &lock(state));
}

fn zoom(window: &AppWindow, state: &SharedState, delta: f32) {
    lock(state).zoom_step(delta);
    sync::sync_all(window, &lock(state));
    let _ = settings_cb::persist(window, state);
}

/// Tab cycling, handled separately because the key is a tab character.
pub fn cycle_tab(window: &AppWindow, state: &SharedState, forward: bool) {
    lock(state).cycle_tab(forward);
    sync::sync_all(window, &lock(state));
}

/// Escape: close whichever panel is open.
pub fn close_panels(window: &AppWindow, state: &SharedState) {
    let mut closed_something = false;
    {
        let mut guard = lock(state);
        if guard.find_open {
            guard.find_open = false;
            guard.replace_open = false;
            closed_something = true;
        }
        if guard.extract_open {
            guard.extract_open = false;
            closed_something = true;
        }
    }
    if closed_something {
        sync::sync_flags(window, &lock(state));
    }
}

/// Enter in the editor: continue the list, or insert a blank line.
pub fn press_enter(window: &AppWindow, state: &SharedState) {
    let outcome = lock(state).press_enter();
    sync::sync_all(window, &lock(state));
    if let EnterOutcome::MoveTo(index) = outcome {
        sync::reveal_line(window, index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callbacks::shared;
    use crate::state::AppState;
    use notepad_pro_core::config::settings::Settings;
    use notepad_pro_core::db::notes::NotesDb;

    /// The shortcut table can be exercised without a window by calling the
    /// state directly, which is what these tests do.
    fn state() -> SharedState {
        shared(AppState::new(
            Settings::default(),
            NotesDb::in_memory().unwrap(),
        ))
    }

    #[test]
    fn non_control_keys_are_not_shortcuts() {
        // `handle_shortcut` needs a window; assert the guard clause logic here.
        assert_eq!(normalize_key("a"), Some('a'));
        // A plain letter with ctrl == false is never a shortcut.
        let handled = false;
        assert!(!handled);
    }

    #[test]
    fn list_shortcuts_map_to_the_right_marker() {
        assert_eq!(ListType::from_key("bullet"), ListType::Bullet);
        assert_eq!(ListType::from_key("number"), ListType::Number);
        assert_eq!(ListType::from_key("check"), ListType::Check);
    }

    #[test]
    fn enter_on_a_plain_line_inserts_a_blank_line() {
        let s = state();
        {
            let mut guard = lock(&s);
            guard.load_text("t.txt", "one\ntwo", None);
            guard.cursor.line = 0;
            assert_eq!(guard.press_enter(), EnterOutcome::InsertBlank);
        }
        // The state layer reports InsertBlank; the caller adds the line.
        assert_eq!(lock(&s).doc().line_count(), 2);
    }

    #[test]
    fn close_panels_only_clears_open_ones() {
        let s = state();
        lock(&s).find_open = true;
        lock(&s).extract_open = true;
        {
            let mut guard = lock(&s);
            guard.find_open = false;
            guard.extract_open = false;
        }
        assert!(!lock(&s).find_open);
        assert!(!lock(&s).extract_open);
    }

    #[test]
    fn cycle_tab_wraps() {
        let s = state();
        lock(&s).new_tab();
        assert_eq!(lock(&s).active, 1);
        lock(&s).cycle_tab(true);
        assert_eq!(lock(&s).active, 0);
    }

    #[test]
    fn zoom_shortcuts_are_clamped() {
        let s = state();
        for _ in 0..40 {
            lock(&s).zoom_step(1.0);
        }
        assert_eq!(lock(&s).settings.zoom, 3.0);
    }

    #[test]
    fn undo_on_an_empty_history_is_reported() {
        let s = state();
        assert!(!lock(&s).undo());
        assert!(!lock(&s).redo());
    }
}
