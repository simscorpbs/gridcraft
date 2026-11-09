//! The accent colour option and the title-bar toggles (Discord button, account avatar): defaults,
//! commands, persistence, hostile saved values, and what the title bar and the sheet paint.

use egui::{Shape, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::theme::{AccentColor, Tokens};
use gridcraft_ui_egui::{SheetApp, UiState};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

#[test]
fn defaults_are_excel_green_no_discord_and_the_avatar_off_macos_only() {
    let ui = UiState::default();
    assert_eq!(ui.accent_color, AccentColor::DEFAULT);
    assert_eq!(ui.accent_color.hex(), "107C41");
    assert!(!ui.show_discord, "the Discord button is off by default");
    assert_eq!(ui.show_account_picture, !cfg!(target_os = "macos"));
    let none: UiState = serde_json::from_str("{}").unwrap();
    assert_eq!(none.accent_color, AccentColor::DEFAULT);
    assert!(!none.show_discord);
    assert_eq!(none.show_account_picture, ui.show_account_picture);
}

#[test]
fn the_choices_survive_a_restart() {
    let state = UiState {
        accent_color: AccentColor::parse("6F42C1").unwrap(),
        show_discord: true,
        show_account_picture: !UiState::default().show_account_picture,
        ..UiState::default()
    };
    let saved = serde_json::to_string(&state).unwrap();
    assert!(saved.contains(r#""accentColor":"6F42C1""#), "{saved}");
    assert!(saved.contains(r#""showDiscord":true"#), "{saved}");
    let back: UiState = serde_json::from_str(&saved).unwrap();
    assert_eq!(back.accent_color, state.accent_color);
    assert!(back.show_discord);
    assert_eq!(back.show_account_picture, state.show_account_picture);
}

#[test]
fn hostile_saved_accents_fall_back_to_the_default_and_keep_the_other_settings() {
    let long = "F".repeat(100_000);
    for value in [
        json!("javascript:alert(1)"),
        json!(""),
        json!("#12"),
        json!("GGGGGG"),
        json!(123456),
        json!(null),
        json!([1, 2, 3]),
        json!({"a": 1}),
        json!(long),
        json!("<script>"),
    ] {
        let text = json!({"accentColor": value, "autosave": true}).to_string();
        let state: UiState = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{text:.80}: {e}"));
        assert_eq!(state.accent_color, AccentColor::DEFAULT, "{text:.80}");
        assert!(state.autosave, "the rest of the file is kept: {text:.80}");
    }
}

#[test]
fn the_command_sets_reads_and_refuses_accents() {
    let mut h = harness();
    let app = h.state_mut();
    assert_eq!(app.run("options.accentColor", json!({})), Ok(json!({"accentColor": "107C41"})));
    assert_eq!(app.run("options.accentColor", json!({"value": "#2173c4"})), Ok(json!({"accentColor": "2173C4"})));
    assert_eq!(app.ui.accent_color.hex(), "2173C4");
    for bad in [json!("nope"), json!(5), json!("12345"), json!(null)] {
        assert!(app.run("options.accentColor", json!({"value": bad})).is_err());
        assert_eq!(app.ui.accent_color.hex(), "2173C4", "a refused value changes nothing");
    }
    assert_eq!(app.run("options.accentColor", json!({"value": "default"})), Ok(json!({"accentColor": "107C41"})));
    assert_eq!(app.run("options.showDiscord", json!({"on": true})), Ok(json!({"on": true})));
    assert_eq!(app.run("options.showDiscord", json!({})), Ok(json!({"on": false})), "no parameters: it toggles");
    let avatar = app.ui.show_account_picture;
    assert_eq!(app.run("options.showAccountPicture", json!({})), Ok(json!({"on": !avatar})));
}

#[test]
fn a_new_accent_reaches_the_tokens_of_the_next_frame() {
    let mut h = harness();
    assert_eq!(Tokens::get(&h.ctx).accent, AccentColor::DEFAULT.color32());
    h.state_mut().run("options.accentColor", json!({"value": "6F42C1"})).unwrap();
    h.run_steps(2);
    let t = Tokens::get(&h.ctx);
    assert_eq!(t.accent, AccentColor::parse("6F42C1").unwrap().color32());
    assert_eq!(h.ctx.global_style().visuals.selection.stroke.color, t.accent);
    assert_ne!(t.sel_border, Tokens::light().sel_border);
    // Back to the default restores the hand-tuned palette exactly.
    h.state_mut().run("options.accentColor", json!({"value": "default"})).unwrap();
    h.run_steps(2);
    assert_eq!(Tokens::get(&h.ctx), Tokens::light());
}

#[test]
fn the_active_row_tint_follows_the_accent() {
    let mut h = harness();
    h.state_mut().run("options.highlightActiveRow", json!({"on": true})).unwrap();
    h.state_mut().run("options.accentColor", json!({"value": "C42B1C"})).unwrap();
    h.run_steps(3);
    let tint = Tokens::get(&h.ctx).active_row;
    assert_ne!(tint, Tokens::light().active_row);
    let painted = h.output().shapes.iter().any(|c| matches!(&c.shape, Shape::Rect(r) if r.fill == tint));
    assert!(painted, "the band is drawn with the derived tint");
}

#[test]
fn the_title_bar_shows_the_avatar_and_the_discord_button_only_when_asked() {
    let mut h = harness();
    assert!(h.query_by_label("Discord").is_none(), "off by default");
    assert_eq!(h.query_by_label("Account avatar").is_some(), h.state().ui.show_account_picture);
    h.state_mut().run("options.showDiscord", json!({"on": true})).unwrap();
    h.state_mut().run("options.showAccountPicture", json!({"on": true})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Discord").is_some());
    assert!(h.query_by_label("Account avatar").is_some());
    h.state_mut().run("options.showDiscord", json!({"on": false})).unwrap();
    h.state_mut().run("options.showAccountPicture", json!({"on": false})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Discord").is_none());
    assert!(h.query_by_label("Account avatar").is_none());
}

#[test]
fn the_options_page_offers_the_three_settings() {
    let mut h = harness();
    h.state_mut().run("ui.backstage", json!({"page": "options"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Accent color").is_some());
    assert!(h.query_by_label("Show the Discord button").is_some());
    assert!(h.query_by_label("Show the account avatar").is_some());
}
