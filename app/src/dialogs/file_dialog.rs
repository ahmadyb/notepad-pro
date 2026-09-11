//! Native file pickers via `rfd`.
//!
//! The raw `*_dialog` helpers block the calling thread inside a Win32/COM
//! modal loop — on the Slint event-loop thread that makes the window show
//! "Not responding" for as long as the picker stays open (the freeze the
//! 1.0.2 field report complained about). Everything user-facing therefore
//! goes through [`run_pick_async`], which runs the blocking call on a
//! worker thread and delivers the result back on the event loop via a
//! repeating `slint::Timer`, so the UI keeps painting and stays responsive
//! the whole time.

use std::path::PathBuf;

use rfd::FileDialog;

/// Runs a blocking picker on a worker thread; `on_done` fires on the Slint
/// event loop (polled every 50 ms) with the picker's result.
///
/// `pick` must be `Send`: it never touches Slint state — only the closure
/// that receives its result does, and that one runs on the UI thread.
pub fn run_pick_async<R, P, F>(pick: P, on_done: F)
where
    R: Send + 'static,
    P: FnOnce() -> R + Send + 'static,
    F: FnOnce(R) + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(pick());
    });

    let on_done = std::cell::RefCell::new(Some(on_done));
    let timer = std::rc::Rc::new(slint::Timer::default());
    let handle = timer.clone();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || match rx.try_recv() {
            Ok(result) => {
                handle.stop();
                if let Some(f) = on_done.borrow_mut().take() {
                    f(result);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            // The worker died without sending: stop polling, stay alive.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => handle.stop(),
        },
    );
    // The closure holds an Rc to the timer and the timer holds the closure;
    // leaking the outer handle keeps that (stopped, silent) cycle alive for
    // the process lifetime — a few dozen bytes per picker opened.
    let _ = std::rc::Rc::into_raw(timer);
}

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
        // Text first: it is the default filter, so a fresh "Untitled"
        // document saves as .txt unless the user opts into .npro.
        .add_filter("Text File", &["txt"])
        .add_filter("Markdown", &["md"])
        .add_filter("NotePad Pro", &["npro"])
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
