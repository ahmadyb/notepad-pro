//! Settings, theme, zoom and view callbacks.

use slint::ComponentHandle;

use crate::callbacks::{lock, toast, SharedState};
use crate::convert;
use crate::sync;
use crate::ui::AppWindow;

pub fn wire(window: &AppWindow, state: &SharedState) {
    // ── The 7 settings API methods ────────────────────────────────────────

    {
        let s = state.clone();
        window.on_get_settings(move || {
            let guard = lock(&s);
            convert::settings_to_ui(&guard.settings)
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_save_settings(move |view| {
            {
                let mut guard = lock(&s);
                convert::ui_to_settings(&view, &mut guard.settings);
                guard.palette = notepad_pro_core::highlight::palette::Palette::new(
                    guard.settings.custom_palette.clone(),
                );
            }
            if let Some(win) = w.upgrade() {
                apply_current_theme(&win, &s);
                sync::sync_all(&win, &lock(&s));
                let _ = persist(&win, &s);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_update_settings(move |key, value| {
            let applied = lock(&s).settings.update(key.as_str(), value.as_str());
            if applied {
                if let Some(win) = w.upgrade() {
                    apply_current_theme(&win, &s);
                    sync::sync_all(&win, &lock(&s));
                    let _ = persist(&win, &s);
                }
            } else {
                tracing::debug!(key = %key, "update-settings ignored an unknown or invalid key");
            }
        });
    }

    {
        let s = state.clone();
        window.on_get_recent_files(move || {
            let guard = lock(&s);
            crate::convert::string_model(&guard.settings.recent_files)
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_clear_recent_files(move || {
            lock(&s).settings.clear_recent_files();
            if let Some(win) = w.upgrade() {
                let _ = persist(&win, &s);
                toast(&win, "Recent files cleared");
            }
        });
    }

    // save-session / load-session live in session_cb.

    // ── View actions ──────────────────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_theme_changed(move |name| {
            {
                let mut guard = lock(&s);
                if !guard.set_theme(name.as_str()) {
                    tracing::warn!(theme = %name, "unknown theme");
                    return;
                }
            }
            if let Some(win) = w.upgrade() {
                apply_current_theme(&win, &s);
                sync::sync_all(&win, &lock(&s));
                let _ = persist(&win, &s);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_zoom_changed(move |delta| {
            lock(&s).zoom_step(delta);
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
                let _ = persist(&win, &s);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_word_wrap_changed(move |enabled| {
            lock(&s).settings.word_wrap = enabled;
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
                let _ = persist(&win, &s);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_sidebar_changed(move |open| {
            lock(&s).settings.sidebar_open = open;
            if let Some(win) = w.upgrade() {
                sync::sync_flags(&win, &lock(&s));
                sync::sync_notes(&win, &lock(&s));
                let _ = persist(&win, &s);
            }
        });
    }

    {
        let w = window.as_weak();
        window.on_about_requested(move || {
            if let Some(win) = w.upgrade() {
                win.set_app_meta(crate::callbacks::app_info());
                win.set_show_about(true);
            }
        });
    }

    {
        let w = window.as_weak();
        window.on_about_dismissed(move || {
            if let Some(win) = w.upgrade() {
                win.set_show_about(false);
            }
        });
    }
}

/// Apply the theme named in the settings.
pub fn apply_current_theme(window: &AppWindow, state: &SharedState) {
    let theme = lock(state).settings.theme.clone();
    apply_theme(window, &theme);
}

/// Switch the global Slint token table to `name`.
pub fn apply_theme(window: &AppWindow, name: &str) {
    match name {
        "dark" => window.invoke_apply_dark_theme(),
        _ => window.invoke_apply_light_theme(),
    }
}

/// Ctrl+Shift+D — swap to the light/dark twin of the current theme.
pub fn toggle_dark_twin(window: &AppWindow, state: &SharedState) {
    let next = lock(state).toggle_dark_twin();
    apply_theme(window, &next);
    sync::sync_all(window, &lock(state));
    let _ = persist(window, state);
    toast(window, &format!("Theme: {next}"));
}

/// Write settings.json. Errors are logged, never fatal.
pub fn persist(window: &AppWindow, state: &SharedState) -> anyhow::Result<()> {
    let settings = lock(state).settings.clone();
    let path = notepad_pro_core::config::settings::settings_path();
    settings.save(&path).map_err(|err| {
        toast(window, &format!("Cannot save settings: {err}"));
        err
    })
}
