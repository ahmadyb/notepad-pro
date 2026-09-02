//! Notes sidebar callbacks (SQLite-backed).

use slint::ComponentHandle;

use crate::callbacks::{lock, toast, SharedState};
use crate::convert;
use crate::state::PendingAction;
use crate::dialogs;
use crate::sync;
use crate::ui::AppWindow;

pub fn wire(window: &AppWindow, state: &SharedState) {
    // ── The 5 notes API methods ───────────────────────────────────────────

    {
        let s = state.clone();
        window.on_get_notes_list(move |query| {
            let guard = lock(&s);
            let notes = guard
                .db
                .list(
                    query.as_str(),
                    notepad_pro_core::db::notes::SortOrder::from_key(&guard.settings.sidebar_sort),
                )
                .unwrap_or_else(|err| {
                    tracing::warn!(%err, "cannot list notes");
                    Vec::new()
                });
            convert::notes_to_model(&notes)
        });
    }

    {
        let s = state.clone();
        window.on_get_note(move |id: i32| {
            let guard = lock(&s);
            match guard.db.get(id as i64) {
                Ok(Some(note)) => convert::note_to_ui_full(&note),
                Ok(None) => convert::note_to_ui_full(&notepad_pro_core::types::note::Note::default()),
                Err(err) => {
                    tracing::warn!(%err, "cannot read note {id}");
                    convert::note_to_ui_full(&notepad_pro_core::types::note::Note::default())
                }
            }
        });
    }

    {
        let s = state.clone();
        window.on_save_note(move |note| {
            let mut guard = lock(&s);
            let record = convert::ui_to_note(&note);
            match guard.db.save(&record) {
                Ok(id) => {
                    guard.selected_note_id = id;
                    id as i32
                }
                Err(err) => {
                    tracing::error!(%err, "cannot save note");
                    -1
                }
            }
        });
    }

    {
        let s = state.clone();
        window.on_delete_note(move |id: i32| {
            let mut guard = lock(&s);
            match guard.db.delete(id as i64) {
                Ok(_) => {
                    if guard.selected_note_id == id as i64 {
                        guard.selected_note_id = -1;
                    }
                }
                Err(err) => tracing::error!(%err, "cannot delete note {id}"),
            }
        });
    }

    {
        let s = state.clone();
        window.on_toggle_pin(move |id: i32| {
            let mut guard = lock(&s);
            guard.toggle_pin(id as i64).unwrap_or(false)
        });
    }

    // ── Sidebar UI actions ────────────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_note_query_changed(move |query| {
            lock(&s).note_query = query.to_string();
            if let Some(win) = w.upgrade() {
                sync::sync_notes(&win, &lock(&s));
                sync::sync_flags(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_note_sort_changed(move |sort| {
            {
                let mut guard = lock(&s);
                guard.settings.sidebar_sort = sort.to_string();
                guard.settings.clamp();
            }
            if let Some(win) = w.upgrade() {
                sync::sync_notes(&win, &lock(&s));
                sync::sync_flags(&win, &lock(&s));
                let _ = crate::callbacks::settings_cb::persist(&win, &s);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_note_chosen(move |id: i32| {
            let Some(win) = w.upgrade() else { return };
            let result = lock(&s).open_note(id as i64);
            match result {
                Ok(()) => sync::sync_all(&win, &lock(&s)),
                Err(err) => toast(&win, &format!("{err}")),
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_note_pinned(move |id: i32| {
            let Some(win) = w.upgrade() else { return };
            let pinned = {
                let mut guard = lock(&s);
                guard.toggle_pin(id as i64)
            };
            match pinned {
                Ok(true) => toast(&win, "Note pinned"),
                Ok(false) => toast(&win, "Note unpinned"),
                Err(err) => toast(&win, &format!("{err}")),
            }
            sync::sync_notes(&win, &lock(&s));
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_note_deleted(move |id: i32| {
            let Some(win) = w.upgrade() else { return };
            let mut guard = lock(&s);
            dialogs::confirm_dialog::ask(&win, &mut guard, PendingAction::DeleteNote(id as i64));
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_note_new(move || {
            let w2 = w.clone();
            let s2 = s.clone();
            let s3 = s.clone();
            // The SQLite insert runs on a worker thread and the sidebar is
            // re-synced back on the event loop — "+ New" must never freeze
            // the window, even if the notes database is momentarily busy.
            dialogs::file_dialog::run_pick_async(
                move || lock(&s3).new_note().map_err(|e| e.to_string()),
                move |result: std::result::Result<i64, String>| {
                    if let Some(win) = w2.upgrade() {
                        match result {
                            Ok(_) => {
                                sync::sync_notes(&win, &lock(&s2));
                                sync::sync_flags(&win, &lock(&s2));
                            }
                            Err(err) => toast(&win, &format!("Cannot create note: {err}")),
                        }
                    }
                },
            );
        });
    }

    // ── Confirm dialog outcomes ───────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_confirm_accepted(move || {
            let Some(win) = w.upgrade() else { return };
            let Some(action) = dialogs::confirm_dialog::take(&win, &mut lock(&s)) else {
                return;
            };
            run_pending(&win, &s, action);
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_confirm_rejected(move || {
            if let Some(win) = w.upgrade() {
                dialogs::confirm_dialog::dismiss(&win, &mut lock(&s));
            }
        });
    }
}

/// Execute a confirmed action.
pub fn run_pending(window: &AppWindow, state: &SharedState, action: PendingAction) {
    match action {
        PendingAction::CloseTab(index) => {
            lock(state).close_tab(index);
            sync::sync_all(window, &lock(state));
        }
        PendingAction::CloseApp => {
            // Persist first so nothing is lost.
            crate::callbacks::session_cb::save_now(window, state);
            slint::quit_event_loop().ok();
        }
        PendingAction::DeleteNote(id) => {
            let removed = lock(state).delete_note(id).unwrap_or(false);
            sync::sync_notes(window, &lock(state));
            toast(
                window,
                if removed {
                    "Note deleted"
                } else {
                    "Note was already gone"
                },
            );
        }
        PendingAction::OverwriteFile(path) => {
            match lock(state).save_to(&path) {
                Ok(()) => {
                    sync::sync_light(window, &lock(state));
                    toast(window, &format!("Overwrote {}", path.display()));
                }
                Err(err) => toast(window, &format!("Save failed: {err}")),
            }
        }
        PendingAction::RevertTab(index) => {
            let note_id = lock(state)
                .tabs
                .get(index)
                .and_then(|t| t.state.note_id);
            match note_id {
                Some(id) => match lock(state).open_note(id) {
                    Ok(()) => sync::sync_all(window, &lock(state)),
                    Err(err) => toast(window, &format!("{err}")),
                },
                None => toast(window, "That tab is not linked to a note"),
            }
        }
    }
}

/// Save the active document into the notes store (Ctrl+Shift+N).
pub fn save_active_to_notes(window: &AppWindow, state: &SharedState) {
    match lock(state).save_active_as_note() {
        Ok(id) => {
            sync::sync_notes(window, &lock(state));
            sync::sync_flags(window, &lock(state));
            toast(window, &format!("Saved as note #{id}"));
        }
        Err(err) => toast(window, &format!("Cannot save note: {err}")),
    }
}
