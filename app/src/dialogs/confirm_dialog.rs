//! The in-app confirm modal.
//!
//! Rust stores the pending action in [`AppState::pending`]; the dialog is just
//! a view of it. Accepting invokes [`crate::state::PendingAction`] handling in
//! the caller.

use crate::state::{AppState, PendingAction};
use crate::ui::AppWindow;

/// Ask for confirmation. Returns `false` when the action needs no prompt.
pub fn ask(window: &AppWindow, state: &mut AppState, action: PendingAction) -> bool {
    let (title, message, label, destructive) = describe(&action, state);
    state.pending = Some(action);
    window.set_confirm_title(title.as_str().into());
    window.set_confirm_message(message.as_str().into());
    window.set_confirm_label(label.as_str().into());
    window.set_confirm_destructive(destructive);
    window.set_show_confirm(true);
    true
}

/// Resolve the pending action and clear it.
pub fn take(window: &AppWindow, state: &mut AppState) -> Option<PendingAction> {
    let pending = state.pending.take();
    window.set_show_confirm(false);
    pending
}

/// Dismiss without running anything.
pub fn dismiss(window: &AppWindow, state: &mut AppState) {
    state.pending = None;
    window.set_show_confirm(false);
}

fn describe(action: &PendingAction, state: &AppState) -> (String, String, String, bool) {
    match action {
        PendingAction::CloseTab(index) => {
            let name = state
                .tabs
                .get(*index)
                .map(|t| t.state.name.clone())
                .unwrap_or_else(|| "this tab".to_string());
            (
                "Discard unsaved changes?".to_string(),
                format!("{name} has unsaved changes. Close it anyway?"),
                "Discard".to_string(),
                true,
            )
        }
        PendingAction::CloseApp => (
            "Quit NotePad Pro?".to_string(),
            "Some tabs have unsaved changes. Quit anyway?".to_string(),
            "Quit".to_string(),
            true,
        ),
        PendingAction::DeleteNote(id) => (
            "Delete note?".to_string(),
            format!("Note #{id} will be removed from the database. This cannot be undone."),
            "Delete".to_string(),
            true,
        ),
        PendingAction::OverwriteFile(path) => (
            "File changed on disk".to_string(),
            format!("{}\n\nOverwrite it with your version?", path.display()),
            "Overwrite".to_string(),
            true,
        ),
        PendingAction::RevertTab(index) => {
            let name = state
                .tabs
                .get(*index)
                .map(|t| t.state.name.clone())
                .unwrap_or_else(|| "this tab".to_string());
            (
                "Discard unsaved changes?".to_string(),
                format!("{name} will be reloaded from the stored note."),
                "Discard".to_string(),
                true,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notepad_pro_core::config::settings::Settings;
    use notepad_pro_core::db::notes::NotesDb;

    fn state() -> AppState {
        AppState::new(Settings::default(), NotesDb::in_memory().unwrap())
    }

    #[test]
    fn close_tab_message_names_the_tab() {
        let mut s = state();
        s.load_text("report.txt", "x", None);
        let (title, message, label, destructive) = describe(&PendingAction::CloseTab(0), &s);
        assert_eq!(title, "Discard unsaved changes?");
        assert!(message.contains("report.txt"));
        assert_eq!(label, "Discard");
        assert!(destructive);
    }

    #[test]
    fn missing_tab_index_falls_back_to_generic_wording() {
        let s = state();
        let (_, message, _, _) = describe(&PendingAction::CloseTab(99), &s);
        assert!(message.contains("this tab"));
    }

    #[test]
    fn every_action_has_copy() {
        let s = state();
        for action in [
            PendingAction::CloseApp,
            PendingAction::DeleteNote(1),
            PendingAction::OverwriteFile("/tmp/a".into()),
            PendingAction::RevertTab(0),
        ] {
            let (title, message, label, _) = describe(&action, &s);
            assert!(!title.is_empty());
            assert!(!message.is_empty());
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn delete_message_includes_the_note_id() {
        let s = state();
        let (_, message, _, _) = describe(&PendingAction::DeleteNote(42), &s);
        assert!(message.contains("#42"));
    }
}
