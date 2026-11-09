//! Previous versions: opened beside the workbook, read only, compared, restored, saved as a
//! copy and closed. The history store itself is tested in `versions`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::version_view::READ_ONLY;
use crate::versions::VersionInfo;
use crate::{EngineError, Session};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gridcraft-version-view-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn p(path: &Path) -> Value {
    json!(path.to_string_lossy())
}

fn put(s: &mut Session, cell: &str, text: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": text})).unwrap();
}

fn text(s: &Session, cell: &str) -> String {
    s.doc().unwrap().wb.active().unwrap().value(gridcraft_core::CellRef::parse(cell).unwrap()).display()
}

fn infos(s: &mut Session, at_least: usize) -> Vec<VersionInfo> {
    let mut list = Vec::new();
    for _ in 0..1000 {
        list = s.version_infos();
        if list.len() >= at_least {
            return list;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    list
}

/// A session with the history under `root/versions`, host AutoSave on, and Notes.xlsx saved twice:
/// A1 was "first", then "second"; A2 holds an unsaved "unsaved" afterwards. Returns the session,
/// the file, and the id of the "first" version.
fn with_history(root: &Path) -> (Session, PathBuf, String) {
    let mut s = Session::new();
    s.prefs.user_name = "Tester Test".into();
    s.enable_versions(root.join("versions"), Arc::new(|| {})).unwrap();
    s.autosave_host = true;
    s.new_workbook();
    let file = root.join("Notes.xlsx");
    put(&mut s, "A1", "first");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    let first = infos(&mut s, 1).first().map(|v| v.id.clone()).unwrap();
    put(&mut s, "A1", "second");
    s.execute("file.save", json!({})).unwrap();
    infos(&mut s, 2);
    put(&mut s, "A2", "unsaved");
    (s, file, first)
}

fn docs(s: &Session) -> usize {
    s.documents().len()
}

#[test]
fn a_version_opens_beside_the_workbook_read_only() {
    let root = temp("open");
    let (mut s, file, first) = with_history(&root);
    let source = s.doc().unwrap().uid;
    assert!(s.doc().unwrap().is_dirty());
    let r = s.execute("file.openVersion", json!({"id": first})).unwrap();
    assert_eq!(r["opened"], json!(first));
    // AutoSave wrote the pending change first: it is a version of its own.
    infos(&mut s, 3);
    assert!(!s.documents().iter().find(|d| d.uid == source).unwrap().is_dirty());
    // The version is another workbook, active, titled with its date, with no file of its own.
    assert_eq!(docs(&s), 2);
    let v = s.doc().unwrap();
    assert!(v.is_previous_version() && v.path.is_none() && !v.is_dirty());
    assert!(v.display_title().starts_with("Notes (version ") && v.display_title().ends_with(").xlsx"), "{}", v.display_title());
    assert_eq!(text(&s, "A1"), "first");
    assert_eq!(text(&s, "A2"), "");
    let view = s.previous_version().unwrap();
    assert_eq!((view.source_uid, view.id.as_str()), (source, first.as_str()));
    assert_eq!(view.source_path, file.to_string_lossy());
    // Opening it again brings the same one forward.
    let again = s.execute("file.openVersion", json!({"id": first})).unwrap();
    assert_eq!(again["alreadyOpen"], json!(true));
    assert_eq!(docs(&s), 2);
    // The history can be listed from the version.
    assert!(s.execute("file.versions", json!({})).unwrap().as_array().unwrap().len() >= 3);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn edits_are_refused_in_a_version_but_looking_and_copying_work() {
    let root = temp("readonly");
    let (mut s, _file, first) = with_history(&root);
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    let revision = s.doc().unwrap().revision;
    for (id, params) in [
        ("cell.set", json!({"cell": "A1", "input": "x"})),
        ("home.bold", json!({"range": "A1"})),
        ("edit.clearContents", json!({"range": "A1"})),
        ("edit.undo", json!({})),
        ("edit.redo", json!({})),
        ("edit.cut", json!({})),
        ("sheet.rename", json!({"name": "Other"})),
        ("file.save", json!({})),
        ("file.rename", json!({"name": "x"})),
    ] {
        match s.execute(id, params) {
            Err(EngineError::Disabled(_, why)) => assert_eq!(why, READ_ONLY, "{id}"),
            other => panic!("{id} was not refused: {other:?}"),
        }
    }
    assert_eq!(text(&s, "A1"), "first");
    assert_eq!(s.doc().unwrap().revision, revision);
    assert!(s.doc().unwrap().undo.is_empty() && !s.doc().unwrap().is_dirty());
    // The ribbon sees the same: an edit is greyed out with the reason, the view is not.
    let info = |s: &Session, id: &str| crate::find_command(id).unwrap().info(s);
    assert!(!info(&s, "home.bold").enabled && info(&s, "home.bold").disabled_reason.as_deref() == Some(READ_ONLY));
    assert!(info(&s, "view.zoom").enabled);
    // Looking around, selecting and copying still work.
    s.execute("selection.set", json!({"cell": "B3"})).unwrap();
    s.execute("selection.set", json!({"range": "A1:A2"})).unwrap();
    s.execute("view.zoom", json!({"percent": 150})).unwrap();
    s.execute("edit.copy", json!({})).unwrap();
    assert!(s.clipboard.is_some());
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    // AutoSave does not apply to it.
    assert_eq!(s.doc().unwrap().autosave_block(), Some(crate::autosave::AutoSaveBlock::PreviousVersion));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn close_returns_to_the_workbook_exactly_as_it_was() {
    let root = temp("close");
    let (mut s, _file, first) = with_history(&root);
    // AutoSave off: the unsaved change must still be there afterwards, with its undo history.
    s.doc_mut().unwrap().autosave = false;
    let source = s.doc().unwrap().uid;
    let (stamp, undo, selection) = {
        let d = s.doc().unwrap();
        (d.stamp.clone(), d.undo.len(), d.selection.clone())
    };
    assert!(stamp.is_some());
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    assert_eq!(docs(&s), 2);
    let r = s.execute("file.closeVersion", json!({})).unwrap();
    assert_eq!(r["closed"], json!(first));
    assert_eq!(docs(&s), 1);
    let d = s.doc().unwrap();
    assert_eq!(d.uid, source, "the workbook it came from is active again");
    assert!(d.is_dirty() && d.undo.len() == undo && d.stamp == stamp && d.selection == selection && !d.autosave);
    assert_eq!((text(&s, "A1").as_str(), text(&s, "A2").as_str()), ("second", "unsaved"));
    // Nothing to close any more.
    assert!(s.execute("file.closeVersion", json!({})).is_err());
    // File › Close on a version closes only the version.
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    assert_eq!(s.execute("file.close", json!({})).unwrap()["version"], json!(true));
    assert_eq!((docs(&s), s.doc().unwrap().uid), (1, source));
    assert!(s.doc().unwrap().is_dirty(), "the workbook still has its changes to ask about");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn new_and_open_first_return_to_the_workbook() {
    let root = temp("newopen");
    let (mut s, file, first) = with_history(&root);
    s.doc_mut().unwrap().autosave = false;
    let source = s.doc().unwrap().uid;
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    s.execute("file.new", json!({})).unwrap();
    assert!(s.previous_versions().next().is_none(), "the version closed first");
    assert_eq!(docs(&s), 2);
    assert!(s.documents().iter().any(|d| d.uid == source && d.is_dirty()), "the workbook keeps its unsaved changes");
    // Open: the same.
    s.set_active(s.document_index(source).unwrap());
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    let other = root.join("Other.xlsx");
    let mut t = Session::new();
    t.new_workbook();
    put(&mut t, "A1", "other");
    t.execute("file.save", json!({"path": p(&other)})).unwrap();
    s.execute("file.open", json!({"path": p(&other)})).unwrap();
    assert!(s.previous_versions().next().is_none());
    assert_eq!(text(&s, "A1"), "other");
    assert!(s.documents().iter().any(|d| d.uid == source && d.path.as_deref() == Some(&*file.to_string_lossy())));
    // Quitting: the interface closes the versions first.
    s.set_active(s.document_index(source).unwrap());
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    s.close_previous_versions();
    assert!(s.previous_versions().next().is_none());
    assert_eq!(s.doc().unwrap().uid, source);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn restore_makes_the_version_current_in_one_undo_step() {
    let root = temp("restore");
    let (mut s, _file, first) = with_history(&root);
    s.doc_mut().unwrap().autosave = false;
    let source = s.doc().unwrap().uid;
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    let r = s.execute("file.restoreVersion", json!({})).unwrap();
    assert_eq!(r["restored"], json!(first));
    assert_eq!((docs(&s), s.doc().unwrap().uid), (1, source), "the version closed and the workbook is active");
    assert_eq!((text(&s, "A1").as_str(), text(&s, "A2").as_str()), ("first", ""));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!((text(&s, "A1").as_str(), text(&s, "A2").as_str()), ("second", "unsaved"), "one Undo brings the workbook back");
    // The restore is a version at once, and the unsaved state before it was kept as one.
    let list = infos(&mut s, 4);
    assert!(list.iter().any(|v| v.kind == crate::versions::VersionKind::Restore));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn save_a_copy_makes_a_new_document_that_remembers_its_origin() {
    let root = temp("copy");
    let (mut s, file, first) = with_history(&root);
    s.doc_mut().unwrap().autosave = false;
    let source = s.doc().unwrap().uid;
    let source_id = s.version_document_id(source).unwrap();
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    // Not over the workbook it came from.
    assert!(s.execute("file.saveVersionCopy", json!({"path": p(&file)})).is_err());
    assert!(s.previous_version().is_some());
    // Without a path the interface is asked.
    s.take_ui_requests();
    s.execute("file.saveVersionCopy", json!({})).unwrap();
    assert!(s.take_ui_requests().iter().any(|q| matches!(q, crate::UiRequest::Dialog(n, _) if n == "saveVersionCopy")));
    let copy = root.join("Notes copy.xlsx");
    let r = s.execute("file.saveVersionCopy", json!({"path": p(&copy)})).unwrap();
    assert_eq!((r["saved"].clone(), r["closed"].clone()), (json!(true), json!(first)));
    assert_eq!((docs(&s), s.doc().unwrap().uid), (1, source), "back to the workbook");
    assert_eq!(text(&s, "A1"), "second");
    // The copy holds the version, under a new document id whose first version names the origin.
    let mut t = Session::new();
    t.execute("file.open", json!({"path": p(&copy)})).unwrap();
    assert_eq!(text(&t, "A1"), "first");
    s.execute("file.open", json!({"path": p(&copy)})).unwrap();
    let list = infos(&mut s, 1);
    let origin = list.iter().find_map(|v| v.details.as_ref().and_then(|d| d.origin.clone())).expect("the copy's first version has an origin");
    assert_eq!(origin.document_id.as_deref(), Some(source_id.as_str()));
    assert_eq!(origin.version_id.as_deref(), Some(first.as_str()), "the version it was made from");
    assert_ne!(s.version_document_id(s.doc().unwrap().uid).unwrap(), source_id);
    s.set_active(s.document_index(source).unwrap());
    // The workbook's own history is unchanged by the copy.
    assert_eq!(s.version_infos().iter().filter(|v| v.details.as_ref().is_some_and(|d| d.origin.is_some())).count(), 0);
    assert_eq!(s.doc().unwrap().uid, source);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compare_highlights_what_differs_and_shows_the_version_again() {
    let root = temp("compare");
    let (mut s, _file, first) = with_history(&root);
    s.doc_mut().unwrap().autosave = false;
    put(&mut s, "C3", "new in current");
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    let plain = s.doc().unwrap().wb.clone();
    let r = s.execute("file.compareVersion", json!({})).unwrap();
    // A1 changed ("first" -> "second"), A2 and C3 exist only in the current workbook.
    assert_eq!(r["differences"], json!(3));
    assert_eq!(r["sheets"][0]["cells"], json!(3));
    assert_eq!(r["on"], json!(true));
    let view = s.previous_version().unwrap();
    assert!(view.compared);
    let d = s.doc().unwrap();
    assert!(!d.is_dirty() && !Arc::ptr_eq(&d.wb, &plain));
    let sheet = d.wb.active().unwrap();
    let fill = |cell: &str| d.wb.styles.get(sheet.style_id(gridcraft_core::CellRef::parse(cell).unwrap())).fill;
    assert_eq!(fill("A1"), crate::cmd::version_view::marker());
    assert_eq!(fill("C3"), crate::cmd::version_view::marker());
    assert_ne!(fill("B1"), crate::cmd::version_view::marker());
    assert_eq!(text(&s, "A1"), "first", "the version's own content is shown");
    // Still read only; Save a copy keeps the version, not the highlights.
    assert!(s.execute("cell.set", json!({"cell": "A1", "input": "x"})).is_err());
    // Compare again shows the version.
    let off = s.execute("file.compareVersion", json!({})).unwrap();
    assert_eq!(off["on"], json!(false));
    assert!(Arc::ptr_eq(&s.doc().unwrap().wb, &plain) && !s.previous_version().unwrap().compared);
    // Compared, then saved as a copy: the copy has no highlights.
    s.execute("file.compareVersion", json!({"on": true})).unwrap();
    let copy = root.join("Compared copy.xlsx");
    s.execute("file.saveVersionCopy", json!({"path": p(&copy)})).unwrap();
    let mut t = Session::new();
    t.execute("file.open", json!({"path": p(&copy)})).unwrap();
    let d = t.doc().unwrap();
    let sheet = d.wb.active().unwrap();
    assert_ne!(d.wb.styles.get(sheet.style_id(gridcraft_core::CellRef::parse("A1").unwrap())).fill, crate::cmd::version_view::marker());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_version_of_a_closed_workbook_can_still_be_viewed_but_not_restored_or_compared() {
    let root = temp("orphan");
    let (mut s, _file, first) = with_history(&root);
    s.doc_mut().unwrap().autosave = false;
    let source = s.doc().unwrap().uid;
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    s.set_active(s.document_index(source).unwrap());
    s.execute("file.close", json!({"force": true})).unwrap();
    assert_eq!(docs(&s), 1);
    assert!(s.previous_version().is_some());
    assert!(s.execute("file.compareVersion", json!({})).is_err());
    assert!(s.execute("file.restoreVersion", json!({})).is_err());
    assert!(s.previous_version().is_some(), "a failed restore leaves the version open");
    // Closing it with no workbook to return to just closes it.
    assert_eq!(s.execute("file.closeVersion", json!({})).unwrap()["doc"], Value::Null);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn opening_needs_the_desktop_history_and_a_saved_workbook() {
    let mut s = Session::new();
    s.new_workbook();
    assert!(s.execute("file.openVersion", json!({"id": "x"})).is_err(), "no history without the desktop host");
    let root = temp("needs");
    let mut t = Session::new();
    t.enable_versions(root.join("versions"), Arc::new(|| {})).unwrap();
    t.new_workbook();
    assert!(t.execute("file.openVersion", json!({"id": "x"})).is_err(), "a workbook that was never saved has no history");
    assert!(t.execute("file.openVersion", json!({})).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rename_moves_the_file_and_the_history_follows() {
    let root = temp("rename");
    let (mut s, file, first) = with_history(&root);
    s.doc_mut().unwrap().autosave = false;
    let history = s.version_infos().len();
    let stamp = s.doc().unwrap().stamp.clone();
    let r = s.execute("file.rename", json!({"name": "Budget 2027"})).unwrap();
    let renamed = root.join("Budget 2027.xlsx");
    assert_eq!(r["renamed"], json!(true));
    assert_eq!(r["path"], p(&renamed));
    assert!(renamed.exists() && !file.exists(), "the extension was kept");
    let d = s.doc().unwrap();
    assert_eq!(d.path.as_deref(), Some(&*renamed.to_string_lossy()));
    assert_eq!(d.display_title(), "Budget 2027.xlsx");
    assert!(d.is_dirty(), "the unsaved change is still unsaved");
    assert_eq!(d.stamp, stamp);
    // The same history, now at the new name; an old version opens from it.
    assert_eq!(s.version_infos().len(), history);
    s.execute("file.openVersion", json!({"id": first})).unwrap();
    assert_eq!(text(&s, "A1"), "first");
    assert!(s.previous_version().unwrap().source_path.ends_with("Budget 2027.xlsx"));
    s.execute("file.closeVersion", json!({})).unwrap();
    // A name that is taken, not a name, or the same name: refused or a no-op.
    let taken = root.join("Taken.xlsx");
    std::fs::write(&taken, b"x").unwrap();
    assert!(s.execute("file.rename", json!({"name": "Taken"})).is_err() && taken.exists());
    for bad in ["", "  ", "a/b", "a\\b", "x:y", "..", "what?"] {
        assert!(s.execute("file.rename", json!({"name": bad})).is_err(), "{bad:?}");
    }
    assert!(s.execute("file.rename", json!({})).is_err());
    assert_eq!(s.execute("file.rename", json!({"name": "Budget 2027.xlsx"})).unwrap()["renamed"], json!(false));
    // Saving after the rename writes the renamed file, as another version of the same history.
    s.execute("file.save", json!({})).unwrap();
    assert!(infos(&mut s, history + 1).len() > history);
    // A workbook with no file has nothing to rename.
    s.new_workbook();
    assert!(s.execute("file.rename", json!({"name": "Nope"})).is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rename_keeps_the_autosave_choice_of_the_file() {
    let root = temp("rename-autosave");
    let (mut s, file, _first) = with_history(&root);
    let memory = root.join("autosave.json");
    s.enable_autosave_memory(memory);
    s.execute("file.autosave", json!({"value": false})).unwrap();
    assert_eq!(s.autosave_memory.get(&file), Some(false));
    s.execute("file.rename", json!({"name": "Moved"})).unwrap();
    let renamed = root.join("Moved.xlsx");
    assert_eq!(s.autosave_memory.get(&renamed), Some(false));
    assert_eq!(s.autosave_memory.get(&file), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_names_typed_for_a_rename() {
    use crate::cmd::file::renamed_file_name as name;
    assert_eq!(name("Budget", Some("xlsx")).as_deref(), Some("Budget.xlsx"));
    assert_eq!(name("Budget.XLSX", Some("xlsx")).as_deref(), Some("Budget.XLSX"));
    assert_eq!(name("  Q1.2026 ", Some("xlsx")).as_deref(), Some("Q1.2026.xlsx"));
    assert_eq!(name("Budget.", Some("xlsx")).as_deref(), Some("Budget.xlsx"));
    assert_eq!(name("Budget", None).as_deref(), Some("Budget"));
    for bad in ["", ".", "..", "a/b", "a\\b", "a:b", "a*b", "a|b", "a\"b", "a<b", "a\nb"] {
        assert_eq!(name(bad, Some("xlsx")), None, "{bad:?}");
    }
    assert_eq!(name(&"x".repeat(300), Some("xlsx")), None);
}

#[test]
fn days_are_counted_on_the_local_calendar() {
    use crate::versions::local_when_at;
    // Noon UTC is the same calendar day for every time zone within a day of the other end.
    let saved = "2027-01-15T12:00:00Z";
    let day = 24 * 3600;
    let noon = 1_800_014_400; // 2027-01-15T12:00:00Z
    assert_eq!(local_when_at(saved, noon).unwrap().days_ago, 0);
    assert_eq!(local_when_at(saved, noon + day).unwrap().days_ago, 1);
    assert_eq!(local_when_at(saved, noon + 5 * day).unwrap().days_ago, 5);
    assert_eq!(local_when_at(saved, noon - day).unwrap().days_ago, -1);
    assert_eq!(local_when_at("not a time", noon), None);
}
