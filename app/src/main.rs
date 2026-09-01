//! NotePad Pro — entry point.
//!
//! 1. Parse the command line.
//! 2. Create the data directory, load settings, open the notes database.
//! 3. Build the window, apply the saved theme, register every callback.
//! 4. Restore the session and open any files given on the command line.
//! 5. Start the autosave loop and the animation driver.
//! 6. Run the event loop; persist on the way out.

// Release builds are a GUI app: no console window behind the editor on
// Windows. Debug/test builds keep the console for --help and logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Result;
use clap::Parser;
use slint::ComponentHandle;

use notepad_pro::callbacks::{self, file_cb, session_cb, settings_cb, SharedState};
use notepad_pro::state::AppState;
use notepad_pro::sync;
use notepad_pro::ui::AppWindow;

use notepad_pro_core::config::session::SessionStore;
use notepad_pro_core::config::settings::{db_path, session_path, settings_path, Settings};
use notepad_pro_core::db::notes::NotesDb;

/// Command line interface.
///
/// `trailing_var_arg` keeps `notepadpro.exe "my file.txt"` working when the
/// shell hands us a path with spaces (bug #4).
#[derive(Debug, Parser)]
#[command(
    name = "notepadpro",
    version = notepad_pro_core::APP_VERSION,
    about = "NotePad Pro — line-oriented text editor with colour highlighting",
    trailing_var_arg = true
)]
struct Cli {
    /// Files to open on startup.
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Start with a specific theme.
    #[arg(long, value_name = "THEME")]
    theme: Option<String>,

    /// Ignore the saved session.
    #[arg(long)]
    no_session: bool,

    /// Start with a single empty tab even when files are given.
    #[arg(long)]
    new_window: bool,

    /// Forget the stored session and exit.
    #[arg(long)]
    reset_session: bool,

    /// Verbose logging.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    if cli.reset_session {
        session_cb::forget_session()?;
        eprintln!("Session cleared.");
        return Ok(());
    }

    let data_dir = notepad_pro_core::config::settings::data_dir();
    std::fs::create_dir_all(&data_dir)?;
    tracing::info!(dir = %data_dir.display(), "data directory ready");

    let mut settings = Settings::load(&settings_path());
    settings.clamp();
    if let Some(theme) = cli.theme.as_deref() {
        if !notepad_pro_core::highlight::palette::is_known_theme(theme) {
            anyhow::bail!(
                "unknown theme '{theme}'. Try one of: {}",
                notepad_pro_core::highlight::palette::THEMES.join(", ")
            );
        }
        settings.theme = theme.to_string();
    }

    let db = NotesDb::open(&db_path())?;
    let state: SharedState = callbacks::shared(AppState::new(settings, db));

    let window = AppWindow::new()?;

    // Theme first, so the very first frame is already correct.
    settings_cb::apply_current_theme(&window, &state);

    // Register all 29 API methods plus the UI wiring.
    callbacks::wire_all(&window, &state);

    // Restore the previous session, then any files from the command line.
    let restored = if cli.no_session || cli.new_window {
        0
    } else {
        session_cb::restore(&window, &state)
    };
    if restored > 0 {
        tracing::info!(tabs = restored, "session restored");
    }

    if !cli.files.is_empty() {
        file_cb::open_paths(&window, &state, &cli.files);
    }

    // Autosave: settings + session, every N seconds.
    session_cb::start_autosave_loop(&window, &state);

    // Liquid background driver: 20 fps, and only while the document is empty.
    let _blob_driver = start_blob_driver(&window);

    sync::sync_all(&window, &callbacks::lock(&state));

    // The caret line takes keyboard focus at startup, so the user can type
    // the moment the window appears.
    {
        let caret = callbacks::lock(&state).cursor.line;
        sync::focus_line(&window, caret);
    }

    tracing::info!(
        "{} {} starting",
        notepad_pro_core::APP_NAME,
        notepad_pro_core::APP_VERSION
    );
    window.run()?;

    // The window is gone; persist without touching the UI.
    session_cb::persist_all(&state);
    Ok(())
}

fn init_tracing(verbose: bool) {
    use tracing_subscriber::EnvFilter;
    let filter = if verbose {
        EnvFilter::new("notepad_pro=debug,notepad_pro_core=debug,warn")
    } else {
        EnvFilter::new("warn")
    };
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// Advances `blob-phase` at 20 fps.
///
/// Driving the drift from Rust instead of an infinite Slint animation means
/// the repaints stop the instant the document gains content or animations are
/// turned off. The returned `Timer` must be kept alive for as long as the
/// animation should run.
fn start_blob_driver(window: &AppWindow) -> slint::Timer {
    let timer = slint::Timer::default();
    let weak = window.as_weak();
    let phase = Rc::new(Cell::new(0.0f32));
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || {
            let Some(win) = weak.upgrade() else { return };
            if !win.get_animations() || !win.get_document_empty() {
                return;
            }
            // One full turn every 32 seconds at 20 fps.
            let next = phase.get() + 0.0015625;
            let wrapped = if next >= 1.0 { next - 1.0 } else { next };
            phase.set(wrapped);
            win.set_blob_phase(wrapped);
        },
    );
    timer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_accepts_files_and_flags() {
        let cli = Cli::try_parse_from([
            "notepadpro",
            "--theme",
            "dark",
            "--no-session",
            "a.txt",
            "b.txt",
        ])
        .expect("parse");
        assert_eq!(cli.files.len(), 2);
        assert_eq!(cli.theme.as_deref(), Some("dark"));
        assert!(cli.no_session);
        assert!(!cli.new_window);
    }

    #[test]
    fn cli_keeps_a_path_with_spaces_as_one_argument() {
        let cli = Cli::try_parse_from(["notepadpro", "my notes.txt"]).expect("parse");
        assert_eq!(cli.files.len(), 1);
        assert_eq!(cli.files[0].to_string_lossy(), "my notes.txt");
    }

    #[test]
    fn cli_defaults_are_inert() {
        let cli = Cli::try_parse_from(["notepadpro"]).expect("parse");
        assert!(cli.files.is_empty());
        assert!(cli.theme.is_none());
        assert!(!cli.no_session);
        assert!(!cli.new_window);
        assert!(!cli.reset_session);
        assert!(!cli.verbose);
    }

    #[test]
    fn unknown_flags_are_rejected() {
        assert!(Cli::try_parse_from(["notepadpro", "--nope"]).is_err());
    }

    #[test]
    fn the_phase_wraps_at_one() {
        // 20 fps for 32 s is one full turn. 1/640 is not a binary fraction,
        // so f32 accumulation lands within one step of either end.
        let mut phase = 0.0f32;
        for _ in 0..640 {
            phase += 0.0015625;
            if phase >= 1.0 {
                phase -= 1.0;
            }
        }
        assert!((0.0..1.0).contains(&phase));
        assert!(
            phase < 0.01 || phase > 0.99,
            "one turn should end where it started, got {phase}"
        );
    }

    #[test]
    fn session_store_paths_are_consistent() {
        assert_eq!(
            SessionStore::new(session_path()).path(),
            session_path().as_path()
        );
    }
}
