//! The two AutoSave questions: what to keep when the file was changed elsewhere (Save Conflict),
//! and where a new workbook should be kept when AutoSave is turned on for it (Save to Cloud).

use super::{Dialog, Json, SheetApp, json};

/// AutoSave found the file changed, moved or deleted elsewhere. Keep both, keep this version,
/// open the other version, decide later; or, for a file that went missing, save it here again.
pub(super) fn save_conflict(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let name = d.values.get("title").and_then(Json::as_str).unwrap_or_default().to_string();
    let missing = d.values.get("missing").and_then(Json::as_bool) == Some(true);
    let heading = if missing { lang.tr("{name} was moved or deleted elsewhere") } else { lang.tr("{name} was changed on another device") };
    ui.label(egui::RichText::new(heading.replace("{name}", &name)).strong());
    ui.label(lang.tr("AutoSave stopped so that neither version is lost."));
    if !missing {
        ui.label(lang.tr("Keep both saves this version as a copy beside the other."));
    }
    ui.add_space(8.0);
    let t = crate::theme::Tokens::get(ui.ctx());
    let primary = |ui: &mut egui::Ui, text: &str| ui.add(egui::Button::new(egui::RichText::new(text).color(t.on_accent)).fill(t.accent)).clicked();
    let mut answer = None;
    ui.horizontal_wrapped(|ui| {
        if missing {
            if primary(ui, lang.tr("Save it here again")) {
                answer = Some("mine");
            }
        } else {
            if primary(ui, lang.tr("Keep both")) {
                answer = Some("both");
            }
            if ui.button(lang.tr("Keep this version")).clicked() {
                answer = Some("mine");
            }
            if ui.button(lang.tr("Open the other version")).clicked() {
                answer = Some("theirs");
            }
        }
        if ui.button(lang.tr("Decide later")).clicked() {
            answer = Some("later");
        }
    });
    match answer {
        Some("later") => *open = false,
        Some(keep) => match app.run("file.resolveConflict", json!({"keep": keep})) {
            Ok(_) => *open = false,
            Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
        },
        None => {}
    }
}

/// Turning AutoSave on for a new workbook: which cloud folder keeps it, or another location.
pub(super) fn save_to_cloud(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    ui.label(egui::RichText::new(lang.tr("Where should AutoSave keep this workbook?")).strong());
    ui.add_space(4.0);
    let mut name = d.values.get("name").and_then(Json::as_str).unwrap_or_default().to_string();
    ui.horizontal(|ui| {
        ui.label(lang.tr("File name:"));
        ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0));
    });
    d.set_value("name", json!(name.clone()));
    ui.add_space(8.0);
    let mut picked = None;
    for root in app.session.cloud_roots.clone() {
        let label = match &root.account {
            Some(account) => format!("{} ({account})", root.provider.name()),
            None => root.provider.name().to_string(),
        };
        if ui.button(label).on_hover_text(root.path.to_string_lossy()).clicked() {
            picked = Some(root);
        }
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button(lang.tr("Other location…")).clicked() {
            *open = false;
            app.open_dialog("saveAs", json!({}));
        }
        if ui.button(lang.tr("Cancel")).clicked() {
            *open = false;
        }
    });
    if let Some(root) = picked {
        // The folder this button names, even when a provider has several accounts.
        match gridcraft_cloud::unique_path(&root.save_dir(), &name, "xlsx") {
            Some(path) => {
                let saved = app.run("file.save", json!({"path": path.to_string_lossy()})).and_then(|_| app.run("file.autosave", json!({"value": true})));
                match saved {
                    Ok(_) => *open = false,
                    Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
                }
            }
            None => app.message = Some((d.title.clone(), app.ui.language.fmt("Too many workbooks named {name} in {provider}", &[("name", name.to_string()), ("provider", root.provider.name().to_string())]))),
        }
    }
}
