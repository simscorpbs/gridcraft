//! Home › Clipboard dialog launcher in the interface: the Clipboard task pane.
//!
//! It lists the last 24 copies and cuts made in the app (`edit.clipboardItems`), newest first, each
//! with a text preview of its first cells and where it came from. Clicking an item pastes it at the
//! active cell (`edit.pasteClipboardItem`), Paste All pastes them all, oldest first, one below the
//! other (`edit.pasteAllClipboard`), Clear All empties the list (`edit.clearClipboardItems`) and
//! each item has a Delete button (`edit.deleteClipboardItem`, also the Delete key on a focused
//! item). The list lives in memory for the session only. A pasted item also goes on the system
//! clipboard.

use egui::{Key, RichText, Stroke, Ui};
use serde_json::json;

use crate::SheetApp;
use crate::theme::{self, Tokens};

/// The pane's name in [`crate::grid::GridState::pane`].
pub const PANE: &str = "clipboard";

/// Open the Clipboard pane.
pub fn open_pane(app: &mut SheetApp) {
    app.grid.pane = Some(PANE.into());
}

/// One row of the list, read from the engine.
struct Row {
    preview: String,
    place: String,
    cut: bool,
}

fn rows(app: &SheetApp) -> Vec<Row> {
    app.session.clip_history.items().iter().map(|it| Row { preview: it.preview(), place: it.place(), cut: it.was_cut }).collect()
}

pub fn content(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let rows = rows(app);
    ui.horizontal(|ui| {
        let all = egui::Button::new(lang.tr("Paste All"));
        if ui.add_enabled(!rows.is_empty(), all).clicked() {
            let ctx = ui.ctx().clone();
            app.paste_from_history(&ctx, None);
        }
        let clear = egui::Button::new(lang.tr("Clear All"));
        if ui.add_enabled(!rows.is_empty(), clear).clicked() {
            app.run_or_alert("edit.clearClipboardItems", json!({}));
        }
    });
    ui.add_space(4.0);
    let hint = if rows.is_empty() { "Nothing collected yet. Items you copy or cut appear here." } else { "Click an item to paste it." };
    ui.label(RichText::new(lang.tr(hint)).font(theme::ui_font(12.0)).color(t.text_dim));
    ui.separator();
    for (i, row) in rows.iter().enumerate() {
        item(app, ui, i, row, t);
        ui.add_space(6.0);
    }
}

/// One collected copy: its preview (click to paste), where it came from, and Delete.
fn item(app: &mut SheetApp, ui: &mut Ui, i: usize, row: &Row, t: Tokens) {
    let lang = app.ui.language;
    let builder = egui::UiBuilder::new().id_salt(("clipboard_item", i));
    ui.scope_builder(builder, |ui| {
        egui::Frame::NONE.fill(t.menu_bg).stroke(Stroke::new(1.0, t.separator)).corner_radius(6).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let text = if row.preview.trim().is_empty() { lang.tr("(blank cells)").to_string() } else { row.preview.clone() };
            let mut job = egui::text::LayoutJob::single_section(
                text.clone(),
                egui::TextFormat { font_id: theme::ui_font(12.5), color: t.text, ..Default::default() },
            );
            job.wrap.max_width = ui.available_width();
            job.wrap.max_rows = 3;
            let paste = ui.add(egui::Button::new(job).frame(false));
            paste.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text.clone()));
            let focused = paste.has_focus();
            if paste.on_hover_text(lang.tr("Paste")).clicked() {
                let ctx = ui.ctx().clone();
                app.paste_from_history(&ctx, Some(i));
            }
            ui.horizontal(|ui| {
                let kind = if row.cut { lang.tr("Cut") } else { lang.tr("Copy") };
                ui.label(RichText::new(format!("{kind} · {}", row.place)).font(theme::ui_font(11.5)).color(t.text_dim));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let delete = ui.small_button(lang.tr("Delete"));
                    delete.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, lang.tr("Delete")));
                    if delete.clicked() || focused && ui.input(|inp| inp.key_pressed(Key::Delete)) {
                        app.run_or_alert("edit.deleteClipboardItem", json!({"index": i}));
                    }
                });
            });
        });
    });
}
