//! The Clipboard task pane (Home › Clipboard dialog launcher): it collects the last copies and
//! cuts, pastes any of them again, Paste All, Clear All, Delete, and the labels in Portuguese.

use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["alpha"], ["beta"], ["gamma"]]})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1300.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::prepare(SheetApp::new(session, Default::default())),
    );
    h.run_steps(4);
    h
}

fn copy_cell(h: &mut Harness, cell: &str) {
    h.state_mut().run("selection.set", json!({"range": cell})).unwrap();
    h.state_mut().run("edit.copy", json!({})).unwrap();
}

fn shown(h: &Harness, cell: &str) -> String {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap()).display()
}

fn launcher(h: &mut Harness) {
    h.get_by_label("Clipboard").click();
    h.run_steps(4);
}

#[test]
fn the_launcher_opens_and_closes_the_pane() {
    let mut h = harness();
    assert!(h.query_by_label("Paste All").is_none());
    launcher(&mut h);
    assert_eq!(h.state().grid.pane.as_deref(), Some("clipboard"));
    assert!(h.query_by_label("Paste All").is_some());
    assert!(h.query_by_label("Nothing collected yet. Items you copy or cut appear here.").is_some());
    // The pane's own Close hides it.
    h.get_by_label("Close").click();
    h.run_steps(3);
    assert!(h.query_by_label("Paste All").is_none());
    // The ui command opens and toggles it too.
    let r = h.state_mut().run("ui.clipboardPane", json!({"on": true})).unwrap();
    assert_eq!(r["on"], true);
    h.run_steps(3);
    assert!(h.query_by_label("Paste All").is_some());
    h.state_mut().run("ui.clipboardPane", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Paste All").is_none());
}

#[test]
fn clicking_an_item_pastes_it_and_paste_all_and_clear_all_work() {
    let mut h = harness();
    for c in ["A1", "A2", "A3"] {
        copy_cell(&mut h, c);
    }
    launcher(&mut h);
    assert!(h.query_by_label("Click an item to paste it.").is_some());
    // The newest item first; click the oldest.
    h.state_mut().run("selection.set", json!({"range": "C1"})).unwrap();
    h.get_by_label("alpha").click();
    h.run_steps(3);
    assert_eq!(shown(&h, "C1"), "alpha");
    h.state_mut().run("edit.undo", json!({})).unwrap();

    h.get_by_label("Paste All").click();
    h.run_steps(3);
    assert_eq!([shown(&h, "C1"), shown(&h, "C2"), shown(&h, "C3")], ["alpha", "beta", "gamma"]);

    h.get_by_label("Clear All").click();
    h.run_steps(3);
    assert!(h.state().session.clip_history.is_empty());
    assert!(h.query_by_label("Nothing collected yet. Items you copy or cut appear here.").is_some());
}

#[test]
fn delete_removes_one_item() {
    let mut h = harness();
    for c in ["A1", "A2"] {
        copy_cell(&mut h, c);
    }
    launcher(&mut h);
    // The ribbon has a Delete of its own (Cells); the pane's are the ones below it.
    let mut deletes: Vec<_> = h.query_all_by_label("Delete").filter(|n| n.rect().top() > 200.0).collect();
    assert_eq!(deletes.len(), 2);
    deletes.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    deletes[0].click();
    h.run_steps(3);
    assert_eq!(h.state().session.clip_history.len(), 1);
    assert_eq!(h.state().session.clip_history.items()[0].preview(), "alpha", "the newest (beta) went");
}

#[test]
fn the_pane_speaks_portuguese() {
    let mut h = harness();
    copy_cell(&mut h, "A1");
    h.state_mut().ui.language = Language::PtBr;
    launcher_pt(&mut h);
    for label in ["Área de Transferência", "Colar Tudo", "Limpar Tudo", "Excluir", "Clique em um item para colá-lo."] {
        assert!(h.query_all_by_label(label).next().is_some(), "{label}");
    }
    assert!(h.query_by_label("Paste All").is_none());
}

fn launcher_pt(h: &mut Harness) {
    h.run_steps(3);
    h.state_mut().run("ui.clipboardPane", json!({"on": true})).unwrap();
    h.run_steps(4);
}
