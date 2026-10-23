//! Round 31: anchors checked against TIDAL 2.46.0's own element tree and
//! computed styles, captured with each theme applied. Proved with made-up
//! themes (their own colours, TIDAL's classes and anchors only).

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

/// A track's title is the words of TIDAL's `span[data-test=table-cell-title]`
/// itself (`._titleText_ .table--inactive-title`), not a link: a theme's
/// `a { color }` leaves it alone and its `[class*="active"]` colours it, as
/// Catppuccin Mocha Blue's blue titles show in TIDAL. Tidalfast's title sat
/// in a link and took the theme's link colour.
#[test]
fn a_track_title_is_not_a_link() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Linky"} */
a { color: rgb(200, 160, 250) !important; }
[class*="active"] { color: rgb(30, 100, 240) !important; }
"#;
    let luna = themed_luna("linky.css", css);
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let list = "album:55";
    let title = dom_anchors::keys::row_cell(list, 1, "title-text");
    let o = wait(&luna, &tree, &|o| {
        o.hosts.get(&title).is_some_and(|h| h.style.color.is_some())
    });
    pd::test_overlay(Some((*o).clone()));
    let window = egui::Color32::from_rgb(10, 10, 10);
    let text = pd::row_text(list, 1, window).expect("the row's colours");
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(text.title, Some(egui::Color32::from_rgb(30, 100, 240)));
    // The artist is a link in TIDAL too.
    assert_eq!(text.artist, Some(egui::Color32::from_rgb(200, 160, 250)));
}

/// A theme that colours every element (`* { color }`, Meteor Night's
/// white) leaves TIDAL's playing playlist name in its teal and the queue's
/// playing song in its accent: TIDAL's own rules for their quality classes
/// (`._qualityHigh_39ef787` on the name, `._textHigh_ee6a2ff` on the
/// title, TIDAL 2.46.0) outweigh it, as TIDAL's captures under Meteor Night
/// show. Tidalfast drew both white. Other names take the colour.
#[test]
fn a_catch_all_colour_leaves_the_playing_items_to_tidal() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::{self as pd, SidePanel, themed, tidal_panel_text};
    let css = r#"/* {"name": "Snowfall"} */
:root { --snow: rgb(240, 240, 250); }
body { background-color: rgb(12, 14, 30) !important; }
* { color: var(--snow); }
"#;
    let mut frame = dom_anchors::sample_frame();
    let row = Some([8.0, 300.0, 224.0, 52.0]);
    frame.sidebar.rows = vec![(row, false), (row, false)];
    frame.sidebar.playing = Some(1);
    let tree = dom_anchors::build(&frame);
    let luna = themed_luna("snowfall.css", css);
    let idle = dom_anchors::keys::lib_row_title(0);
    let playing = dom_anchors::keys::lib_row_title(1);
    let o = wait(&luna, &tree, &|o| {
        o.hosts.get(&idle).is_some_and(|h| h.style.color.is_some())
            && o.hosts.contains_key(dom_anchors::keys::QUEUE_ROLE_TEXT)
    });
    luna.shutdown();
    let playing_color = o.hosts.get(&playing).and_then(|h| h.style.color.clone());
    assert_eq!(playing_color, None);
    pd::test_overlay(Some((*o).clone()));
    let base = Palette::dark();
    let queue = tidal_panel_text(themed(&base, &o), SidePanel::Queue);
    pd::test_overlay(None);
    assert_eq!(queue.accent, base.accent);
    assert_eq!(queue.text, egui::Color32::from_rgb(240, 240, 250));
}

/// A rule that names the queue's titles with more weight than TIDAL's own
/// still colours the playing one (the cotton candy family's white).
#[test]
fn a_heavier_rule_still_colours_the_playing_song() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::{self as pd, SidePanel, themed, tidal_panel_text};
    let css = r#"/* {"name": "Pinned words"} */
._playQueueItemsContainer_01c88fe ._marketText_1lyag_1._medium20_1lyag_192 { color: rgb(250, 200, 230); }
"#;
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let luna = themed_luna("pinned-words.css", css);
    let o = wait(&luna, &tree, &|o| {
        o.hosts
            .get(dom_anchors::keys::QUEUE_ROLE_PLAYING)
            .is_some_and(|h| h.style.color.is_some())
    });
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let queue = tidal_panel_text(themed(&Palette::dark(), &o), SidePanel::Queue);
    pd::test_overlay(None);
    assert_eq!(queue.accent, egui::Color32::from_rgb(250, 200, 230));
}

/// The queue's artist names are the line's grey, a followed artist's too:
/// TIDAL 2.46.0 draws Eminem (followed) in the same 55 % white as every
/// other name in its queue, captured with no theme. Tidalfast drew a
/// followed artist in the accent cyan.
#[test]
fn the_queue_draws_a_followed_artist_like_the_others() {
    use crate::theme::Palette;
    use crate::ui::now_playing::queue_name_colors;
    let (plain, followed) = queue_name_colors(&Palette::dark());
    assert_eq!(followed, plain);
}

/// A collection page's boxes are TIDAL 2.46.0's: its grid of cards is
/// `section._grid_97562f5[data-test=grid]` and its filter
/// `div._container_080b5dc._contentFilter_2d85a1d`, which the cotton candy
/// family makes a glass panel and a tinted pill; an album's list sits in
/// its `ALBUM_ITEMS` module and a playlist's in `div._items_8615603`.
/// Tidalfast gave them the classes of earlier TIDAL builds, which those
/// rules miss.
#[test]
fn collection_and_list_boxes_have_tidals_classes() {
    use crate::ui::plugin_dom::{self as pd, fill};
    use dom_anchors::keys::{CARD_GRID, LIST_PANEL, PAGE_FILTER_BOX};
    let css = r#"/* {"name": "Glass boxes"} */
._grid_97562f5 { border: 2px solid rgb(146, 190, 255); border-radius: 1.1em; background-color: rgb(20, 10, 60); }
[class="_container_080b5dc _contentFilter_2d85a1d"] { background-color: rgb(70, 20, 90); }
.is-intersecting._occlusion_de6b9d3._defaultStyles_a1b9da9.ALBUM_ITEMS.tidal-ui__occlusion { background-color: rgb(10, 80, 40); }
._items_8615603 { background-color: rgb(90, 60, 10); }
"#;
    let luna = themed_luna("glass-boxes.css", css);
    let collection = {
        let mut frame = dom_anchors::sample_frame();
        let page = frame.page.as_mut().unwrap();
        page.page_id = Some("mycollection_albums".into());
        page.filter = Some(dom_anchors::FilterInput {
            rect: Some([240.0, 160.0, 1000.0, 42.0]),
            value: String::new(),
        });
        page.grid = Some([240.0, 240.0, 1000.0, 600.0]);
        page.cards = vec![dom_anchors::GridCard {
            rect: Some([240.0, 240.0, 200.0, 280.0]),
            ..dom_anchors::GridCard::default()
        }];
        dom_anchors::build(&frame)
    };
    let o = wait(&luna, &collection, &|o| {
        o.hosts.contains_key(CARD_GRID) && o.hosts.contains_key(PAGE_FILTER_BOX)
    });
    pd::test_overlay(Some((*o).clone()));
    let grid = pd::anchor_look(CARD_GRID).expect("the grid's look");
    let filter = fill(PAGE_FILTER_BOX);
    pd::test_overlay(None);
    assert_eq!(grid.fill, Some(egui::Color32::from_rgb(20, 10, 60)));
    assert_eq!(grid.corners, Some(egui::CornerRadius::same(15)));
    assert!(grid.layers, "the border");
    assert_eq!(filter, Some(egui::Color32::from_rgb(70, 20, 90)));
    for (page_id, want) in [
        ("album", egui::Color32::from_rgb(10, 80, 40)),
        ("playlist", egui::Color32::from_rgb(90, 60, 10)),
    ] {
        let mut frame = dom_anchors::sample_frame();
        let page = frame.page.as_mut().unwrap();
        page.page_id = Some(page_id.into());
        page.list_panel = Some([240.0, 290.0, 1040.0, 400.0]);
        let tree = dom_anchors::build(&frame);
        let o = wait(&luna, &tree, &|o| {
            o.hosts
                .get(LIST_PANEL)
                .and_then(|h| h.style.background_color.as_deref())
                .is_some_and(|c| c.contains(&want.r().to_string()))
        });
        pd::test_overlay(Some((*o).clone()));
        let panel = fill(LIST_PANEL);
        pd::test_overlay(None);
        assert_eq!(panel, Some(want), "{page_id}");
    }
    luna.shutdown();
}
