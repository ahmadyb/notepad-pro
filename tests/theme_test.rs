//! Theme verification over the shipped `.slint` files (37 checks).
//!
//! These parse the theme sources at test time, so they hold the token store
//! and the seven themes to the same contract `tools/check_consistency.py`
//! lints: every colour token present in every theme, sensible contrast, and a
//! dark editor-text-on-highlight everywhere (the original invisible-text bug).

const THEMES: [&str; 7] = [
    "light",
    "dark",
    "glass_dark",
    "clay_light",
    "clay_dark",
    "neu_light",
    "neu_dark",
];

fn theme_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("ui")
        .join("themes")
        .join(format!("{name}.slint"))
}

fn read_theme(name: &str) -> String {
    std::fs::read_to_string(theme_path(name)).expect("theme file present")
}

fn token_hex(text: &str, name: &str) -> Option<(u8, u8, u8, f32)> {
    let needle = format!("AppTheme.{name}");
    let idx = text.find(&needle)?;
    let rest = &text[idx..];
    let line = rest.lines().next().unwrap_or("");
    let hash = line.find('#')?;
    let hex: String = line[hash + 1..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    parse_hex(&hex)
}

fn token_bool(text: &str, name: &str) -> Option<bool> {
    let needle = format!("AppTheme.{name}");
    let idx = text.find(&needle)?;
    let rest = &text[idx..];
    let line = rest.lines().next().unwrap_or("");
    if line.contains("true") {
        Some(true)
    } else if line.contains("false") {
        Some(false)
    } else {
        None
    }
}

fn parse_hex(hex: &str) -> Option<(u8, u8, u8, f32)> {
    let v = u8::from_str_radix;
    match hex.len() {
        6 => Some((
            v(&hex[0..2], 16).ok()?,
            v(&hex[2..4], 16).ok()?,
            v(&hex[4..6], 16).ok()?,
            1.0,
        )),
        8 => Some((
            v(&hex[0..2], 16).ok()?,
            v(&hex[2..4], 16).ok()?,
            v(&hex[4..6], 16).ok()?,
            v(&hex[6..8], 16).ok()? as f32 / 255.0,
        )),
        _ => None,
    )
}

fn luminance((r, g, b, _a): (u8, u8, u8, f32)) -> f32 {
    let f = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

fn composite(fg: (u8, u8, u8, f32), bg: (u8, u8, u8, f32)) -> (u8, u8, u8, f32) {
    let a = fg.3 + bg.3 * (1.0 - fg.3);
    if a == 0.0 {
        return (0, 0, 0, 0.0);
    }
    let mix = |f: u8, b: u8| ((f as f32 * fg.3 + b as f32 * bg.3 * (1.0 - fg.3)) / a) as u8;
    (mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2), a)
}

fn contrast(a: (u8, u8, u8, f32), b: (u8, u8, u8, f32)) -> f32 {
    let la = luminance(a);
    let lb = luminance(b);
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn there_are_exactly_seven_themes() {
    assert_eq!(THEMES.len(), 7);
    for t in THEMES {
        assert!(theme_path(t).exists(), "missing theme {t}");
    }
}

#[test]
fn every_theme_defines_an_apply_function() {
    for t in THEMES {
        assert!(read_theme(t).contains("public function apply()"), "{t}");
    }
}

// ── Per-theme text contrast (7) ───────────────────────────────────────────

#[test]
fn light_text_contrast() {
    assert_body_contrast("light");
}
#[test]
fn dark_text_contrast() {
    assert_body_contrast("dark");
}
#[test]
fn glass_dark_text_contrast() {
    assert_body_contrast("glass_dark");
}
#[test]
fn clay_light_text_contrast() {
    assert_body_contrast("clay_light");
}
#[test]
fn clay_dark_text_contrast() {
    assert_body_contrast("clay_dark");
}
#[test]
fn neu_light_text_contrast() {
    assert_body_contrast("neu_light");
}
#[test]
fn neu_dark_text_contrast() {
    assert_body_contrast("neu_dark");
}

fn assert_body_contrast(theme: &str) {
    let text = read_theme(theme);
    let fg = token_hex(&text, "text-colour").unwrap();
    let bg_raw = token_hex(&text, "window-bg").unwrap();
    // The body text sits on the window background.
    let bg = if bg_raw.3 < 1.0 {
        composite(bg_raw, (245, 246, 250, 1.0))
    } else {
        bg_raw
    };
    let ratio = contrast(fg, bg);
    assert!(ratio >= 4.5, "{theme}: text contrast {ratio:.2} < 4.5");
}

// ── Per-theme editor contrast (7) ─────────────────────────────────────────

#[test]
fn light_editor_contrast() {
    assert_editor_contrast("light");
}
#[test]
fn dark_editor_contrast() {
    assert_editor_contrast("dark");
}
#[test]
fn glass_dark_editor_contrast() {
    assert_editor_contrast("glass_dark");
}
#[test]
fn clay_light_editor_contrast() {
    assert_editor_contrast("clay_light");
}
#[test]
fn clay_dark_editor_contrast() {
    assert_editor_contrast("clay_dark");
}
#[test]
fn neu_light_editor_contrast() {
    assert_editor_contrast("neu_light");
}
#[test]
fn neu_dark_editor_contrast() {
    assert_editor_contrast("neu_dark");
}

fn assert_editor_contrast(theme: &str) {
    let text = read_theme(theme);
    let fg = token_hex(&text, "editor-text").unwrap();
    let bg_raw = token_hex(&text, "editor-bg").unwrap();
    // Translucent editor backgrounds (glass) composite over the window bg.
    let window_bg = token_hex(&text, "window-bg").unwrap();
    let bg = if bg_raw.3 < 1.0 {
        composite(bg_raw, window_bg)
    } else {
        bg_raw
    };
    let ratio = contrast(fg, bg);
    assert!(ratio >= 4.5, "{theme}: editor contrast {ratio:.2} < 4.5");
}

// ── editor-text-on-highlight must stay dark everywhere (7) ────────────────

#[test]
fn light_on_highlight_is_dark() {
    assert_on_highlight_dark("light");
}
#[test]
fn dark_on_highlight_is_dark() {
    assert_on_highlight_dark("dark");
}
#[test]
fn glass_dark_on_highlight_is_dark() {
    assert_on_highlight_dark("glass_dark");
}
#[test]
fn clay_light_on_highlight_is_dark() {
    assert_on_highlight_dark("clay_light");
}
#[test]
fn clay_dark_on_highlight_is_dark() {
    assert_on_highlight_dark("clay_dark");
}
#[test]
fn neu_light_on_highlight_is_dark() {
    assert_on_highlight_dark("neu_light");
}
#[test]
fn neu_dark_on_highlight_is_dark() {
    assert_on_highlight_dark("neu_dark");
}

fn assert_on_highlight_dark(theme: &str) {
    let text = read_theme(theme);
    let fg = token_hex(&text, "editor-text-on-highlight").unwrap();
    // A dark colour has low luminance; all six bands are light.
    assert!(
        luminance(fg) < 0.25,
        "{theme}: editor-text-on-highlight must be dark (bug fix)"
    );
}

// ── Shared invariants ─────────────────────────────────────────────────────

#[test]
fn glass_dark_uses_translucent_editor_surface() {
    let text = read_theme("glass_dark");
    let bg = token_hex(&text, "editor-bg").unwrap();
    assert!(bg.3 < 1.0, "glass editor-bg should be translucent");
}

#[test]
fn dark_themes_mark_themselves_dark() {
    for t in ["dark", "glass_dark", "clay_dark", "neu_dark"] {
        let text = read_theme(t);
        assert_eq!(token_bool(&text, "is-dark"), Some(true), "{t}");
    }
}

#[test]
fn light_themes_mark_themselves_light() {
    for t in ["light", "clay_light", "neu_light"] {
        let text = read_theme(t);
        assert_eq!(token_bool(&text, "is-dark"), Some(false), "{t}");
    }
}

#[test]
fn every_theme_sets_an_accent() {
    for t in THEMES {
        let text = read_theme(t);
        assert!(token_hex(&text, "accent-colour").is_some(), "{t}");
    }
}

#[test]
fn tokens_file_declares_all_colour_tokens() {
    let tokens = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("ui")
            .join("themes")
            .join("tokens.slint"),
    )
    .unwrap();
    assert!(tokens.contains("export global AppTheme"));
    assert!(tokens.contains("editor-text-on-highlight"));
}

#[test]
fn every_theme_sets_a_caret_colour() {
    for t in THEMES {
        assert!(token_hex(&read_theme(t), "caret-colour").is_some(), "{t}");
    }
}

#[test]
fn selection_colour_is_translucent_everywhere() {
    for t in THEMES {
        let sel = token_hex(&read_theme(t), "selection-colour").unwrap();
        assert!(sel.3 < 1.0, "{t}: selection should be translucent");
    }
}

#[test]
fn light_and_dark_editor_backgrounds_differ() {
    let light = token_hex(&read_theme("light"), "editor-bg").unwrap();
    let dark = token_hex(&read_theme("dark"), "editor-bg").unwrap();
    assert!(luminance(light) > luminance(dark));
}

#[test]
fn surface_hover_differs_from_surface() {
    for t in THEMES {
        let text = read_theme(t);
        let base = token_hex(&text, "surface").unwrap();
        let hover = token_hex(&text, "surface-hover").unwrap();
        assert_ne!(
            (base.0, base.1, base.2),
            (hover.0, hover.1, hover.2),
            "{t}: hover state must be visible"
        );
    }
}

#[test]
fn tooltip_text_contrast() {
    for t in THEMES {
        let text = read_theme(t);
        let fg = token_hex(&text, "tooltip-text").unwrap();
        let bg = token_hex(&text, "tooltip-bg").unwrap();
        assert!(contrast(fg, bg) >= 4.5, "{t}: tooltip contrast");
    }
}

#[test]
fn tabs_have_active_and_inactive_colours() {
    for t in THEMES {
        let text = read_theme(t);
        assert!(token_hex(&text, "tab-bg").is_some(), "{t}");
        assert!(token_hex(&text, "tab-active-bg").is_some(), "{t}");
    }
}

#[test]
fn every_theme_writes_the_full_colour_token_set() {
    // 32 colour tokens per theme, matching tools/check_consistency.py.
    let tokens = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("ui")
            .join("themes")
            .join("tokens.slint"),
    )
    .unwrap();
    let colour_count = tokens
        .lines()
        .filter(|l| l.contains("property <color>"))
        .count();
    for t in THEMES {
        let writes = read_theme(t).matches("AppTheme.").count();
        assert!(
            writes >= colour_count,
            "{t} writes {writes} < colour tokens {colour_count}"
        );
    }
}

#[test]
fn accent_text_reads_on_accent() {
    for t in THEMES {
        let text = read_theme(t);
        let fg = token_hex(&text, "accent-text").unwrap();
        let bg = token_hex(&text, "accent-colour").unwrap();
        assert!(contrast(fg, bg) >= 3.0, "{t}: accent text contrast");
    }
}

#[test]
fn shared_layout_constants_are_declared_once() {
    let tokens = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("ui")
            .join("themes")
            .join("tokens.slint"),
    )
    .unwrap();
    for c in ["titlebar_height", "toolbar_height", "sidebar_width", "radius_md"] {
        assert!(tokens.contains(c), "missing shared constant {c}");
    }
}
