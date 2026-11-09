//! Chrome geometry: the sizes of the title and tab frame, ribbon, notice bar, formula bar, headers,
//! sheet tabs and status bar, in one place.
//!
//! Two profiles. [`Metrics::STANDARD`] is what GridCraft has always drawn (Windows and Linux).
//! [`Metrics::MAC`] is Excel for Mac 16 (Microsoft 365, light theme) as measured on Retina
//! screenshots, 2x pixels converted to points; it is chosen with `cfg(target_os = "macos")`
//! through [`Metrics::current`]. Tests force the Mac profile with [`force_mac`] (per thread), so
//! the macOS branches run, and are pinned, on every platform.
//!
//! Colours are not here: the light theme's Excel for Mac tones are tokens (`theme.rs`) on every
//! platform. Where each number comes from is tabulated in `docs/ui-parity.md`.

/// Sheet units (96 per inch, as in XLSX) per font point.
pub const SHEET_PX_PER_PT: f32 = 96.0 / 72.0;

/// How many points one sheet unit is drawn at 100% (a default 20-unit row is 16 pt tall). The grid
/// multiplies sheet units by this and by the zoom; see `grid::DISPLAY_SCALE`.
pub const DISPLAY_SCALE: f32 = 0.8;

/// Points one sheet unit is drawn at horizontally on a Mac at 100%: Excel for Mac draws the default
/// 64-unit column 65 points wide (64 plus the gridline), while a 20-unit row stays 16 points.
pub const MAC_COL_SCALE: f32 = 65.0 / 64.0;

/// Screen points a font point takes at 100% on the screen: Excel for Mac draws 11 pt text at 11
/// screen points (72 per inch), Excel for Windows at 96 per inch.
pub const MAC_SCREEN_PT_PER_FONT_PT: f32 = 1.0;
/// The same for 96 per inch: the long-standing GridCraft size (11 pt text is 11.7 points).
pub const STANDARD_SCREEN_PT_PER_FONT_PT: f32 = SHEET_PX_PER_PT * DISPLAY_SCALE;

/// The chrome's sizes, in points. See the module documentation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Excel for Mac geometry.
    pub mac: bool,
    /// The title bar (window controls, title, Share).
    pub title_bar_h: f32,
    /// The ribbon tab row under it.
    pub tab_row_h: f32,
    /// The white ribbon card.
    pub ribbon_card_h: f32,
    /// Gap between the ribbon's card and the window edge, each side.
    pub ribbon_inset_x: f32,
    /// Space between the card's edge and its controls, top and bottom.
    pub ribbon_inset_y: f32,
    /// Vertical gap between stacked ribbon controls.
    pub ribbon_spacing_y: f32,
    /// The notice (message) bar; `None` sizes it to its content.
    pub notice_bar_h: Option<f32>,
    /// The formula bar at one line, at least.
    pub formula_bar_h: f32,
    /// What the bar adds around its field's text rows.
    pub formula_pad: f32,
    /// Name Box, ✕ ✓ fx buttons and the field's frame, vertical size.
    pub formula_control_h: f32,
    /// Margin above and below the formula bar's controls.
    pub formula_margin_y: f32,
    /// Margin between the field's frame and its text, top and bottom.
    pub formula_text_inset_y: f32,
    /// Column header band.
    pub col_header_h: f32,
    /// Row header width: per digit (at least two) plus the padding.
    pub row_header_digit_w: f32,
    pub row_header_pad: f32,
    /// The sheet tab bar.
    pub sheet_tabs_h: f32,
    pub status_bar_h: f32,
    /// A filled Share button in the title bar; `None` keeps the ordinary ribbon button.
    pub share_button_h: Option<f32>,
    /// Screen points per font point at 100% zoom.
    pub screen_pt_per_font_pt: f32,
    /// Screen points per sheet unit along a row (column widths, x positions) at 100% zoom; rows
    /// use [`DISPLAY_SCALE`].
    pub col_scale: f32,
}

impl Metrics {
    /// Windows, Linux and the web.
    pub const STANDARD: Metrics = Metrics {
        mac: false,
        title_bar_h: 38.0,
        tab_row_h: 32.0,
        ribbon_card_h: 88.0,
        ribbon_inset_x: 10.0,
        ribbon_inset_y: 6.0,
        ribbon_spacing_y: 2.0,
        notice_bar_h: None,
        formula_bar_h: 30.0,
        formula_pad: 14.0,
        formula_control_h: 24.0,
        formula_margin_y: 3.0,
        formula_text_inset_y: 3.0,
        col_header_h: 21.0,
        row_header_digit_w: 7.0,
        row_header_pad: 14.0,
        sheet_tabs_h: 32.0,
        status_bar_h: 26.0,
        share_button_h: None,
        screen_pt_per_font_pt: STANDARD_SCREEN_PT_PER_FONT_PT,
        col_scale: DISPLAY_SCALE,
    };

    /// Excel for Mac 16.x, light theme.
    pub const MAC: Metrics = Metrics {
        mac: true,
        // AppKit's title-bar band (the traffic lights sit centred in it) plus the tab row make
        // the 70 pt from the window top to the ribbon card.
        title_bar_h: 28.0,
        tab_row_h: 42.0,
        ribbon_card_h: 74.0,
        ribbon_inset_x: 12.0,
        // 74 = 0.5 + 73 + 0.5: the tallest control column (73 pt) fits.
        ribbon_inset_y: 0.5,
        ribbon_spacing_y: 1.5,
        notice_bar_h: Some(27.0),
        formula_bar_h: 19.0,
        formula_pad: 3.0,
        formula_control_h: 19.0,
        formula_margin_y: 0.0,
        formula_text_inset_y: 1.0,
        col_header_h: 25.0,
        row_header_digit_w: 6.0,
        row_header_pad: 14.0,
        sheet_tabs_h: 21.0,
        status_bar_h: 27.0,
        share_button_h: Some(17.0),
        screen_pt_per_font_pt: MAC_SCREEN_PT_PER_FONT_PT,
        col_scale: MAC_COL_SCALE,
    };

    /// The profile this build draws: Mac on macOS, standard elsewhere; a test can force Mac.
    pub fn current() -> Metrics {
        if FORCE_MAC.with(std::cell::Cell::get) {
            return Metrics::MAC;
        }
        if cfg!(target_os = "macos") { Metrics::MAC } else { Metrics::STANDARD }
    }

    /// From the window top to the ribbon card: the title bar and the tab row.
    pub fn top_frame_h(&self) -> f32 {
        self.title_bar_h + self.tab_row_h
    }

    /// The ribbon panel's size: the tab row, and with the card open the card and its margin below.
    pub fn ribbon_panel_h(&self, collapsed: bool) -> f32 {
        let tabs = self.tab_row_h + 2.0;
        if collapsed { tabs } else { tabs + self.ribbon_card_h + 4.0 }
    }

    /// A ribbon tab's cell: the row less its 2 pt of air.
    pub fn tab_cell_h(&self) -> f32 {
        self.tab_row_h - 2.0
    }

    /// The row header's width for a sheet whose last visible row has `digits` digits, at `zoom`.
    pub fn row_header_w(&self, digits: i32, zoom: f32) -> f32 {
        ((digits.max(2) as f32 * self.row_header_digit_w + self.row_header_pad) * zoom.max(0.75)).round()
    }

    /// The column header's height at `zoom`.
    pub fn col_header_h_at(&self, zoom: f32) -> f32 {
        (self.col_header_h * zoom.max(0.75)).round()
    }

    /// A control in the sheet tab bar (arrows, New sheet): `size` at most, one point of air each side.
    pub fn sheet_tab_control(&self, size: f32) -> f32 {
        size.min(self.sheet_tabs_h - 2.0)
    }

    /// The sheet tab cell height.
    pub fn sheet_tab_cell_h(&self) -> f32 {
        self.sheet_tabs_h - 2.0
    }

    /// The formula bar's height for one line of a font whose rows are `row_h` tall: the profile's
    /// bar, unless the rows need more. `n` lines take `one_line + (n - 1) * row_h`.
    pub fn formula_one_line(&self, row_h: f32) -> f32 {
        self.formula_bar_h.max(row_h + self.formula_pad)
    }

    /// Screen points per sheet-unit-sized font pixel: a font of `pt` points is laid out at
    /// `pt * px_per_pt() * z`, `z` being the grid's unit scale (`zoom * DISPLAY_SCALE`).
    pub fn px_per_pt(&self) -> f32 {
        self.screen_pt_per_font_pt / DISPLAY_SCALE
    }
}

/// Font points to the pixels the grid multiplies by its scale `z`: 96/72 per point on Windows and
/// Linux, 1.25 on a Mac (so 11 pt text is 11 screen points at 100%). The one place the "100% on
/// Mac is 72 per inch" rule lives; measuring text in sheet units (column widths) still uses
/// [`SHEET_PX_PER_PT`].
pub fn px_per_pt() -> f32 {
    Metrics::current().px_per_pt()
}

/// The centre of a line `width` points wide whose far edge falls at `edge`, moved so both sides
/// land on whole device pixels at `pixels_per_point`. At 1 pixel per point and width 1 this is the
/// familiar `edge.round() - 0.5`.
pub fn snap_line(edge: f32, width: f32, pixels_per_point: f32) -> f32 {
    let ppp = pixels_per_point.max(0.25);
    ((edge - width) * ppp).round() / ppp + width / 2.0
}

thread_local! {
    static FORCE_MAC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Draws this thread's chrome with the Mac profile until the guard is dropped. For tests (the
/// windows of the egui test harness run on the test's thread); the application never calls it.
#[doc(hidden)]
pub fn force_mac() -> ForcedMac {
    ForcedMac(FORCE_MAC.with(|f| f.replace(true)))
}

/// Restores the profile `force_mac` replaced.
#[doc(hidden)]
pub struct ForcedMac(bool);

impl Drop for ForcedMac {
    fn drop(&mut self) {
        FORCE_MAC.with(|f| f.set(self.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_measurements_are_pinned() {
        let m = Metrics::MAC;
        assert_eq!(m.top_frame_h(), 70.0, "window top to the ribbon card");
        assert_eq!(m.ribbon_card_h, 74.0);
        assert_eq!(m.ribbon_inset_x, 12.0);
        assert_eq!(m.notice_bar_h, Some(27.0));
        assert_eq!(m.formula_bar_h, 19.0);
        assert_eq!(m.col_header_h, 25.0);
        assert_eq!(m.row_header_w(2, 1.0), 26.0);
        assert_eq!(m.sheet_tabs_h, 21.0);
        assert_eq!(m.status_bar_h, 27.0);
        assert_eq!(m.share_button_h, Some(17.0));
        assert!(m.ribbon_inset_y * 2.0 + 3.0 * 23.0 + 2.0 * m.ribbon_spacing_y <= m.ribbon_card_h, "a stack of three small controls fits the card");
    }

    #[test]
    fn standard_profile_keeps_the_sizes_it_always_had() {
        let s = Metrics::STANDARD;
        assert_eq!(s.title_bar_h, 38.0);
        assert_eq!(s.ribbon_panel_h(true), 34.0);
        assert_eq!(s.ribbon_panel_h(false), 126.0);
        assert_eq!(s.tab_cell_h(), 30.0);
        assert_eq!(s.formula_one_line(17.28), 31.28);
        assert_eq!(s.formula_one_line(10.0), 30.0);
        assert_eq!((s.col_header_h, s.sheet_tabs_h, s.status_bar_h), (21.0, 32.0, 26.0));
        assert_eq!(s.row_header_w(2, 1.0), 28.0);
        assert_eq!(s.row_header_w(5, 1.0), 49.0);
        assert!(s.notice_bar_h.is_none() && s.share_button_h.is_none());
    }

    #[test]
    fn one_hundred_percent_is_72_per_inch_on_a_mac_only() {
        // 11 pt text at 100%: 11 screen points on a Mac, 96-per-inch text elsewhere.
        let at_100 = |m: Metrics| 11.0 * m.px_per_pt() * DISPLAY_SCALE;
        assert!((at_100(Metrics::MAC) - 11.0).abs() < 1e-4);
        assert!((at_100(Metrics::STANDARD) - 11.0 * 96.0 / 72.0 * 0.8).abs() < 1e-4);
        // The standard factor is the old 96/72 times the sheet scale, unchanged.
        assert!((Metrics::STANDARD.px_per_pt() - SHEET_PX_PER_PT).abs() < 1e-5);
        // A default 20-unit row is 16 points on both.
        assert!((20.0 * DISPLAY_SCALE - 16.0).abs() < 1e-5);
    }

    #[test]
    fn the_forced_profile_is_scoped_to_its_guard() {
        assert_eq!(Metrics::current().mac, cfg!(target_os = "macos"));
        {
            let _mac = force_mac();
            assert_eq!(Metrics::current(), Metrics::MAC);
            assert!((px_per_pt() - 1.25).abs() < 1e-6);
        }
        assert_eq!(Metrics::current().mac, cfg!(target_os = "macos"));
    }

    #[test]
    fn grid_lines_land_on_whole_device_pixels() {
        // 1 pixel per point, 1 pt wide: the old rule.
        assert_eq!(snap_line(10.4, 1.0, 1.0), 10.0 - 0.5);
        assert_eq!(snap_line(10.6, 1.0, 1.0), 11.0 - 0.5);
        for ppp in [1.0_f32, 1.25, 1.5, 2.0, 3.0] {
            for edge in [3.1_f32, 17.37, 64.5, 129.99] {
                let c = snap_line(edge, 1.0, ppp);
                // The near edge is on a device pixel; with a whole pixels-per-point so is the far one.
                let (a, b) = ((c - 0.5) * ppp, (c + 0.5) * ppp);
                assert!((a - a.round()).abs() < 1e-3, "ppp {ppp} edge {edge}: {a}");
                assert!(ppp.fract() != 0.0 || (b - b.round()).abs() < 1e-3, "ppp {ppp} edge {edge}: {b}");
            }
        }
    }
}
