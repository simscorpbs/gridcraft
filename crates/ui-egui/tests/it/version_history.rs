//! Version History in the interface (docs/cloud.md): the pane, the title bar's file menu, a
//! version opened read only beside the workbook with its Previous version bar, Restore, Save a
//! copy, Compare and Close, and the labels in Brazilian Portuguese.

use std::sync::Arc;

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::versions::LocalWhen;
use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::versions_pane::{date_and_time, day_heading, time_of_day};
use serde_json::json;

use crate::common::{Harness, app, builder, harness_with};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gc-ui-versions-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn harness_of(app: gridcraft_ui_egui::SheetApp) -> Harness {
    harness_with(builder().with_size(egui::vec2(1300.0, 800.0)), app, 4)
}

fn put(h: &mut Harness, cell: &str, text: &str) {
    h.state_mut().run("cell.set", json!({"cell": cell, "input": text})).unwrap();
}

fn a1(h: &Harness) -> String {
    let d = h.state().session.active().unwrap();
    d.wb.active().unwrap().value(gridcraft_engine::core::CellRef::parse("A1").unwrap()).display()
}

fn wait_versions(h: &mut Harness, n: usize) -> Vec<gridcraft_engine::versions::VersionInfo> {
    for _ in 0..1000 {
        let list = h.state_mut().session.version_infos();
        if list.len() >= n {
            return list;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the history never reached {n} versions");
}

/// Notes.xlsx saved as "first" then "second", with "unsaved" typed after. Returns the harness,
/// the folder and the id of the "first" version.
fn with_history(name: &str) -> (Harness, std::path::PathBuf, String) {
    let dir = scratch(name);
    let mut session = Session::new();
    session.prefs.user_name = "Tester Test".into();
    session.enable_versions(dir.join("versions"), Arc::new(|| {})).unwrap();
    session.new_workbook();
    let mut h = harness_of(app(session));
    put(&mut h, "A1", "first");
    let file = dir.join("Notes.xlsx");
    h.state_mut().run("file.save", json!({"path": file.to_string_lossy()})).unwrap();
    let first = wait_versions(&mut h, 1)[0].id.clone();
    put(&mut h, "A1", "second");
    h.state_mut().run("file.save", json!({})).unwrap();
    wait_versions(&mut h, 2);
    put(&mut h, "A2", "unsaved");
    h.state_mut().session.doc_mut().unwrap().autosave = false;
    h.run_steps(2);
    (h, dir, first)
}

/// A real pointer click (the menus open on one, as for a person).
fn click_at(h: &mut Harness, pos: egui::Pos2, button: egui::PointerButton) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
}

fn click_title(h: &mut Harness) {
    let pos = h.get_by_label("File name and Version History").rect().center();
    click_at(h, pos, egui::PointerButton::Primary);
}

fn open_pane(h: &mut Harness) {
    h.state_mut().grid.pane = Some("versions".into());
    h.run_steps(4);
}

fn language(h: &mut Harness, lang: Language) {
    h.state_mut().ui.language = lang;
    h.run_steps(3);
}

#[test]
fn the_pane_lists_versions_newest_first_under_a_day_heading() {
    let (mut h, dir, _first) = with_history("list");
    open_pane(&mut h);
    assert!(h.query_by_label("Version History").is_some());
    assert_eq!(h.query_all_by_label("Today").count(), 1, "both saves are from today: one heading");
    assert_eq!(h.query_all_by_label("Open version").count(), 2, "every row can be opened");
    assert!(h.query_all_by_label("Tester Test").next().is_some(), "with its author");
    // The pane's Close hides it again.
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.query_by_label("Today").is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_pane_says_why_it_is_empty() {
    // Never saved.
    let dir = scratch("empty");
    let mut session = Session::new();
    session.enable_versions(dir.join("versions"), Arc::new(|| {})).unwrap();
    session.new_workbook();
    let mut h = harness_of(app(session));
    open_pane(&mut h);
    assert!(h.query_by_label("Save the workbook to start its version history.").is_some());
    // Saved, but no version written for it yet (the file was opened, not saved here).
    let file = dir.join("Opened.xlsx");
    let mut other = Session::new();
    other.new_workbook();
    other.execute("file.save", json!({"path": file.to_string_lossy()})).unwrap();
    h.state_mut().run("file.open", json!({"path": file.to_string_lossy()})).unwrap();
    open_pane(&mut h);
    assert!(h.query_by_label("No versions yet.").is_some());
    // No history in this build (the browser, a headless host).
    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness_of(app(session));
    open_pane(&mut h);
    assert!(h.query_by_label("Version history is kept by the desktop app.").is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn day_headings_and_times_follow_the_interface_language() {
    let at = |days_ago| LocalWhen { year: 2026, month: 2, day: 27, hour: 15, minute: 5, days_ago };
    assert_eq!(day_heading(&at(0), Language::En), "Today");
    assert_eq!(day_heading(&at(1), Language::En), "Yesterday");
    assert_eq!(day_heading(&at(2), Language::En), "February 27, 2026");
    assert_eq!(day_heading(&at(0), Language::PtBr), "Hoje");
    assert_eq!(day_heading(&at(1), Language::PtBr), "Ontem");
    assert_eq!(day_heading(&at(2), Language::PtBr), "27 de fevereiro de 2026");
    assert_eq!(day_heading(&at(9), Language::De), "2026-02-27", "an ISO date where there are no month names");
    assert_eq!(time_of_day(&at(0), Language::En), "3:05 PM");
    assert_eq!(time_of_day(&LocalWhen { hour: 0, minute: 7, ..at(0) }, Language::En), "12:07 AM");
    assert_eq!(time_of_day(&LocalWhen { hour: 12, minute: 0, ..at(0) }, Language::En), "12:00 PM");
    assert_eq!(time_of_day(&at(0), Language::PtBr), "15:05");
    // An unreadable timestamp is shown as it is.
    assert_eq!(date_and_time("not a time", Language::En), "not a time");
    let full = date_and_time("2027-01-15T12:00:00Z", Language::PtBr);
    assert!(full.contains(" de janeiro de 2027, ") || full.contains(" de janeiro de 2027,"), "{full}");
}

#[test]
fn opening_a_version_shows_it_read_only_with_its_bar_and_highlighted_row() {
    let (mut h, dir, first) = with_history("open");
    open_pane(&mut h);
    let list = h.state_mut().session.version_infos();
    let row = list.iter().position(|v| v.id == first).unwrap();
    h.query_all_by_label("Open version").nth(row).unwrap().click();
    h.run_steps(4);
    // The version is a separate workbook beside the current one.
    assert_eq!(h.state().session.documents().len(), 2);
    assert_eq!(a1(&h), "first");
    assert!(h.query_by_label("Previous version").is_some(), "the bar names it");
    for button in ["Compare", "Restore", "Save a copy"] {
        assert!(h.query_by_label(button).is_some(), "{button}");
    }
    assert_eq!(h.query_all_by_label("Open version").count(), 1, "its row has no link: it is highlighted");
    // The title reads `Name (version <date time>) — Previous version`.
    let title = h.state().session.active().unwrap().display_title();
    assert!(title.starts_with("Notes (version "), "{title}");
    assert_eq!(gridcraft_ui_egui::versions_pane::shown_title(h.state(), &title), format!("{title} — Previous version"));
    // Edits are refused with the reason shown; the version stays as it was.
    let refused = h.state_mut().run("cell.set", json!({"cell": "A1", "input": "x"}));
    assert!(refused.is_err());
    assert_eq!(a1(&h), "first");
    assert!(!h.state().session.active().unwrap().is_dirty());
    let message = h.state().session.locked_cell_message(0, gridcraft_engine::core::CellRef::parse("A1").unwrap());
    assert_eq!(message.as_deref(), Some(gridcraft_engine::version_view::READ_ONLY), "typing into a cell is not started");
    h.state_mut().run_or_alert("edit.undo", json!({}));
    h.run_steps(2);
    assert!(h.state().toast.is_some() || h.state().message.is_some(), "Undo is refused with a message");
    // View, selection and copy still work.
    h.state_mut().run("selection.set", json!({"cell": "B2"})).unwrap();
    h.state_mut().run("view.zoom", json!({"percent": 120})).unwrap();
    h.state_mut().run("edit.copy", json!({})).unwrap();
    // The sheet tabs, the ribbon and the grid draw it without trouble.
    h.run_steps(3);
    // Close (the bar's, not the pane's) returns to the workbook exactly as it was.
    h.query_all_by_label("Close").next().unwrap().click();
    h.run_steps(4);
    assert_eq!(h.state().session.documents().len(), 1);
    assert_eq!(a1(&h), "second");
    assert!(h.state().session.active().unwrap().is_dirty(), "its unsaved change is still there");
    assert!(h.query_by_label("Previous version").is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn restore_from_the_bar_is_one_undo_step() {
    let (mut h, dir, first) = with_history("restore");
    h.state_mut().run("file.openVersion", json!({"id": first})).unwrap();
    h.run_steps(4);
    h.get_by_label("Restore").click();
    h.run_steps(4);
    assert_eq!(h.state().session.documents().len(), 1);
    assert_eq!(a1(&h), "first");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(a1(&h), "second");
    let value = h.state().session.active().unwrap().wb.active().unwrap().value(gridcraft_engine::core::CellRef::parse("A2").unwrap()).display();
    assert_eq!(value, "unsaved", "one Undo brings back the workbook with its unsaved change");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn restore_from_a_row_menu_restores_that_version() {
    let (mut h, dir, first) = with_history("row-menu");
    open_pane(&mut h);
    let list = h.state_mut().session.version_infos();
    let row = list.iter().position(|v| v.id == first).unwrap();
    // Right-click the row's author line (not its link): the menu belongs to the row.
    let author = h.query_all_by_label("Tester Test").nth(row).unwrap().rect().center();
    click_at(&mut h, author, egui::PointerButton::Secondary);
    assert_eq!(h.query_all_by_label("Restore").count(), 1, "the row's menu offers Restore");
    h.get_by_label("Restore").click();
    h.run_steps(4);
    assert_eq!(a1(&h), "first");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_a_copy_from_the_bar_asks_where_saves_and_goes_back() {
    let (mut h, dir, first) = with_history("copy");
    let copy = dir.join("Copy of first.xlsx");
    let target = copy.to_string_lossy().into_owned();
    h.state_mut().services.pick_save = Some(Box::new(move |_| Some(target.clone())));
    h.state_mut().run("file.openVersion", json!({"id": first})).unwrap();
    h.run_steps(4);
    h.get_by_label("Save a copy").click();
    h.run_steps(4);
    assert!(copy.exists(), "the version was saved as a new file");
    assert_eq!(h.state().session.documents().len(), 1, "and the window went back to the workbook");
    assert_eq!(a1(&h), "second");
    assert!(h.query_by_label("Previous version").is_none());
    let mut t = Session::new();
    t.execute("file.open", json!({"path": copy.to_string_lossy()})).unwrap();
    let v = t.doc().unwrap().wb.active().unwrap().value(gridcraft_engine::core::CellRef::parse("A1").unwrap()).display();
    assert_eq!(v, "first");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn compare_highlights_differences_and_pressing_it_again_shows_the_version() {
    let (mut h, dir, first) = with_history("compare");
    h.state_mut().run("file.openVersion", json!({"id": first})).unwrap();
    h.run_steps(4);
    h.get_by_label("Compare").click();
    h.run_steps(3);
    assert!(h.state().session.previous_version().unwrap().compared);
    h.get_by_label("Compare").click();
    h.run_steps(3);
    assert!(!h.state().session.previous_version().unwrap().compared);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn new_open_and_quit_first_return_to_the_workbook() {
    let (mut h, dir, first) = with_history("discard");
    let source = h.state().session.active().unwrap().uid;
    h.state_mut().run("file.openVersion", json!({"id": first})).unwrap();
    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().session.previous_versions().next().is_none(), "New closed the version first");
    assert!(h.state().session.documents().iter().any(|d| d.uid == source && d.is_dirty()), "the workbook keeps its changes");
    // Quit: the versions close, then the workbook with unsaved changes is asked about.
    let back = h.state().session.document_index(source).unwrap();
    h.state_mut().session.set_active(back);
    h.state_mut().run("file.openVersion", json!({"id": first})).unwrap();
    let ctx = egui::Context::default();
    h.state_mut().request_quit(&ctx);
    h.run_steps(3);
    assert!(h.state().session.previous_versions().next().is_none());
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("saveChanges"));
    let _ = std::fs::remove_dir_all(dir);
}

fn press(h: &mut Harness, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
    }
    h.run_steps(2);
}

#[test]
fn the_title_opens_a_file_menu_that_renames_the_file() {
    let (mut h, dir, _first) = with_history("menu");
    click_title(&mut h);
    assert!(h.query_by_label(dir.to_string_lossy().as_ref()).is_some(), "the folder that holds the file");
    assert!(h.query_by_label("Version History").is_some());
    // Change the name and apply it with Enter.
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "File name").click();
    h.run_steps(2);
    press(&mut h, Key::A, Modifiers::COMMAND);
    h.input_mut().events.push(Event::Text("Budget 2027".into()));
    h.run_steps(2);
    press(&mut h, Key::Enter, Modifiers::NONE);
    h.run_steps(3);
    let renamed = dir.join("Budget 2027.xlsx");
    assert!(renamed.exists() && !dir.join("Notes.xlsx").exists());
    assert_eq!(h.state().session.active().unwrap().display_title(), "Budget 2027.xlsx");
    // Version History from the same menu opens the pane (a right click opens the menu too).
    let pos = h.get_by_label("File name and Version History").rect().center();
    click_at(&mut h, pos, egui::PointerButton::Secondary);
    h.get_by_label("Version History").click();
    h.run_steps(4);
    assert_eq!(h.state().grid.pane.as_deref(), Some("versions"));
    assert_eq!(h.query_all_by_label("Open version").count(), 2, "the history followed the file");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_file_menu_of_a_workbook_never_saved_says_so() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness_of(app(session));
    click_title(&mut h);
    assert!(h.query_by_label("Not saved").is_some());
}

#[test]
fn labels_are_in_portuguese() {
    let (mut h, dir, first) = with_history("pt");
    language(&mut h, Language::PtBr);
    open_pane(&mut h);
    assert!(h.query_by_label("Histórico de Versões").is_some());
    assert!(h.query_by_label("Hoje").is_some());
    assert_eq!(h.query_all_by_label("Abrir versão").count(), 2);
    h.state_mut().run("file.openVersion", json!({"id": first})).unwrap();
    h.run_steps(4);
    for label in ["Versão anterior", "Comparar", "Restaurar", "Salvar uma cópia"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert!(h.query_all_by_label("Fechar").next().is_some());
    assert_eq!(gridcraft_ui_egui::versions_pane::shown_title(h.state(), "X"), "X — Versão anterior");
    h.state_mut().run("file.closeVersion", json!({})).unwrap();
    let pos = h.get_by_label("Nome do arquivo e Histórico de Versões").rect().center();
    click_at(&mut h, pos, egui::PointerButton::Primary);
    assert!(h.query_by_label("Pasta").is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn autosave_does_not_apply_to_a_version() {
    let (mut h, dir, first) = with_history("autosave");
    h.state_mut().run("file.openVersion", json!({"id": first})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().autosave_block(), Some(gridcraft_engine::autosave::AutoSaveBlock::PreviousVersion));
    assert!(!h.state().autosave_on());
    assert!(h.state().autosave_tooltip().contains("previous version"));
    let _ = std::fs::remove_dir_all(dir);
}
