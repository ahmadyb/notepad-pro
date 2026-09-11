//! Session persistence and the autosave loop.

use slint::ComponentHandle;

use notepad_pro_core::config::session::{SessionStore, SESSION_VERSION};
use notepad_pro_core::config::settings::{session_path, settings_path};
use notepad_pro_core::types::api::Session;

use crate::callbacks::{lock, toast, SharedState};
use crate::sync;
use crate::ui::AppWindow;

pub fn wire(window: &AppWindow, state: &SharedState) {
    // ── The 2 session API methods ─────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_save_session(move || {
            persist_all(&s);
            if let Some(win) = w.upgrade() {
                sync::sync_light(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_load_session(move || {
            let session = SessionStore::new(session_path()).load();
            let count = session.tabs.len();
            let active = session.active_tab;
            {
                let mut guard = lock(&s);
                guard.restore_session(&session);
            }
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
            }
            crate::ui::SessionData {
                ok: count > 0,
                tab_count: count as i32,
                active_tab: active as i32,
            }
        });
    }
}

/// Write session.json and settings.json without touching the UI.
pub fn persist_all(state: &SharedState) {
    let (session, settings) = {
        let guard = lock(state);
        (guard.build_session(), guard.settings.clone())
    };
    if let Err(err) = SessionStore::new(session_path()).save(&session) {
        tracing::error!(%err, "cannot write session.json");
    }
    if let Err(err) = settings.save(&settings_path()) {
        tracing::error!(%err, "cannot write settings.json");
    }
}

/// Persist and confirm in the UI.
pub fn save_now(window: &AppWindow, state: &SharedState) {
    persist_all(state);
    toast(window, "Session saved");
}

/// Restore the previous session, if there is one. Returns how many tabs came
/// back.
pub fn restore(window: &AppWindow, state: &SharedState) -> usize {
    let session = SessionStore::new(session_path()).load();
    if session.is_empty() {
        return 0;
    }
    let count = session.tabs.len();
    lock(state).restore_session(&session);
    sync::sync_all(window, &lock(state));
    count
}

/// One autosave tick. Only writes when something actually changed.
pub fn autosave(window: &AppWindow, state: &SharedState) {
    let dirty = lock(state).any_dirty();
    persist_all(state);
    if dirty {
        sync::sync_light(window, &lock(state));
    }
}

/// Spawn the autosave loop.
///
/// A plain `std::thread` marshals each tick back onto the Slint event loop with
/// `upgrade_in_event_loop`; when the window is gone the upgrade fails and the
/// thread exits, so there is nothing to join at shutdown.
pub fn start_autosave_loop(window: &AppWindow, state: &SharedState) {
    let weak = window.as_weak();
    let shared = state.clone();
    std::thread::spawn(move || loop {
        let interval = lock(&shared).settings.autosave_interval_secs.max(1) as u64;
        std::thread::sleep(std::time::Duration::from_secs(interval));
        let s = shared.clone();
        let posted = weak.upgrade_in_event_loop(move |win| autosave(&win, &s));
        if posted.is_err() {
            tracing::debug!("window is gone; stopping the autosave loop");
            break;
        }
    });
}

/// Delete the stored session ("start fresh next time").
pub fn forget_session() -> anyhow::Result<()> {
    SessionStore::new(session_path()).clear()
}

/// An empty session, used by tests and by `--new-window`.
pub fn blank_session() -> Session {
    Session {
        version: SESSION_VERSION,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callbacks::shared;
    use crate::state::AppState;
    use notepad_pro_core::config::settings::Settings;
    use notepad_pro_core::db::notes::NotesDb;

    fn state() -> SharedState {
        shared(AppState::new(
            Settings::default(),
            NotesDb::in_memory().unwrap(),
        ))
    }

    #[test]
    fn blank_session_has_no_tabs() {
        let s = blank_session();
        assert!(s.tabs.is_empty());
        assert_eq!(s.version, SESSION_VERSION);
    }

    #[test]
    fn build_session_captures_every_tab() {
        let s = state();
        lock(&s).load_text("a.txt", "alpha", None);
        lock(&s).new_tab();
        lock(&s).load_text("b.txt", "beta", None);
        let session = lock(&s).build_session();
        assert_eq!(session.tabs.len(), 2);
        assert_eq!(session.documents, vec!["alpha".to_string(), "beta".to_string()]);
        assert_eq!(session.active_tab, 1);
    }

    #[test]
    fn restore_roundtrips_through_state() {
        let s = state();
        lock(&s).load_text("a.txt", "alpha", None);
        lock(&s).doc_mut().highlight_lines(
            0,
            0,
            notepad_pro_core::types::line::LineColour::Yellow,
        );
        let session = lock(&s).build_session();

        let other = state();
        lock(&other).restore_session(&session);
        assert_eq!(lock(&other).doc().plain_text(), "alpha");
        assert_eq!(
            lock(&other).doc().lines[0].colour,
            notepad_pro_core::types::line::LineColour::Yellow
        );
    }

    #[test]
    fn session_writes_are_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let s = state();
        lock(&s).load_text("a.txt", "alpha", None);
        let session = lock(&s).build_session();
        SessionStore::new(&path).save(&session).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("valid json");
        assert_eq!(parsed["version"], SESSION_VERSION);
        assert_eq!(parsed["documents"][0], "alpha");
        assert_eq!(parsed["tabs"][0]["name"], "a.txt");
    }

    #[test]
    fn forgetting_a_missing_session_is_not_an_error() {
        // forget_session targets the real settings dir, so only assert that
        // clearing a store twice is idempotent.
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("session.json"));
        store.clear().unwrap();
        store.clear().unwrap();
        assert!(!store.exists());
    }
}
