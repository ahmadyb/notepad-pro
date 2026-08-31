//! Window-control verification (28 checks).
//!
//! Slint 1.6 has no published headless testing backend (`i-slint-backend-testing`
//! only exists from 1.7.0, and the `slint` crate exposes no `testing` feature),
//! so this suite verifies the window layer two ways instead: metric/wiring
//! facts parsed from `ui/app.slint`, and behaviour exercised on the
//! window-free state layer that the window callbacks delegate to.

use notepad_pro::callbacks::{self, normalize_key, SharedState};
use notepad_pro::state::AppState;
use notepad_pro::ui::WindowStateData;
use notepad_pro_core::config::settings::Settings;
use notepad_pro_core::db::notes::NotesDb;

fn app_slint() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("ui")
            .join("app.slint"),
    )
    .unwrap()
}

fn tokens() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("ui")
            .join("themes")
            .join("tokens.slint"),
    )
    .unwrap()
}

fn state() -> SharedState {
    callbacks::shared(AppState::new(
        Settings::default(),
        NotesDb::in_memory().unwrap(),
    ))
}

// ── Win11 metrics & wiring (parsed) ───────────────────────────────────────

#[test]
fn three_window_buttons_are_46_wide() {
    assert_eq!(app_slint().matches("width: 46px").count(), 3);
}

#[test]
fn titlebar_height_is_declared() {
    assert!(tokens().contains("titlebar-height"));
}

#[test]
fn native_frame_is_on_by_default() {
    assert!(app_slint().contains("native-frame: true"));
}

#[test]
fn custom_titlebar_only_when_frameless() {
    assert!(app_slint().contains("if !root.native-frame : Rectangle"));
}

#[test]
fn drag_region_present() {
    assert!(app_slint().contains("mouse-cursor: move"));
}

#[test]
fn double_click_toggles_maximise() {
    assert!(app_slint().contains("double-clicked => { root.toggle-maximise(); }"));
}

#[test]
fn close_button_is_intercepted_not_quit_directly() {
    // Slint 1.6's Window has no `close-requested` interception, so the
    // unsaved-changes guard sits behind the custom close button: the markup
    // must route through the `close-window` callback, never a direct quit,
    // and nothing may fake a handler the platform does not provide.
    let src = app_slint();
    assert!(
        src.contains("root.close-window();"),
        "the close button must call the Rust close-window callback"
    );
    assert!(
        !src.contains("close-requested =>"),
        "Slint 1.6 has no close-requested handler; nothing may fake one"
    );
}

#[test]
fn close_hover_uses_danger_colour() {
    assert!(app_slint().contains("close-touch.has-hover ? AppTheme.danger-colour"));
}

#[test]
fn window_buttons_wire_the_three_callbacks() {
    let src = app_slint();
    assert!(src.contains("root.minimise();"));
    assert!(src.contains("root.toggle-maximise();"));
    assert!(src.contains("root.close-window();"));
}

#[test]
fn default_geometry_is_1200_by_800() {
    let src = app_slint();
    assert!(src.contains("width: 1200px"));
    assert!(src.contains("height: 800px"));
}

#[test]
fn minimise_glyph_is_a_dash() {
    assert!(app_slint().contains("text: \"─\""));
}

#[test]
fn maximise_glyph_present() {
    assert!(app_slint().contains("text: \"❐\""));
}

#[test]
fn close_glyph_present() {
    assert!(app_slint().contains("text: \"✕\""));
}

#[test]
fn titlebar_shows_the_window_title() {
    assert!(app_slint().contains("text: root.window-title;"));
}

#[test]
fn drag_region_is_declared_before_the_buttons() {
    let src = app_slint();
    let drag = src.find("mouse-cursor: move").unwrap();
    let first_button = src.find("width: 46px").unwrap();
    assert!(drag < first_button, "drag area must come first so buttons win hits");
}

#[test]
fn all_four_window_api_callbacks_are_declared() {
    let src = app_slint();
    for cb in ["callback window-state", "callback minimise", "callback toggle-maximise", "callback close-window"] {
        assert!(src.contains(cb), "missing {cb}");
    }
}

#[test]
fn move_window_is_wired_to_the_drag_region() {
    let src = app_slint();
    let drag = src.find("mouse-cursor: move").unwrap();
    let call = src.find("root.move-window();").unwrap();
    assert!(call > drag && call - drag < 200, "drag pointer-event must call move-window");
}

#[test]
fn rust_close_handler_checks_dirty_tabs_before_quitting() {
    // The Rust half of the interception: `on_close_window` -> `request_close`,
    // which prompts via the confirm dialog before discarding unsaved tabs.
    let rs = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("callbacks")
            .join("window_cb.rs"),
    )
    .unwrap();
    assert!(rs.contains("on_close_window"), "close-window callback must be registered");
    let at = rs.find("pub fn request_close").expect("request_close present");
    let block = &rs[at..at + 400];
    assert!(block.contains("any_dirty()"), "close must check for unsaved tabs");
    assert!(
        block.contains("PendingAction::CloseApp"),
        "a dirty close must go through the confirm dialog"
    );
}

// ── normalize_key ─────────────────────────────────────────────────────────

#[test]
fn normalize_maps_ascii_control_to_letter() {
    // Ctrl+F delivers 0x06 on platforms that still fill `text`.
    assert_eq!(normalize_key("\u{6}"), Some('f'));
}

#[test]
fn normalize_lowercases_plain_letters() {
    assert_eq!(normalize_key("S"), Some('s'));
}

#[test]
fn normalize_empty_is_none() {
    assert_eq!(normalize_key(""), None);
}

// ── Window-state mirror ───────────────────────────────────────────────────

#[test]
fn mirror_defaults_are_false() {
    let s = AppState::new(Settings::default(), NotesDb::in_memory().unwrap());
    assert!(!s.window_minimised);
    assert!(!s.window_maximised);
    assert!(!s.window_fullscreen);
}

#[test]
fn window_state_data_carries_the_mirror() {
    let s = state();
    {
        let mut g = callbacks::lock(&s);
        g.window_maximised = true;
        g.window_minimised = false;
        g.window_fullscreen = true;
    }
    let g = callbacks::lock(&s);
    let ws = WindowStateData {
        maximised: g.window_maximised,
        minimised: g.window_minimised,
        fullscreen: g.window_fullscreen,
    };
    assert!(ws.maximised);
    assert!(!ws.minimised);
    assert!(ws.fullscreen);
}

#[test]
fn a_poisoned_state_mutex_is_recovered() {
    let s = state();
    let doomed = s.clone();
    let _ = std::thread::spawn(move || {
        let _g = callbacks::lock(&doomed);
        panic!("deliberate poison");
    })
    .join();
    assert_eq!(callbacks::lock(&s).tabs.len(), 1);
}

// ── State behind the window actions ───────────────────────────────────────

#[test]
fn window_title_gains_a_dirty_dot() {
    let s = state();
    callbacks::lock(&s).load_text("t.txt", "a", None);
    callbacks::lock(&s).set_line_text(0, "dirty");
    assert!(callbacks::lock(&s).window_title().contains('\u{25cf}'));
}

#[test]
fn closing_the_only_tab_leaves_a_fresh_one() {
    let s = state();
    callbacks::lock(&s).close_tab(0);
    assert_eq!(callbacks::lock(&s).tabs.len(), 1);
}

#[test]
fn cycle_tab_wraps() {
    let s = state();
    callbacks::lock(&s).new_tab();
    callbacks::lock(&s).cycle_tab(true);
    assert_eq!(callbacks::lock(&s).active, 0);
}

#[test]
fn any_dirty_tracks_unsaved_edits() {
    let s = state();
    assert!(!callbacks::lock(&s).any_dirty());
    callbacks::lock(&s).load_text("t.txt", "a", None);
    callbacks::lock(&s).set_line_text(0, "x");
    assert!(callbacks::lock(&s).any_dirty());
}
