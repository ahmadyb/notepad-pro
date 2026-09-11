//! Toggleable runtime diagnostics log.
//!
//! The app ships silent by default; flipping the toolbar **Logs** button (or
//! `--logs`, or `"logging": true` in `settings.json`) starts appending
//! timestamped lines to `notepadpro.log` in the data directory. Every edit,
//! paste, clipboard access and sync carries a line with its timing, so a
//! "not responding" repro leaves a trail that pinpoints the stuck stage.
//!
//! The writer is a plain `File` behind a mutex, gated by an atomic bool so
//! the toggle takes effect immediately without re-initialising any global
//! subscriber. When disabled, `log()` is a single atomic load.

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static ENABLED: AtomicBool = AtomicBool::new(false);
static FILE: Mutex<Option<File>> = Mutex::new(None);
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Open (or create-append) the log file and set the initial enabled state.
/// Called once from `main` after settings are loaded.
pub fn init(enabled: bool, path: PathBuf) {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    *FILE.lock().unwrap() = file;
    *PATH.lock().unwrap() = Some(path);
    ENABLED.store(enabled, Ordering::Relaxed);
    if enabled {
        log("diag", "logging enabled");
    }
}

/// Runtime toggle (toolbar button / settings). Takes effect immediately.
pub fn set_enabled(enabled: bool) {
    let was = ENABLED.swap(enabled, Ordering::Relaxed);
    if enabled && !was {
        // Make sure a file exists even if init() could not open one.
        let mut guard = FILE.lock().unwrap();
        if guard.is_none() {
            if let Some(path) = PATH.lock().unwrap().clone() {
                *guard = OpenOptions::new().create(true).append(true).open(path).ok();
            }
        }
        drop(guard);
        log("diag", "logging enabled");
    } else if !enabled && was {
        log("diag", "logging disabled");
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Where the log lives (for toasts / startup messages).
pub fn path() -> Option<PathBuf> {
    PATH.lock().unwrap().clone()
}

/// Append one timestamped line: `[2026-09-09T12:34:56.789 +1234ms] [area] msg`.
/// The `+ms` field is milliseconds since the previous log line, which makes a
/// hang visible as a huge gap between two stages.
pub fn log(area: &str, msg: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut guard = FILE.lock().unwrap();
    let Some(file) = guard.as_mut() else { return };

    let now = SystemTime::now();
    let since_epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let (wall, delta) = {
        static LAST: Mutex<Option<u128>> = Mutex::new(None);
        let ms = since_epoch.as_millis();
        let mut last = LAST.lock().unwrap();
        let delta = last.map(|l| ms.saturating_sub(l)).unwrap_or(0);
        *last = Some(ms);
        (ms, delta)
    };

    let secs = (wall / 1000) as i64;
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let mut line = String::with_capacity(area.len() + msg.len() + 48);
    let _ = write!(
        line,
        "[{:02}:{:02}:{:02}.{:03} +{}ms] [{}] {}\n",
        h,
        m,
        s,
        wall % 1000,
        delta,
        area,
        msg
    );
    let _ = file.write_all(line.as_bytes());
    let _ = file.flush();
}

/// Install a panic hook that records the panic into the log (and chains the
/// default hook), so a crash — not just a hang — leaves evidence.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        log("panic", &format!("PANIC at {loc}: {info}"));
        default(info);
    }));
}
