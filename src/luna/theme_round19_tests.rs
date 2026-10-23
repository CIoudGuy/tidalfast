//! Luna themes round 19: rules on views Tidalfast draws itself reach their
//! painters through TIDAL's classes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom_anchors;
use super::*;

/// The cells' colours (`[id^="titleCell"]`, Inrixia's example theme), the
/// sidebar's `_navigationItem_` rows, the collection's `_cellContainer_`
/// cells, the context menu's `_actionItem_` rows, the search pill and the
/// shuffle and repeat glyphs.
#[test]
fn theme_rules_reach_track_row_text_nav_rows_collection_cells_menus_search_and_transport() {
    use crate::ui::plugin_dom::{
        color, grid_card_style, nav_style, next_menu_row_style, row_text, search_style,
        test_overlay,
    };
    use dom_anchors::keys::*;
    let css = r#"/* {"name": "Round 19"} */
[id^="titleCell"], [id^="albumText"] { color: #c4ab60; }
[id^="contributionText"] { color: #112233; }
._navigationItem_4e9deae { background: transparent !important; border-radius: 8px !important; color: rgba(255, 255, 255, 0.78) !important; }
._navigationItem_4e9deae._selected_29027d1 { background-color: #123456 !important; }
[data-track--page-id^="mycollection_"] ._cellContainer_19b23ad { background-color: #654321; padding: .6em; border-radius: 1.1em; }
._actionItem_e0c1e65 { border-radius: 4px; }
._actionTextInner_a14f37c { color: #abcdef; }
div._searchField_b2eb7f7 { background-color: #fefefe; border-radius: 6px; }
div._searchField_b2eb7f7 input { color: #222222; }
._shuffleButton_545c75a, ._repeatButton_729613e { color: #fedcba; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/r19.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-round19");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let mut frame = dom_anchors::sample_frame();
    let list = frame.lists[0].id.clone();
    if let Some(page) = frame.page.as_mut() {
        page.page_id = Some("mycollection_playlists".into());
        page.cards = vec![dom_anchors::GridCard {
            rect: Some([240.0, 300.0, 173.0, 240.0]),
            artist: false,
            cover: Some([240.0, 300.0, 173.0, 173.0]),
            shelf: None,
        }];
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/r19.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(&card(0)) {
            break o;
        }
        assert!(Instant::now() < end, "the page was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    test_overlay(Some((*o).clone()));
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    let window = egui::Color32::from_rgb(10, 10, 12);
    let text = row_text(&list, 0, window).expect("a themed row text");
    assert_eq!(text.title, Some(egui::Color32::from_rgb(0xc4, 0xab, 0x60)));
    assert_eq!(text.album, Some(egui::Color32::from_rgb(0xc4, 0xab, 0x60)));
    assert_eq!(text.time, None, "the time column is not styled");
    assert_eq!(
        text.artist, None,
        "a colour that would not read over the window is left out"
    );
    let nav = nav_style(&sidebar_item("sidebar-music")).expect("a themed navigation row");
    assert_eq!(nav.radius, Some(8.0));
    assert!(nav.color.is_some_and(|c| c.a() > 150));
    assert!(nav.fill.is_some(), "the theme's fill, transparent or not");
    let open = nav_style(&sidebar_item("sidebar-music")).expect("a themed navigation row");
    assert_eq!(open.fill, Some(egui::Color32::from_rgb(0x12, 0x34, 0x56)));
    let cell = grid_card_style(&card(0)).expect("a themed collection cell");
    assert_eq!(cell.fill, Some(egui::Color32::from_rgb(0x65, 0x43, 0x21)));
    assert_eq!(cell.radius, 1.1 * 14.0);
    assert_eq!(cell.padding, [0.6 * 14.0; 4]);
    let menu = next_menu_row_style().expect("a themed menu row");
    assert_eq!(menu.radius, Some(4.0));
    assert_eq!(menu.color, Some(egui::Color32::from_rgb(0xab, 0xcd, 0xef)));
    let search = search_style(&crate::theme::Palette::dark()).expect("a themed search pill");
    assert_eq!(search.fill, Some(egui::Color32::from_rgb(0xfe, 0xfe, 0xfe)));
    assert_eq!(search.radius, Some(6.0));
    assert_eq!(
        search.color,
        Some(egui::Color32::from_rgb(0x22, 0x22, 0x22))
    );
    let glyph = Some(egui::Color32::from_rgb(0xfe, 0xdc, 0xba));
    assert_eq!(color(SHUFFLE), glyph);
    assert_eq!(color(REPEAT), glyph);
    test_overlay(None);
    assert!(row_text(&list, 0, window).is_none());
    assert!(nav_style(&sidebar_item("sidebar-music")).is_none());
    assert!(search_style(&crate::theme::Palette::dark()).is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// The library's rows (`._contentItem_`: Solo's clear rows with a rounded
/// hover and open-page fill) reach the sidebar painter.
#[test]
fn theme_rules_reach_the_library_rows() {
    use crate::ui::plugin_dom::{library_row, test_overlay};
    let css = r#"/* {"name": "Round 19 rows"} */
._contentItem_684cbbe { background: transparent !important; color: rgba(255, 255, 255, 0.78) !important; }
._contentItem_684cbbe._selected_ae83ba5 { background-color: #123456 !important; border-radius: 8px !important; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/r19b.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-round19-rows");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.rows = vec![
        (Some([8.0, 200.0, 224.0, 56.0]), false),
        (Some([8.0, 256.0, 224.0, 56.0]), true),
    ];
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/r19b.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(&dom_anchors::keys::lib_row(1)) {
            break o;
        }
        assert!(Instant::now() < end, "the rows were never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    test_overlay(Some((*o).clone()));
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    let plain = library_row(
        egui::Rect::from_min_size(egui::pos2(8.0, 200.0), egui::vec2(224.0, 56.0)),
        false,
        false,
    )
    .expect("a themed library row");
    assert_eq!(plain.fill, Some(egui::Color32::TRANSPARENT));
    assert!(plain.color.is_some_and(|c| c.a() > 150));
    let open = library_row(
        egui::Rect::from_min_size(egui::pos2(8.0, 256.0), egui::vec2(224.0, 56.0)),
        true,
        false,
    )
    .expect("a themed open row");
    assert_eq!(open.fill, Some(egui::Color32::from_rgb(0x12, 0x34, 0x56)));
    assert_eq!(open.radius, Some(8.0));
    test_overlay(None);
    assert!(library_row(egui::Rect::NOTHING, false, false).is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// Explore's icon links (`a._item_8056dae[data-test^="page-link-"]` under
/// `[data-test-module-type="PAGE_LINKS"]`, Solo's near-black buttons).
#[test]
fn theme_rules_reach_explores_icon_links() {
    use crate::ui::plugin_dom::{page_link, test_overlay};
    let css = r#"/* {"name": "Round 19 links"} */
[data-test-module-type="PAGE_LINKS"] a._item_8056dae[data-test^="page-link-"] { background:#09090b !important; color:#eeeeee !important; }
[data-test-module-type="PAGE_LINKS"] a._item_8056dae[data-test^="page-link-"]:hover { background:#1c1c1f !important; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/r19c.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-round19-links");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.page_links = vec![(
            Some([248.0, 300.0, 200.0, 36.0]),
            Some([295.0, 308.0, 90.0, 20.0]),
        )];
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/r19c.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(&dom_anchors::keys::page_link(0)) {
            break o;
        }
        assert!(Instant::now() < end, "the links were never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    test_overlay(Some((*o).clone()));
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    let style = page_link(
        egui::Rect::from_min_size(egui::pos2(248.0, 300.0), egui::vec2(200.0, 36.0)),
        egui::Rect::from_min_size(egui::pos2(295.0, 308.0), egui::vec2(90.0, 20.0)),
    )
    .expect("a themed icon link");
    assert_eq!(style.fill, Some(egui::Color32::from_rgb(0x09, 0x09, 0x0b)));
    assert_eq!(style.color, Some(egui::Color32::from_rgb(0xee, 0xee, 0xee)));
    test_overlay(None);
    assert!(page_link(egui::Rect::NOTHING, egui::Rect::NOTHING).is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// A rule for the playing row's `div`s (`[data-track-id="<id>"] div`)
/// reaches a playlist's Added by and Date added cells too, which Tidalfast
/// left white (it had no element for them). scroll-to-playing's rules
/// for a row holding a `/track/` link reach nothing in a track list:
/// TIDAL 2.46.0's rows hold no such link (round 31), its title is a span.
#[test]
fn the_playing_rows_colour_reaches_its_added_by_and_date_added_cells() {
    use crate::ui::plugin_dom::{row_text, test_overlay};
    let mut frame = dom_anchors::sample_frame();
    let list = frame.lists[0].id.clone();
    let row = &mut frame.lists[0].rows[0];
    row.added_by = Some("Sam".into());
    row.added = Some("Oct 20, 2026".into());
    let id = row.track_id.clone();
    let css = format!(
        r#"/* {{"name": "Playing row"}} */
[role="row"]:has(a[href*="/track/{id}"]) a,
[role="row"]:has(a[href*="/track/{id}"]) span,
[role="row"]:has(a[href*="/track/{id}"]) p {{ color: rgb(250, 20, 20) !important; }}
[data-track-id="{id}"] div {{ color: rgb(126, 251, 238) !important; }}
"#
    );
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/stp.css".into(), css.into_bytes());
    let base = serve(files);
    let data = temp_dir("theme-playing-row-cells");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let tree = dom_anchors::build(&frame);
    let added = dom_anchors::keys::row_cell(&list, 0, "date-added");
    luna.install_theme(&format!("{base}/stp.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(&added) {
            break o;
        }
        assert!(Instant::now() < end, "the row's cells were never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    test_overlay(Some((*o).clone()));
    let window = egui::Color32::from_rgb(10, 10, 12);
    let text = row_text(&list, 0, window).expect("the playing row's colours");
    let turquoise = Some(egui::Color32::from_rgb(126, 251, 238));
    assert_eq!(text.title, turquoise);
    assert_eq!(text.added_by, turquoise, "Added by");
    assert_eq!(text.added, turquoise, "Date added");
    assert_eq!(row_text(&list, 1, window), None, "another row");
    test_overlay(None);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}
