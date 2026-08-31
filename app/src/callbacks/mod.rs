//! Slint callback wiring.
//!
//! Each module registers the closures for one area of the app. They are all
//! thin: they lock [`AppState`], do one thing, then re-sync the view.

pub mod file_cb;
pub mod highlight_cb;
pub mod notes_cb;
pub mod session_cb;
pub mod settings_cb;
pub mod window_cb;

use std::sync::{Arc, Mutex, MutexGuard};

use slint::ComponentHandle;

use crate::state::AppState;
use crate::ui::AppWindow;

/// The shared handle every callback closes over.
pub type SharedState = Arc<Mutex<AppState>>;

pub fn shared(state: AppState) -> SharedState {
    Arc::new(Mutex::new(state))
}

/// Lock the state, recovering from a poisoned mutex rather than panicking
/// inside a UI callback.
pub fn lock(state: &SharedState) -> MutexGuard<'_, AppState> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::error!("state mutex was poisoned; recovering");
            poisoned.into_inner()
        }
    }
}

/// Show a toast and schedule it to disappear.
///
/// A background thread marshals the hide back onto the event loop; `slint::Timer`
/// would work too but is not `Send`, and this keeps the helper usable from
/// anywhere.
pub fn toast(window: &AppWindow, message: &str) {
    window.invoke_show_toast(message.into());
    let weak = window.as_weak();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(2_400));
        let _ = weak.upgrade_in_event_loop(|win| win.invoke_hide_toast());
    });
}

/// Register every callback. Called once from `main`.
pub fn wire_all(window: &AppWindow, state: &SharedState) {
    file_cb::wire(window, state);
    notes_cb::wire(window, state);
    settings_cb::wire(window, state);
    highlight_cb::wire(window, state);
    window_cb::wire(window, state);
    session_cb::wire(window, state);
    wire_meta(window);
}

/// The two meta methods.
fn wire_meta(window: &AppWindow) {
    window.on_ping(|| "pong".into());
    window.on_app_info(app_info);
}

/// Build the About payload.
pub fn app_info() -> crate::ui::AppInfo {
    let backend = std::env::var("SLINT_BACKEND").unwrap_or_else(|_| "default".to_string());
    crate::ui::AppInfo {
        name: notepad_pro_core::APP_NAME.into(),
        version: notepad_pro_core::APP_VERSION.into(),
        data_dir: notepad_pro_core::config::settings::data_dir()
            .to_string_lossy()
            .into_owned()
            .into(),
        slint_backend: backend.into(),
    }
}

/// Normalise a key event's text into the character a shortcut cares about.
///
/// Platforms differ: some deliver `"s"` with Ctrl held, others deliver the raw
/// control character `"\u{13}"`. Both are accepted here.
pub fn normalize_key(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    if first.is_control() {
        // ASCII control characters map back onto @, a..z, [ \ ] ^ _
        let mapped = match first as u32 {
            0x00..=0x1a => char::from_u32(first as u32 + 0x60)?,
            _ => return None,
        };
        Some(mapped)
    } else {
        Some(first.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notepad_pro_core::config::settings::Settings;
    use notepad_pro_core::db::notes::NotesDb;

    #[test]
    fn plain_letters_are_lowercased() {
        assert_eq!(normalize_key("s"), Some('s'));
        assert_eq!(normalize_key("S"), Some('s'));
        assert_eq!(normalize_key("+"), Some('+'));
        assert_eq!(normalize_key("-"), Some('-'));
    }

    #[test]
    fn control_characters_map_back_to_letters() {
        assert_eq!(normalize_key("\u{13}"), Some('s'), "Ctrl+S");
        assert_eq!(normalize_key("\u{0e}"), Some('n'), "Ctrl+N");
        assert_eq!(normalize_key("\u{1a}"), Some('z'), "Ctrl+Z");
        assert_eq!(normalize_key("\u{06}"), Some('f'), "Ctrl+F");
        assert_eq!(normalize_key("\u{08}"), Some('h'), "Ctrl+H");
        assert_eq!(normalize_key("\u{17}"), Some('w'), "Ctrl+W");
        assert_eq!(normalize_key("\u{02}"), Some('b'), "Ctrl+B");
        assert_eq!(normalize_key("\u{19}"), Some('y'), "Ctrl+Y");
    }

    #[test]
    fn empty_and_unmappable_keys_are_rejected() {
        assert_eq!(normalize_key(""), None);
        assert_eq!(normalize_key("\u{7f}"), None, "DEL is not a shortcut");
        assert_eq!(normalize_key("\u{1b}"), None, "Escape is handled separately");
    }

    #[test]
    fn a_poisoned_mutex_is_recovered_not_panicked() {
        let state = shared(AppState::new(Settings::default(), NotesDb::in_memory().unwrap()));
        let doomed = state.clone();
        let _ = std::thread::spawn(move || {
            let _guard = lock(&doomed);
            panic!("deliberate panic while holding the lock");
        })
        .join();
        // The next lock must still hand out usable state.
        assert_eq!(lock(&state).tabs.len(), 1);
    }
}
