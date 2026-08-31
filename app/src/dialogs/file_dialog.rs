//! Native file pickers via `rfd`.
//!
//! These block the calling thread. They are only ever called from a Slint
//! callback on the event-loop thread, so the window simply stops repainting
//! for the duration — which is exactly how the platform pickers behave anyway.

use std::path::PathBuf;

use rfd::FileDialog;

/// Extensions offered by the Open dialog.
const TEXT_EXTENSIONS: &[&str] = &["txt", "npro", "md", "rs", "toml", "json", "log", "csv"];

pub fn open_dialog() -> Vec<PathBuf> {
    let picked = FileDialog::new()
        .set_title("Open")
        .add_filter("Text Files", TEXT_EXTENSIONS)
        .add_filter("All Files", &["*"])
        .pick_files();
    match picked {
        Some(paths) => paths,
        None => {
            tracing::debug!("open dialog was cancelled or unavailable");
            Vec::new()
        }
    }
}

pub fn save_dialog(default_name: &str) -> Option<PathBuf> {
    FileDialog::new()
        .set_title("Save As")
        .set_file_name(default_name)
        .add_filter("NotePad Pro", &["npro"])
        .add_filter("Text File", &["txt"])
        .add_filter("All Files", &["*"])
        .save_file()
}

pub fn export_dialog(default_name: &str) -> Option<PathBuf> {
    FileDialog::new()
        .set_title("Export")
        .set_file_name(default_name)
        .add_filter("Text File", &["txt"])
        .add_filter("Markdown", &["md"])
        .add_filter("All Files", &["*"])
        .save_file()
}

/// `true` when a native picker is likely to work. On a headless Linux box
/// there is no GTK/portal to talk to, so the callers fall back to a toast.
pub fn pickers_available() -> bool {
    if cfg!(target_os = "linux") {
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
    } else {
        true
    }
}
