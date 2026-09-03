//! Application settings.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::types::note::CustomColour;

/// Directory holding settings.json, session.json and notes.db.
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("NotePadPro")
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn session_path() -> PathBuf {
    data_dir().join("session.json")
}

pub fn db_path() -> PathBuf {
    data_dir().join("notes.db")
}

/// Every user-visible preference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: String,
    pub font_family: String,
    pub font_size: u8,
    pub word_wrap: bool,
    pub zoom: f32,
    pub animations: bool,
    pub sidebar_open: bool,
    pub sidebar_sort: String,
    pub autosave_interval_secs: u32,
    pub extract_order: String,
    /// Use the operating system's window frame instead of the custom one.
    pub native_frame: bool,
    /// Custom highlight swatches added through the colour picker.
    pub custom_palette: Vec<CustomColour>,
    /// Most recently opened files, newest first.
    pub recent_files: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            font_family: "Consolas".into(),
            font_size: 14,
            word_wrap: true,
            zoom: 1.0,
            animations: true,
            sidebar_open: false,
            sidebar_sort: "modified".into(),
            autosave_interval_secs: 4,
            extract_order: "document".into(),
            native_frame: true,
            custom_palette: Vec::new(),
            recent_files: Vec::new(),
        }
    }
}

impl Settings {
    /// Load and merge. Unknown keys are ignored, missing keys fall back to
    /// the defaults, and a corrupt file never prevents startup.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        Self::from_json(&text)
    }

    /// Merge strategy: defaults overlaid with whatever the file provides.
    pub fn from_json(text: &str) -> Self {
        let mut base: serde_json::Value = match serde_json::to_value(Self::default()) {
            Ok(v) => v,
            Err(_) => return Self::default(),
        };
        let Ok(saved) = serde_json::from_str::<serde_json::Value>(text) else {
            tracing::warn!("settings.json is not valid JSON; using defaults");
            return Self::default();
        };
        if let (Some(base_map), Some(saved_map)) = (base.as_object_mut(), saved.as_object()) {
            for (key, value) in saved_map {
                // Only overlay keys we actually know about, so a typo in the
                // file cannot inject an unexpected field.
                if base_map.contains_key(key) {
                    base_map.insert(key.clone(), value.clone());
                } else {
                    tracing::debug!(key = %key, "ignoring unknown settings key");
                }
            }
            // A hand-edited number outside the Rust type's range (fontSize
            // 500 against a u8 field) must not sink the whole document:
            // coerce the known integer fields into range first; `clamp`
            // applies the user-facing ranges afterwards.
            if let Some(v) = base_map.get_mut("fontSize") {
                if let Some(n) = v.as_i64() {
                    *v = serde_json::json!(n.clamp(0, 255));
                }
            }
            if let Some(v) = base_map.get_mut("autosaveIntervalSecs") {
                if let Some(n) = v.as_i64() {
                    *v = serde_json::json!(n.clamp(0, u32::MAX as i64));
                }
            }
        }
        let mut settings: Settings = serde_json::from_value(base).unwrap_or_default();
        settings.clamp();
        settings
    }

    /// Atomic write (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json.as_bytes())
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("cannot install {}", path.display()))?;
        Ok(())
    }

    /// Apply a single `update-settings(key, json_value)` call.
    ///
    /// Returns `true` when the key was recognised and applied.
    pub fn update(&mut self, key: &str, json_value: &str) -> bool {
        let value: serde_json::Value = match serde_json::from_str(json_value) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let mut as_map = match serde_json::to_value(self.clone()) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => return false,
        };
        let camel = to_camel_case(key);
        if !as_map.contains_key(&camel) {
            return false;
        }
        as_map.insert(camel, value);
        match serde_json::from_value::<Settings>(serde_json::Value::Object(as_map)) {
            Ok(mut updated) => {
                updated.clamp();
                *self = updated;
                true
            }
            Err(_) => false,
        }
    }

    /// Keep values inside sane ranges. Called after every load and update.
    pub fn clamp(&mut self) {
        self.font_size = self.font_size.clamp(8, 72);
        self.zoom = self.zoom.clamp(0.5, 3.0);
        self.autosave_interval_secs = self.autosave_interval_secs.clamp(1, 3_600);
        if self.theme.is_empty() {
            self.theme = "light".into();
        }
        if self.font_family.trim().is_empty() {
            self.font_family = "Consolas".into();
        }
        if !crate::highlight::palette::is_known_theme(&self.theme) {
            tracing::warn!(theme = %self.theme, "unknown theme, falling back to light");
            self.theme = "light".into();
        }
        if self.extract_order != "document" && self.extract_order != "grouped" {
            self.extract_order = "document".into();
        }
        if self.sidebar_sort != "modified"
            && self.sidebar_sort != "title"
            && self.sidebar_sort != "created"
        {
            self.sidebar_sort = "modified".into();
        }
        self.recent_files.retain(|p| !p.trim().is_empty());
        self.recent_files.truncate(20);
    }

    /// Push a path onto the recent list, de-duplicating and capping at 20.
    pub fn remember_file(&mut self, path: &str) {
        if path.trim().is_empty() {
            return;
        }
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_string());
        self.recent_files.truncate(20);
    }

    pub fn clear_recent_files(&mut self) {
        self.recent_files.clear();
    }

    pub fn add_custom_colour(&mut self, colour: CustomColour) {
        self.custom_palette.retain(|c| c.hex != colour.hex);
        self.custom_palette.push(colour);
        self.custom_palette.truncate(64);
    }
}

/// `sidebar_sort` -> `sidebarSort` (accepts both spellings from the UI).
fn to_camel_case(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper_next = false;
    for c in key.chars() {
        if c == '_' || c == '-' {
            upper_next = true;
        } else if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_usable_out_of_the_box() {
        let s = Settings::default();
        assert_eq!(s.theme, "light");
        assert_eq!(s.font_size, 14);
        assert_eq!(s.zoom, 1.0);
        assert!(s.word_wrap);
        assert!(s.animations);
        assert!(s.native_frame);
        assert_eq!(s.autosave_interval_secs, 4);
    }

    #[test]
    fn loading_a_missing_file_yields_defaults() {
        let s = Settings::load(Path::new("/nope/settings.json"));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn loading_corrupt_json_yields_defaults() {
        assert_eq!(Settings::from_json("{ not json"), Settings::default());
        assert_eq!(Settings::from_json(""), Settings::default());
        assert_eq!(Settings::from_json("[]"), Settings::default());
    }

    #[test]
    fn partial_json_keeps_defaults_for_missing_keys() {
        let s = Settings::from_json(r#"{"theme":"dark"}"#);
        assert_eq!(s.theme, "dark");
        assert_eq!(s.font_size, 14, "untouched keys keep their default");
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let s = Settings::from_json(r#"{"theme":"dark","evilKey":true}"#);
        assert_eq!(s.theme, "dark");
    }

    #[test]
    fn out_of_range_values_are_clamped_on_load() {
        let s = Settings::from_json(r#"{"fontSize":500,"zoom":99,"autosaveIntervalSecs":0}"#);
        assert_eq!(s.font_size, 72);
        assert_eq!(s.zoom, 3.0);
        assert_eq!(s.autosave_interval_secs, 1);
    }

    #[test]
    fn one_bad_value_does_not_sink_the_rest_of_the_file() {
        let s = Settings::from_json(r#"{"theme":"dark","fontSize":500,"zoom":99}"#);
        assert_eq!(s.theme, "dark", "valid keys survive a bad sibling");
        assert_eq!(s.font_size, 72);
        assert_eq!(s.zoom, 3.0);
    }

    #[test]
    fn an_unknown_theme_falls_back_to_light() {
        let s = Settings::from_json(r#"{"theme":"solarised-ultra"}"#);
        assert_eq!(s.theme, "light");
    }

    #[test]
    fn every_known_theme_is_accepted() {
        for theme in crate::highlight::palette::THEMES {
            let s = Settings::from_json(&format!(r#"{{"theme":"{theme}"}}"#));
            assert_eq!(s.theme, *theme);
        }
    }

    #[test]
    fn update_accepts_snake_case_keys() {
        let mut s = Settings::default();
        assert!(s.update("theme", r#""dark""#));
        assert_eq!(s.theme, "dark");
        assert!(s.update("sidebar_sort", r#""title""#));
        assert_eq!(s.sidebar_sort, "title");
    }

    #[test]
    fn update_accepts_camel_case_keys_too() {
        let mut s = Settings::default();
        assert!(s.update("wordWrap", "false"));
        assert!(!s.word_wrap);
    }

    #[test]
    fn update_rejects_unknown_keys_and_bad_values() {
        let mut s = Settings::default();
        assert!(!s.update("nope", "1"));
        assert!(!s.update("theme", "not json"));
        assert!(!s.update("fontSize", r#""not a number""#));
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn update_clamps_the_value_it_sets() {
        let mut s = Settings::default();
        assert!(s.update("zoom", "99"));
        assert_eq!(s.zoom, 3.0);
    }

    #[test]
    fn save_and_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = Settings::default();
        s.theme = "dark".into();
        s.remember_file("/tmp/a.txt");
        s.add_custom_colour(CustomColour::new("Sunset", "#ff8800"));
        s.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded, s);
    }

    #[test]
    fn save_creates_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        Settings::default().save(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn save_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        Settings::default().save(&path).unwrap();
        assert!(dir.path().join("settings.json.tmp").exists() == false);
    }

    #[test]
    fn remember_file_deduplicates_and_caps() {
        let mut s = Settings::default();
        for i in 0..30 {
            s.remember_file(&format!("/tmp/{i}.txt"));
        }
        assert_eq!(s.recent_files.len(), 20);
        assert_eq!(s.recent_files[0], "/tmp/29.txt");
        s.remember_file("/tmp/0.txt");
        assert_eq!(s.recent_files[0], "/tmp/0.txt");
        assert_eq!(s.recent_files.len(), 20, "no duplicate added");
        assert_eq!(
            s.recent_files.iter().filter(|p| *p == "/tmp/0.txt").count(),
            1
        );
    }

    #[test]
    fn remember_file_ignores_blank_paths() {
        let mut s = Settings::default();
        s.remember_file("   ");
        assert!(s.recent_files.is_empty());
    }

    #[test]
    fn clear_recent_files_empties_the_list() {
        let mut s = Settings::default();
        s.remember_file("/tmp/a.txt");
        s.clear_recent_files();
        assert!(s.recent_files.is_empty());
    }

    #[test]
    fn custom_colours_are_deduplicated_by_hex() {
        let mut s = Settings::default();
        s.add_custom_colour(CustomColour::new("A", "#ff8800"));
        s.add_custom_colour(CustomColour::new("B", "#ff8800"));
        assert_eq!(s.custom_palette.len(), 1);
        assert_eq!(s.custom_palette[0].name, "B");
    }

    #[test]
    fn camel_case_conversion_handles_both_separators() {
        assert_eq!(to_camel_case("sidebar_sort"), "sidebarSort");
        assert_eq!(to_camel_case("sidebar-sort"), "sidebarSort");
        assert_eq!(to_camel_case("theme"), "theme");
        assert_eq!(to_camel_case("autosave_interval_secs"), "autosaveIntervalSecs");
    }

    #[test]
    fn data_paths_live_under_one_directory() {
        let dir = data_dir();
        assert_eq!(settings_path().parent().unwrap(), dir);
        assert_eq!(session_path().parent().unwrap(), dir);
        assert_eq!(db_path().parent().unwrap(), dir);
        assert_eq!(db_path().file_name().unwrap(), "notes.db");
    }
}
