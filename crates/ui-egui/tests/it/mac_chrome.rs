//! The window drawn with Excel for Mac's measurements (the Mac profile forced on any platform):
//! the sizes of the panels the layout adds up, pinned. `docs/ui-parity.md` lists the numbers.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::formula_bar::BarLayout;
use gridcraft_ui_egui::metrics::{self, Metrics};
use gridcraft_ui_egui::ribbon::RibbonOverflow;
use gridcraft_ui_egui::SheetApp;

const HEIGHT: f32 = 800.0;

fn window() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(vec2(1200.0, HEIGHT)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(6);
    h
}

/// The test window keeps an 8 pt margin around the egui screen.
const MARGIN: f32 = 8.0;

#[test]
fn the_mac_window_adds_up_to_the_measured_heights() {
    let _mac = metrics::force_mac();
    let h = window();
    let ctx = h.ctx.clone();
    let grid = h.state().grid.rect.expect("the grid was laid out");
    // The sheet tab bar (21) sits right above the status bar (27): their middles are 24 apart.
    let tabs = h.get_by_label("Previous sheet").rect().center().y;
    let status = h.get_by_label("Zoom Out").rect().center().y;
    assert_eq!(status - tabs, 21.0 / 2.0 + 27.0 / 2.0, "sheet tabs 21 + status bar 27");
    // The ribbon card holds 73 points of controls (74 less half a point of air each side).
    let ribbon = RibbonOverflow::get(&ctx);
    assert_eq!(ribbon.visible_height, 73.0);
    assert!(ribbon.content_height <= ribbon.visible_height + 0.5, "the tallest group fits the card: {ribbon:?}");
    assert_eq!(BarLayout::get(&ctx).height, 19.0, "formula bar");
    // Everything above the grid: title bar + tab row (70) + the card with its margin + the bar.
    let m = Metrics::MAC;
    assert_eq!(grid.top(), MARGIN + m.top_frame_h() + (m.ribbon_panel_h(false) - m.tab_row_h) + 19.0, "above the grid");
}

#[test]
fn the_standard_window_keeps_its_heights() {
    let h = window();
    let grid = h.state().grid.rect.expect("the grid was laid out");
    let tabs = h.get_by_label("Previous sheet").rect().center().y;
    let status = h.get_by_label("Zoom Out").rect().center().y;
    assert_eq!(status - tabs, 32.0 / 2.0 + 26.0 / 2.0, "sheet tabs 32 + status bar 26");
    assert_eq!(grid.top(), MARGIN + 38.0 + 126.0 + 30.0, "title bar + ribbon + formula bar");
}
