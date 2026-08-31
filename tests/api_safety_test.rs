//! API safety: the serde DTOs must tolerate old, partial and hostile JSON
//! (5 checks). These guard the session/settings load path against corrupt or
//! forward-incompatible files.

use notepad_pro_core::config::session::Session;
use notepad_pro_core::types::api::TabState;
use notepad_pro_core::types::note::Note;

#[test]
fn session_deserializes_camel_case_keys() {
    let json = r#"{
        "version": 2,
        "activeTab": 0,
        "tabs": [ { "id": "a", "name": "n", "cursorLine": 3, "cursorCol": 1 } ],
        "documents": ["hello"],
        "highlights": ["{}"],
        "listStructures": ["[]"],
        "window": { "width": 100, "height": 50 }
    }"#;
    let s: Session = serde_json::from_str(json).unwrap();
    assert_eq!(s.active_tab, 0);
    assert_eq!(s.tabs[0].cursor_line, 3);
    assert_eq!(s.window.width, 100);
}

#[test]
fn session_fills_defaults_for_missing_fields() {
    let s: Session = serde_json::from_str("{}").unwrap();
    assert!(s.tabs.is_empty());
    assert_eq!(s.window.width, 1200);
    assert_eq!(s.window.height, 800);
}

#[test]
fn session_repair_clamps_active_tab_and_vectors() {
    let mut s = Session {
        version: 2,
        active_tab: 99,
        tabs: vec![TabState::new("a")],
        documents: vec![],
        highlights: vec![],
        list_structures: vec![],
        window: Default::default(),
    };
    s.repair();
    assert_eq!(s.documents.len(), 1);
    assert_eq!(s.highlights.len(), 1);
    assert_eq!(s.active_tab, 0, "active must clamp to the last tab");
}

#[test]
fn session_repair_drops_idless_tabs() {
    let mut s = Session {
        version: 2,
        active_tab: 0,
        tabs: vec![TabState::new("ok"), TabState { id: String::new(), ..TabState::new("bad") }],
        documents: vec!["a".into(), "b".into()],
        highlights: vec!["{}".into(), "{}".into()],
        list_structures: vec!["[]".into(), "[]".into()],
        window: Default::default(),
    };
    s.repair();
    assert_eq!(s.tabs.len(), 1);
    assert_eq!(s.documents.len(), 1);
}

#[test]
fn note_data_roundtrips_through_note() {
    let mut note = Note::default();
    note.title = "t".into();
    note.content = "body".into();
    note.pinned = true;
    let data = notepad_pro_core::types::api::NoteData::from_note(&note);
    let back = data.to_note();
    assert_eq!(back.title, "t");
    assert_eq!(back.content, "body");
    assert_eq!(back.pinned, true);
}
