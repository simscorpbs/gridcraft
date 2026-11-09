//! The interface accent colour (File ▸ Options ▸ General): one colour chosen by the user, from
//! which every accent-dependent token is derived for both themes.
//!
//! The default is Excel's green and reproduces the hand-tuned palette exactly. Any other colour is
//! first pulled far enough from the chrome surfaces to keep the WCAG ratios `contrast_tests`
//! holds the palette to (the original is kept when it already clears them), then the soft fills,
//! the selection tints and the text-on-accent colour are derived from it.

use egui::Color32;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{Tokens, contrast};

/// The user-selectable interface accent: an opaque sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AccentColor(pub [u8; 3]);

impl AccentColor {
    /// Excel green.
    pub const DEFAULT: Self = Self([0x10, 0x7C, 0x41]);

    /// The named choices of File ▸ Options, with the English label they are shown (and
    /// translated) under.
    pub const PRESETS: [(&'static str, Self); 6] = [
        ("Green", Self::DEFAULT),
        ("Blue", Self([0x1E, 0x6B, 0xB8])),
        ("Purple", Self([0x6F, 0x42, 0xC1])),
        ("Red", Self([0xC4, 0x2B, 0x1C])),
        ("Orange", Self([0xB8, 0x4F, 0x0B])),
        ("Gray", Self([0x5A, 0x5A, 0x5A])),
    ];

    /// `RRGGBB`, with or without a leading `#`: exactly six ASCII hexadecimal digits, nothing
    /// else (no sign, no space, no short form).
    pub fn parse(value: &str) -> Option<Self> {
        let digits = value.strip_prefix('#').unwrap_or(value);
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let n = u32::from_str_radix(digits, 16).ok()?;
        Some(Self([(n >> 16) as u8, (n >> 8) as u8, n as u8]))
    }

    /// `RRGGBB`, upper case, no `#`.
    pub fn hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2])
    }

    pub fn color32(self) -> Color32 {
        Color32::from_rgb(self.0[0], self.0[1], self.0[2])
    }

    /// The tokens of one theme with this accent in place of Excel's green.
    pub(super) fn tokens(self, dark: bool) -> Tokens {
        let mut t = Tokens::palette(dark);
        if self == Self::DEFAULT {
            return t;
        }
        let light = self.on_light();
        let tint = |c: Color32, alpha: u8| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha);
        if dark {
            let a = self.on_dark();
            t.accent = a;
            t.accent_dark = mix(a, Color32::BLACK, 0.28);
            t.accent_soft = mix(Color32::from_rgb(0x1F, 0x1F, 0x1F), a, 0.14);
            t.header_sel_text = lift(mix(a, Color32::WHITE, 0.2), t.header_sel_bg);
            t.header_all_bg = mix(Color32::from_rgb(0x1F, 0x1F, 0x1F), a, 0.26);
            t.sel_fill = tint(light, 0x24);
            t.active_row = tint(a, 0x30);
            t.on_accent = on_accent(a);
        } else {
            t.accent = light;
            t.accent_dark = mix(light, Color32::BLACK, 0.25);
            t.accent_soft = mix(Color32::WHITE, light, 0.12);
            t.header_sel_text = sink(t.accent_dark, t.header_sel_bg);
            t.header_all_bg = mix(Color32::WHITE, light, 0.22);
            t.sel_fill = tint(light, 0x1C);
            t.active_row = tint(light, 0x2C);
            t.on_accent = on_accent(light);
        }
        t.share = t.accent;
        // The sheet stays white in both themes, so what is drawn on it uses the light value.
        t.sel_border = light;
        t.sheet_fill = light;
        t.sheet_focus = light;
        t
    }

    /// The accent as chrome text and glyphs on the light surfaces: darkened until it reads on the
    /// window (4.5:1) and stays visible on the hover and pressed fills.
    fn on_light(self) -> Color32 {
        let mut c = self.color32();
        for _ in 0..64 {
            if contrast(c, Color32::from_rgb(0xF2, 0xF2, 0xF2)) >= 4.5 && contrast(c, Color32::from_rgb(0xDD, 0xDD, 0xDD)) >= 3.5 {
                break;
            }
            c = mix(c, Color32::BLACK, 0.06);
        }
        c
    }

    /// The same on the dark surfaces: lightened.
    fn on_dark(self) -> Color32 {
        let mut c = self.color32();
        for _ in 0..64 {
            if contrast(c, Color32::from_rgb(0x29, 0x29, 0x29)) >= 4.5 && contrast(c, Color32::from_rgb(0x4D, 0x4D, 0x4D)) >= 3.4 {
                break;
            }
            c = mix(c, Color32::WHITE, 0.06);
        }
        c
    }
}

impl Default for AccentColor {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Serialize for AccentColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hex())
    }
}

/// A saved accent is read leniently: anything that is not a valid colour (a number, a longer
/// string, a hand-edited file) reads as the default instead of failing the whole `UiState`.
impl<'de> Deserialize<'de> for AccentColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Other(serde::de::IgnoredAny),
        }
        Ok(match Raw::deserialize(deserializer)? {
            Raw::Text(s) => Self::parse(&s).unwrap_or_default(),
            Raw::Other(_) => Self::default(),
        })
    }
}

/// `a` moved towards `b` by `t` (0 to 1).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// Text on an accent fill: white or the dark ink, whichever reads better.
fn on_accent(background: Color32) -> Color32 {
    let ink = Color32::from_rgb(0x0F, 0x1A, 0x14);
    if contrast(Color32::WHITE, background) >= contrast(ink, background) { Color32::WHITE } else { ink }
}

/// Darkens `c` until it reads at 4.5:1 on `bg`.
fn sink(mut c: Color32, bg: Color32) -> Color32 {
    for _ in 0..64 {
        if contrast(c, bg) >= 4.5 {
            break;
        }
        c = mix(c, Color32::BLACK, 0.06);
    }
    c
}

/// Lightens `c` until it reads at 4.5:1 on `bg`.
fn lift(mut c: Color32, bg: Color32) -> Color32 {
    for _ in 0..64 {
        if contrast(c, bg) >= 4.5 {
            break;
        }
        c = mix(c, Color32::WHITE, 0.06);
    }
    c
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn default_is_excel_green_and_keeps_the_hand_tuned_palette() {
        assert_eq!(AccentColor::default().hex(), "107C41");
        assert_eq!(AccentColor::PRESETS[0].1, AccentColor::DEFAULT);
        for dark in [false, true] {
            assert_eq!(AccentColor::DEFAULT.tokens(dark), Tokens::palette(dark));
        }
        assert_eq!(Tokens::light().accent, Color32::from_rgb(0x10, 0x7C, 0x41));
    }

    #[test]
    fn parse_takes_six_hex_digits_only() {
        assert_eq!(AccentColor::parse("2173c4"), Some(AccentColor([0x21, 0x73, 0xC4])));
        assert_eq!(AccentColor::parse("#2173C4"), Some(AccentColor([0x21, 0x73, 0xC4])));
        for bad in [
            "",
            "#",
            "123",
            "12345",
            "1234567",
            "##123456",
            "+12345",
            "-12345",
            " 123456",
            "123456 ",
            "12345G",
            "ééé",
            "１２３４５６",
            "0x1234",
            "red",
        ] {
            assert_eq!(AccentColor::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn hex_round_trips_through_parse() {
        for c in [[0, 0, 0], [255, 255, 255], [1, 2, 3], [0x10, 0x7C, 0x41]] {
            assert_eq!(AccentColor::parse(&AccentColor(c).hex()), Some(AccentColor(c)));
        }
    }

    #[test]
    fn hostile_saved_values_read_as_the_default() {
        for json in
            [r#""not a colour""#, r#""12345678901234567890""#, r#""\u0000\u0000\u0000""#, "42", "null", "true", "[1,2,3]", r#"{"r":1}"#, "-1.5e300"]
        {
            let c: AccentColor = serde_json::from_str(json).expect(json);
            assert_eq!(c, AccentColor::DEFAULT, "{json}");
        }
        let long = format!("\"{}\"", "F".repeat(1_000_000));
        assert_eq!(serde_json::from_str::<AccentColor>(&long).expect("long"), AccentColor::DEFAULT);
    }

    #[test]
    fn serde_round_trips() {
        let c = AccentColor([0x6F, 0x42, 0xC1]);
        let s = serde_json::to_string(&c).expect("serialize");
        assert_eq!(s, r#""6F42C1""#);
        assert_eq!(serde_json::from_str::<AccentColor>(&s).expect("parse"), c);
    }

    #[test]
    fn a_colour_that_already_reads_is_kept() {
        let c = AccentColor::PRESETS[1].1;
        assert_eq!(c.tokens(false).accent, c.color32());
    }

    #[test]
    fn a_chosen_accent_reaches_every_accent_token() {
        let green = Tokens::light();
        let purple = AccentColor::PRESETS[2].1.tokens(false);
        assert_ne!(purple.accent, green.accent);
        assert_ne!(purple.sel_fill, green.sel_fill);
        assert_ne!(purple.active_row, green.active_row);
        assert_ne!(purple.sel_border, green.sel_border);
        assert_ne!(purple.sheet_fill, green.sheet_fill);
        assert_eq!(purple.sheet_fill, AccentColor::PRESETS[2].1.tokens(true).sheet_fill, "the sheet does not follow the theme");
        assert_eq!(purple.sel_border, purple.sheet_fill);
    }
}
