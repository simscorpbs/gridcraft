//! File › Info › Version History in the interface: the right-hand history pane, the file menu the
//! title bar opens, and the Previous version bar shown above a version opened from the pane.
//!
//! The pane lists the versions of the workbook (or of the workbook a shown version came from)
//! newest first under day headings, each with its time, its author and an Open version link. A
//! version opens as a separate read-only workbook beside the current one (the engine refuses
//! edits, `gridcraft_engine::version_view`): the bar above it offers Compare, Restore, Save a
//! copy and Close, its row in the pane is highlighted and has no link. All of it is commands
//! (`file.openVersion`, `file.closeVersion`, `file.compareVersion`, `file.restoreVersion`,
//! `file.saveVersionCopy`, `file.versions`, `file.rename`); the behaviour is described in
//! `docs/cloud.md`.

#[cfg(not(target_arch = "wasm32"))]
use egui::CornerRadius;
use egui::{Align, Key, RichText, Stroke, Ui};
use serde_json::{Value as Json, json};

use crate::SheetApp;
#[cfg(not(target_arch = "wasm32"))]
use crate::i18n::Language;
use crate::theme::{self, Tokens};

/// The pane's name in [`crate::grid::GridState::pane`].
pub const PANE: &str = "versions";

/// Open the Version History pane.
pub fn open_pane(app: &mut SheetApp) {
    app.grid.pane = Some(PANE.into());
}

/// Whether this build keeps a version history at all (the desktop app does, with its store).
fn has_history(app: &SheetApp) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.session.has_versions()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        false
    }
}

/// Whether there is no saved workbook whose history could be listed.
fn no_file(app: &SheetApp) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.session.history_source().is_none() && app.session.previous_version().is_none()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        true
    }
}

/// Why the pane has no versions to list, in English (the catalogs translate it).
fn empty_message(app: &SheetApp) -> &'static str {
    if !has_history(app) {
        "Version history is kept by the desktop app."
    } else if no_file(app) {
        "Save the workbook to start its version history."
    } else {
        "No versions yet."
    }
}

// ---------------------------------------------------------------- dates and times

#[cfg(not(target_arch = "wasm32"))]
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
#[cfg(not(target_arch = "wasm32"))]
const MONTHS_PT: [&str; 12] =
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];

/// The month's name in English or Brazilian Portuguese.
#[cfg(not(target_arch = "wasm32"))]
fn month_name(names: &[&'static str; 12], month: u32) -> &'static str {
    usize::try_from(month).ok().and_then(|m| m.checked_sub(1)).and_then(|m| names.get(m)).copied().unwrap_or("")
}

/// A day written the way the interface language writes it: with the month's name in English and
/// Portuguese, as an ISO date in the other languages (never wrong, only plainer).
#[cfg(not(target_arch = "wasm32"))]
pub fn full_date(day: &gridcraft_engine::versions::LocalWhen, lang: Language) -> String {
    match lang {
        Language::En => format!("{} {}, {}", month_name(&MONTHS_EN, day.month), day.day, day.year),
        Language::PtBr => format!("{} de {} de {}", day.day, month_name(&MONTHS_PT, day.month), day.year),
        _ => format!("{:04}-{:02}-{:02}", day.year, day.month, day.day),
    }
}

/// A day heading: Today, Yesterday, or the date.
#[cfg(not(target_arch = "wasm32"))]
pub fn day_heading(day: &gridcraft_engine::versions::LocalWhen, lang: Language) -> String {
    match day.days_ago {
        0 => lang.tr("Today").to_string(),
        1 => lang.tr("Yesterday").to_string(),
        _ => full_date(day, lang),
    }
}

/// A version's time of day: `3:05 PM` in English, `15:05` elsewhere.
#[cfg(not(target_arch = "wasm32"))]
pub fn time_of_day(day: &gridcraft_engine::versions::LocalWhen, lang: Language) -> String {
    if lang == Language::En {
        let hour = match day.hour % 12 {
            0 => 12,
            h => h,
        };
        format!("{hour}:{:02} {}", day.minute, if day.hour < 12 { "AM" } else { "PM" })
    } else {
        format!("{:02}:{:02}", day.hour, day.minute)
    }
}

/// A version's full date and time, for tooltips and the Previous version bar.
#[cfg(not(target_arch = "wasm32"))]
pub fn date_and_time(saved_at: &str, lang: Language) -> String {
    match gridcraft_engine::versions::local_when(saved_at) {
        Some(day) => format!("{}, {}", full_date(&day, lang), time_of_day(&day, lang)),
        None => gridcraft_engine::versions::local_timestamp(saved_at),
    }
}

// ---------------------------------------------------------------- the pane

/// The Version History pane's content (the task pane draws its frame and title).
pub fn content(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let versions = app.session.version_infos();
        if !versions.is_empty() {
            list(app, ui, &versions, t);
            return;
        }
    }
    ui.label(RichText::new(lang.tr(empty_message(app))).font(theme::ui_font(12.5)).color(t.text_dim));
}

#[cfg(not(target_arch = "wasm32"))]
fn list(app: &mut SheetApp, ui: &mut Ui, versions: &[gridcraft_engine::versions::VersionInfo], t: Tokens) {
    let lang = app.ui.language;
    let shown = app.session.previous_version().map(|v| v.id.clone());
    let mut day: Option<(i32, u32, u32)> = None;
    for info in versions {
        let when = gridcraft_engine::versions::local_when(&info.saved_at);
        let this_day = when.map(|w| (w.year, w.month, w.day));
        if day.is_none() || this_day != day {
            day = this_day;
            ui.add_space(8.0);
            let heading = match when {
                Some(w) => day_heading(&w, lang),
                None => gridcraft_engine::versions::local_timestamp(&info.saved_at).chars().take(10).collect(),
            };
            ui.label(RichText::new(heading).font(theme::ui_bold(12.5)).color(t.text));
            ui.add_space(2.0);
        }
        let time = match when {
            Some(w) => time_of_day(&w, lang),
            None => gridcraft_engine::versions::local_timestamp(&info.saved_at),
        };
        row(app, ui, info, &time, shown.as_deref() == Some(info.id.as_str()), t);
    }
}

/// One version: its time and author, and Open version. The version shown is highlighted and has
/// no link (it is already open). Right-click offers Open version and Restore.
#[cfg(not(target_arch = "wasm32"))]
fn row(app: &mut SheetApp, ui: &mut Ui, info: &gridcraft_engine::versions::VersionInfo, time: &str, shown: bool, t: Tokens) {
    let lang = app.ui.language;
    let fill = if shown { t.accent_soft } else { egui::Color32::TRANSPARENT };
    let builder = egui::UiBuilder::new().id_salt(("version_row", info.id.as_str()));
    let inner = ui.scope_builder(builder, |ui| {
        egui::Frame::NONE.fill(fill).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(time).font(theme::ui_bold(12.5)).color(t.text));
            if !info.author.is_empty() {
                ui.label(RichText::new(&info.author).font(theme::ui_font(11.5)).color(t.text_dim));
            }
            if !shown {
                let link = egui::Button::new(RichText::new(lang.tr("Open version")).font(theme::ui_font(12.0)).color(t.accent)).frame(false);
                if ui.add(link).clicked() {
                    app.run_or_alert("file.openVersion", json!({"id": info.id}));
                }
            }
        });
    });
    let response = inner.response.on_hover_text(date_and_time(&info.saved_at, lang));
    // Anywhere in the row, also on its link.
    response.container_context_menu(|ui| {
        if !shown && ui.button(lang.tr("Open version")).clicked() {
            app.run_or_alert("file.openVersion", json!({"id": info.id}));
            ui.close();
        }
        if ui.button(lang.tr("Restore")).clicked() {
            restore(app, &info.id);
            ui.close();
        }
    });
    ui.add_space(2.0);
}

/// Restore version `id` of the workbook: from the shown version's own bar it goes back to the
/// workbook first (`file.restoreVersion`); any other version of its history is restored the same
/// way after leaving the version being shown.
pub fn restore(app: &mut SheetApp, id: &str) {
    match app.session.previous_version().map(|v| v.id == id) {
        Some(true) => app.run_or_alert("file.restoreVersion", json!({})),
        Some(false) => {
            if let Ok(back) = app.run("file.closeVersion", json!({})) {
                if back.get("doc").is_some_and(Json::is_u64) {
                    app.run_or_alert("file.versions", json!({"restore": id}));
                } else {
                    app.alert("the workbook this version came from is closed".into());
                }
            }
        }
        None => app.run_or_alert("file.versions", json!({"restore": id})),
    }
}

// ---------------------------------------------------------------- the Previous version bar

/// The bar Excel shows above a workbook opened from Version History: which version this is, and
/// Compare, Restore, Save a copy and Close.
pub fn previous_bar(app: &mut SheetApp, ui: &mut Ui) {
    let Some(view) = app.session.previous_version().cloned() else { return };
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    #[cfg(not(target_arch = "wasm32"))]
    let when = date_and_time(&view.saved_at, lang);
    #[cfg(target_arch = "wasm32")]
    let when = view.saved_at.clone();
    let can_compare = app.session.document_index(view.source_uid).is_some();
    egui::Panel::top("previous_version_bar")
        .exact_size(36.0)
        .frame(egui::Frame::NONE.fill(t.accent_soft).inner_margin(egui::Margin::symmetric(12, 6)).stroke(Stroke::new(1.0, t.ribbon_border)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new(lang.tr("Previous version")).font(theme::ui_bold(12.5)).color(t.text));
                let who = if view.author.is_empty() { when } else { format!("{when} · {}", view.author) };
                ui.label(RichText::new(who).font(theme::ui_font(12.0)).color(t.text_dim));
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(lang.tr("Close")).on_hover_text(lang.tr("Back to the current version")).clicked() {
                        app.run_or_alert("file.closeVersion", json!({}));
                    }
                    if ui.button(lang.tr("Save a copy")).clicked() {
                        app.run_or_alert("file.saveVersionCopy", json!({}));
                    }
                    if ui.add(egui::Button::new(RichText::new(lang.tr("Restore")).color(t.on_accent)).fill(t.accent)).clicked() {
                        restore(app, &view.id);
                    }
                    let tip = if view.compared { "Show the previous version again" } else { "Show what changed since this version" };
                    let compare = egui::Button::new(lang.tr("Compare")).selected(view.compared);
                    if ui.add_enabled(can_compare, compare).on_hover_text(lang.tr(tip)).clicked() {
                        app.run_or_alert("file.compareVersion", json!({}));
                    }
                });
            });
        });
}

// ---------------------------------------------------------------- the title bar's file menu

/// The menu the title bar opens when the document name is clicked: the file name, which can be
/// changed there and applied with Enter (`file.rename`); the folder that holds the file, or Not
/// saved; and Version History.
pub fn file_menu(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let Some(d) = app.session.active() else { return };
    let (uid, title, path) = (d.uid, d.display_title(), d.path.clone());
    let version = d.version.as_ref().map(|v| v.source_path.clone());
    ui.set_min_width(280.0);
    ui.label(RichText::new(lang.tr("File name")).font(theme::ui_font(11.5)).color(t.text_dim));
    match &path {
        Some(file) => {
            let stem = std::path::Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let id = ui.make_persistent_id(("file_menu_name", uid));
            // The file's own name unless the field is being edited (a rename elsewhere shows).
            let mut draft: String = ui.ctx().data_mut(|m| m.get_temp(id)).unwrap_or_default();
            if !ui.ctx().memory(|m| m.has_focus(id)) {
                draft.clone_from(&stem);
            }
            let field = ui.add(egui::TextEdit::singleline(&mut draft).id(id).desired_width(f32::INFINITY));
            field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, lang.tr("File name")));
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.ctx().data_mut(|m| m.insert_temp(id, draft.clone()));
            if enter && draft != stem {
                if rename(app, &draft) {
                    ui.ctx().data_mut(|m| m.remove::<String>(id));
                    ui.close();
                    return;
                }
            } else if enter {
                ui.close();
            }
        }
        None => {
            ui.label(RichText::new(&title).font(theme::ui_bold(13.0)).color(t.text));
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new(lang.tr("Folder")).font(theme::ui_font(11.5)).color(t.text_dim));
    let shown_path = path.as_ref().or(version.as_ref());
    match shown_path.and_then(|f| std::path::Path::new(f).parent().map(|p| p.to_string_lossy().into_owned())) {
        Some(folder) if !folder.is_empty() => {
            ui.label(RichText::new(folder).font(theme::ui_font(12.5)).color(t.text));
        }
        _ => {
            ui.label(RichText::new(lang.tr("Not saved")).font(theme::ui_font(12.5)).color(t.text));
        }
    }
    ui.separator();
    if ui.button(lang.tr("Version History")).clicked() {
        open_pane(app);
        ui.close();
    }
}

/// Rename the active workbook's file to `typed` (File › Info › Rename), keeping the Recent list
/// in step. Returns whether it was renamed.
fn rename(app: &mut SheetApp, typed: &str) -> bool {
    match app.run("file.rename", json!({"name": typed})) {
        Ok(r) => {
            if let (Some(from), Some(to)) = (r.get("from").and_then(Json::as_str), r.get("path").and_then(Json::as_str))
                && from != to
            {
                let key = gridcraft_engine::autosave::path_text_key(from);
                for recent in &mut app.ui.recent {
                    if gridcraft_engine::autosave::path_text_key(recent) == key {
                        *recent = to.to_string();
                    }
                }
            }
            true
        }
        Err(e) => {
            app.alert(e);
            false
        }
    }
}

/// The title the title bar and the window show for a workbook: a previous version says so.
pub fn shown_title(app: &SheetApp, title: &str) -> String {
    if app.session.previous_version().is_some() {
        format!("{title} — {}", app.ui.language.tr("Previous version"))
    } else {
        title.to_string()
    }
}
