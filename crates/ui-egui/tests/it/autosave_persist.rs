//! AutoSave for each file (docs/cloud.md): the default is a preference that survives a restart,
//! the title-bar switch follows the file and says why it is off, closing a changed workbook writes
//! it instead of asking, and a file changed elsewhere asks what to keep.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::autosave::AutoSaveBlock;
use gridcraft_engine::{Prefs, Session};
use serde_json::json;

use crate::common::{Harness, app, builder, harness_with};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gc-ui-autosave-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    harness_with(builder().with_size(egui::vec2(1200.0, 800.0)), app(session), 4)
}

fn path_of(dir: &std::path::Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}

/// Edit `A1` and let the frames run.
fn edit(h: &mut Harness, text: &str) {
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": text})).unwrap();
    h.run_steps(3);
}

fn dialog_name(h: &Harness) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name().to_string())
}

#[test]
fn the_default_is_a_preference_that_survives_a_restart() {
    let app = app(Session::new());
    assert!(app.session.prefs.autosave_by_default, "on by default");
    let mut prefs: Prefs = app.session.prefs.clone();
    prefs.autosave_by_default = false;
    // What the desktop app writes to prefs.json, and reads back on the next launch.
    let restored: Prefs = serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
    assert!(!restored.autosave_by_default);
    let old: Prefs = serde_json::from_str(r#"{"formulaBar": false}"#).unwrap();
    assert!(old.autosave_by_default && !old.formula_bar, "an old prefs.json without the key follows the default");
}

#[test]
fn an_old_ui_json_with_the_global_switch_still_loads() {
    // Earlier versions kept one global AutoSave flag in ui.json; it is ignored now, nothing else is lost.
    let restored: gridcraft_ui_egui::UiState = serde_json::from_str(r#"{"dark": true, "autosave": true}"#).unwrap();
    assert!(restored.dark, "the rest of the old file still applies");
}

#[test]
fn the_switch_follows_the_file_and_says_why_it_is_off() {
    let dir = scratch("switch");
    let mut h = harness();
    assert!(h.query_by_label("AutoSave").is_some());
    assert!(!h.state().autosave_on());
    assert_eq!(h.state().autosave_block(), Some(AutoSaveBlock::Unsaved));
    assert!(h.state().autosave_tooltip().contains("Save the workbook to turn on AutoSave"));

    // Saved to a file: AutoSave is on at once, with nothing else to turn on.
    edit(&mut h, "1");
    h.state_mut().run("file.save", json!({"path": path_of(&dir, "a.xlsx")})).unwrap();
    assert!(h.state().autosave_on());
    // Clicking the switch turns it off for this file only; clicking again turns it back on.
    h.state_mut().click_autosave_switch();
    assert!(!h.state().autosave_on());
    assert!(h.state().autosave_tooltip().contains("off for this workbook"));
    edit(&mut h, "2");
    assert!(h.state().session.active().unwrap().is_dirty(), "off: nothing is written");
    h.state_mut().click_autosave_switch();
    assert!(h.state().autosave_on());
    h.run_steps(3);
    assert!(!h.state().session.active().unwrap().is_dirty(), "on: the change is written");

    // A format that loses things: off, with the file's name in the reason, and clicking says so.
    let csv = path_of(&dir, "t.csv");
    std::fs::write(&csv, "a,b\n1,2\n").unwrap();
    h.state_mut().run("file.open", json!({"path": csv})).unwrap();
    assert!(!h.state().autosave_on());
    assert!(h.state().autosave_tooltip().contains("t.csv"));
    h.state_mut().click_autosave_switch();
    assert!(h.state().toast.as_ref().is_some_and(|(m, _)| m.contains("t.csv")));
}

#[test]
fn a_changed_workbook_closes_without_asking() {
    let dir = scratch("close");
    let path = path_of(&dir, "book.xlsx");
    let mut h = harness();
    edit(&mut h, "kept");
    h.state_mut().run("file.save", json!({"path": path})).unwrap();
    h.state_mut().run("file.new", json!({})).unwrap();
    h.state_mut().run("file.open", json!({"path": path})).unwrap();
    // Close before the clock writes: the close itself writes, and no Save Changes question opens.
    h.state_mut().run("cell.set", json!({"cell": "B1", "input": "late"})).unwrap();
    assert!(h.state().session.active().unwrap().is_dirty());
    h.state_mut().run("file.close", json!({})).unwrap();
    assert_eq!(dialog_name(&h), None);
    let (saved, _) = gridcraft_engine::io::open_bytes("book.xlsx", &std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.active().unwrap().value(gridcraft_engine::core::CellRef::parse("B1").unwrap()).display(), "late");

    // With the switch off for the file, the question stays.
    h.state_mut().run("file.open", json!({"path": path})).unwrap();
    h.state_mut().run("file.autosave", json!({"value": false})).unwrap();
    h.state_mut().run("cell.set", json!({"cell": "B2", "input": "unsaved"})).unwrap();
    h.state_mut().run("file.close", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(dialog_name(&h).as_deref(), Some("saveChanges"));
}

#[test]
fn quitting_writes_the_changes_and_asks_only_about_what_it_cannot_write() {
    let dir = scratch("quit");
    let path = path_of(&dir, "q.xlsx");
    let mut h = harness();
    edit(&mut h, "x");
    h.state_mut().run("file.save", json!({"path": path})).unwrap();
    h.state_mut().run("cell.set", json!({"cell": "A2", "input": "y"})).unwrap();
    let ctx = h.ctx.clone();
    h.state_mut().request_quit(&ctx);
    assert!(h.state().allow_close, "nothing to ask: the changes were written");
    assert_eq!(dialog_name(&h), None);
    assert!(!h.state().session.active().unwrap().is_dirty());

    // A new workbook has no file: the question stays.
    let mut h = harness();
    edit(&mut h, "z");
    let ctx = h.ctx.clone();
    h.state_mut().request_quit(&ctx);
    assert!(!h.state().allow_close);
    assert_eq!(dialog_name(&h).as_deref(), Some("saveChanges"));
}

#[test]
fn a_file_changed_elsewhere_asks_what_to_keep() {
    let dir = scratch("conflict");
    let path = path_of(&dir, "shared.xlsx");
    let mut h = harness();
    edit(&mut h, "mine");
    h.state_mut().run("file.save", json!({"path": path.clone()})).unwrap();
    // Another device saves a different version.
    let mut other = Session::new();
    other.new_workbook();
    other.execute("cell.set", json!({"cell": "A1", "input": "theirs"})).unwrap();
    other.execute("file.save", json!({"path": path.clone()})).unwrap();
    let theirs = std::fs::read(&path).unwrap();

    edit(&mut h, "mine again");
    h.run_steps(3);
    assert_eq!(dialog_name(&h).as_deref(), Some("saveConflict"), "AutoSave stopped and asks");
    assert_eq!(std::fs::read(&path).unwrap(), theirs, "their version is untouched");
    assert!(h.state().autosave_tooltip().contains("changed on another device"));
    h.get_by_label("Decide later").click();
    h.run_steps(3);
    assert_eq!(dialog_name(&h), None);
    h.run_steps(3);
    assert_eq!(dialog_name(&h), None, "Decide later is not asked again at once");

    // Closing now asks about the conflict, never about Save Changes, and the workbook stays.
    h.state_mut().run("file.close", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(dialog_name(&h).as_deref(), Some("saveConflict"));
    assert_eq!(h.state().session.documents().len(), 1);

    // Keep both: this version goes beside the other and editing continues there.
    h.get_by_label("Keep both").click();
    h.run_steps(3);
    assert_eq!(dialog_name(&h), None);
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    let copy = h.state().session.active().unwrap().path.clone().unwrap();
    assert!(copy.contains("(conflict copy)"), "{copy}");
    assert!(h.state().autosave_on(), "AutoSave covers the copy");
}

#[test]
fn a_write_that_keeps_failing_shows_in_the_status_bar_and_goes_away_when_it_works() {
    let dir = scratch("failing");
    let path = path_of(&dir, "locked.xlsx");
    let mut h = harness();
    edit(&mut h, "one");
    h.state_mut().run("file.save", json!({"path": path.clone()})).unwrap();
    let hold = format!("{path}.sctmp");
    std::fs::write(&hold, b"held by another program").unwrap();
    edit(&mut h, "two");
    h.run_steps(3);
    let message = h.state().autosave_failure_message().expect("the failed write is reported");
    assert!(message.starts_with("AutoSave couldn't write the file"), "{message}");
    assert!(h.query_by_label_contains("AutoSave couldn't write the file").is_some(), "shown in the status bar");
    assert!(h.state().autosave_on(), "the switch stays on");
    // The user saves by hand once the other program lets go: the message goes away.
    let _ = std::fs::remove_file(&hold);
    h.state_mut().run("file.save", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().autosave_failure_message().is_none());
    assert!(h.query_by_label_contains("AutoSave couldn't write the file").is_none());
}

#[test]
fn save_to_cloud_keeps_a_new_workbook_in_the_chosen_provider_folder() {
    let dir = scratch("cloud");
    let onedrive = dir.join("OneDrive-Personal");
    std::fs::create_dir_all(&onedrive).unwrap();
    let mut h = harness();
    h.state_mut().session.cloud_roots =
        vec![gridcraft_cloud::CloudRoot { provider: gridcraft_cloud::Provider::OneDrive, path: onedrive.clone(), account: Some("Personal".into()) }];
    edit(&mut h, "plan");
    // The switch on a new workbook asks where to keep it.
    h.state_mut().click_autosave_switch();
    h.run_steps(3);
    assert_eq!(dialog_name(&h).as_deref(), Some("saveToCloud"));
    h.get_by_label_contains("OneDrive").click();
    h.run_steps(3);
    assert_eq!(dialog_name(&h), None);
    let saved = h.state().session.active().unwrap().path.clone().unwrap();
    assert!(std::path::Path::new(&saved).starts_with(&onedrive), "{saved}");
    assert!(h.state().autosave_on());
    assert!(!h.state().session.active().unwrap().is_dirty());
}
