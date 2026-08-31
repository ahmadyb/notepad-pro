//! File and tab callbacks.
//!
//! Note on shape: every closure captures a `Weak<AppWindow>`, never a strong
//! one, and the window handle is never stored inside `AppState`. That is what
//! prevents the re-entrant serialisation recursion that crashed the original
//! PyWebView build (bug #1) — there is no path from a data structure back to
//! the window.

use std::path::{Path, PathBuf};

use slint::{ComponentHandle, Model, SharedString};

use notepad_pro_core::files::line_endings::LineEnding;
use notepad_pro_core::files::manager;

use crate::callbacks::{lock, toast, SharedState};
use crate::dialogs;
use crate::state::PendingAction;
use crate::sync;
use crate::ui::{AppWindow, LoadedFileData};

/// Failure payload for `load-file`; the generated struct has no constructors.
fn failed(path: &str, err: impl Into<SharedString>) -> LoadedFileData {
    LoadedFileData {
        ok: false,
        error: err.into(),
        path: path.into(),
        content: SharedString::default(),
        encoding: SharedString::default(),
        line_ending: SharedString::default(),
    }
}

pub fn wire(window: &AppWindow, state: &SharedState) {
    // ── The 8 file API methods ────────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_open_file_dialog(move || {
            let paths = dialogs::file_dialog::open_dialog();
            if let Some(win) = w.upgrade() {
                if paths.is_empty() {
                    toast(&win, "No file selected");
                } else {
                    open_paths(&win, &s, &paths);
                }
            }
            crate::convert::string_model(
                &paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
            )
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_load_file(move |path: SharedString| {
            let result = {
                let mut guard = lock(&s);
                guard.open_path(Path::new(path.as_str()))
            };
            let Some(win) = w.upgrade() else {
                return failed(path.as_str(), "window closed");
            };
            match result {
                Ok(_) => {
                    let guard = lock(&s);
                    let tab = guard.tab();
                    let payload = LoadedFileData {
                        ok: true,
                        error: SharedString::default(),
                        path: path.clone(),
                        content: tab.doc.plain_text().as_str().into(),
                        encoding: tab.encoding.as_str().into(),
                        line_ending: tab.line_ending.label().into(),
                    };
                    drop(guard);
                    sync::sync_all(&win, &lock(&s));
                    payload
                }
                Err(err) => {
                    toast(&win, &format!("Cannot open: {err}"));
                    failed(path.as_str(), err.to_string())
                }
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_save_file(
            move |path: SharedString,
                  content: SharedString,
                  encoding: SharedString,
                  ending: SharedString| {
                let target = PathBuf::from(path.as_str());
                let text = content.to_string();
                let enc = encoding.to_string();
                let le = LineEnding::from_label(ending.as_str());
                let saved = manager::save_file(&target, &text, &enc, le);

                let mut guard = lock(&s);
                match saved {
                    Ok(()) => {
                        let tab = guard.tab_mut();
                        tab.state.path = Some(path.to_string());
                        tab.state.name = manager::file_name(path.as_str());
                        tab.state.dirty = false;
                        tab.doc.mark_saved();
                        guard.settings.remember_file(path.as_str());
                        drop(guard);
                        if let Some(win) = w.upgrade() {
                            sync::sync_light(&win, &lock(&s));
                        }
                    }
                    Err(err) => {
                        drop(guard);
                        tracing::error!(%err, "cannot save {}", path);
                        if let Some(win) = w.upgrade() {
                            toast(&win, &format!("Save failed: {err}"));
                        }
                    }
                }
            },
        );
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_save_file_as(move |content: SharedString, default_name: SharedString| {
            let Some(target) = dialogs::file_dialog::save_dialog(default_name.as_str()) else {
                return SharedString::default();
            };
            let text = content.to_string();
            let path_string = target.to_string_lossy().into_owned();

            let (encoding, ending) = {
                let guard = lock(&s);
                (guard.tab().encoding.clone(), guard.tab().line_ending)
            };
            if let Err(err) = manager::save_file(&target, &text, &encoding, ending) {
                if let Some(win) = w.upgrade() {
                    toast(&win, &format!("Save failed: {err}"));
                }
                return SharedString::default();
            }

            {
                let mut guard = lock(&s);
                let tab = guard.tab_mut();
                tab.state.path = Some(path_string.clone());
                tab.state.name = manager::file_name(&path_string);
                tab.state.dirty = false;
                tab.doc.mark_saved();
                guard.settings.remember_file(&path_string);
            }
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
                toast(&win, &format!("Saved {}", path_string));
            }
            SharedString::from(path_string.as_str())
        });
    }

    window.on_file_exists(|path: SharedString| manager::file_exists(path.as_str()));

    {
        let w = window.as_weak();
        window.on_save_extracted_text(move |content: SharedString, colours_label: SharedString| {
            let default = format!("extract-{}.txt", sanitise(colours_label.as_str()));
            let Some(target) = dialogs::file_dialog::export_dialog(&default) else {
                return;
            };
            let text = content.to_string();
            let result = manager::save_file(&target, &text, "utf-8", LineEnding::Lf);
            if let Some(win) = w.upgrade() {
                match result {
                    Ok(()) => toast(&win, &format!("Saved {}", target.display())),
                    Err(err) => toast(&win, &format!("Export failed: {err}")),
                }
            }
        });
    }

    {
        let s = state.clone();
        window.on_set_startup_files(move |files| {
            let mut guard = lock(&s);
            guard.startup_files = files
                .iter()
                .map(|f| f.to_string())
                .filter(|f| !f.trim().is_empty())
                .collect();
        });
    }

    {
        let s = state.clone();
        window.on_get_startup_files(move || {
            let guard = lock(&s);
            crate::convert::string_model(&guard.startup_files)
        });
    }

    // ── Tab actions ───────────────────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_tab_new(move || {
            lock(&s).new_tab();
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_tab_chosen(move |index: i32| {
            {
                let mut guard = lock(&s);
                guard.stash_cursor();
                guard.select_tab(index.max(0) as usize);
            }
            if let Some(win) = w.upgrade() {
                sync::sync_all(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_tab_closed(move |index: i32| {
            let index = index.max(0) as usize;
            let Some(win) = w.upgrade() else { return };
            let needs_confirm = {
                let guard = lock(&s);
                guard.tabs.get(index).map(|t| t.is_dirty()).unwrap_or(false)
            };
            if needs_confirm {
                let mut guard = lock(&s);
                dialogs::confirm_dialog::ask(&win, &mut guard, PendingAction::CloseTab(index));
            } else {
                lock(&s).close_tab(index);
                sync::sync_all(&win, &lock(&s));
            }
        });
    }
}

/// New empty tab, from a shortcut or the toolbar.
pub fn new_tab(window: &AppWindow, state: &SharedState) {
    lock(state).new_tab();
    sync::sync_all(window, &lock(state));
}

/// Close the active tab, prompting when it is dirty.
pub fn close_active_tab(window: &AppWindow, state: &SharedState) {
    let index = lock(state).active;
    let needs_confirm = lock(state)
        .tabs
        .get(index)
        .map(|t| t.is_dirty())
        .unwrap_or(false);
    if needs_confirm {
        let mut guard = lock(state);
        dialogs::confirm_dialog::ask(window, &mut guard, PendingAction::CloseTab(index));
    } else {
        lock(state).close_tab(index);
        sync::sync_all(window, &lock(state));
    }
}

/// Save the active tab, prompting for a path when it has none.
pub fn save_active(window: &AppWindow, state: &SharedState) {
    let (existing, default_name) = {
        let guard = lock(state);
        (
            guard.tab().state.path.clone(),
            guard.tab().state.name.clone(),
        )
    };

    let target = match existing {
        Some(path) => PathBuf::from(path),
        None => match dialogs::file_dialog::save_dialog(&default_name) {
            Some(path) => path,
            None => {
                toast(window, "Save cancelled");
                return;
            }
        },
    };

    match lock(state).save_to(&target) {
        Ok(()) => {
            sync::sync_all(window, &lock(state));
            toast(window, &format!("Saved {}", target.display()));
        }
        Err(err) => toast(window, &format!("Save failed: {err}")),
    }
}

/// "Save As" — always prompts.
pub fn save_as(window: &AppWindow, state: &SharedState) {
    let default_name = lock(state).tab().state.name.clone();
    let Some(target) = dialogs::file_dialog::save_dialog(&default_name) else {
        toast(window, "Save cancelled");
        return;
    };
    match lock(state).save_to(&target) {
        Ok(()) => {
            sync::sync_all(window, &lock(state));
            toast(window, &format!("Saved {}", target.display()));
        }
        Err(err) => toast(window, &format!("Save failed: {err}")),
    }
}

/// Open files given on the command line.
pub fn open_paths(window: &AppWindow, state: &SharedState, paths: &[PathBuf]) {
    let mut failures = Vec::new();
    {
        let mut guard = lock(state);
        for path in paths {
            if let Err(err) = guard.open_path(path) {
                failures.push(format!("{}: {err}", path.display()));
            }
        }
    }
    sync::sync_all(window, &lock(state));
    if failures.is_empty() {
        let count = paths.len();
        toast(
            window,
            &if count == 1 {
                "Opened 1 file".to_string()
            } else {
                format!("Opened {count} files")
            },
        );
    } else {
        toast(window, &failures.join("; "));
    }
}

/// Turn an arbitrary label into something safe for a file name.
fn sanitise(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' => '-',
            ' ' => '_',
            other => other,
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').trim_matches('_');
    if trimmed.is_empty() {
        "colours".to_string()
    } else {
        trimmed.chars().take(40).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitise_strips_path_separators() {
        assert_eq!(sanitise("Yellow · Green"), "Yellow_·_Green");
        assert_eq!(sanitise("a/b:c"), "a-b-c");
        assert_eq!(sanitise("##"), "colours");
        assert_eq!(sanitise(""), "colours");
    }

    #[test]
    fn sanitise_caps_the_length() {
        assert_eq!(sanitise(&"x".repeat(200)).chars().count(), 40);
    }

    #[test]
    fn opening_paths_reports_per_file_failures() {
        // No window here, so this exercises the state half of the path only.
        let state = crate::callbacks::shared(crate::state::AppState::new(
            notepad_pro_core::config::settings::Settings::default(),
            notepad_pro_core::db::notes::NotesDb::in_memory().unwrap(),
        ));
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.txt");
        std::fs::write(&good, "content").unwrap();
        {
            let mut guard = crate::callbacks::lock(&state);
            assert!(guard.open_path(&good).is_ok());
            assert!(guard.open_path(Path::new("/nope/bad.txt")).is_err());
        }
        assert_eq!(crate::callbacks::lock(&state).tabs.len(), 1);
    }
}
