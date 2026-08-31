//! Highlight, palette, extraction and clipboard callbacks.

use slint::{ComponentHandle, Model};

use crate::callbacks::{lock, toast, SharedState};
use crate::convert;
use crate::sync;
use crate::ui::AppWindow;

pub fn wire(window: &AppWindow, state: &SharedState) {
    // ── The 3 highlight API methods ───────────────────────────────────────

    {
        let s = state.clone();
        window.on_highlight_stats(move || {
            let guard = lock(&s);
            let breakdown =
                notepad_pro_core::highlight::stats::breakdown(&guard.doc().lines, &guard.palette);
            convert::stats_to_ui(&breakdown.to_api())
        });
    }

    {
        let s = state.clone();
        window.on_extract_by_colour(move |keys, grouped| {
            let guard = lock(&s);
            let colours: Vec<notepad_pro_core::types::line::LineColour> = keys
                .iter()
                .map(|k| notepad_pro_core::types::line::LineColour::from_key(k.as_str()))
                .filter(|c| c.is_highlighted())
                .collect();
            let order = if grouped {
                notepad_pro_core::highlight::extractor::ExtractionOrder::GroupByColour
            } else {
                notepad_pro_core::highlight::extractor::ExtractionOrder::Document
            };
            let result =
                notepad_pro_core::highlight::extractor::extract(&guard.doc().lines, &colours, order);
            result.text.as_str().into()
        });
    }

    {
        let w = window.as_weak();
        window.on_copy_to_clipboard(move |text| match copy_to_clipboard(text.as_str()) {
            Ok(()) => true,
            Err(err) => {
                if let Some(win) = w.upgrade() {
                    toast(&win, &format!("Clipboard unavailable: {err}"));
                }
                false
            }
        });
    }

    // ── Highlight actions ─────────────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_highlight_chosen(move |key| {
            let applied = lock(&s).toggle_highlight_key(key.as_str());
            if let Some(win) = w.upgrade() {
                match applied {
                    Some(true) => sync::sync_all(&win, &lock(&s)),
                    Some(false) => sync::sync_all(&win, &lock(&s)),
                    None => toast(&win, &format!("Unknown colour: {key}")),
                }
            }
        });
    }

    {
        let w = window.as_weak();
        window.on_custom_colour_requested(move || {
            if let Some(win) = w.upgrade() {
                win.set_picker_hex("#ffe27a".into());
                win.set_picker_name("Custom".into());
                win.set_picker_hex_valid(true);
                win.set_show_colour_picker(true);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_colour_hex_changed(move |hex| {
            let rgba = notepad_pro_core::highlight::palette::rgba_from_hex(hex.as_str());
            if let Some(win) = w.upgrade() {
                let mut guard = lock(&s);
                guard.picker_hex = hex.to_string();
                match rgba {
                    Some(value) => {
                        win.set_picker_hex_valid(true);
                        win.set_picker_colour(convert::rgba_to_color(value));
                    }
                    None => win.set_picker_hex_valid(false),
                }
            }
        });
    }

    {
        let s = state.clone();
        window.on_colour_name_changed(move |name| {
            lock(&s).picker_name = name.to_string();
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_colour_saved(move || {
            let (name, hex) = {
                let guard = lock(&s);
                (guard.picker_name.clone(), guard.picker_hex.clone())
            };
            let added = lock(&s).add_custom_colour(&name, &hex);
            if let Some(win) = w.upgrade() {
                win.set_show_colour_picker(false);
                if added {
                    sync::sync_palette(&win, &lock(&s));
                    sync::sync_extract(&win, &lock(&s));
                    let _ = crate::callbacks::settings_cb::persist(&win, &s);
                    toast(&win, &format!("Added {name}"));
                } else {
                    toast(&win, "That is not a colour");
                }
            }
        });
    }

    {
        let w = window.as_weak();
        window.on_colour_cancelled(move || {
            if let Some(win) = w.upgrade() {
                win.set_show_colour_picker(false);
            }
        });
    }

    // ── Extract panel ─────────────────────────────────────────────────────

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_extract_panel_requested(move || {
            {
                let mut guard = lock(&s);
                guard.extract_open = !guard.extract_open;
            }
            if let Some(win) = w.upgrade() {
                sync::sync_extract(&win, &lock(&s));
                sync::sync_flags(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_extract_colour_toggled(move |key, _active| {
            lock(&s).toggle_extract_colour(key.as_str());
            if let Some(win) = w.upgrade() {
                sync::sync_extract(&win, &lock(&s));
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_extract_order_changed(move |grouped| {
            {
                let mut guard = lock(&s);
                guard.extract_grouped = grouped;
                guard.settings.extract_order = if grouped { "grouped" } else { "document" }.to_string();
            }
            if let Some(win) = w.upgrade() {
                sync::sync_extract(&win, &lock(&s));
                sync::sync_flags(&win, &lock(&s));
                let _ = crate::callbacks::settings_cb::persist(&win, &s);
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_extract_copy(move || {
            let Some(win) = w.upgrade() else { return };
            let text = lock(&s).extract().text;
            if text.is_empty() {
                toast(&win, "Nothing to copy — tick a colour first");
                return;
            }
            match copy_to_clipboard(&text) {
                Ok(()) => toast(&win, &format!("Copied {} lines", text.lines().count())),
                Err(err) => toast(&win, &format!("Clipboard unavailable: {err}")),
            }
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_extract_export(move || {
            let Some(win) = w.upgrade() else { return };
            let result = lock(&s).extract();
            if result.text.is_empty() {
                toast(&win, "Nothing to export — tick a colour first");
                return;
            }
            let label = lock(&s)
                .extract_selected
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join("-");
            win.invoke_save_extracted_text(result.text.as_str().into(), label.as_str().into());
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_extract_open_in_tab(move || {
            let Some(win) = w.upgrade() else { return };
            let result = lock(&s).extract();
            if result.text.is_empty() {
                toast(&win, "Nothing to open — tick a colour first");
                return;
            }
            {
                let mut guard = lock(&s);
                guard.new_tab();
                guard.load_text("Extracted", &result.text, None);
            }
            sync::sync_all(&win, &lock(&s));
            toast(&win, &format!("{} lines extracted", result.line_count));
        });
    }

    {
        let s = state.clone();
        let w = window.as_weak();
        window.on_extract_closed(move || {
            lock(&s).extract_open = false;
            if let Some(win) = w.upgrade() {
                sync::sync_flags(&win, &lock(&s));
            }
        });
    }
}

/// Copy text to the system clipboard.
///
/// On a headless machine there is no clipboard to talk to; the error is
/// surfaced as a toast rather than silently doing nothing (bug #10).
pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| err.to_string())?;
    clipboard.set_text(text.to_string()).map_err(|err| err.to_string())
}

/// Highlight the current selection with the armed colour (Ctrl+Shift+H).
pub fn toggle_armed(window: &AppWindow, state: &SharedState) {
    let key = lock(state).armed_colour.clone();
    let key = if key.is_empty() {
        "yellow".to_string()
    } else {
        key
    };
    match lock(state).toggle_highlight_key(&key) {
        Some(true) => {
            sync::sync_all(window, &lock(state));
            toast(window, &format!("Highlighted {key}"));
        }
        Some(false) => {
            sync::sync_all(window, &lock(state));
            toast(window, "Highlight removed");
        }
        None => toast(window, &format!("Unknown colour: {key}")),
    }
}

/// Insert the current date and time at the cursor line.
pub fn insert_datetime(window: &AppWindow, state: &SharedState) {
    let stamp = current_datetime();
    {
        let mut guard = lock(state);
        let index = guard.cursor.line;
        let existing = guard.doc().lines[index].text.clone();
        let joined = if existing.is_empty() {
            stamp.clone()
        } else {
            format!("{existing} {stamp}")
        };
        guard.set_line_text(index, &joined);
        guard.cursor.col = joined.chars().count();
    }
    sync::sync_all(window, &lock(state));
}

/// Local date-time without pulling in `chrono`.
pub fn current_datetime() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let hh = (secs % 86_400) / 3_600;
    let mm = (secs % 3_600) / 60;
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

/// Howard Hinnant's civil_from_days — days since epoch to y/m/d.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notepad_pro_core::types::line::LineColour;

    fn state() -> crate::state::AppState {
        crate::state::AppState::new(
            notepad_pro_core::config::settings::Settings::default(),
            notepad_pro_core::db::notes::NotesDb::in_memory().unwrap(),
        )
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        // 1970-01-01 is day 0.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2000-03-01 is day 11017.
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        // 2024-02-29 (a leap day) is day 19781.
        assert_eq!(civil_from_days(19_781), (2024, 2, 29));
    }

    #[test]
    fn current_datetime_is_well_formed() {
        let stamp = current_datetime();
        assert_eq!(stamp.len(), 16, "{stamp}");
        assert_eq!(stamp.chars().nth(4), Some('-'));
        assert_eq!(stamp.chars().nth(10), Some(' '));
        assert_eq!(stamp.chars().nth(13), Some(':'));
        assert!(stamp.starts_with("20"), "{stamp}");
    }

    #[test]
    fn appending_a_timestamp_keeps_existing_text() {
        let mut s = state();
        s.load_text("t.txt", "Meeting", None);
        s.cursor.line = 0;
        let existing = s.doc().lines[0].text.clone();
        let joined = format!("{existing} {}", current_datetime());
        s.set_line_text(0, &joined);
        assert!(s.doc().lines[0].text.starts_with("Meeting 20"));
    }

    #[test]
    fn toggle_armed_uses_the_last_chosen_colour() {
        let mut s = state();
        s.load_text("t.txt", "a\nb", None);
        s.armed_colour = "green".into();
        assert_eq!(s.toggle_highlight_key("green"), Some(true));
        assert_eq!(s.doc().lines[0].colour, LineColour::Green);
    }

    #[test]
    fn extraction_of_an_unticked_palette_is_empty() {
        let mut s = state();
        s.load_text("t.txt", "a", None);
        s.extract_selected.clear();
        assert_eq!(s.extract().text, "");
    }
}
