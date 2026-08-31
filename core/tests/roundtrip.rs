//! End-to-end checks that exercise the whole core pipeline: text on disk in,
//! highlights applied, note stored in SQLite, session written, everything
//! read back unchanged.

use notepad_pro_core::config::session::SessionStore;
use notepad_pro_core::config::settings::Settings;
use notepad_pro_core::db::notes::{NotesDb, SortOrder};
use notepad_pro_core::editor::{Document, FindEngine, ListEngine};
use notepad_pro_core::files::line_endings::LineEnding;
use notepad_pro_core::files::manager::{load_file, save_file};
use notepad_pro_core::highlight::extractor::{extract, ExtractionOrder};
use notepad_pro_core::highlight::palette::{
    apply_highlights_json, highlights_json_for, list_structure_json_for, Palette,
};
use notepad_pro_core::highlight::stats::breakdown;
use notepad_pro_core::types::line::{EditorLine, LineColour, ListType};
use notepad_pro_core::types::note::{Note, NoteMetadata};

const DOCUMENT: &str = "\
Sprint review
- ship the highlight feature
- fix the CRLF bug
[] remember to sign the build
1. write the release notes
2. tag v1.0.2
blocked on legal review
done: packaging";

#[test]
fn full_document_roundtrip_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sprint.txt");

    save_file(&path, DOCUMENT, "utf-8", LineEnding::Crlf).unwrap();
    let loaded = load_file(&path).unwrap();

    assert_eq!(loaded.content, DOCUMENT);
    assert_eq!(loaded.line_ending, LineEnding::Crlf);
    assert_eq!(loaded.encoding, "utf-8");

    let doc = Document::from_plain_text(&loaded.content);
    assert_eq!(doc.line_count(), DOCUMENT.lines().count());
    assert_eq!(doc.plain_text(), DOCUMENT);
}

#[test]
fn highlight_extract_and_persist_pipeline() {
    let mut doc = Document::from_plain_text(DOCUMENT);

    // Highlight every "- " line yellow and every "[]"/numbered line pink.
    let mut yellow = Vec::new();
    let mut pink = Vec::new();
    for (index, line) in doc.lines.iter().enumerate() {
        if line.text.starts_with("- ") {
            yellow.push(index);
        } else if line.text.starts_with("[] ") || line.text.starts_with("1. ") {
            pink.push(index);
        }
    }
    assert!(!yellow.is_empty() && !pink.is_empty());
    for i in &yellow {
        doc.highlight_lines(*i, *i, LineColour::Yellow);
    }
    for i in &pink {
        doc.highlight_lines(*i, *i, LineColour::Pink);
    }

    // Statistics agree with what was applied.
    let palette = Palette::default();
    let b = breakdown(&doc.lines, &palette);
    assert_eq!(b.highlighted_lines, yellow.len() + pink.len());
    assert_eq!(b.counts.len(), 2);

    // Extraction in document order keeps the original sequence.
    let result = extract(
        &doc.lines,
        &[LineColour::Pink, LineColour::Yellow],
        ExtractionOrder::Document,
    );
    let first_yellow = doc.lines[yellow[0]].text.clone();
    assert!(result.text.starts_with(&first_yellow));
    assert_eq!(result.line_count, yellow.len() + pink.len());

    // Extraction grouped by colour puts pink first because it was requested first.
    let grouped = extract(
        &doc.lines,
        &[LineColour::Pink, LineColour::Yellow],
        ExtractionOrder::GroupByColour,
    );
    assert!(grouped.text.starts_with("# Pink"));

    // Persist to SQLite and read the highlights back.
    let db = NotesDb::in_memory().unwrap();
    let mut note = Note::new("Sprint review", doc.plain_text());
    note.highlights_json = highlights_json_for(&doc.lines);
    note.list_structure_json = list_structure_json_for(&doc.lines);
    let id = db.save(&note).unwrap();

    let stored = db.get(id).unwrap().unwrap();
    let mut restored = Document::from_plain_text(&stored.content);
    let applied = apply_highlights_json(&mut restored.lines, &stored.highlights_json);
    assert_eq!(applied, yellow.len() + pink.len());
    for i in &yellow {
        assert_eq!(restored.lines[*i].colour, LineColour::Yellow);
    }
    for i in &pink {
        assert_eq!(restored.lines[*i].colour, LineColour::Pink);
    }
}

#[test]
fn markdown_shortcuts_then_enter_produces_a_nested_list() {
    let mut doc = Document::from_lines(vec![EditorLine::default()]);

    doc.set_text(0, "- top level");
    assert_eq!(doc.lines[0].list_type, ListType::Bullet);

    let caret = doc.lines[0].char_len();
    match ListEngine::handle_enter(&mut doc.lines, 0, caret) {
        notepad_pro_core::editor::list_engine::EnterOutcome::MoveTo(i) => assert_eq!(i, 1),
        other => panic!("expected MoveTo, got {other:?}"),
    }
    doc.lines[1].list_type = ListType::Bullet;
    doc.set_text(1, "nested");
    doc.change_indent(1, 1, 1);

    ListEngine::renumber(&mut doc.lines);
    assert_eq!(doc.lines[0].indent, 0);
    assert_eq!(doc.lines[1].indent, 1);
    assert_eq!(ListEngine::marker_text(&doc.lines[0]).as_deref(), Some("•"));
    assert_eq!(ListEngine::marker_text(&doc.lines[1]).as_deref(), Some("◦"));
}

#[test]
fn numbered_lists_renumber_after_an_insert() {
    let mut doc = Document::from_lines(vec![
        EditorLine {
            text: "one".into(),
            list_type: ListType::Number,
            ..Default::default()
        },
        EditorLine {
            text: "two".into(),
            list_type: ListType::Number,
            ..Default::default()
        },
        EditorLine {
            text: "three".into(),
            list_type: ListType::Number,
            ..Default::default()
        },
    ]);
    assert_eq!(
        doc.lines.iter().map(|l| l.number).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    doc.mutate(|lines| {
        lines.insert(
            1,
            EditorLine {
                text: "inserted".into(),
                list_type: ListType::Number,
                ..Default::default()
            },
        )
    });
    assert_eq!(
        doc.lines.iter().map(|l| l.number).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );

    // Removing a plain paragraph resets the counter for the list after it.
    doc.mutate(|lines| lines.insert(2, EditorLine::new("a paragraph")));
    assert_eq!(
        doc.lines.iter().map(|l| l.number).collect::<Vec<_>>(),
        vec![1, 2, 0, 1, 2]
    );
}

#[test]
fn find_replace_then_undo_restores_the_document() {
    let mut doc = Document::from_plain_text("the cat sat on the cat mat\nanother cat");

    let mut engine = FindEngine::new();
    engine.set_query("cat");
    engine.set_replacement("dog");
    engine.search(&doc.lines);
    assert_eq!(engine.match_count(), 3);

    let replaced = engine.replace_all(&mut doc.lines);
    assert_eq!(replaced, 3);
    assert_eq!(doc.lines[0].text, "the dog sat on the dog mat");
    assert_eq!(doc.lines[1].text, "another dog");
    // Commit the in-place replacement so it becomes an undoable step.
    doc.mutate(|_| {});

    assert!(doc.undo(), "the replace must be undoable");
    assert_eq!(doc.lines[0].text, "the cat sat on the cat mat");
    assert_eq!(doc.lines[1].text, "another cat");

    assert!(doc.redo(), "and redoable");
    assert_eq!(doc.lines[0].text, "the dog sat on the dog mat");
}

#[test]
fn settings_session_and_database_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    let session_path = dir.path().join("session.json");
    let db_path = dir.path().join("notes.db");

    // --- first run -------------------------------------------------------
    let mut settings = Settings::default();
    settings.theme = "clay-dark".into();
    settings.zoom = 1.25;
    settings.remember_file("/tmp/sprint.txt");
    settings.save(&settings_path).unwrap();

    let db = NotesDb::open(&db_path).unwrap();
    let note_id = db.save(&Note::new("Sprint", DOCUMENT)).unwrap();

    let session = notepad_pro_core::types::api::Session {
        version: 2,
        active_tab: 0,
        tabs: vec![notepad_pro_core::types::api::TabState {
            note_id: Some(note_id),
            ..notepad_pro_core::types::api::TabState::new("sprint.txt")
        }],
        documents: vec![DOCUMENT.to_string()],
        highlights: vec![r#"{"0":"yellow"}"#.to_string()],
        list_structures: vec!["[]".to_string()],
        window: Default::default(),
    };
    SessionStore::new(&session_path).save(&session).unwrap();
    drop(db);

    // --- second run ------------------------------------------------------
    let settings2 = Settings::load(&settings_path);
    assert_eq!(settings2.theme, "clay-dark");
    assert_eq!(settings2.zoom, 1.25);
    assert_eq!(settings2.recent_files[0], "/tmp/sprint.txt");

    let db2 = NotesDb::open(&db_path).unwrap();
    assert_eq!(db2.count().unwrap(), 1);

    let session2 = SessionStore::new(&session_path).load();
    assert_eq!(session2.tabs.len(), 1);
    assert_eq!(session2.documents[0], DOCUMENT);

    let note = db2
        .get(session2.tabs[0].note_id.unwrap())
        .unwrap()
        .unwrap();
    let meta = NoteMetadata::summarise(&note);
    assert_eq!(meta.title, "Sprint");
    assert_eq!(meta.snippet, "Sprint review");

    // The sidebar search finds it.
    let found = db2.list("sprint", SortOrder::Modified).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, note_id);
}

#[test]
fn every_builtin_colour_can_be_applied_extracted_and_cleared() {
    for (key, _, _) in notepad_pro_core::highlight::palette::BUILTIN {
        let colour = LineColour::from_key(key);
        let mut doc = Document::from_plain_text("one\ntwo\nthree");
        assert!(doc.toggle_highlight(0, 2, colour));

        let result = extract(&doc.lines, &[colour], ExtractionOrder::Document);
        assert_eq!(result.text, "one\ntwo\nthree", "colour {key}");

        assert!(!doc.toggle_highlight(0, 2, colour), "second toggle clears");
        assert_eq!(doc.highlighted_count(), 0, "colour {key}");
    }
}

#[test]
fn documents_are_never_left_with_zero_lines() {
    let mut doc = Document::from_plain_text("only line");
    doc.remove_line(0);
    assert_eq!(doc.line_count(), 1);
    assert_eq!(doc.plain_text(), "");
}
