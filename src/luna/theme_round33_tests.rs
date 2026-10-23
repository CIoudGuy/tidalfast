//! Round 33: TIDAL 2.46.0's hovered menu row and the icons in its menus,
//! under themes. Proved with made-up themes (their own colours, TIDAL's
//! classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::{DomEvent, DomOverlay, DomTarget};
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

/// A row that opens a submenu ("Share"), drawn with the pointer on it: the
/// fills of the rects drawn.
fn hovered_submenu_row(palette: &crate::theme::Palette) -> Vec<egui::Color32> {
    use crate::ui::widgets;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut fills = Vec::new();
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events: vec![egui::Event::PointerMoved(egui::pos2(60.0, 30.0))],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.set_width(widgets::MENU_MIN_CONTENT);
            widgets::menu_submenu(ui, palette, None, "Share", |_| {});
        });
        out.textures_delta.clear();
        fills = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) => Some(r.fill),
                _ => None,
            })
            .collect();
    }
    fills
}

/// TIDAL's hovered context menu row is its own `._actionItem_:hover` white
/// at 15 %, whatever the theme's colours (TIDAL 2.46.0 under Cotton Candy
/// Nights, over its pink gradient, with the black words the theme gives
/// the rows; captured). Tidalfast drew a themed palette's dark hover there,
/// under which those black words could not be read.
#[test]
fn a_themed_menus_hovered_row_is_tidals_white() {
    use crate::theme::Palette;
    crate::ui::plugin_dom::test_overlay(None);
    let candy = Palette {
        themed: true,
        own_surfaces: false,
        overlay: egui::Color32::from_rgb(230, 170, 240),
        surface_hover: egui::Color32::from_rgb(40, 20, 60),
        ..Palette::dark()
    };
    let fills = hovered_submenu_row(&candy);
    assert!(
        fills.contains(&egui::Color32::from_white_alpha(38)),
        "TIDAL's white: {fills:?}"
    );
    // With no theme a palette keeps its own hover (the app's Light).
    let light = Palette::light();
    assert_eq!(crate::ui::widgets::action_hover(&light), light.menu_hover());
}

/// A theme's `:hover` rule for TIDAL's menu rows reaches the rows that have
/// no anchor of their own (a row opening a submenu, a submenu's rows):
/// Solo v2's 8 % white and 6-point corners, Tidal Light's grey, as TIDAL
/// 2.46.0 draws them (captured). Tidalfast drew its own hover there.
#[test]
fn a_themes_row_hover_rule_reaches_rows_without_an_anchor() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{THEME_MENU_HOVER_ROW, THEME_MENU_HOVER_TEXT};
    let css = r#"/* {"name": "Soft menu rows"} */
._actionItem_e0c1e65 { background: transparent !important; border-radius: 6px !important; }
._actionItem_e0c1e65:hover { background: rgb(60, 70, 110) !important; }
"#;
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let luna = themed_luna("soft-menu-rows.css", css);
    let _ = wait(&luna, &tree, &|o| {
        o.hosts.contains_key(THEME_MENU_HOVER_ROW)
    });
    luna.dom_event(DomEvent {
        target: DomTarget::Host(THEME_MENU_HOVER_TEXT.into()),
        kind: "hover".into(),
        data: serde_json::Value::Null,
    });
    let o = wait(&luna, &tree, &|o| {
        o.hosts
            .get(THEME_MENU_HOVER_ROW)
            .and_then(|h| h.style.background_color.as_deref())
            .is_some_and(|c| c.contains("60"))
    });
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let ctx = egui::Context::default();
    let r = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(192.0, 42.0));
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        pd::paint_menu_hover(ui, r, egui::Color32::from_white_alpha(38));
    });
    out.textures_delta.clear();
    pd::test_overlay(None);
    let fills: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r) => Some((r.fill, r.corner_radius)),
            _ => None,
        })
        .collect();
    assert_eq!(
        fills,
        vec![(
            egui::Color32::from_rgb(60, 70, 110),
            egui::CornerRadius::same(6)
        )]
    );
}

/// A theme's fill for the icons inside TIDAL's context menu (Cotton Candy
/// Nights' `._contextMenu_ { ._icon_ > use { fill: black } }`) colours the
/// chevron of a row that opens a submenu and Share's link icons, black in
/// TIDAL 2.46.0 (captured). Tidalfast kept its grey chevron and white icons,
/// faint on the theme's pink menu.
#[test]
fn a_themes_menu_icon_fill_reaches_the_menus_icons() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom as pd;
    let ink = egui::Color32::from_rgb(5, 0, 20);
    let css = r#"/* {"name": "Inked menu"} */
._contextMenu_8606b2d {
  background-color: rgb(240, 170, 230);
  ._icon_77f3f89 > use { fill: rgb(5, 0, 20); }
}
"#;
    let o = overlay_for(
        "inked-menu.css",
        css,
        dom_anchors::keys::THEME_MENU_CHEVRON_USE,
    );
    pd::test_overlay(Some((*o).clone()));
    let chevron = crate::ui::widgets::menu_chevron_color(&Palette::dark());
    let link = pd::menu_icon(false);
    pd::test_overlay(None);
    assert_eq!(chevron, ink);
    assert_eq!(link, Some(ink));
}

/// With no theme TIDAL 2.46.0's chevron is its `--wave-color-solid-contrast-
/// darkest`, rgb(120, 120, 135) (`span._subMenuIcon_`, captured), darker than
/// the menu's secondary words, which Tidalfast drew it in.
#[test]
fn without_a_theme_the_chevron_is_tidals_darkest_grey() {
    use crate::theme::Palette;
    crate::ui::plugin_dom::test_overlay(None);
    assert_eq!(crate::ui::plugin_dom::menu_icon(true), None);
    assert_eq!(
        crate::ui::widgets::menu_chevron_color(&Palette::dark()),
        egui::Color32::from_rgb(120, 120, 135)
    );
}

/// TIDAL 2.46.0's header search pill sits in the header's bar,
/// `div._container_cb5cbe4[data-test=header]`, which themes name to reach
/// it: Cotton Candy Nights' `:is(._container_cb5cbe4
/// ._searchFieldWithPopover_267e090) .searchFieldWithinSearchFieldWithPopover
/// { background: ... !Important }` makes TIDAL's pill dark indigo over its
/// plainer pink `._container_f1be359` rule (captured). Tidalfast's pill had
/// no such bar round it and showed the pink, under which the typed words
/// were faint.
#[test]
fn the_search_pill_sits_in_the_headers_bar() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Indigo search"} */
._container_f1be359 { background-color: rgb(250, 190, 250); }
:is(._container_cb5cbe4 ._searchFieldWithPopover_267e090) .searchFieldWithinSearchFieldWithPopover {
    background: rgb(31, 17, 75) !Important;
}
"#;
    let o = overlay_for("indigo-search.css", css, dom_anchors::keys::SEARCH_FIELD);
    pd::test_overlay(Some((*o).clone()));
    let style = pd::search_style(&Palette::dark());
    pd::test_overlay(None);
    let fill = style.and_then(|s| s.fill);
    assert_eq!(fill, Some(egui::Color32::from_rgb(31, 17, 75)));
}
