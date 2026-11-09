use super::controls::{act, control_keytip};
use super::{Color32, Icon, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, icon_button, json, pos2, small_button, theme, theme_choices, vec2};

/// macOS draws the window into the title bar (`with_fullsize_content_view`) and AppKit paints the
/// traffic lights itself, centred in its standard 28 pt title-bar band. Moving them would take
/// AppKit calls (`unsafe`, forbidden here), so on macOS the title bar is that band: the lights sit
/// vertically centred in it, with the controls beside them (#170).
const MAC: bool = cfg!(target_os = "macos");

pub const MAC_TITLE_BAR: f32 = 28.0;

/// The traffic lights as AppKit lays them out: button diameter, centre-to-centre pitch and the
/// first button's left edge (8–10 pt depending on the macOS version; the larger value keeps the
/// gap after them from shrinking).
const MAC_LIGHT: f32 = 12.0;

const MAC_LIGHT_PITCH: f32 = 20.0;

const MAC_LIGHTS_LEFT: f32 = 10.0;

/// Where title-bar content starts on macOS: the end of the three lights plus the gap the lights
/// keep from the top of the window.
pub const MAC_CONTENT_LEFT: f32 = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT + (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;

/// Leave room for the macOS traffic lights when the content extends into the title bar.
pub const TITLE_LEFT_PAD: f32 = if MAC { MAC_CONTENT_LEFT } else { 8.0 };

pub(super) const TITLE_BAR_H: f32 = if MAC { MAC_TITLE_BAR } else { 38.0 };

/// The tallest title-bar controls, smaller on macOS to keep a margin in the shorter bar.
pub(super) const TITLE_CONTROL_H: f32 = if MAC { 22.0 } else { 26.0 };

/// Draws the title bar: drag/maximize, AutoSave toggle, Home/Save/Undo/Redo/More, search, share, comments and theme controls, with a centered truncated title.
pub fn title_bar(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ch = TITLE_CONTROL_H;
    egui::Panel::top("title_bar").exact_size(TITLE_BAR_H).frame(egui::Frame::NONE.fill(t.window)).show(ui, |ui| {
        let rect = ui.max_rect();
        // Dragging the empty title area moves the window.
        let bg = ui.interact(rect, ui.scope_id().with("drag_title"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        ui.horizontal_centered(|ui| {
            ui.add_space(TITLE_LEFT_PAD);
            // AutoSave switch for this workbook's file: on when AutoSave writes it, off (with the
            // reason in the tooltip) for a new workbook, a format that loses things or a conflict.
            ui.label(egui::RichText::new(app.ui.language.tr("AutoSave")).font(theme::ui_font(12.5)).color(t.text_dim));
            let on = app.autosave_on();
            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 16.0), Sense::click());
            ui.painter().rect_filled(r, 8.0, if on { t.accent } else { Color32::TRANSPARENT });
            ui.painter().rect_stroke(r, 8.0, Stroke::new(1.0, if on { t.accent } else { t.text_dim }), StrokeKind::Inside);
            let knob = if on { pos2(r.right() - 8.0, r.center().y) } else { pos2(r.left() + 8.0, r.center().y) };
            ui.painter().circle_filled(knob, 5.0, if on { t.on_accent } else { t.text_dim });
            let tip = app.autosave_tooltip();
            if resp.on_hover_text(tip).clicked() {
                app.click_autosave_switch();
            }
            ui.add_space(6.0);
            let __k = icon_button(ui, Icon::Home, t.text_dim, "Home", vec2(26.0, ch));
            control_keytip(app, ui, &__k, "start");
            if __k.clicked() {
                app.open_dialog("start", json!({}));
            }
            if icon_button(ui, Icon::Save, t.text_dim, "Save", vec2(26.0, ch)).clicked()
                // File operations must include the cell the user is still editing (as the File > Save menu item already does).
                && app.commit_edit(0, 0, false, false)
            {
                app.run_or_alert("file.save", json!({}));
            }
            // The commands' own availability: there must be a step, and no undo group may be open on
            // the document (an action that spans several calls is still running).
            let can_undo = gridcraft_engine::cmd::can_undo(&app.session).is_ok();
            let can_redo = gridcraft_engine::cmd::can_redo(&app.session).is_ok();
            // Named and marked enabled or not for assistive technology (and the UI tests).
            let undo = icon_button(ui, Icon::Undo, if can_undo { t.text_dim } else { t.text_disabled }, "Undo", vec2(26.0, ch));
            undo.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, can_undo, "Undo"));
            if undo.clicked() && can_undo {
                app.run_or_alert("edit.undo", json!({}));
            }
            let undo_list = icon_button(ui, Icon::Chevron, t.text_dim, "Undo list", vec2(14.0, ch));
            egui::Popup::menu(&undo_list).show(|ui| {
                let labels: Vec<String> = if can_undo {
                    app.session.active().map(|d| d.undo.iter().rev().take(20).map(|e| e.label.clone()).collect()).unwrap_or_default()
                } else {
                    Vec::new()
                };
                if labels.is_empty() {
                    ui.label(crate::i18n::tr(ui, "Can't Undo"));
                }
                for (i, l) in labels.iter().enumerate() {
                    if ui.button(crate::i18n::fmt(ui, "Undo {label}", &[("label", l)])).clicked() {
                        app.run_or_alert("edit.undo", json!({"steps": i + 1}));
                    }
                }
            });
            let redo = icon_button(ui, Icon::Redo, if can_redo { t.text_dim } else { t.text_disabled }, "Redo", vec2(26.0, ch));
            redo.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, can_redo, "Redo"));
            if redo.clicked() && can_redo {
                app.run_or_alert("edit.redo", json!({}));
            }
            let more = icon_button(ui, Icon::More, t.text_dim, "More commands", vec2(26.0, ch));
            egui::Popup::menu(&more).show(|ui| {
                for (label, id) in [
                    ("New Workbook", "file.new"),
                    ("Open…", "file.open"),
                    ("Save As…", "file.saveAs"),
                    ("Sort A to Z", "data.sortAscending"),
                    ("Calculate Now", "formulas.calculateNow"),
                ] {
                    if ui.button(crate::i18n::tr(ui, label)).clicked() {
                        app.run_or_alert(id, json!({}));
                    }
                }
                if ui.button(crate::i18n::tr(ui, "Print…")).clicked() {
                    app.print_now(json!({}));
                }
            });
            let left_end = ui.min_rect().right();
            let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                // Account avatar (Options ▸ General): the user's initials on the accent colour.
                if app.ui.show_account_picture {
                    let name = app.session.prefs.user_name.clone();
                    let name = if name.trim().is_empty() { "GridCraft".to_string() } else { name };
                    let (r, resp) = ui.allocate_exact_size(vec2(ch, ch), Sense::click());
                    ui.painter().circle_filled(r.center(), ch / 2.0 - 1.0, t.accent);
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, initials(&name), theme::ui_bold(10.5), t.on_accent);
                    let label = app.ui.language.tr("Account avatar");
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
                    if resp.on_hover_text(name).clicked() {
                        app.run_or_alert("ui.backstage", json!({"page": "account"}));
                    }
                    ui.add_space(8.0);
                }
                // The Discord community button (Options ▸ General, off by default).
                if app.ui.show_discord {
                    let label = "Discord";
                    let (r, resp) = ui.allocate_exact_size(vec2(72.0, 22.0), Sense::click());
                    ui.painter().rect_filled(r, 11.0, if resp.hovered() { t.accent_dark } else { t.accent });
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, label, theme::ui_bold(12.0), t.on_accent);
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
                    if resp.on_hover_text(app.ui.language.tr("Join the ArtCraft Discord")).clicked() {
                        match &app.services.open_url {
                            Some(open) => open(DISCORD_URL),
                            None => ui.ctx().open_url(egui::OpenUrl::new_tab(DISCORD_URL)),
                        }
                    }
                    ui.add_space(8.0);
                }
                let __k = icon_button(ui, Icon::Search, t.text_dim, "Search commands", vec2(28.0, ch));
                control_keytip(app, ui, &__k, "commandSearch");
                if __k.clicked() {
                    app.open_dialog("commandSearch", json!({}));
                }
                let share = small_button(ui, Icon::Share, "Share", "Share: export or save a copy", true);
                egui::Popup::menu(&share).show(|ui| {
                    if ui.button(crate::i18n::tr(ui, "Export as PDF…")).clicked() {
                        app.run_or_alert("file.exportPdf", json!({}));
                    }
                    for (label, fmt) in
                        [("Save a Copy as Excel Workbook (.xlsx)…", "xlsx"), ("Export as CSV…", "csv"), ("Export as Web Page (.html)…", "html")]
                    {
                        if ui.button(crate::i18n::tr(ui, label)).clicked() {
                            app.open_dialog("saveCopy", json!({"format": fmt}));
                        }
                    }
                });
                let __k = icon_button(ui, Icon::Comment, t.text_dim, "Comments", vec2(28.0, ch));
                control_keytip(app, ui, &__k, "comments");
                if __k.clicked() {
                    app.open_dialog("comments", json!({}));
                }
                // Quick light/dark switch (#194); right-click offers System as well. The same
                // choice lives in File › Display theme and the View tab.
                let lang = app.ui.language;
                let theme = icon_button(ui, Icon::Appearance, t.text_dim, "Dark Mode", vec2(28.0, 26.0));
                let dark = ui.ctx().theme() == egui::Theme::Dark;
                theme.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, dark, lang.tr("Dark Mode")));
                if theme.clicked() {
                    act(app, "view.darkMode", json!({}));
                }
                theme.context_menu(|ui| theme_choices(app, ui));
                ui.min_rect().left()
            });
            // Centred title, cut with "…" so it never runs under the controls on either side.
            let title = app.session.active().map(|d| crate::versions_pane::shown_title(app, &d.display_title())).unwrap_or_default();
            let dirty = app.session.active().is_some_and(|d| d.is_dirty());
            let max_w = title_max_width(rect.center().x, left_end, right.inner);
            let title_rect = paint_title(ui, rect.center(), &title, dirty, max_w, t.text);
            // Clicking the name, with either button, opens the file menu: the file name (which can
            // be changed there), its folder, and Version History.
            if app.session.active().is_some() {
                let hit = ui.interact(title_rect.expand2(vec2(8.0, 4.0)), ui.scope_id().with("title_file_menu"), Sense::click());
                let label = app.ui.language.tr("File name and Version History");
                hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
                // One popup for both buttons: a click toggles it, a right click opens it.
                let open = if hit.secondary_clicked() {
                    Some(egui::SetOpenCommand::Bool(true))
                } else if hit.clicked() {
                    Some(egui::SetOpenCommand::Toggle)
                } else {
                    None
                };
                // The name field is inside it: clicking in the menu must not close it.
                egui::Popup::menu(&hit)
                    .open_memory(open)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| crate::versions_pane::file_menu(app, ui));
            }
        });
    });
}

/// The ArtCraft community, the same link the About box and the Help menu use.
const DISCORD_URL: &str = "https://discord.gg/artcraft";

/// The avatar's letters: the first letters of the first two words of the name, upper case, or a
/// single "?" for a blank name.
fn initials(name: &str) -> String {
    let letters: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).flat_map(char::to_uppercase).collect();
    if letters.is_empty() { "?".into() } else { letters }
}

/// Lay out the title truncated to fit `max_w` together with the unsaved marker (when `dirty`).
fn title_galleys(
    painter: &egui::Painter,
    title: &str,
    dirty: bool,
    max_w: f32,
    color: Color32,
) -> (std::sync::Arc<egui::Galley>, Option<std::sync::Arc<egui::Galley>>) {
    let font = theme::ui_bold(13.0);
    let dot = dirty.then(|| painter.layout_no_wrap(" •".to_string(), font.clone(), color));
    let dot_w = dot.as_ref().map_or(0.0, |g| g.size().x);
    let mut job = egui::text::LayoutJob::simple_singleline(title.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((max_w - dot_w).max(0.0));
    (painter.layout_job(job), dot)
}

/// Gap kept between the title and the controls beside it.
const TITLE_GAP: f32 = 12.0;

/// The widest the centred title may be without reaching the controls that end at `left_end` or
/// start at `right_start`.
fn title_max_width(center_x: f32, left_end: f32, right_start: f32) -> f32 {
    let half = (center_x - left_end).min(right_start - center_x) - TITLE_GAP;
    (2.0 * half).max(0.0)
}

/// Paint the workbook title centred on `center`, truncated with "…" to `max_w`, and return the
/// space it took. The unsaved marker stays visible however short the title gets.
fn paint_title(ui: &Ui, center: egui::Pos2, title: &str, dirty: bool, max_w: f32, color: Color32) -> egui::Rect {
    let (galley, dot) = title_galleys(ui.painter(), title, dirty, max_w, color);
    let dot_w = dot.as_ref().map_or(0.0, |g| g.size().x);
    let w = galley.size().x + dot_w;
    let left = center.x - w / 2.0;
    let top = center.y - galley.size().y / 2.0;
    let galley_w = galley.size().x;
    let height = galley.size().y;
    ui.painter().galley(pos2(left, top), galley, color);
    if let Some(dot) = dot {
        ui.painter().galley(pos2(left + galley_w, top), dot, color);
    }
    egui::Rect::from_min_size(pos2(left, top), vec2(w, height))
}

#[cfg(test)]
mod title_bar_tests {
    use super::*;

    #[test]
    fn mac_title_bar_centres_and_clears_the_traffic_lights() {
        // The lights are centred in the bar, and content keeps the same gap after them.
        let margin = (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;
        let lights_end = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT;
        assert_eq!(MAC_CONTENT_LEFT - lights_end, margin);
    }

    #[test]
    fn avatar_initials_come_from_the_first_two_words() {
        assert_eq!(initials("GridCraft User"), "GU");
        assert_eq!(initials("ana maria souza"), "AM");
        assert_eq!(initials("  Brenno "), "B");
        assert_eq!(initials(""), "?");
        assert_eq!(initials("   "), "?");
    }

    #[test]
    fn title_fits_between_the_controls() {
        // Room limited by the nearer side, symmetric about the centre.
        assert_eq!(title_max_width(500.0, 300.0, 900.0), 2.0 * (200.0 - TITLE_GAP));
        assert_eq!(title_max_width(500.0, 100.0, 650.0), 2.0 * (150.0 - TITLE_GAP));
        // Controls overlapping the centre leave no room rather than a negative width.
        assert_eq!(title_max_width(500.0, 520.0, 900.0), 0.0);
    }

    /// Lay out a title in a real egui frame, returning (title text, title width, dot width).
    fn layout_title(title: &str, dirty: bool, max_w: f32) -> (String, f32, f32) {
        let ctx = egui::Context::default();
        ctx.set_fonts(theme::font_definitions());
        let mut out = (String::new(), 0.0, 0.0);
        let mut frame = ctx.run_ui(egui::RawInput::default(), |ui| {
            let (galley, dot) = title_galleys(ui.painter(), title, dirty, max_w, Color32::WHITE);
            // The glyphs actually drawn: `galley.text()` is the untruncated source.
            let shown = galley.rows.iter().flat_map(|r| r.glyphs.iter().map(|g| g.chr)).collect();
            out = (shown, galley.size().x, dot.map_or(0.0, |d| d.size().x));
        });
        frame.textures_delta.clear(); // nothing consumes the atlas in a headless test
        out
    }

    #[test]
    fn long_title_is_truncated_with_an_ellipsis() {
        let long = format!("upload-{}-cleaned (1).xlsx", "3FMyNTYg2gDaANiAAACq20AAAAQMBqug".repeat(4));
        let (text, w, _) = layout_title(&long, false, 300.0);
        assert!(text.ends_with('…'), "truncated title ends with an ellipsis: {text:?}");
        assert!(text.chars().count() < long.chars().count());
        assert!(w <= 300.0, "title stays within its room: {w}");
    }

    #[test]
    fn short_title_is_left_whole() {
        let (text, _, _) = layout_title("Book1.xlsx", false, 300.0);
        assert_eq!(text, "Book1.xlsx");
    }

    #[test]
    fn unsaved_dot_survives_truncation() {
        let long = "a very long workbook name ".repeat(20);
        let (text, w, dot_w) = layout_title(&long, true, 200.0);
        assert!(dot_w > 0.0, "the unsaved marker is laid out");
        assert!(text.ends_with('…'));
        assert!(w + dot_w <= 200.0, "title and marker fit together: {}", w + dot_w);
    }

    #[test]
    fn no_room_draws_nothing_but_does_not_crash() {
        let (_, w, _) = layout_title("Book1.xlsx", true, 0.0);
        assert!(w <= 20.0, "a zero-width slot leaves at most the ellipsis: {w}");
    }
}
