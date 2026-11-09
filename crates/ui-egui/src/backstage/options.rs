use super::{Key, Language, Modifiers, SheetApp, Ui, dim, i18n, json, section, vec2};

/// Excel's File › Options page: General (display theme, interface language and Show formula bar),
/// Editing (AutoComplete) and Save (the Turn on AutoSave by default checkbox, `file.options` with `autosaveByDefault`, and the
/// Version History folders, device name and delete buttons), plus Formulas
/// (calculation mode and iterative calculation) and the display options for this workbook (sheet
/// tabs and scroll bars) when a document is active. Theme, language, formula
/// bar, AutoComplete and calculation changes run their commands through `run_or_alert`; calculation
/// settings are saved with the workbook.
pub(super) fn options(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    section(ui, lang, "Options|General");
    general_options(app, ui, lang);
    ui.add_space(6.0);
    let mut formula_bar = app.session.prefs.formula_bar;
    if ui.checkbox(&mut formula_bar, lang.tr("Show formula bar")).changed() {
        app.run_or_alert("view.formulaBar", json!({"on": formula_bar}));
    }
    let mut highlight_row = app.ui.highlight_active_row;
    if ui.checkbox(&mut highlight_row, lang.tr("Highlight the active cell's row")).changed() {
        app.run_or_alert("options.highlightActiveRow", json!({"on": highlight_row}));
    }
    ui.add_space(18.0);
    section(ui, lang, "Options|Editing");
    let mut autocomplete = app.ui.autocomplete;
    if ui.checkbox(&mut autocomplete, lang.tr("Enable AutoComplete for cell values")).changed() {
        app.run_or_alert("options.autoComplete", json!({"on": autocomplete}));
    }
    ui.add_space(18.0);
    section(ui, lang, "Options|Save");
    let mut autosave = app.session.prefs.autosave_by_default;
    if ui.checkbox(&mut autosave, lang.tr("Turn on AutoSave by default")).changed() {
        app.set_autosave_by_default(autosave);
    }
    dim(ui, lang.tr("AutoSave keeps .xlsx, .xlsm and .xltx workbooks saved as you work, on this computer or in iCloud Drive, Google Drive or OneDrive. The title-bar switch sets it for one file."));
    #[cfg(not(target_arch = "wasm32"))]
    {
        ui.add_space(12.0);
        version_history(app, ui, lang);
    }
    let Some((calc, view)) = app.session.active().map(|d| (d.wb.calc.clone(), d.wb.view)) else { return };
    ui.add_space(18.0);
    // Excel's Advanced › Display options for this workbook: saved with the workbook (#3101).
    section(ui, lang, "Display options for this workbook");
    for (on, label, id) in [
        (view.show_horizontal_scroll, "Show horizontal scroll bar", "view.showHorizontalScrollBar"),
        (view.show_vertical_scroll, "Show vertical scroll bar", "view.showVerticalScrollBar"),
        (view.show_sheet_tabs, "Show sheet tabs", "view.showSheetTabs"),
    ] {
        let mut on = on;
        if ui.checkbox(&mut on, lang.tr(label)).changed() {
            app.run_or_alert(id, json!({"on": on}));
        }
    }
    ui.add_space(18.0);
    section(ui, lang, "Formulas");
    ui.label(lang.tr("Workbook calculation"));
    calc_options(app, ui, lang, &calc);
    ui.add_space(6.0);
    dim(ui, lang.tr("Calculation settings are saved with the workbook."));
}

/// File › Options › Save › Version History: where the history is kept (the settings folder,
/// another folder, or both), the device name versions record, and deleting history. Every change
/// runs `file.options` or `file.versions` (docs/cloud.md).
#[cfg(not(target_arch = "wasm32"))]
fn version_history(app: &mut SheetApp, ui: &mut Ui, lang: Language) {
    ui.label(egui::RichText::new(lang.tr("Version History")).strong());
    let saved = app.session.prefs.versions_folder.clone().unwrap_or_default();
    let folder_id = ui.make_persistent_id("options_versions_folder");
    let existing_id = ui.make_persistent_id("options_versions_existing");
    let mut folder = ui.data(|d| d.get_temp::<String>(folder_id)).unwrap_or_else(|| saved.clone());
    let mut existing = ui.data(|d| d.get_temp::<String>(existing_id)).unwrap_or_else(|| "copy".to_owned());
    ui.label(lang.tr("Version history folder:"));
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut folder).desired_width(320.0).hint_text(lang.tr("Not set: the settings folder")));
        let apply = ui.add_enabled(folder.trim() != saved, egui::Button::new(lang.tr("Apply"))).clicked();
        if apply {
            let value = if folder.trim().is_empty() { "default" } else { folder.trim() };
            if run_checked(app, "file.options", json!({"versionsFolder": value, "versionsExisting": existing})) {
                folder = app.session.prefs.versions_folder.clone().unwrap_or_default();
            }
        }
        if ui.add_enabled(!saved.is_empty(), egui::Button::new(lang.tr("Use the settings folder"))).clicked()
            && run_checked(app, "file.options", json!({"versionsFolder": "default", "versionsExisting": existing}))
        {
            folder.clear();
        }
    });
    ui.horizontal(|ui| {
        ui.label(lang.tr("History already kept:"));
        for (value, label) in [("copy", "Copy it there"), ("move", "Move it there"), ("leave", "Leave it where it is")] {
            ui.radio_value(&mut existing, value.to_owned(), lang.tr(label));
        }
    });
    let mut mirror = app.session.prefs.versions_mirror;
    if ui
        .add_enabled(!saved.is_empty(), egui::Checkbox::new(&mut mirror, lang.tr("Mirror folder: also keep the history in the settings folder")))
        .changed()
    {
        app.run_or_alert("file.options", json!({"versionsMirror": mirror, "versionsExisting": existing}));
    }
    ui.data_mut(|d| {
        d.insert_temp(folder_id, folder);
        d.insert_temp(existing_id, existing);
    });
    let (_, unavailable, relocating) = app.session.version_folders();
    if relocating {
        dim(ui, lang.tr("Copying the version history to the new folder…"));
    }
    for away in &unavailable {
        dim(ui, &format!("{} {}", lang.tr("Not reachable now; brought up to date when it is back:"), away.display()));
    }
    dim(
        ui,
        lang.tr("A history folder inside iCloud Drive, OneDrive or Google Drive is as private as that account: anyone with access to it can open your earlier versions."),
    );
    device_label(app, ui, lang);
    ui.add_space(6.0);
    delete_history(app, ui, lang);
}

/// Runs a command and shows its error in a message box; whether it worked.
#[cfg(not(target_arch = "wasm32"))]
fn run_checked(app: &mut SheetApp, id: &str, params: serde_json::Value) -> bool {
    match app.run(id, params) {
        Ok(_) => true,
        Err(e) => {
            app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
            false
        }
    }
}

/// The device name versions record instead of the computer's own name.
#[cfg(not(target_arch = "wasm32"))]
fn device_label(app: &mut SheetApp, ui: &mut Ui, lang: Language) {
    let label_id = ui.make_persistent_id("options_device_label");
    let saved = app.session.prefs.device_label.clone().unwrap_or_default();
    let mut label = ui.data(|d| d.get_temp::<String>(label_id)).unwrap_or_else(|| saved.clone());
    let default_label = gridcraft_engine::versions::default_device_label();
    ui.horizontal(|ui| {
        ui.label(lang.tr("Device name in the history:"));
        ui.add(
            egui::TextEdit::singleline(&mut label)
                .desired_width(200.0)
                .char_limit(gridcraft_engine::versions::MAX_DEVICE_LABEL_CHARS)
                .hint_text(default_label),
        );
        if ui.add_enabled(label.trim() != saved, egui::Button::new(lang.tr("Apply"))).clicked() {
            let value = if label.trim().is_empty() { "auto" } else { label.trim() };
            app.run_or_alert("file.options", json!({"deviceLabel": value}));
        }
    });
    ui.data_mut(|d| d.insert_temp(label_id, label));
}

/// The two delete buttons; each asks once more, in place, before `file.versions` deletes.
#[cfg(not(target_arch = "wasm32"))]
fn delete_history(app: &mut SheetApp, ui: &mut Ui, lang: Language) {
    let confirm_id = ui.make_persistent_id("options_versions_delete");
    let mut confirm = ui.data(|d| d.get_temp::<String>(confirm_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        let has_file = app.session.active().is_some_and(|d| d.path.is_some());
        if ui.add_enabled(has_file, egui::Button::new(lang.tr("Delete this document's history…"))).clicked() {
            confirm = "document".into();
        }
        if ui.button(lang.tr("Delete all version history…")).clicked() {
            confirm = "all".into();
        }
    });
    if !confirm.is_empty() {
        ui.label(if confirm == "all" {
            lang.tr("Delete every document's version history, in every history folder? This cannot be undone.")
        } else {
            lang.tr("Delete this document's version history, in every history folder? This cannot be undone.")
        });
        ui.horizontal(|ui| {
            if ui.button(lang.tr("Delete")).clicked() {
                app.run_or_alert("file.versions", json!({"delete": confirm}));
                confirm.clear();
            }
            if ui.button(lang.tr("Cancel")).clicked() {
                confirm.clear();
            }
        });
    }
    ui.data_mut(|d| d.insert_temp(confirm_id, confirm));
}

/// The display theme and interface language controls of the General section.
fn general_options(app: &mut SheetApp, ui: &mut Ui, lang: Language) {
    egui::Grid::new("backstage_options_general").num_columns(2).spacing(vec2(18.0, 10.0)).show(ui, |ui| {
        let label = ui.label(lang.tr("Display theme"));
        let mode = app.ui.theme_mode();
        let current = match mode {
            "system" => "System",
            "dark" => "Dark",
            _ => "Light",
        };
        craft_ui::ComboBox::from_id_salt("backstage_theme")
            .width(160.0)
            .selected_text(lang.tr(current))
            .show_ui(ui, |ui| {
                for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                    if ui.selectable_label(mode == value, lang.tr(label)).clicked() {
                        app.run_or_alert("view.theme", json!({"mode": value}));
                    }
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();
        let label = ui.label(lang.tr("Interface language"));
        craft_ui::ComboBox::from_id_salt("backstage_language")
            .width(160.0)
            .selected_text(lang.name())
            .show_ui(ui, |ui| {
                for l in i18n::Language::for_picker() {
                    if ui.selectable_label(l == lang, l.name()).clicked() {
                        app.run_or_alert("app.language.set", json!({"language": l.code()}));
                    }
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();
        ui.label(lang.tr("Accent color"));
        accent_picker(app, ui, lang);
        ui.end_row();
    });
    ui.add_space(6.0);
    let mut discord = app.ui.show_discord;
    if ui.checkbox(&mut discord, lang.tr("Show the Discord button")).changed() {
        app.run_or_alert("options.showDiscord", json!({"on": discord}));
    }
    let mut avatar = app.ui.show_account_picture;
    if ui.checkbox(&mut avatar, lang.tr("Show the account avatar")).changed() {
        app.run_or_alert("options.showAccountPicture", json!({"on": avatar}));
    }
}

/// The accent colour choices: the named presets as swatches, the default among them, and a
/// colour picker for any other colour. Every change runs `options.accentColor`.
fn accent_picker(app: &mut SheetApp, ui: &mut Ui, lang: Language) {
    let current = app.ui.accent_color;
    ui.horizontal(|ui| {
        for (name, preset) in crate::theme::AccentColor::PRESETS {
            let name = lang.tr(name);
            let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
            let selected = preset == current;
            ui.painter().rect_filled(rect, 3.0, preset.color32());
            if selected {
                ui.painter().rect_stroke(rect.expand(2.0), 4.0, egui::Stroke::new(2.0, ui.visuals().text_color()), egui::StrokeKind::Outside);
            }
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, name));
            if resp.on_hover_text(name).clicked() {
                app.run_or_alert("options.accentColor", json!({"value": preset.hex()}));
            }
        }
        let mut custom = current.0;
        let picker = ui.color_edit_button_srgb(&mut custom);
        picker.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, lang.tr("Custom accent color")));
        if picker.changed() {
            app.run_or_alert("options.accentColor", json!({"value": crate::theme::AccentColor(custom).hex()}));
        }
    });
}

/// The calculation mode, and iterative calculation, of the active workbook.
fn calc_options(app: &mut SheetApp, ui: &mut Ui, lang: Language, calc: &gridcraft_engine::model::CalcSettings) {
    use gridcraft_engine::model::CalcMode;
    for (mode, id, label) in [
        (CalcMode::Automatic, "automatic", "Automatic"),
        (CalcMode::AutomaticExceptTables, "automaticExceptTables", "Automatic Except for Data Tables"),
        (CalcMode::Manual, "manual", "Manual"),
    ] {
        if ui.radio(calc.mode == mode, lang.tr(label)).clicked() && calc.mode != mode {
            app.run_or_alert("formulas.calculationOptions", json!({"mode": id}));
        }
    }
    ui.add_space(4.0);
    let mut iterative = calc.iterative;
    if ui.checkbox(&mut iterative, lang.tr("Enable iterative calculation")).changed() {
        app.run_or_alert("formulas.calculationOptions", json!({"iterative": iterative}));
    }
    // Maximum Iterations and Maximum Change apply only while iteration is on, as in Excel.
    iteration_options(app, ui, lang, calc, iterative);
    // TODO: add "Set precision as displayed" here once `formulas.calculationOptions` takes
    // `precisionAsDisplayed` (PR #1366); the model already stores it.
}

/// Maximum Iterations and Maximum Change, live only while iterative calculation is on.
fn iteration_options(app: &mut SheetApp, ui: &mut Ui, lang: Language, calc: &gridcraft_engine::model::CalcSettings, iterative: bool) {
    ui.add_enabled_ui(iterative, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            egui::Grid::new("backstage_options_iteration").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                let label = ui.label(lang.tr("Maximum Iterations:"));
                let id = ui.make_persistent_id("backstage_max_iterations");
                // Text left in the field when iteration is switched off (it loses focus) is dropped.
                let committed = crate::widgets::committed_text(ui, id, &calc.max_iterations.to_string(), Some(80.0));
                if let Some(response) = ui.ctx().read_response(id) {
                    response.labelled_by(label.id);
                }
                if let Some(text) = committed.filter(|_| iterative) {
                    match parse_max_iterations(&text) {
                        Some(n) if n != calc.max_iterations => {
                            app.run_or_alert("formulas.calculationOptions", json!({"maxIterations": n}));
                        }
                        Some(_) => {}
                        None => refuse(app, ui, "Maximum Iterations must be a whole number from 1 to 32767."),
                    }
                }
                ui.end_row();
                let label = ui.label(lang.tr("Maximum Change:"));
                let id = ui.make_persistent_id("backstage_max_change");
                let committed = crate::widgets::committed_text(ui, id, &calc.max_change.to_string(), Some(80.0));
                if let Some(response) = ui.ctx().read_response(id) {
                    response.labelled_by(label.id);
                }
                if let Some(text) = committed.filter(|_| iterative) {
                    match parse_max_change(&text, lang.formula_locale()) {
                        Some(d) if d != calc.max_change => {
                            app.run_or_alert("formulas.calculationOptions", json!({"maxChange": d}));
                        }
                        Some(_) => {}
                        None => refuse(app, ui, "Maximum Change must be a number of 0 or more."),
                    }
                }
                ui.end_row();
            });
        });
    });
}

/// Refuses a typed setting with a message, leaving the workbook as it was. The Enter that
/// committed the field is consumed so it doesn't also dismiss the message box in this frame.
fn refuse(app: &mut SheetApp, ui: &mut Ui, text: &str) {
    ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    app.message = Some(("GridCraft".into(), app.ui.language.tr(text).into()));
}

/// Maximum Iterations as typed: a whole number from 1 to 32767, or `None` (Excel refuses others).
fn parse_max_iterations(text: &str) -> Option<u32> {
    let n = text.trim().parse::<u32>().ok()?;
    (1..=gridcraft_engine::model::CalcSettings::MAX_ITERATIONS).contains(&n).then_some(n)
}

/// Maximum Change as typed: a finite number of 0 or more, or `None`. The interface's decimal
/// separator is accepted as well as a dot.
fn parse_max_change(text: &str, locale: gridcraft_engine::formula::FormulaLocale) -> Option<f64> {
    let d = text.trim().replace(locale.decimal_separator(), ".").parse::<f64>().ok()?;
    (d.is_finite() && d >= 0.0).then_some(d)
}

/// The platform the app was built for, e.g. "macOS (aarch64)"; the web build reports "Web".
pub(super) fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "netbsd" => "NetBSD",
        _ if cfg!(target_arch = "wasm32") => return "Web".into(),
        other => other,
    };
    format!("{os} ({})", std::env::consts::ARCH)
}
