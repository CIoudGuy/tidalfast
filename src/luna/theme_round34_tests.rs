//! Round 34: TIDAL 2.46.0's playing track row, its other search fields, the
//! sidebar's create menu and a playlist's panel round its filter, captured
//! under themes. Proved with made-up themes (their own colours, TIDAL's
//! classes and anchors only).

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

/// The overlay `css` gives `frame`'s tree, once `key` is styled.
fn overlay_of(frame: &dom_anchors::Frame, name: &str, css: &str, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
    let luna = themed_luna(name, css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(key));
    luna.shutdown();
    o
}

fn overlay_for(name: &str, css: &str, key: &str) -> Arc<DomOverlay> {
    overlay_of(&dom_anchors::sample_frame(), name, css, key)
}

const WINDOW: egui::Color32 = egui::Color32::from_rgb(10, 10, 10);

/// TIDAL 2.46.0 marks the playing row of a track list
/// `div._tableRow_._tableRowIsPlaying_a59b9fa` and its title
/// `span._titleText_._textHigh_bbc1075.table--inactive-title` (captured,
/// paused and playing): rules naming them reach them (TidalCustomTheme's
/// `[class*=_tableRowIsPlaying_]`, the cotton candy family's
/// `._textHigh_bbc1075`). Tidalfast's row had a made-up class and its title
/// neither, so those rules missed it.
#[test]
fn a_themes_rules_for_the_playing_row_reach_it() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Lit row"} */
[class*=_tableRowIsPlaying_] { background-color: rgb(20, 60, 90); }
._textHigh_bbc1075 { color: rgb(250, 120, 40); }
"#;
    let list = "album:55";
    let row = dom_anchors::keys::row(list, 0);
    let o = overlay_for("lit-row.css", css, &row);
    let fill = |i: usize| {
        o.hosts
            .get(&dom_anchors::keys::row(list, i))
            .and_then(|h| h.style.background_color.clone())
    };
    assert!(
        fill(0).is_some_and(|c| c.contains("20, 60, 90")),
        "{:?}",
        fill(0)
    );
    assert_eq!(fill(1), None);
    pd::test_overlay(Some((*o).clone()));
    let playing = pd::playing_title(list, 0, WINDOW);
    let idle = pd::playing_title(list, 1, WINDOW);
    pd::test_overlay(None);
    assert_eq!(playing, Some(egui::Color32::from_rgb(250, 120, 40)));
    assert_eq!(idle, None);
}

/// The playing title keeps TIDAL's `table--inactive-title`, so a theme's
/// `[class*="active"]` colours it as the other titles (Catppuccin Mocha
/// Blue's blue playing title in TIDAL 2.46.0, captured). Tidalfast left the
/// class off the playing title and drew it in the accent.
#[test]
fn the_playing_title_keeps_tidals_title_class() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Active blue"} */
[class*="active"] { color: rgb(30, 100, 240) !important; }
"#;
    let list = "album:55";
    let key = dom_anchors::keys::row_cell(list, 0, "title-text");
    let o = overlay_for("active-blue.css", css, &key);
    pd::test_overlay(Some((*o).clone()));
    let playing = pd::playing_title(list, 0, WINDOW);
    pd::test_overlay(None);
    assert_eq!(playing, Some(egui::Color32::from_rgb(30, 100, 240)));
}

/// A catch-all `* { color }` (Meteor Night's white) leaves the playing
/// title to TIDAL's own rule for its quality class, the accent, while the
/// other titles take it (TIDAL 2.46.0 under Meteor Night, captured).
#[test]
fn a_catch_all_colour_leaves_the_playing_title_in_the_accent() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Snow rows"} */
* { color: rgb(240, 240, 250); }
"#;
    let list = "album:55";
    let key = dom_anchors::keys::row_cell(list, 1, "title-text");
    let o = overlay_for("snow-rows.css", css, &key);
    pd::test_overlay(Some((*o).clone()));
    let playing = pd::playing_title(list, 0, WINDOW);
    let idle = pd::row_text(list, 1, WINDOW).and_then(|t| t.title);
    pd::test_overlay(None);
    assert_eq!(playing, None);
    assert_eq!(idle, Some(egui::Color32::from_rgb(240, 240, 250)));
}

/// TIDAL's rule for the playing title is the accent token: a theme that
/// sets the token colours it (Tidalify's red, Minimalist Black's green in
/// TIDAL 2.46.0, captured). Tidalfast drew it in its own cyan there, where
/// the queue's playing song already took the theme's accent.
#[test]
fn a_themes_accent_colours_the_playing_title() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Red accent"} */
:root { --wave-color-solid-accent-fill: rgb(255, 1, 1); }
"#;
    let list = "album:55";
    let key = dom_anchors::keys::row_cell(list, 0, "title-text");
    let o = overlay_for("red-accent.css", css, &key);
    pd::test_overlay(Some((*o).clone()));
    let playing = pd::playing_title(list, 0, WINDOW);
    pd::test_overlay(None);
    assert_eq!(playing, Some(egui::Color32::from_rgb(255, 1, 1)));
}

/// A page's track list sits in its panel's `div._tableContainer_556e9aa`
/// (TIDAL 2.46.0), so rules nested in that container reach the rows: the
/// cotton candy family's `[class="_tableContainer_556e9aa"] { a { color } }`
/// gives the artist links its words' colour (Cotton Candy Nights' lilac in
/// TIDAL, captured). Tidalfast's rows sat beside the container.
#[test]
fn a_track_list_sits_in_its_table_container() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Container words"} */
[class="_tableContainer_556e9aa"] {
a { color: rgb(216, 218, 255); }
._textHigh_bbc1075 { color: rgb(216, 218, 255) !important; }
}
"#;
    let mut frame = dom_anchors::sample_frame();
    let page = frame.page.get_or_insert_with(dom_anchors::Page::default);
    page.page_id = Some("album".into());
    page.list_panel = Some([260.0, 264.0, 1000.0, 400.0]);
    let list = "album:55";
    let key = dom_anchors::keys::row_cell(list, 0, "title-text");
    let o = overlay_of(&frame, "container-words.css", css, &key);
    pd::test_overlay(Some((*o).clone()));
    let artist = pd::row_text(list, 1, WINDOW).and_then(|t| t.artist);
    let playing = pd::playing_title(list, 0, WINDOW);
    pd::test_overlay(None);
    assert_eq!(artist, Some(egui::Color32::from_rgb(216, 218, 255)));
    assert_eq!(playing, Some(egui::Color32::from_rgb(216, 218, 255)));
}

/// TIDAL 2.46.0's playlist chooser has a search field of its own
/// (`div._container_f1be359._searchInput_79eecb3`, not the header's
/// `.searchFieldWithinSearchFieldWithPopover`): a theme's rule for the
/// header pill leaves it alone and its rule for `._container_f1be359`
/// reaches it (Cotton Candy Nights' pink glass on the chooser, its indigo on
/// the pill; captured). Tidalfast's playlist picker took the header pill's
/// look.
#[test]
fn the_playlist_choosers_search_takes_its_own_rules() {
    use crate::theme::Palette;
    use crate::ui::{plugin_dom as pd, widgets};
    let css = r#"/* {"name": "Two searches"} */
._container_f1be359 { background-color: rgb(250, 190, 250); }
.searchFieldWithinSearchFieldWithPopover { background-color: rgb(31, 17, 75) !important; }
"#;
    let o = overlay_for(
        "two-searches.css",
        css,
        dom_anchors::keys::CHOOSER_SEARCH_FIELD,
    );
    pd::test_overlay(Some((*o).clone()));
    let palette = Palette::dark();
    let header = widgets::search_field_style(&palette, widgets::SearchLook::Header);
    let chooser = widgets::search_field_style(&palette, widgets::CHOOSER_SEARCH);
    pd::test_overlay(None);
    assert_eq!(
        header.and_then(|s| s.fill),
        Some(egui::Color32::from_rgb(31, 17, 75))
    );
    assert_eq!(
        chooser.and_then(|s| s.fill),
        Some(egui::Color32::from_rgb(250, 190, 250))
    );
}

/// A theme that styles only the header pill leaves the chooser's field
/// native, as TIDAL 2.46.0 does under Blur (its white on the pill only,
/// captured).
#[test]
fn a_header_only_rule_leaves_the_choosers_search_native() {
    use crate::theme::Palette;
    use crate::ui::{plugin_dom as pd, widgets};
    let css = r#"/* {"name": "Pill only"} */
.searchFieldWithinSearchFieldWithPopover { background-color: rgba(255, 255, 255, 0.15); }
"#;
    let o = overlay_for("pill-only.css", css, dom_anchors::keys::SEARCH_FIELD);
    pd::test_overlay(Some((*o).clone()));
    let chooser = widgets::search_field_style(&Palette::dark(), widgets::CHOOSER_SEARCH);
    pd::test_overlay(None);
    assert_eq!(chooser, None);
}

/// TidaLuna's store search is an MUI text field, which the cotton candy
/// family colours by its classes (`.css-1xqp9nb` dark violet with white
/// words under Cotton Candy Nights in TIDAL 2.46.0, captured). The Plugins
/// page's search takes that, not the header pill's look.
#[test]
fn the_plugin_store_search_takes_tidalunas_field_rules() {
    use crate::theme::Palette;
    use crate::ui::{plugin_dom as pd, widgets};
    let css = r#"/* {"name": "Store field"} */
.css-1xqp9nb { color: #ffffff !important; background-color: rgb(47, 10, 61) !important; }
.searchFieldWithinSearchFieldWithPopover { background-color: rgb(250, 190, 250); }
"#;
    let o = overlay_for(
        "store-field.css",
        css,
        dom_anchors::keys::STORE_SEARCH_FIELD,
    );
    pd::test_overlay(Some((*o).clone()));
    let store = widgets::search_field_style(&Palette::dark(), widgets::STORE_SEARCH);
    pd::test_overlay(None);
    assert_eq!(
        store.and_then(|s| s.fill),
        Some(egui::Color32::from_rgb(47, 10, 61))
    );
    assert_eq!(store.and_then(|s| s.color), Some(egui::Color32::WHITE));
}

/// The sidebar's create menu ("Playlist", "Folder") is a TIDAL context menu
/// whose rows carry a leading icon (TIDAL 2.46.0, captured): the hovered
/// row is TIDAL's white wash under a theme, and the words take a theme's
/// rule for the menu's words (Cotton Candy Nights' black). Tidalfast drew
/// the palette's own hover and white words there.
#[test]
fn the_create_menus_rows_take_the_menus_look() {
    use crate::theme::Palette;
    use crate::ui::{plugin_dom as pd, widgets};
    let ink = egui::Color32::from_rgb(5, 0, 20);
    let css = r#"/* {"name": "Inked create menu"} */
._contextMenu_8606b2d { background-color: rgb(240, 170, 230); }
._actionTextInner_a14f37c { color: rgb(5, 0, 20); }
"#;
    let o = overlay_for("inked-create.css", css, dom_anchors::keys::THEME_MENU_TEXT);
    pd::test_overlay(Some((*o).clone()));
    let candy = Palette {
        themed: true,
        own_surfaces: false,
        overlay: egui::Color32::from_rgb(240, 170, 230),
        surface_hover: egui::Color32::from_rgb(40, 20, 60),
        ..Palette::dark()
    };
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut fills = Vec::new();
    let mut words = Vec::new();
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events: vec![egui::Event::PointerMoved(egui::pos2(60.0, 20.0))],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.set_width(widgets::MENU_MIN_CONTENT);
            let _ = widgets::menu_icon_row(ui, &candy, crate::theme::Icon::NotePlus, "Playlist");
        });
        out.textures_delta.clear();
        fills.clear();
        words.clear();
        for c in &out.shapes {
            match &c.shape {
                egui::Shape::Rect(r) => fills.push(r.fill),
                egui::Shape::Text(t) => words.push(t.fallback_color),
                _ => {}
            }
        }
    }
    pd::test_overlay(None);
    assert!(
        fills.contains(&egui::Color32::from_white_alpha(38)),
        "TIDAL's white: {fills:?}"
    );
    assert_eq!(words, vec![ink]);
}

/// A playlist's filter sits inside TIDAL's `div._items_8615603` with the
/// list (TIDAL 2.46.0, captured): the cotton candy family's glass panel
/// runs round both, its padding round them. Tidalfast's panel began at the
/// list, under the filter.
#[test]
fn a_playlists_panel_holds_its_filter() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Glass items"} */
._items_8615603 { background-color: rgba(40, 10, 60, 0.5); padding: 10px 12px 0px; }
"#;
    let mut frame = dom_anchors::sample_frame();
    let page = frame.page.get_or_insert_with(dom_anchors::Page::default);
    page.page_id = Some("playlist".into());
    page.list_panel = Some([260.0, 400.0, 1000.0, 300.0]);
    let o = overlay_of(
        &frame,
        "glass-items.css",
        css,
        dom_anchors::keys::LIST_PANEL,
    );
    pd::test_overlay(Some((*o).clone()));
    pd::test_page_frame_as("playlist");
    let ctx = egui::Context::default();
    let filter = egui::Rect::from_min_size(egui::pos2(260.0, 300.0), egui::vec2(1000.0, 42.0));
    let list = egui::Rect::from_min_size(egui::pos2(260.0, 382.0), egui::vec2(1000.0, 300.0));
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        pd::filter_panel_slot(ui, filter);
        ui.painter()
            .rect_filled(filter, 12.0, egui::Color32::from_rgb(1, 2, 3));
        pd::list_panel(ui, list);
    });
    out.textures_delta.clear();
    let panel = pd::test_list_panel();
    pd::test_overlay(None);
    let panel = panel.expect("the panel is noted");
    assert_eq!(panel, [248.0, 290.0, 1024.0, 392.0]);
    // Painted under the filter: the panel's shapes come first.
    let first_filter = out
        .shapes
        .iter()
        .position(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == egui::Color32::from_rgb(1, 2, 3)))
        .expect("the filter");
    let panel_at = out
        .shapes
        .iter()
        .position(|c| !matches!(&c.shape, egui::Shape::Noop))
        .expect("something drawn");
    assert!(panel_at < first_filter, "panel under the filter");
}

/// TIDAL 2.46.0 has many `_titleText_` modules, one hash each (the track
/// list's `_titleText_51cccae`, the queue's `_titleText_6563d98`, the
/// sidebar heading's `_titleText_1d0e44e`, the mini header's
/// `_titleText_e186d6e`, ...), which themes colour apart: Cotton Candy
/// Nights' colourways make the mini header's their text colour with
/// `!important`, and TIDAL draws the queue's playing title white all the
/// same (captured under all eight). Tidalfast matched any `_titleText_`
/// rule to all of them, so the queue's playing title took the mini header's
/// colour.
#[test]
fn one_titletext_module_does_not_take_anothers_rules() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Mini header"} */
._titleText_e186d6e { color: rgb(255, 221, 244) !important; }
._playQueueItemsContainer_01c88fe ._marketText_1lyag_1._medium20_1lyag_192 { color: rgb(255, 255, 255); }
._titleText_51cccae { color: rgb(200, 210, 250); }
"#;
    let list = "album:55";
    let title = dom_anchors::keys::row_cell(list, 1, "title-text");
    let o = overlay_for("mini-header.css", css, &title);
    let color = |key: &str| o.hosts.get(key).and_then(|h| h.style.color.clone());
    assert_eq!(
        color(dom_anchors::keys::QUEUE_ROLE_PLAYING).as_deref(),
        Some("rgb(255, 255, 255)")
    );
    pd::test_overlay(Some((*o).clone()));
    let idle = pd::row_text(list, 1, WINDOW).and_then(|t| t.title);
    pd::test_overlay(None);
    assert_eq!(idle, Some(egui::Color32::from_rgb(200, 210, 250)));
}
