//! Round 32: TIDAL 2.46.0's classes and token scopes for the queue's
//! artist line, the sidebar's playlist rows and the context menu's words,
//! and the native parts painted with their anchor's whole look. Proved with
//! made-up themes (their own colours, TIDAL's classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

fn themed_luna(name: &str, css: &str) -> Luna {
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert(format!("/{name}"), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir(name);
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    luna.install_theme(&format!("{base}/{name}"));
    luna
}

fn wait(
    luna: &Luna,
    tree: &super::dom::HostTree,
    want: &dyn Fn(&DomOverlay) -> bool,
) -> Arc<DomOverlay> {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && want(&o) {
            return o;
        }
        assert!(Instant::now() < end, "the overlay never got there");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// The overlay `css` gives the sample tree, once `key` is styled.
fn overlay_for(name: &str, css: &str, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let luna = themed_luna(name, css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(key));
    luna.shutdown();
    o
}

/// A dark theme's rule for TIDAL's market text colours the queue's artist
/// names: TIDAL 2.46.0 draws them near white under Blur's
/// `[class*="_marketText_"] { color: #ffffffee !important }` (captured),
/// where Tidalfast kept its grey for every dark window. A catch-all
/// `* { color }` still leaves them grey, as TIDAL's own rule for the line
/// does in TIDAL (Meteor Night).
#[test]
fn a_dark_themes_market_text_rule_colours_the_queue_artists() {
    use crate::theme::Palette;
    use crate::ui::now_playing::queue_name_colors;
    use crate::ui::plugin_dom::{self as pd, SidePanel, themed, tidal_panel_text};
    let names = |name: &str, css: &str| {
        let o = overlay_for(name, css, dom_anchors::keys::QUEUE_ROLE_TEXT);
        pd::test_overlay(Some((*o).clone()));
        let queue = tidal_panel_text(themed(&Palette::dark(), &o), SidePanel::Queue);
        let colors = queue_name_colors(&queue);
        pd::test_overlay(None);
        colors
    };
    let pale = |c: egui::Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        r >= 248 && g >= 248 && b >= 250 && (230..=240).contains(&a)
    };
    let (plain, followed) = names(
        "frosted-words.css",
        r#"/* {"name": "Frosted words"} */
body { background-color: rgb(14, 12, 24) !important; }
[class*="_marketText_"] { color: rgba(250, 250, 255, .933) !important; }
"#,
    );
    assert!(pale(plain), "near white: {plain:?}");
    assert_eq!(followed, plain);
    let native = queue_name_colors(&Palette::dark());
    let (plain, _) = names(
        "snowdrift.css",
        r#"/* {"name": "Snowdrift"} */
body { background-color: rgb(14, 12, 24) !important; }
* { color: rgb(240, 240, 250); }
"#,
    );
    assert_eq!(plain, native.0, "a catch-all leaves TIDAL's grey");
}

/// TIDAL 2.46.0's playlist names in the sidebar carry no `_textWhite_`
/// class (TIDAL 2.43.2's did), and their second lines carry
/// `_subtitle_c0fd197` (captured under five themes): a rule naming the
/// first leaves the names alone in TIDAL, one naming the second colours
/// the lines. Tidalfast kept 2.43's classes.
#[test]
fn sidebar_playlist_rows_have_tidal_2_46s_classes() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Old white"} */
._marketText_1lyag_1._textWhite_1lyag_319 { color: rgb(9, 0, 26); font-weight: 600; }
._subtitle_c0fd197._text20_1lyag_291 { color: rgb(120, 40, 160); }
"#;
    let mut frame = dom_anchors::sample_frame();
    let row = Some([8.0, 300.0, 224.0, 52.0]);
    frame.sidebar.rows = vec![(row, false)];
    let tree = dom_anchors::build(&frame);
    let luna = themed_luna("old-white.css", css);
    let line = dom_anchors::keys::lib_row_subtitle(0);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(&line));
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let r = egui::Rect::from_min_size(egui::pos2(8.0, 300.0), egui::vec2(224.0, 52.0));
    let style = pd::library_row(r, false, false);
    pd::test_overlay(None);
    let style = style.expect("the line's colour");
    assert_eq!(style.title_color, None, "no _textWhite_ on the name");
    assert_eq!(style.title_weight, None);
    assert_eq!(style.secondary, Some(egui::Color32::from_rgb(120, 40, 160)));
}

/// A theme that sets TIDAL's contrast token on the context menu alone
/// (Cotton Candy Nights: `._contextMenu_ { --wave-color-solid-contrast-fill:
/// #000 }` with its root's white, and `._actionTextInner_ { color: black }`
/// nested in it) has TIDAL 2.46.0 draw the menu's title, a submenu's
/// heading and the rows that open submenus in those dark colours (captured).
/// Tidalfast drew the title and those rows in the page's pale words, faint
/// on the theme's pink menu.
#[test]
fn a_menus_own_contrast_token_colours_its_title_and_submenu_rows() {
    use crate::theme::Palette;
    use crate::ui::{plugin_dom as pd, widgets};
    let css = r#"/* {"name": "Candy menu"} */
:root { --wave-color-solid-contrast-fill: rgb(250, 250, 250); }
._contextMenu_8606b2d {
  background-color: rgb(240, 170, 230);
  --wave-color-solid-contrast-fill: rgb(10, 0, 30) !important;
  ._actionTextInner_a14f37c { color: rgb(20, 0, 40) !important; }
}
"#;
    let o = overlay_for("candy-menu.css", css, dom_anchors::keys::THEME_MENU_TITLE);
    pd::test_overlay(Some((*o).clone()));
    let palette = Palette::dark();
    let title = pd::menu_words(palette.menu_bg());
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        widgets::menu_frame(&palette).show(ui, |ui| {
            ui.set_width(widgets::MENU_MIN_CONTENT);
            widgets::menu_submenu(ui, &palette, None, "Add to playlist", |_| {});
        });
    });
    out.textures_delta.clear();
    pd::test_overlay(None);
    assert_eq!(title, Some(egui::Color32::from_rgb(10, 0, 30)));
    let row = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Add to playlist" => {
                Some(t.galley.job.sections[0].format.color)
            }
            _ => None,
        })
        .expect("the row's words");
    assert_eq!(row, egui::Color32::from_rgb(20, 0, 40));
}

/// With no theme the menu's words keep the app's own colours.
#[test]
fn without_a_theme_the_menus_words_are_the_apps() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom as pd;
    pd::test_overlay(None);
    let palette = Palette::dark();
    assert_eq!(pd::menu_words(palette.menu_bg()), None);
    assert_eq!(pd::menu_row_words(palette.menu_bg()), None);
}

/// The leaf shapes of `shape`.
fn leaves(shape: &egui::Shape) -> usize {
    match shape {
        egui::Shape::Vec(v) => v.iter().map(leaves).sum(),
        egui::Shape::Noop => 0,
        _ => 1,
    }
}

/// The top bar's search pill is painted with its anchor's whole look: a
/// theme's outer shadow on TIDAL's `div._searchField_b2eb7f7` is drawn
/// round its fill. Only the fill and corners were drawn.
#[test]
fn the_header_search_pill_takes_its_anchors_whole_look() {
    use super::dom::{HostOverride, Style};
    use crate::theme::Palette;
    use crate::ui::{plugin_dom as pd, widgets};
    let pink = egui::Color32::from_rgb(250, 200, 240);
    let shapes = |shadow: Option<&str>| {
        let mut o = DomOverlay {
            version: 3,
            ..DomOverlay::default()
        };
        o.hosts.insert(
            dom_anchors::keys::SEARCH_FIELD.into(),
            HostOverride {
                style: Style {
                    background_color: Some("rgb(250, 200, 240)".into()),
                    box_shadow: shadow.map(str::to_string),
                    ..Style::default()
                },
                ..HostOverride::default()
            },
        );
        pd::test_overlay(Some(o));
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut text = String::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            widgets::header_search_field(
                ui,
                &Palette::dark(),
                crate::i18n::Locale::English,
                egui::Id::new("search"),
                &mut text,
                "Search",
                300.0,
            );
        });
        out.textures_delta.clear();
        pd::test_overlay(None);
        let filled = out
            .shapes
            .iter()
            .any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == pink));
        let n: usize = out.shapes.iter().map(|c| leaves(&c.shape)).sum();
        (n, filled)
    };
    let (flat, filled) = shapes(None);
    assert!(filled, "the theme's fill");
    let (shadowed, _) = shapes(Some("0 0 1em rgba(0, 0, 0, .4)"));
    assert!(shadowed > flat + 4, "outer shadow: {flat} -> {shadowed}");
}

/// TIDAL 2.46.0's search pill is `div._container_f1be359
/// .searchFieldWithinSearchFieldWithPopover._searchField_b2eb7f7` round
/// `input._searchField_4b7fbda` (2 points in top and bottom): a theme's
/// `[class*="_header_"] [class*="_searchField_"]` white lies on both, which
/// makes Blur's pill lighter in TIDAL (about 77 against Tidalfast's 47).
/// Tidalfast's input had no such class and its fill was not drawn.
#[test]
fn a_search_field_rule_fills_the_pill_and_its_input() {
    use crate::theme::Palette;
    use crate::ui::{plugin_dom as pd, widgets};
    let css = r#"/* {"name": "Frosted search"} */
[class*="_header_"] [class*="_searchField_"] { background-color: rgba(255, 255, 255, .15) !important; border-radius: 24px !important; }
"#;
    let o = overlay_for("frosted-search.css", css, dom_anchors::keys::SEARCH_INPUT);
    pd::test_overlay(Some((*o).clone()));
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut text = String::new();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        widgets::header_search_field(
            ui,
            &Palette::dark(),
            crate::i18n::Locale::English,
            egui::Id::new("search"),
            &mut text,
            "Search",
            300.0,
        );
    });
    out.textures_delta.clear();
    pd::test_overlay(None);
    let white = |c: egui::Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        r > 250 && g > 250 && b > 250 && (35..=42).contains(&a)
    };
    let layers: Vec<egui::Rect> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r) if white(r.fill) => Some(r.rect),
            _ => None,
        })
        .collect();
    assert_eq!(layers.len(), 2, "the pill and its input: {layers:?}");
    assert!((layers[0].height() - layers[1].height() - 4.0).abs() < 0.01);
}

/// The play queue's rows take the look a theme gives TIDAL's
/// `div._container_aab70e3[data-test=playqueue-item]`: the idle rows its
/// plain rule, the row under the pointer its `:hover` rule (Solo v2's soft
/// fill), through the hovered row's stand-in that the queue puts over it.
/// The rows only had Tidalfast's white hover.
#[test]
fn queue_rows_take_their_states_look() {
    use super::dom::{DomEvent, DomTarget};
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{QUEUE_ROLE_HOVER_ROW, QUEUE_ROLE_ROW};
    let css = r#"/* {"name": "Soft rows"} */
._container_aab70e3 { border-radius: 9px; }
._container_aab70e3:hover { background: rgb(60, 70, 110) !important; }
"#;
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let luna = themed_luna("soft-rows.css", css);
    let _ = wait(&luna, &tree, &|o| o.hosts.contains_key(QUEUE_ROLE_ROW));
    luna.dom_event(DomEvent {
        target: DomTarget::Host(QUEUE_ROLE_HOVER_ROW.into()),
        kind: "hover".into(),
        data: serde_json::Value::Null,
    });
    let o = wait(&luna, &tree, &|o| {
        o.hosts
            .get(QUEUE_ROLE_HOVER_ROW)
            .is_some_and(|h| h.style.background_color.is_some())
    });
    luna.shutdown();
    let fills = |hovered: bool| {
        pd::test_overlay(Some((*o).clone()));
        let ctx = egui::Context::default();
        let r = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(300.0, 56.0));
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            pd::paint_queue_row(
                ui,
                r,
                false,
                hovered,
                egui::CornerRadius::same(6),
                egui::Color32::from_white_alpha(if hovered { 12 } else { 0 }),
            );
        });
        out.textures_delta.clear();
        pd::test_overlay(None);
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) => Some((r.fill, r.corner_radius)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        fills(true),
        vec![(
            egui::Color32::from_rgb(60, 70, 110),
            egui::CornerRadius::same(9)
        )],
        "the hovered row"
    );
    assert!(fills(false).is_empty(), "an idle row has no fill");
}
