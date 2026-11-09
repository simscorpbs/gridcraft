//! The version history through the commands: what each way of saving records, the identity of a
//! workbook, folders and options, and restoring. The store itself is tested in `tests.rs`.

use std::sync::Arc;

use serde_json::{Value, json};

use super::tests::{same_parts, save_bytes_at, temp};
use super::*;
use crate::Session;

/// A session with the history in `root/versions`, never the real profile, and one blank workbook.
fn session_in(root: &Path) -> Session {
    let mut s = Session::new();
    s.prefs.user_name = "Tester Test".into();
    s.enable_versions(root.join("versions"), Arc::new(|| {})).unwrap();
    s.new_workbook();
    s
}

/// A session with no history, as the command line and MCP have.
fn headless() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn put(s: &mut Session, cell: &str, text: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": text})).unwrap();
}

fn text(s: &Session, cell: &str) -> String {
    s.doc().unwrap().wb.active().unwrap().value(gridcraft_core::CellRef::parse(cell).unwrap()).display()
}

fn p(path: &Path) -> Value {
    json!(path.to_string_lossy())
}

fn uid(s: &Session) -> u64 {
    s.doc().unwrap().uid
}

fn id_of(s: &Session) -> Option<String> {
    s.version_document_id(uid(s))
}

/// The versions the session's store holds for `file` (under the active workbook's id, if any)
/// once the worker is idle.
fn listing(s: &mut Session, file: &Path) -> Vec<VersionInfo> {
    wait_idle(s);
    let id = id_of(s);
    let store = s.version_store.as_ref().unwrap();
    let shared = hold(&store.shared);
    store_list(&shared.roots, file, id.as_deref(), false, &[]).unwrap()
}

/// [`listing`], waiting until at least `count` versions are there.
fn wait_versions(s: &mut Session, file: &Path, count: usize) -> Vec<VersionInfo> {
    let mut list = Vec::new();
    for _ in 0..500 {
        list = listing(s, file);
        if list.len() >= count {
            return list;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    list
}

fn wait_idle(s: &mut Session) {
    for _ in 0..1000 {
        s.versions_tick();
        let store = s.version_store.as_ref().unwrap();
        if !store.busy && !hold(&store.shared).relocating {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn version_bytes(s: &Session, file: &Path, id: &str) -> Vec<u8> {
    s.version_store.as_ref().unwrap().read(file, id_of(s).as_deref(), id).unwrap().1
}

/// Every version record (manifest or metadata file) under `dir`, as text.
fn records(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(records(&path));
            } else if path.to_string_lossy().ends_with(".json") {
                out.push(fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    out
}

/// A workbook file written by a session with no history.
fn file_with(path: &Path, cell_text: &str) -> Vec<u8> {
    let mut s = headless();
    put(&mut s, "A1", cell_text);
    s.execute("file.save", json!({"path": p(path)})).unwrap();
    fs::read(path).unwrap()
}

// ---- cadence ----------------------------------------------------------------------------------

#[test]
fn every_save_path_creates_a_version_and_autosave_windows_follow_the_ten_minute_rule() {
    let root = temp("windows");
    let file = root.join("Plan.xlsx");
    let mut s = session_in(&root);
    s.autosave_host = true;
    put(&mut s, "A1", "one");
    assert_eq!(s.execute("file.save", json!({"path": p(&file)})).unwrap()["saved"], true);
    let list = wait_versions(&mut s, &file, 1);
    assert_eq!((list.len(), list[0].kind), (1, VersionKind::Save));
    // The first AutoSave write of the editing session starts a version...
    put(&mut s, "A2", "two");
    s.execute("file.save", json!({"ifUnchanged": true})).unwrap();
    let list = wait_versions(&mut s, &file, 2);
    assert_eq!((list.len(), list[0].kind), (2, VersionKind::Autosave));
    assert!(list[0].window_start.is_some() || list[0].session.is_some());
    let started = list[0].id.clone();
    // ...and later writes update it, as a new immutable version that supersedes the old one.
    put(&mut s, "A3", "three");
    s.execute("file.save", json!({"ifUnchanged": true})).unwrap();
    wait_idle(&mut s);
    let list = listing(&mut s, &file);
    assert_eq!(list.len(), 2, "{list:?}");
    assert_ne!(list[0].id, started);
    assert_eq!(list[0].supersedes.as_deref(), Some(started.as_str()));
    // An explicit save is always a version of its own.
    put(&mut s, "A4", "four");
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(wait_versions(&mut s, &file, 3).len(), 3);
    // Closing writes the last change, and that write is in the history.
    put(&mut s, "A5", "five");
    let uid = uid(&s);
    assert_eq!(s.autosave_before_discard(uid), crate::cmd::cloud::Flush::Saved);
    let list = wait_versions(&mut s, &file, 4);
    assert_eq!(list.len(), 4);
    assert_eq!(list[0].details.as_ref().map(|d| d.trigger.as_str()), Some("close"));
    assert!(same_parts(&version_bytes(&s, &file, &list[0].id), &fs::read(&file).unwrap()), "exactly the parts that went to disk");
    // A newer editing session never overwrites the version of the earlier one.
    drop(s);
    let mut t = session_in(&root);
    t.execute("file.open", json!({"path": p(&file)})).unwrap();
    put(&mut t, "A6", "six");
    t.execute("file.save", json!({"ifUnchanged": true})).unwrap();
    assert_eq!(wait_versions(&mut t, &file, 5).len(), 5);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn autosave_over_a_file_without_history_keeps_its_original_bytes() {
    let root = temp("original");
    let file = root.join("Report.xlsx");
    let original = file_with(&file, "written elsewhere");
    let mut s = session_in(&root);
    s.execute("file.open", json!({"path": p(&file)})).unwrap();
    put(&mut s, "A2", "edited here");
    assert_eq!(s.execute("file.save", json!({"ifUnchanged": true})).unwrap()["saved"], true);
    let list = wait_versions(&mut s, &file, 2);
    assert_eq!(list.len(), 2, "{list:?}");
    let first = list.iter().find(|v| v.kind == VersionKind::Original).unwrap();
    assert_eq!(version_bytes(&s, &file, &first.id), original, "the bytes that were there before the first overwrite, byte for byte");
    assert_eq!(first.storage, VersionStorage::Whole);
    let newest = &list[0];
    assert_eq!(newest.kind, VersionKind::Autosave);
    assert_eq!(newest.storage, VersionStorage::Parts);
    put(&mut s, "A3", "edited again");
    s.execute("file.save", json!({})).unwrap();
    let list = wait_versions(&mut s, &file, 3);
    assert_eq!(list.iter().filter(|v| v.kind == VersionKind::Original).count(), 1, "pinned, and kept once");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn bytes_changed_by_another_program_are_kept_before_saving_over_them() {
    let root = temp("outside");
    let file = root.join("Notes.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "mine");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    assert_eq!(wait_versions(&mut s, &file, 1).len(), 1);
    let id = id_of(&s).unwrap();
    // Another program writes the file, keeping the document id (as Excel does).
    let mut other = headless();
    put(&mut other, "A1", "their edit");
    let theirs =
        gridcraft_xlsx::set_custom_property(&crate::io::save_bytes(&other.doc().unwrap().wb, "x.xlsx").unwrap(), meta::DOC_ID_PROP, &id).unwrap();
    fs::write(&file, &theirs).unwrap();
    put(&mut s, "A2", "mine, later");
    s.execute("file.save", json!({})).unwrap();
    let list = wait_versions(&mut s, &file, 3);
    let kept = list.iter().find(|v| v.kind == VersionKind::External).unwrap();
    assert_eq!(version_bytes(&s, &file, &kept.id), theirs);
    assert_eq!(id_of(&s).as_deref(), Some(id.as_str()), "the same document");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_broken_history_never_stops_or_changes_the_save() {
    let root = temp("broken");
    let file = root.join("Safe.xlsx");
    file_with(&file, "before");
    let mut s = session_in(&root);
    s.execute("file.open", json!({"path": p(&file)})).unwrap();
    let versions = root.join("versions");
    fs::remove_dir_all(&versions).unwrap();
    fs::write(&versions, b"not a folder").unwrap();
    put(&mut s, "A2", "saved anyway");
    assert_eq!(s.execute("file.save", json!({"ifUnchanged": true})).unwrap()["saved"], true);
    let mut reopened = headless();
    reopened.execute("file.open", json!({"path": p(&file)})).unwrap();
    assert_eq!(text(&reopened, "A2"), "saved anyway");
    let mut reported = None;
    for _ in 0..500 {
        reported = s.versions_error();
        if reported.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(reported.is_some(), "the failure is reported, not hidden");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn headless_sessions_write_no_history_and_opening_never_does() {
    let root = temp("headless");
    let file = root.join("Quiet.xlsx");
    let mut s = headless();
    put(&mut s, "A1", "x");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    assert!(!root.join("versions").exists());
    assert!(s.execute("file.versions", json!({"delete": "all"})).is_err(), "no history to delete without the desktop host");
    assert_eq!(s.execute("file.versions", json!({})).unwrap(), json!([]));
    // A session with a history writes it for a save, not for opening or editing.
    let mut t = session_in(&root);
    t.execute("file.open", json!({"path": p(&file)})).unwrap();
    put(&mut t, "A2", "unsaved");
    assert!(t.version_infos().is_empty());
    wait_idle(&mut t);
    assert!(records(&root.join("versions")).is_empty());
    assert_eq!(t.execute("file.save", json!({"path": p(&root.join("Notes.csv"))})).unwrap()["saved"], true);
    assert!(records(&root.join("versions")).is_empty(), "a copy in another format is an export, not a version");
    let _ = fs::remove_dir_all(root);
}

// ---- identity -----------------------------------------------------------------------------------

#[test]
fn a_document_gets_an_id_at_its_first_save_and_keeps_it_through_a_move_outside_the_app() {
    let root = temp("doc-id");
    let docs = root.join("docs");
    fs::create_dir_all(docs.join("archive")).unwrap();
    let file = docs.join("Plan.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "plan");
    assert!(id_of(&s).is_none());
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    let id = id_of(&s).expect("an id at the first save");
    assert!(meta::document_id_ok(&id));
    assert_eq!(meta::file_document_id(&file).as_deref(), Some(id.as_str()), "written into docProps/custom.xml");
    // The file is still an ordinary workbook for the reader, and a later save keeps the id.
    let (wb, _) = crate::io::open_bytes("Plan.xlsx", &fs::read(&file).unwrap()).unwrap();
    assert_eq!(wb.active().unwrap().value(gridcraft_core::CellRef::parse("A1").unwrap()).display(), "plan");
    put(&mut s, "A2", "more");
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(meta::file_document_id(&file).as_deref(), Some(id.as_str()), "the same id, not a new one");
    assert_eq!(id_of(&s).as_deref(), Some(id.as_str()));
    assert_eq!(wait_versions(&mut s, &file, 2).len(), 2);
    // Reopened and saved again: the same id and the same history.
    let mut t = session_in(&root);
    t.execute("file.open", json!({"path": p(&file)})).unwrap();
    assert_eq!(id_of(&t).as_deref(), Some(id.as_str()), "read from the file when it is opened");
    put(&mut t, "A3", "plan, revised");
    t.execute("file.save", json!({})).unwrap();
    let list = wait_versions(&mut t, &file, 3);
    assert_eq!(list.len(), 3);
    assert!(list.iter().all(|v| v.document_id.as_deref() == Some(id.as_str())));
    assert!(root.join("versions").join(meta::id_bucket(&id)).is_dir(), "kept under the id");
    // Renamed and moved in File Explorer: the history is the id's, so it follows at once.
    let moved = docs.join("archive").join("Plan final.xlsx");
    fs::rename(&file, &moved).unwrap();
    let mut u = session_in(&root);
    u.execute("file.open", json!({"path": p(&moved)})).unwrap();
    assert_eq!(u.execute("file.versions", json!({})).unwrap().as_array().map(Vec::len), Some(3));
    put(&mut u, "A4", "moved");
    u.execute("file.save", json!({})).unwrap();
    assert_eq!(id_of(&u).as_deref(), Some(id.as_str()), "a move is not a copy");
    assert_eq!(wait_versions(&mut u, &moved, 4).len(), 4);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn removing_personal_information_keeps_the_id_and_the_author_out() {
    let root = temp("private");
    let file = root.join("Private.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "private");
    std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).remove_personal_info = true;
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    assert!(meta::file_document_id(&file).is_none(), "no id in the file");
    assert!(id_of(&s).is_none());
    let list = wait_versions(&mut s, &file, 1);
    assert_eq!(list.len(), 1, "the history is kept by path");
    assert!(list[0].author.is_empty());
    let details = list[0].details.as_ref().unwrap();
    assert!(details.initials.is_empty());
    assert!(details.properties.as_ref().is_some_and(|p| p.creator.is_empty()));
    // An id the file already carried is taken out when it is saved again.
    let mut other = headless();
    put(&mut other, "A1", "had an id");
    let with_id = gridcraft_xlsx::set_custom_property(
        &crate::io::save_bytes(&other.doc().unwrap().wb, "x.xlsx").unwrap(),
        meta::DOC_ID_PROP,
        &meta::new_document_id().unwrap(),
    )
    .unwrap();
    let carried = root.join("Carried.xlsx");
    fs::write(&carried, with_id).unwrap();
    let mut t = session_in(&root);
    t.execute("file.open", json!({"path": p(&carried)})).unwrap();
    std::sync::Arc::make_mut(&mut t.doc_mut().unwrap().wb).remove_personal_info = true;
    t.execute("file.save", json!({})).unwrap();
    assert!(meta::file_document_id(&carried).is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn save_as_makes_a_new_document_that_records_where_it_came_from() {
    let root = temp("save-as");
    let first = root.join("Draft.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "one");
    s.execute("file.save", json!({"path": p(&first)})).unwrap();
    let source = id_of(&s).unwrap();
    let source_version = wait_versions(&mut s, &first, 1)[0].id.clone();
    let copy = root.join("Copy.xlsx");
    s.execute("file.saveAs", json!({"path": p(&copy)})).unwrap();
    let id = id_of(&s).unwrap();
    assert_ne!(id, source, "a new file, a new id");
    assert_eq!(meta::file_document_id(&first).as_deref(), Some(source.as_str()), "the first file keeps its own");
    let list = wait_versions(&mut s, &copy, 1);
    assert_eq!(list[0].kind, VersionKind::SaveAs);
    assert_eq!(list[0].details.as_ref().map(|d| d.trigger.as_str()), Some("saveAs"));
    let origin = list[0].details.as_ref().and_then(|d| d.origin.clone()).unwrap();
    assert_eq!(origin.document_id.as_deref(), Some(source.as_str()));
    assert_eq!(origin.version_id.as_deref(), Some(source_version.as_str()));
    // The first file keeps its own history.
    wait_idle(&mut s);
    let roots = s.version_store.as_ref().unwrap().roots();
    assert_eq!(store_list(&roots, &first, Some(&source), false, &[]).unwrap().len(), 1);
    // Save As over a file that exists keeps what it held, in the new document's history.
    let target = root.join("Existing.xlsx");
    let existing = file_with(&target, "somebody else's");
    s.execute("file.saveAs", json!({"path": p(&target)})).unwrap();
    let list = wait_versions(&mut s, &target, 2);
    let original = list.iter().find(|v| v.kind == VersionKind::Original).expect("the replaced file is kept");
    assert_eq!(version_bytes(&s, &target, &original.id), existing);
    // A copy in another format is an export, not a version.
    let csv = root.join("Copy.csv");
    s.execute("file.saveAs", json!({"path": p(&csv)})).unwrap();
    wait_idle(&mut s);
    assert!(!document_dir(&root.join("versions"), &csv).exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn keeping_both_after_a_conflict_records_a_conflict_copy() {
    let root = temp("conflict-copy");
    let file = root.join("Joint.xlsx");
    let mut s = session_in(&root);
    s.autosave_host = true;
    put(&mut s, "A1", "mine");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    let source = id_of(&s).unwrap();
    fs::write(&file, file_with(&root.join("theirs.xlsx"), "theirs")).unwrap();
    put(&mut s, "A2", "later");
    assert_eq!(s.execute("file.save", json!({"ifUnchanged": true})).unwrap()["conflict"], "changed");
    s.execute("file.resolveConflict", json!({"keep": "both"})).unwrap();
    let copy = root.join("Joint (conflict copy).xlsx");
    assert!(copy.is_file());
    let list = wait_versions(&mut s, &copy, 1);
    assert_eq!(list[0].kind, VersionKind::ConflictCopy);
    assert_eq!(list[0].details.as_ref().map(|d| d.trigger.as_str()), Some("conflictCopy"));
    assert_eq!(list[0].details.as_ref().and_then(|d| d.origin.as_ref()).and_then(|o| o.document_id.as_deref()), Some(source.as_str()));
    assert_ne!(id_of(&s).unwrap(), source);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_copy_made_outside_the_app_gets_its_own_id() {
    let root = temp("copy-id");
    let docs = root.join("docs");
    fs::create_dir_all(&docs).unwrap();
    let original = docs.join("Plan.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "plan");
    s.execute("file.save", json!({"path": p(&original)})).unwrap();
    let id = id_of(&s).unwrap();
    let version = wait_versions(&mut s, &original, 1)[0].id.clone();
    // Copied in File Explorer while the original is still there.
    let copy = docs.join("Plan copy.xlsx");
    fs::copy(&original, &copy).unwrap();
    let mut t = session_in(&root);
    t.execute("file.open", json!({"path": p(&copy)})).unwrap();
    assert_eq!(id_of(&t).as_deref(), Some(id.as_str()), "the copy carries the id until it is saved");
    put(&mut t, "A2", "copy");
    t.execute("file.save", json!({"ifUnchanged": true})).unwrap();
    let copy_id = id_of(&t).unwrap();
    assert_ne!(copy_id, id, "the copy gets an id of its own");
    assert_eq!(meta::file_document_id(&copy).as_deref(), Some(copy_id.as_str()));
    let copy_list = wait_versions(&mut t, &copy, 1);
    let newest = &copy_list[0];
    let origin = newest.details.as_ref().and_then(|d| d.origin.clone()).unwrap();
    assert_eq!((origin.document_id.as_deref(), origin.version_id.as_deref()), (Some(id.as_str()), Some(version.as_str())));
    assert_eq!(newest.details.as_ref().map(|d| d.trigger.as_str()), Some("copy"));
    assert_eq!(wait_versions(&mut s, &original, 1).len(), 1, "the original keeps its history alone");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn history_kept_by_path_moves_under_the_id_at_the_first_save() {
    let root = temp("by-path");
    let file = root.join("Old.xlsx");
    let old = file_with(&file, "old");
    let versions = root.join("versions");
    // What an earlier build kept for this file, by path.
    let _ = save_bytes_at(&versions, &file, &old, 1_800_000_000);
    let by_path = document_dir(&versions, &file);
    assert!(by_path.is_dir());
    let mut s = session_in(&root);
    s.execute("file.open", json!({"path": p(&file)})).unwrap();
    assert_eq!(s.execute("file.versions", json!({})).unwrap().as_array().map(Vec::len), Some(1), "listed before the first save");
    put(&mut s, "A2", "new");
    s.execute("file.save", json!({})).unwrap();
    let id = id_of(&s).unwrap();
    let list = wait_versions(&mut s, &file, 2);
    assert_eq!(list.len(), 2, "{list:?}");
    assert!(!by_path.exists(), "the folder kept by path was taken over");
    assert!(versions.join(meta::id_bucket(&id)).is_dir());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_json_workbook_is_kept_by_path_and_follows_a_rename_and_a_move() {
    let root = temp("by-path-move");
    let docs = root.join("docs");
    fs::create_dir_all(docs.join("archive")).unwrap();
    let file = docs.join("Letter.json");
    let mut s = session_in(&root);
    put(&mut s, "A1", "letter");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    assert_eq!(wait_versions(&mut s, &file, 1).len(), 1);
    assert!(id_of(&s).is_none(), "other formats carry no id");
    // The host renames it (there is no file.rename command): the history follows.
    let renamed = docs.join("Letter final.json");
    let from = canonical_path(&file);
    fs::rename(&file, &renamed).unwrap();
    s.doc_mut().unwrap().path = Some(renamed.to_string_lossy().into_owned());
    let uid = uid(&s);
    s.versions_renamed(uid, from, &renamed);
    assert_eq!(wait_versions(&mut s, &renamed, 1).len(), 1, "the history follows the rename");
    wait_idle(&mut s);
    // Moved in File Explorer: found again by the bytes now in the file.
    let moved = docs.join("archive").join("Letter final.json");
    fs::rename(&renamed, &moved).unwrap();
    let mut t = session_in(&root);
    t.execute("file.open", json!({"path": p(&moved)})).unwrap();
    let mut found = Vec::new();
    for _ in 0..500 {
        found = t.version_infos();
        if !found.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(found.len(), 1, "the moved file's history is found by its bytes");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn workbooks_open_together_keep_separate_histories() {
    let root = temp("two-docs");
    let (a, b) = (root.join("A.xlsx"), root.join("B.xlsx"));
    let mut s = session_in(&root);
    put(&mut s, "A1", "a");
    s.execute("file.save", json!({"path": p(&a)})).unwrap();
    s.new_workbook();
    put(&mut s, "A1", "b");
    s.execute("file.save", json!({"path": p(&b)})).unwrap();
    put(&mut s, "A2", "b again");
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(wait_versions(&mut s, &b, 2).len(), 2);
    let b_id = id_of(&s).unwrap();
    s.set_active(0);
    let a_id = id_of(&s).unwrap();
    assert_ne!(a_id, b_id);
    assert_eq!(wait_versions(&mut s, &a, 1).len(), 1);
    assert_eq!(s.execute("file.versions", json!({})).unwrap().as_array().map(Vec::len), Some(1), "the active workbook's own history");
    // Deleting one workbook's history leaves the other's.
    assert_eq!(s.execute("file.versions", json!({"delete": "document"})).unwrap()["deleted"], 1);
    s.set_active(1);
    assert_eq!(s.execute("file.versions", json!({})).unwrap().as_array().map(Vec::len), Some(2));
    let _ = fs::remove_dir_all(root);
}

// ---- folders and options ------------------------------------------------------------------------

#[test]
fn history_folders_change_from_options_and_history_moves_along() {
    let root = temp("relocate");
    let elsewhere = root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let file = root.join("Moving.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "moving");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    assert_eq!(wait_versions(&mut s, &file, 1).len(), 1);
    let id = id_of(&s).unwrap();
    let r = s.execute("file.options", json!({"versionsFolder": p(&elsewhere), "versionsExisting": "move"})).unwrap();
    assert_eq!(r["versionsFolder"], json!(elsewhere.to_string_lossy()));
    wait_idle(&mut s);
    assert!(elsewhere.join(meta::id_bucket(&id)).is_dir(), "moved to the new folder");
    assert!(!root.join("versions").join(meta::id_bucket(&id)).exists(), "and gone from the old one");
    assert_eq!(s.execute("file.versions", json!({})).unwrap().as_array().map(Vec::len), Some(1));
    // Mirror: both folders, every write to both.
    let r = s.execute("file.options", json!({"versionsMirror": true, "versionsExisting": "copy"})).unwrap();
    assert_eq!(r["versionsMirror"], true);
    wait_idle(&mut s);
    assert_eq!(s.version_folders().0, vec![root.join("versions"), elsewhere.clone()]);
    put(&mut s, "A2", "mirrored");
    s.execute("file.save", json!({})).unwrap();
    wait_idle(&mut s);
    let a = root.join("versions").join(meta::id_bucket(&id));
    let b = elsewhere.join(meta::id_bucket(&id));
    assert_eq!(visible(&read_index(&a)).len(), 2);
    assert_eq!(visible(&read_index(&b)).len(), 2);
    // Delete this document's history: in both folders.
    assert_eq!(s.execute("file.versions", json!({"delete": "document"})).unwrap()["deleted"], 2);
    assert!(!a.exists() && !b.exists());
    // Bad settings change nothing.
    assert!(s.execute("file.options", json!({"versionsFolder": "relative/path"})).is_err());
    assert!(s.execute("file.options", json!({"versionsFolder": p(&root.join("not there"))})).is_err());
    assert!(s.execute("file.options", json!({"versionsExisting": "sideways", "versionsMirror": false})).is_err());
    assert!(s.execute("file.versions", json!({"delete": "everything"})).is_err());
    assert_eq!(s.prefs.versions_folder.as_deref(), Some(elsewhere.to_string_lossy().as_ref()));
    // Back to the settings folder alone.
    s.execute("file.options", json!({"versionsFolder": "default", "versionsMirror": false})).unwrap();
    assert!(s.prefs.versions_folder.is_none());
    assert_eq!(s.version_folders().0, vec![root.join("versions")]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deleting_all_history_empties_every_folder() {
    let root = temp("delete-all");
    let mut s = session_in(&root);
    put(&mut s, "A1", "one");
    for name in ["A.xlsx", "B.xlsx", "c.json"] {
        s.execute("file.save", json!({"path": p(&root.join(name))})).unwrap();
    }
    wait_idle(&mut s);
    assert!(s.execute("file.versions", json!({"delete": "all"})).unwrap()["deleted"].as_u64().unwrap() >= 3);
    let left: Vec<_> = fs::read_dir(root.join("versions")).unwrap().flatten().filter(|e| e.path().is_dir()).collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(root.join("installation-id").is_file(), "the installation id is not history");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn versions_record_where_and_how_without_naming_the_computer_or_quoting_the_contents() {
    let root = temp("details");
    let file = root.join("Contract.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "Confidential settlement clause number 4821 applies.");
    put(&mut s, "B1", "=1+1");
    s.execute("file.options", json!({"deviceLabel": "Office laptop"})).unwrap();
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    put(&mut s, "A2", "Confidential amendment");
    s.execute("file.save", json!({"trigger": "close"})).unwrap();
    let list = wait_versions(&mut s, &file, 2);
    let newest = &list[0];
    let d = newest.details.as_ref().unwrap();
    assert_eq!(d.trigger, "close");
    assert_eq!(d.parent.as_deref(), Some(list[1].id.as_str()), "the chain of versions");
    assert!(d.changed_parts.iter().any(|p| p.starts_with("xl/worksheets/")), "{:?}", d.changed_parts);
    assert_eq!(d.device, "Office laptop");
    assert_eq!(d.installation.len(), 32);
    assert_eq!(d.os, std::env::consts::OS);
    assert_eq!(d.app_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(d.provider, "local");
    assert!(d.path.ends_with("Contract.xlsx"));
    assert!(d.session_start.is_some() && d.editing_seconds.is_some());
    assert_eq!(d.initials, "TT");
    let before = d.before.as_ref().unwrap();
    assert_eq!(before.sha256.as_deref(), list[1].digest.as_deref(), "what was on disk before: the previous version");
    let stats = d.stats.as_ref().unwrap();
    assert_eq!((stats.sheets, stats.used_cells, stats.formulas, stats.charts, stats.comments), (1, 3, 1, 0, 0));
    assert_eq!(newest.author, "Tester Test");
    // The records hold no host name, no account name and no contents of the workbook.
    let all = records(&root.join("versions")).join("\n").to_lowercase();
    assert!(!all.is_empty());
    for word in ["confidential", "settlement", "4821", "amendment"] {
        assert!(!all.contains(word), "the text {word:?} is in a version record");
    }
    for key in ["COMPUTERNAME", "HOSTNAME", "USERNAME", "USER", "USERDOMAIN"] {
        if let Ok(value) = std::env::var(key)
            && value.len() > 2
        {
            assert!(!all.contains(&value.to_lowercase()), "{key} ({value}) is in a version record");
        }
    }
    // The file itself carries only the random id.
    let custom = {
        use std::io::Read;
        let mut archive = zip::ZipArchive::new(fs::File::open(&file).unwrap()).unwrap();
        let mut xml = String::new();
        archive.by_name("docProps/custom.xml").unwrap().read_to_string(&mut xml).unwrap();
        xml
    };
    assert_eq!(custom.matches("<property").count(), 1);
    assert!(custom.contains(meta::DOC_ID_PROP));
    // `file.versions` lists them with the added fields.
    let shown = s.execute("file.versions", json!({})).unwrap();
    let first = &shown.as_array().unwrap()[0];
    assert_eq!(first["details"]["trigger"], "close");
    assert!(first["documentId"].is_string() && first["stored"].is_u64() && first["size"].is_u64());
    // Device label back to the system's.
    let r = s.execute("file.options", json!({"deviceLabel": "auto"})).unwrap();
    assert!(r["deviceLabel"].is_null());
    assert!(!r["deviceLabelDefault"].as_str().unwrap_or("").is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_first_listing_after_opening_is_complete() {
    let root = temp("first-list");
    let file = root.join("Minutes.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "one");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    put(&mut s, "A2", "two");
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(wait_versions(&mut s, &file, 2).len(), 2);
    let mut other = session_in(&root);
    other.execute("file.open", json!({"path": p(&file)})).unwrap();
    let listed = other.execute("file.versions", json!({})).unwrap();
    assert_eq!(listed.as_array().map(Vec::len), Some(2), "{listed}");
    let _ = fs::remove_dir_all(root);
}

// ---- open, restore and the command's parameters ---------------------------------------------

#[test]
fn restore_is_one_undo_step_records_at_once_and_keeps_the_unsaved_state_before_it() {
    let root = temp("restore-now");
    let file = root.join("Letter.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "first draft");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    let first = wait_versions(&mut s, &file, 1);
    put(&mut s, "A1", "unsaved work");
    put(&mut s, "B2", "unsaved too");
    let r = s.execute("file.versions", json!({"restore": first[0].id.clone()})).unwrap();
    assert_eq!(r["restored"], json!(first[0].id));
    assert_eq!(text(&s, "A1"), "first draft");
    assert_eq!(text(&s, "B2"), "");
    assert!(s.doc().unwrap().is_dirty(), "AutoSave writes the restored content next");
    let list = wait_versions(&mut s, &file, 3);
    assert_eq!(list.len(), 3, "{list:?}");
    assert_eq!(list[0].kind, VersionKind::Restore);
    assert_eq!(list[0].details.as_ref().map(|d| d.trigger.clone()), Some(format!("restore:{}", first[0].id)));
    assert!(same_parts(&version_bytes(&s, &file, &list[0].id), &version_bytes(&s, &file, &first[0].id)), "with the restored bytes");
    let kept = &list[1];
    assert_eq!(kept.details.as_ref().map(|d| d.trigger.as_str()), Some("beforeRestore"));
    let (wb, _) = crate::io::open_bytes_with("Letter.xlsx", &version_bytes(&s, &file, &kept.id), None, false).unwrap();
    assert_eq!(wb.active().unwrap().value(gridcraft_core::CellRef::parse("B2").unwrap()).display(), "unsaved too");
    // One Undo step brings the unsaved state back.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(text(&s, "A1"), "unsaved work");
    assert_eq!(text(&s, "B2"), "unsaved too");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn opening_a_version_returns_its_bytes_and_changes_nothing() {
    let root = temp("open-version");
    let file = root.join("Letter.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "first");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    let first = wait_versions(&mut s, &file, 1);
    put(&mut s, "A1", "second");
    s.execute("file.save", json!({})).unwrap();
    wait_versions(&mut s, &file, 2);
    let docs = s.documents().len();
    let r = s.execute("file.versions", json!({"open": first[0].id.clone()})).unwrap();
    assert_eq!(r["kind"], "save");
    assert!(r["name"].as_str().unwrap().starts_with("Letter (version "), "{r}");
    let bytes = crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
    assert!(same_parts(&bytes, &version_bytes(&s, &file, &first[0].id)));
    let (wb, _) = crate::io::open_bytes("Letter.xlsx", &bytes).unwrap();
    assert_eq!(wb.active().unwrap().value(gridcraft_core::CellRef::parse("A1").unwrap()).display(), "first");
    assert_eq!((s.documents().len(), text(&s, "A1").as_str()), (docs, "second"));
    assert!(s.execute("file.versions", json!({"open": "00000000000000000000000000000000"})).is_err());
    assert!(s.execute("file.versions", json!({"open": "../../x"})).is_err());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn invalid_version_parameters_return_errors() {
    let root = temp("invalid");
    let mut s = session_in(&root);
    assert!(s.execute("file.versions", json!({"unknown": true})).is_err());
    assert!(s.execute("file.versions", json!({"open": 7})).is_err());
    assert!(s.execute("file.versions", json!({"restore": []})).is_err());
    assert!(s.execute("file.versions", json!({"delete": "all", "open": "x"})).is_err());
    assert!(s.execute("file.versions", json!({"open": "x", "restore": "y"})).is_err());
    assert!(s.execute("file.versions", json!({"restore": "0123456789abcdef0123456789abcdef"})).is_err(), "the workbook was never saved");
    assert!(s.execute("file.save", json!({"path": p(&root.join("x.xlsx")), "trigger": "rm -rf /"})).is_err());
    assert!(!root.join("x.xlsx").exists(), "nothing was written");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_damaged_object_is_written_again_by_the_next_save_and_repairs_the_versions_that_use_it() {
    let root = temp("repair");
    let file = root.join("Heal.xlsx");
    let mut s = session_in(&root);
    put(&mut s, "A1", "keep");
    s.execute("file.save", json!({"path": p(&file)})).unwrap();
    let first = wait_versions(&mut s, &file, 1);
    let id = id_of(&s).unwrap();
    let dir = root.join("versions").join(meta::id_bucket(&id));
    // Damage every stored object, as a bad sector or a half-synced file would.
    for entry in fs::read_dir(dir.join("objects")).unwrap().flatten() {
        let mut bytes = fs::read(entry.path()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(entry.path(), bytes).unwrap();
    }
    assert!(s.read_version(uid(&s), &file, &first[0].id).is_err(), "a damaged version is refused, not returned wrong");
    // The next save writes the same parts again whole, which repairs the first version too.
    s.execute("file.save", json!({})).unwrap();
    wait_versions(&mut s, &file, 2);
    assert!(s.read_version(uid(&s), &file, &first[0].id).is_ok(), "{:?}", s.read_version(uid(&s), &file, &first[0].id).err());
    let _ = fs::remove_dir_all(root);
}
