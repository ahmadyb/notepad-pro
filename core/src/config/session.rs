//! Session persistence: which tabs were open, and what they contained.

use std::path::Path;

use anyhow::{Context, Result};

use crate::types::api::Session;

/// Current on-disk session format.
pub const SESSION_VERSION: u32 = 2;

/// Reads and writes `session.json`.
#[derive(Debug, Clone)]
pub struct SessionStore {
    path: std::path::PathBuf,
}

impl SessionStore {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the session, tolerating a missing or corrupt file.
    pub fn load(&self) -> Session {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Session {
                version: SESSION_VERSION,
                ..Default::default()
            };
        };
        Self::parse(&text)
    }

    /// Parse without touching the filesystem. Unknown versions and truncated
    /// documents are repaired rather than rejected.
    pub fn parse(text: &str) -> Session {
        let mut session: Session = match serde_json::from_str(text) {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(%err, "session.json is unreadable; starting fresh");
                return Session {
                    version: SESSION_VERSION,
                    ..Default::default()
                };
            }
        };
        session.version = SESSION_VERSION;
        session.repair();
        session
    }

    /// Atomic write.
    pub fn save(&self, session: &Session) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let mut to_write = session.clone();
        to_write.version = SESSION_VERSION;
        let json = serde_json::to_string(&to_write)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json.as_bytes())
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("cannot install {}", self.path.display()))?;
        Ok(())
    }

    /// Delete the stored session ("Start with a blank document").
    pub fn clear(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("cannot delete {}", self.path.display())),
        }
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}

impl Session {
    /// Fix up anything that a hand-edited or partially written file could
    /// have broken.
    pub fn repair(&mut self) {
        // Keep the parallel vectors the same length as `tabs`.
        self.documents.resize(self.tabs.len(), String::new());
        self.highlights.resize(self.tabs.len(), "{}".to_string());
        self.list_structures.resize(self.tabs.len(), "[]".to_string());

        // Drop tabs with no id (a half-written entry).
        let mut keep = Vec::with_capacity(self.tabs.len());
        for (i, tab) in self.tabs.iter().enumerate() {
            if tab.id.is_empty() {
                continue;
            }
            keep.push(i);
        }
        if keep.len() != self.tabs.len() {
            self.tabs = keep.iter().map(|&i| self.tabs[i].clone()).collect();
            self.documents = keep.iter().map(|&i| self.documents[i].clone()).collect();
            self.highlights = keep.iter().map(|&i| self.highlights[i].clone()).collect();
            self.list_structures = keep
                .iter()
                .map(|&i| self.list_structures[i].clone())
                .collect();
        }

        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len().saturating_sub(1);
        }
        for tab in self.tabs.iter_mut() {
            if tab.name.trim().is_empty() {
                tab.name = match &tab.path {
                    Some(p) => crate::files::manager::file_name(p),
                    None => "Untitled".to_string(),
                };
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::api::TabState;

    fn store() -> (tempfile::TempDir, SessionStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        (dir, SessionStore::new(path))
    }

    fn sample_session() -> Session {
        Session {
            version: SESSION_VERSION,
            active_tab: 1,
            tabs: vec![TabState::new("a.txt"), TabState::new("b.txt")],
            documents: vec!["alpha".into(), "beta".into()],
            highlights: vec!["{}".into(), r#"{"0":"pink"}"#.into()],
            list_structures: vec!["[]".into(), "[]".into()],
            window: Default::default(),
        }
    }

    #[test]
    fn missing_file_yields_an_empty_session() {
        let (_dir, store) = store();
        let session = store.load();
        assert!(session.is_empty());
        assert_eq!(session.version, SESSION_VERSION);
    }

    #[test]
    fn corrupt_file_yields_an_empty_session() {
        let (_dir, store) = store();
        std::fs::write(store.path(), "{{{").unwrap();
        assert!(store.load().is_empty());
    }

    #[test]
    fn save_and_load_roundtrips() {
        let (_dir, store) = store();
        let original = sample_session();
        store.save(&original).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.tabs.len(), 2);
        assert_eq!(loaded.active_tab, 1);
        assert_eq!(loaded.documents[0], "alpha");
        assert_eq!(loaded.highlights[1], r#"{"0":"pink"}"#);
    }

    #[test]
    fn save_stamps_the_current_version() {
        let (_dir, store) = store();
        let mut session = sample_session();
        session.version = 1;
        store.save(&session).unwrap();
        assert_eq!(store.load().version, SESSION_VERSION);
    }

    #[test]
    fn parse_pads_truncated_parallel_vectors() {
        let json = r#"{"version":2,"activeTab":0,
            "tabs":[{"id":"t1","name":"a.txt","path":null,"dirty":false,"noteId":null,
                     "lineEnding":"LF","encoding":"utf-8","cursorLine":0,"cursorCol":0,"scrollTop":0.0}],
            "documents":[],"highlights":[],"listStructures":[],
            "window":{"width":1200,"height":800}}"#;
        let session = SessionStore::parse(json);
        assert_eq!(session.documents.len(), 1);
        assert_eq!(session.highlights.len(), 1);
        assert_eq!(session.list_structures.len(), 1);
        assert_eq!(session.highlights[0], "{}");
    }

    #[test]
    fn parse_clamps_an_out_of_range_active_tab() {
        let mut session = sample_session();
        session.active_tab = 99;
        let json = serde_json::to_string(&session).unwrap();
        assert_eq!(SessionStore::parse(&json).active_tab, 1);
    }

    #[test]
    fn parse_drops_tabs_without_an_id() {
        let mut session = sample_session();
        session.tabs.push(TabState {
            id: String::new(),
            ..TabState::new("ghost")
        });
        session.documents.push("ghost".into());
        session.highlights.push("{}".into());
        session.list_structures.push("[]".into());
        let json = serde_json::to_string(&session).unwrap();
        let parsed = SessionStore::parse(&json);
        assert_eq!(parsed.tabs.len(), 2);
        assert_eq!(parsed.documents.len(), 2);
        assert!(parsed.documents.iter().all(|d| d != "ghost"));
    }

    #[test]
    fn parse_renames_blank_tabs_from_their_path() {
        let mut session = sample_session();
        session.tabs[0].name = "  ".into();
        session.tabs[0].path = Some("/tmp/report.txt".into());
        let json = serde_json::to_string(&session).unwrap();
        let parsed = SessionStore::parse(&json);
        assert_eq!(parsed.tabs[0].name, "report.txt");
    }

    #[test]
    fn clear_removes_the_file_and_is_idempotent() {
        let (_dir, store) = store();
        store.save(&sample_session()).unwrap();
        assert!(store.exists());
        store.clear().unwrap();
        assert!(!store.exists());
        store.clear().unwrap();
    }

    #[test]
    fn clear_on_a_missing_file_is_not_an_error() {
        let (_dir, store) = store();
        assert!(store.clear().is_ok());
    }

    #[test]
    fn save_creates_missing_directories() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("deep/session.json"));
        store.save(&sample_session()).unwrap();
        assert!(store.exists());
    }

    #[test]
    fn repair_on_an_empty_session_is_a_noop() {
        let mut session = Session::default();
        session.repair();
        assert!(session.is_empty());
        assert_eq!(session.active_tab, 0);
    }
}
