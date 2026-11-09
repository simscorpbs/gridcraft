//! Cloud AutoSave: the conflict guard, the switch remembered for each file, closing without a
//! Save Changes question, and the retry clock (docs/cloud.md).

use gridcraft_cloud::{CloudRoot, DiskState, Provider};
use serde_json::{Value, json};

use crate::autosave::{AutoSaveBlock, AutoSaveMemory, DEBOUNCE_MS, Failure, MAX_ENTRIES, Outcome, Scheduler};
use crate::cmd::cloud::Flush;
use crate::{Session, UiRequest};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gc-engine-autosave-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn session() -> Session {
    let mut s = Session::new();
    s.autosave_host = true;
    s.new_workbook();
    s
}

fn put(s: &mut Session, cell: &str, text: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": text})).unwrap();
}

fn text(s: &Session, cell: &str) -> String {
    s.doc().unwrap().wb.active().unwrap().value(gridcraft_core::CellRef::parse(cell).unwrap()).display()
}

fn p(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn save(s: &mut Session, path: &std::path::Path) {
    s.execute("file.save", json!({"path": p(path)})).unwrap();
}

fn auto(s: &mut Session) -> Value {
    s.execute("file.save", json!({"ifUnchanged": true})).unwrap()
}

/// Another device saves the file: AutoSave stops instead of overwriting it, and each way of
/// resolving the conflict keeps what it says.
#[test]
fn autosave_never_overwrites_a_file_changed_elsewhere() {
    let dir = scratch("conflict");
    let path = dir.join("Notes.xlsx");
    let mut s = session();
    put(&mut s, "A1", "first");
    save(&mut s, &path);

    // Unchanged on disk: AutoSave writes.
    put(&mut s, "A2", "here");
    let r = auto(&mut s);
    assert_eq!(r["saved"], true);
    assert!(!s.doc().unwrap().is_dirty());

    // Another device's version arrives.
    let mut other = session();
    put(&mut other, "A1", "from the phone");
    save(&mut other, &path);
    let theirs = std::fs::read(&path).unwrap();

    put(&mut s, "A3", "and more");
    let r = auto(&mut s);
    assert_eq!((r["saved"].clone(), r["conflict"].clone()), (json!(false), json!("changed")));
    assert_eq!(std::fs::read(&path).unwrap(), theirs, "the other version is untouched");
    assert!(s.doc().unwrap().is_dirty());
    // It stays paused until resolved, even after more typing.
    put(&mut s, "A4", "!");
    assert_eq!(auto(&mut s)["conflict"], "changed");
    assert_eq!(s.execute("file.cloud", json!({})).unwrap()["conflict"], "changed");
    assert!(!s.autosave_covers(s.doc().unwrap().uid), "a conflict blocks AutoSave");

    // Keep both: this version goes beside the other, and editing continues there.
    let r = s.execute("file.resolveConflict", json!({"keep": "both"})).unwrap();
    let copy = dir.join("Notes (conflict copy).xlsx");
    assert_eq!(r["path"], p(&copy));
    assert_eq!(s.doc().unwrap().path.as_deref(), Some(p(&copy).as_str()));
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    assert!(s.execute("file.cloud", json!({})).unwrap()["conflict"].is_null());
    assert!(s.execute("file.resolveConflict", json!({"keep": "mine"})).is_err(), "nothing is waiting now");

    // Take theirs: the file is reopened as it is now, in place of the workbook.
    std::fs::copy(&path, &copy).unwrap();
    put(&mut s, "B1", "lost");
    assert_eq!(auto(&mut s)["conflict"], "changed");
    s.execute("file.resolveConflict", json!({"keep": "theirs"})).unwrap();
    assert_eq!(s.documents().len(), 1, "the workbook was replaced, not duplicated");
    assert_eq!(text(&s, "A1"), "from the phone");
    assert!(!s.doc().unwrap().is_dirty());
    // ...and AutoSave carries on from that version.
    put(&mut s, "B1", "!");
    assert_eq!(auto(&mut s)["saved"], true);

    // Keep this version: overwrite.
    std::fs::write(&copy, b"not a workbook").unwrap();
    put(&mut s, "B2", "?");
    assert_eq!(auto(&mut s)["conflict"], "changed");
    s.execute("file.resolveConflict", json!({"keep": "mine"})).unwrap();
    let (reread, _) = crate::io::open_bytes(&p(&copy), &std::fs::read(&copy).unwrap()).unwrap();
    assert_eq!(reread.active().unwrap().value(gridcraft_core::CellRef::parse("B2").unwrap()).display(), "?");

    // Deleted or moved elsewhere: missing, and there is nothing to reopen.
    std::fs::remove_file(&copy).unwrap();
    put(&mut s, "B3", ".");
    assert_eq!(auto(&mut s)["conflict"], "missing");
    assert!(!copy.exists(), "AutoSave doesn't bring back a file removed elsewhere");
    assert!(s.execute("file.resolveConflict", json!({"keep": "theirs"})).is_err());
    assert!(s.execute("file.resolveConflict", json!({"keep": "sideways"})).is_err());
    s.execute("file.resolveConflict", json!({"keep": "mine"})).unwrap();
    assert!(copy.exists());
}

/// A plain Save is the person's own decision and still writes; only AutoSave asks first.
#[test]
fn explicit_save_and_opened_files_are_stamped() {
    let dir = scratch("explicit");
    let path = dir.join("a.xlsx");
    let mut s = session();
    put(&mut s, "A1", "one");
    save(&mut s, &path);

    // A file opened here is stamped as read.
    let mut t = Session::new();
    t.execute("file.open", json!({"path": p(&path)})).unwrap();
    assert!(t.doc().unwrap().stamp.is_some());
    put(&mut t, "A2", "two");
    assert_eq!(auto(&mut t)["saved"], true);
    // Now `s` is behind: its AutoSave stops, its explicit Save overwrites.
    put(&mut s, "A3", "three");
    assert_eq!(auto(&mut s)["conflict"], "changed");
    assert_eq!(s.execute("file.save", json!({})).unwrap()["saved"], true);
    assert!(s.execute("file.cloud", json!({})).unwrap()["conflict"].is_null());

    // A new workbook has no file to protect, and a workbook opened from bytes has no stamp.
    s.execute("file.new", json!({})).unwrap();
    assert!(s.doc().unwrap().stamp.is_none());
    let bytes = std::fs::read(&path).unwrap();
    s.execute("file.open", json!({"name": "bytes.xlsx", "base64": crate::io::base64_encode(&bytes)})).unwrap();
    let d = s.doc().unwrap();
    assert!(d.stamp.is_none() && d.autosave_block() == Some(AutoSaveBlock::Unsaved));
}

#[test]
fn what_blocks_autosave_and_what_does_not() {
    let dir = scratch("blocks");
    let mut s = session();
    put(&mut s, "A1", "x");
    assert_eq!(s.doc().unwrap().autosave_block(), Some(AutoSaveBlock::Unsaved));
    let err = s.execute("file.autosave", json!({"value": true})).unwrap_err().to_string();
    assert!(err.contains("Save the workbook"), "{err}");

    // Every format that keeps everything is covered.
    for name in ["a.xlsx", "b.XLSM", "c.xltx", "d.xltm", "e.json"] {
        let path = dir.join(name);
        s.execute("file.saveAs", json!({"path": p(&path)})).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.autosave_block(), None, "{name}");
        assert!(s.autosave_covers(d.uid), "{name}");
    }
    // A csv opened and edited is not.
    let csv = dir.join("t.csv");
    std::fs::write(&csv, "a,b\n1,2\n").unwrap();
    s.execute("file.open", json!({"path": p(&csv)})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.autosave_block(), Some(AutoSaveBlock::Format { name: "t.csv".into() }));
    assert!(!s.autosave_covers(d.uid));
    assert!(s.execute("file.autosave", json!({"value": true})).unwrap_err().to_string().contains("t.csv"));
    let cloud = s.execute("file.cloud", json!({})).unwrap();
    assert_eq!(cloud["autosave"]["covers"], false);
    assert!(cloud["autosave"]["blocked"].as_str().unwrap().contains("t.csv"));
    // Nothing is written for a workbook AutoSave does not cover.
    put(&mut s, "A1", "edited");
    let uid = s.doc().unwrap().uid;
    assert_eq!(s.autosave_write(uid), Ok(crate::autosave::AutoSaveWrite::NotCovered));
    assert_eq!(std::fs::read_to_string(&csv).unwrap(), "a,b\n1,2\n");
}

#[test]
fn the_switch_is_remembered_for_each_file() {
    let dir = scratch("switch");
    let store = dir.join("settings").join("autosave.json");
    let a = dir.join("a.xlsx");
    let b = dir.join("b.xlsx");
    let mut s = session();
    s.enable_autosave_memory(store.clone());
    assert!(s.doc().unwrap().autosave, "on by default");
    save(&mut s, &a);
    assert_eq!(s.execute("file.autosave", json!({"value": false})).unwrap(), json!({"value": false}));
    assert_eq!(s.execute("file.cloud", json!({})).unwrap()["autosave"]["remembered"], false);
    // Save As: the new file gets the switch the workbook had.
    s.execute("file.saveAs", json!({"path": p(&b)})).unwrap();
    assert!(!s.doc().unwrap().autosave);

    // A later run opens a.xlsx with its own choice, and a new workbook has the default.
    let mut t = Session::new();
    t.enable_autosave_memory(store.clone());
    t.execute("file.open", json!({"path": p(&a)})).unwrap();
    assert!(!t.doc().unwrap().autosave, "remembered off for a.xlsx");
    t.execute("file.new", json!({})).unwrap();
    assert!(t.doc().unwrap().autosave, "a new workbook has the default");
    let cloud = t.execute("file.cloud", json!({})).unwrap();
    assert_eq!(cloud["autosave"]["default"], true);

    // The default off: files with no choice come off, a file turned on stays on and is remembered.
    let c = dir.join("c.xlsx");
    save(&mut t, &c);
    assert!(t.doc().unwrap().autosave);
    t.execute("file.options", json!({"autosaveByDefault": false})).unwrap();
    assert!(!t.doc().unwrap().autosave);
    assert!(t.execute("file.options", json!({"autosaveByDefault": false})).unwrap()["autosaveByDefault"] == false);
    assert!(t.ui_requests.is_empty(), "setting the default doesn't open Options");
    t.execute("file.autosave", json!({"value": true})).unwrap();
    let mut u = Session::new();
    u.prefs.autosave_by_default = false;
    u.enable_autosave_memory(store);
    u.execute("file.open", json!({"path": p(&c)})).unwrap();
    assert!(u.doc().unwrap().autosave, "turned on with the default off, and remembered");
    u.execute("file.open", json!({"path": p(&a)})).unwrap();
    assert!(!u.doc().unwrap().autosave);
    // Toggling without a value flips what the switch shows.
    u.execute("file.autosave", json!({})).unwrap();
    assert!(u.doc().unwrap().autosave);
}

#[test]
fn windows_paths_lose_the_verbatim_prefix_and_case_in_recent_keys() {
    assert_eq!(crate::autosave::path_text_key("a"), "a");
    if cfg!(windows) {
        assert_eq!(crate::autosave::path_text_key(r"C:\Docs\A.xlsx"), crate::autosave::path_text_key("c:/docs/a.xlsx"));
        assert_eq!(crate::autosave::path_text_key(r"\\?\C:\Docs\A.xlsx"), crate::autosave::path_text_key(r"c:\docs\a.xlsx"));
    }
}

#[test]
fn a_damaged_or_huge_memory_file_remembers_nothing_and_the_list_is_bounded() {
    let dir = scratch("hostile");
    let store = dir.join("autosave.json");
    std::fs::write(&store, b"{not json").unwrap();
    let m = AutoSaveMemory::load(store.clone());
    assert!(m.is_empty() && m.error.is_some());
    std::fs::write(&store, vec![b' '; 1024 * 1024 + 10]).unwrap();
    assert!(AutoSaveMemory::load(store.clone()).is_empty());
    std::fs::write(&store, br#"{"version": 99, "files": []}"#).unwrap();
    assert!(AutoSaveMemory::load(store).error.is_some());
    // The settings folder is a file: choices are kept in memory and the error is reported.
    let blocked = dir.join("blocked");
    std::fs::write(&blocked, b"x").unwrap();
    let mut m = AutoSaveMemory::load(blocked.join("autosave.json"));
    m.remember(&dir.join("x.xlsx"), false, true);
    assert_eq!(m.get(&dir.join("x.xlsx")), Some(false));
    assert!(m.error.is_some());

    let mut m = AutoSaveMemory::load(dir.join("fresh.json"));
    for i in 0..MAX_ENTRIES + 5 {
        m.remember(&dir.join(format!("f{i}.xlsx")), false, true);
    }
    assert_eq!(m.len(), MAX_ENTRIES);
    assert_eq!(m.get(&dir.join("f0.xlsx")), None, "the oldest choice went first");
    assert_eq!(m.get(&dir.join(format!("f{}.xlsx", MAX_ENTRIES + 4))), Some(false));
    assert_eq!(AutoSaveMemory::load(dir.join("fresh.json")).len(), MAX_ENTRIES);
}

#[test]
fn choices_persist_follow_renames_and_forget_the_default() {
    let dir = scratch("memory");
    let store = dir.join("settings").join("autosave.json");
    let doc = dir.join("Report.xlsx");
    std::fs::write(&doc, b"x").unwrap();
    let mut m = AutoSaveMemory::load(store.clone());
    m.remember(&doc, false, true);
    assert_eq!(AutoSaveMemory::load(store.clone()).get(&doc), Some(false), "kept between runs");
    let renamed = dir.join("Final.xlsx");
    std::fs::rename(&doc, &renamed).unwrap();
    m.rename(&doc, &renamed);
    let m2 = AutoSaveMemory::load(store.clone());
    assert_eq!((m2.get(&doc), m2.get(&renamed)), (None, Some(false)));
    m.remember(&renamed, true, true);
    assert!(m.is_empty(), "turning it back on forgets the choice");
    assert!(AutoSaveMemory::load(store).is_empty());
}

/// With AutoSave on, closing a changed workbook writes it and closes without the Save Changes
/// question; with it off, or on a conflict, the document stays and the right dialog is asked for.
#[test]
fn closing_writes_the_changes_instead_of_asking() {
    let dir = scratch("close");
    let path = dir.join("Plan.xlsx");
    let mut s = session();
    put(&mut s, "A1", "kept");
    save(&mut s, &path);
    s.execute("file.new", json!({})).unwrap();
    s.execute("file.open", json!({"path": p(&path)})).unwrap();
    put(&mut s, "A2", "more");
    s.ui_requests.clear();
    let r = s.execute("file.close", json!({})).unwrap();
    assert_eq!(r["closed"], true);
    assert!(s.ui_requests.is_empty(), "no Save Changes question: {:?}", s.ui_requests);
    let (saved, _) = crate::io::open_bytes("Plan.xlsx", &std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.active().unwrap().value(gridcraft_core::CellRef::parse("A2").unwrap()).display(), "more");

    // AutoSave off for the file: the question stays.
    s.execute("file.open", json!({"path": p(&path)})).unwrap();
    s.execute("file.autosave", json!({"value": false})).unwrap();
    put(&mut s, "A3", "unsaved");
    let r = s.execute("file.close", json!({})).unwrap();
    assert_eq!((r["closed"].clone(), r["dirty"].clone()), (json!(false), json!(true)));
    assert!(matches!(s.ui_requests.last(), Some(UiRequest::Dialog(name, _)) if name == "saveChanges"));
    s.ui_requests.clear();
    s.execute("file.autosave", json!({"value": true})).unwrap();

    // Another device saved meanwhile: the workbook stays open and the conflict is asked about.
    let mut other = session();
    put(&mut other, "A1", "theirs");
    save(&mut other, &path);
    let theirs = std::fs::read(&path).unwrap();
    let r = s.execute("file.close", json!({})).unwrap();
    assert_eq!((r["closed"].clone(), r["conflict"].clone()), (json!(false), json!(true)));
    assert!(matches!(s.ui_requests.last(), Some(UiRequest::Dialog(name, v)) if name == "saveConflict" && v["missing"] == false));
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    assert!(s.doc().unwrap().is_dirty());
    // Asked again, the conflict is still the question (never Save Changes).
    s.ui_requests.clear();
    s.execute("file.close", json!({})).unwrap();
    assert!(matches!(s.ui_requests.last(), Some(UiRequest::Dialog(name, _)) if name == "saveConflict"));
    // Closing a window by the workbook's id works the same way.
    s.ui_requests.clear();
    let window = s.active_window().unwrap();
    let r = s.execute("window.close", json!({"window": window})).unwrap();
    assert_eq!(r["closed"], false);
    assert!(matches!(s.ui_requests.last(), Some(UiRequest::Dialog(name, _)) if name == "saveConflict"));
    // Close without saving is still the user's say-so.
    assert_eq!(s.execute("file.close", json!({"force": true})).unwrap()["closed"], true);
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
}

#[test]
fn flushing_before_a_quit_reports_what_happened() {
    let dir = scratch("flush");
    let path = dir.join("q.xlsx");
    let mut s = session();
    put(&mut s, "A1", "x");
    save(&mut s, &path);
    let uid = s.doc().unwrap().uid;
    assert_eq!(s.autosave_before_discard(uid), Flush::Ask, "clean: nothing to write");
    put(&mut s, "A2", "y");
    assert_eq!(s.autosave_before_discard(uid), Flush::Saved);
    assert!(!s.doc().unwrap().is_dirty());
    std::fs::write(&path, b"changed elsewhere").unwrap();
    put(&mut s, "A3", "z");
    assert_eq!(s.autosave_before_discard(uid), Flush::Conflict(DiskState::Changed));
    assert_eq!(s.autosave_before_discard(uid), Flush::Conflict(DiskState::Changed), "a waiting conflict stays the question");
    assert_eq!(s.autosave_before_discard(uid + 100), Flush::Ask);
}

#[test]
fn save_to_cloud_uses_the_providers_documents_folder() {
    let dir = scratch("cloud");
    let icloud = dir.join("com~apple~CloudDocs");
    let onedrive = dir.join("OneDrive-Personal");
    std::fs::create_dir_all(icloud.join("Documents")).unwrap();
    std::fs::create_dir_all(&onedrive).unwrap();
    let mut s = session();
    put(&mut s, "A1", "draft");

    // Headless sessions know no cloud folders until the host looks for them.
    assert!(s.execute("file.saveToCloud", json!({})).is_err());

    s.cloud_roots = vec![
        CloudRoot { provider: Provider::ICloudDrive, path: icloud.clone(), account: None },
        CloudRoot { provider: Provider::OneDrive, path: onedrive.clone(), account: Some("Personal".into()) },
    ];
    s.execute("file.saveToCloud", json!({"name": "Plan"})).unwrap();
    let r = s.execute("file.cloud", json!({})).unwrap();
    assert_eq!(r["provider"], "icloud");
    assert_eq!(s.doc().unwrap().path.as_deref(), Some(p(&icloud.join("Documents").join("Plan.xlsx")).as_str()));
    assert!(!s.doc().unwrap().is_dirty());
    assert_eq!(r["name"], "iCloud Drive");
    assert_eq!(r["roots"].as_array().unwrap().len(), 2);
    assert_eq!(r["autosave"]["covers"], true, "AutoSave covers the new cloud file at once");

    // Named provider and name; a taken name is numbered.
    s.execute("file.saveToCloud", json!({"provider": "onedrive", "name": "Plan"})).unwrap();
    s.execute("file.saveToCloud", json!({"provider": "onedrive", "name": "Plan"})).unwrap();
    assert_eq!(s.doc().unwrap().path.as_deref(), Some(p(&onedrive.join("Plan 2.xlsx")).as_str()));
    assert_eq!(s.execute("file.cloud", json!({})).unwrap()["provider"], "onedrive");
    assert!(s.execute("file.saveToCloud", json!({"provider": "gdrive"})).is_err());
    assert!(s.execute("file.saveToCloud", json!({"provider": "floppy"})).is_err());
    // Junk never panics, and a name with separators stays inside the provider's folder.
    for junk in [json!(null), json!({"provider": 5, "name": []}), json!({"name": "../../x"})] {
        let _ = s.execute("file.saveToCloud", junk.clone());
        let _ = s.execute("file.resolveConflict", junk.clone());
        let _ = s.execute("file.cloud", junk.clone());
        let _ = s.execute("file.autosave", junk);
    }
    let parent = std::path::PathBuf::from(s.doc().unwrap().path.clone().unwrap()).parent().unwrap().to_path_buf();
    assert!(parent == icloud.join("Documents") || parent == onedrive, "{parent:?}");
}

/// AutoSave writes a changed workbook, at most once every two seconds, and after a failed write
/// waits 5 s, then twice as long each time up to a minute, without turning itself off.
#[test]
fn the_retry_clock_backs_off_and_recovers() {
    assert_eq!([1, 2, 3, 4, 5, 6, 7].map(Failure::retry_after), [5_000.0, 10_000.0, 20_000.0, 40_000.0, 60_000.0, 60_000.0, 60_000.0]);
    assert_eq!(Failure::retry_after(0), 5_000.0);
    assert_eq!(Failure::retry_after(u32::MAX), 60_000.0);

    let dir = scratch("clock");
    let path = dir.join("Clock.xlsx");
    let mut s = session();
    put(&mut s, "A1", "x");
    save(&mut s, &path);
    let uid = s.doc().unwrap().uid;
    let mut clock = Scheduler::default();
    let tick = |s: &mut Session, clock: &mut Scheduler, now: f64| clock.tick(s, now, true, &mut |_, _| {});
    // The file another program is holding: the temporary file beside it is taken.
    let hold = || std::fs::write(format!("{}.sctmp", path.to_string_lossy()), b"held").unwrap();

    assert!(tick(&mut s, &mut clock, 0.0).is_empty(), "nothing changed, nothing to write");
    assert_eq!(clock.next_wake(&s, 0.0), None);

    // The first change is written at once; a second one waits for the two seconds to pass.
    put(&mut s, "A2", "1");
    assert_eq!(tick(&mut s, &mut clock, 10_000.0), vec![Outcome::Saved(uid)]);
    put(&mut s, "A2", "2");
    assert_eq!(clock.next_wake(&s, 10_500.0), Some(DEBOUNCE_MS - 500.0));
    assert!(tick(&mut s, &mut clock, 10_000.0 + DEBOUNCE_MS).is_empty(), "exactly two seconds is not yet past two seconds");
    // While the person is typing into a cell, nothing is written.
    assert!(clock.tick(&mut s, 20_000.0, false, &mut |_, _| {}).is_empty());
    assert!(s.doc().unwrap().is_dirty());

    // Writes keep failing: the wait doubles, the switch stays on, nothing else changes.
    let mut now = 20_000.0;
    let mut waits = vec![];
    for expected_failures in 1..=6u32 {
        hold();
        let out = tick(&mut s, &mut clock, now);
        assert!(matches!(&out[..], [Outcome::Failed { failures, .. }] if *failures == expected_failures), "{out:?}");
        let wait = clock.failing(uid).unwrap().retry_at - now;
        waits.push(wait);
        // Before the wait is over nothing is tried (even with the file free again).
        let _ = std::fs::remove_file(format!("{}.sctmp", path.to_string_lossy()));
        assert!(tick(&mut s, &mut clock, now + wait - 1.0).is_empty());
        assert_eq!(clock.next_wake(&s, now), Some(wait));
        now += wait;
    }
    assert_eq!(waits, [5_000.0, 10_000.0, 20_000.0, 40_000.0, 60_000.0, 60_000.0]);
    assert!(s.doc().unwrap().autosave, "failing doesn't turn AutoSave off");
    assert!(s.doc().unwrap().is_dirty());

    // The file is free again: the next try writes, and the failure is forgotten.
    assert_eq!(tick(&mut s, &mut clock, now), vec![Outcome::Saved(uid)]);
    assert!(clock.failing(uid).is_none());
    assert!(!s.doc().unwrap().is_dirty());
    assert!(std::fs::read(&path).unwrap().len() > 100);

    // A failure ends when the user saves by hand, too.
    put(&mut s, "A3", "3");
    hold();
    assert!(matches!(&tick(&mut s, &mut clock, now + 10_000.0)[..], [Outcome::Failed { failures: 1, .. }]));
    let _ = std::fs::remove_file(format!("{}.sctmp", path.to_string_lossy()));
    s.execute("file.save", json!({})).unwrap();
    assert!(tick(&mut s, &mut clock, now + 11_000.0).is_empty());
    assert!(clock.failing(uid).is_none());
}

/// The clock covers every changed workbook, not only the active one, and leaves the active one active.
#[test]
fn the_clock_writes_background_workbooks_too() {
    let dir = scratch("background");
    let (a, b) = (dir.join("a.xlsx"), dir.join("b.xlsx"));
    let mut s = session();
    save(&mut s, &a);
    let first = s.doc().unwrap().uid;
    s.execute("file.new", json!({})).unwrap();
    save(&mut s, &b);
    let second = s.doc().unwrap().uid;
    s.set_active(0);
    put(&mut s, "A1", "first");
    s.set_active(1);
    put(&mut s, "A1", "second");
    let mut clock = Scheduler::default();
    let mut stored = vec![];
    let mut out = clock.tick(&mut s, 5_000.0, true, &mut |_, uid| stored.push(uid));
    out.sort_by_key(|o| match o {
        Outcome::Saved(uid) => *uid,
        _ => u64::MAX,
    });
    assert_eq!(out, vec![Outcome::Saved(first), Outcome::Saved(second)]);
    assert_eq!(stored.len(), 2);
    assert_eq!(s.active().unwrap().uid, second, "the active workbook stays active");
    assert!(s.documents().iter().all(|d| !d.is_dirty()));
}

/// A headless session (CLI, MCP, a script) has no AutoSave clock and keeps refusing to close a
/// changed workbook until it is saved: closing never writes a file behind its caller's back.
#[test]
fn a_headless_session_never_writes_on_close() {
    let dir = scratch("headless");
    let path = dir.join("h.xlsx");
    let mut s = Session::new();
    s.new_workbook();
    put(&mut s, "A1", "x");
    save(&mut s, &path);
    let before = std::fs::read(&path).unwrap();
    put(&mut s, "A2", "y");
    let r = s.execute("file.close", json!({})).unwrap();
    assert_eq!((r["closed"].clone(), r["dirty"].clone()), (json!(false), json!(true)));
    assert!(matches!(s.ui_requests.last(), Some(UiRequest::Dialog(name, _)) if name == "saveChanges"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
