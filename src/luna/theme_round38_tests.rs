//! Round 38: differences the parity checker found between TIDAL 2.46.0's
//! computed styles under a theme and Tidalfast's resolved anchor looks, and
//! a theme's `backdrop-filter` drawn as TIDAL draws it. Proved with
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

fn overlay(name: &str, css: &str, frame: &dom_anchors::Frame, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
    let luna = themed_luna(name, css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(key));
    luna.shutdown();
    o
}

fn queue_frame() -> dom_anchors::Frame {
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([1795.0, 60.0, 334.0, 900.0]), false));
    frame
}

/// A theme's frosted glass (`backdrop-filter: blur() brightness()` under a
/// see-through fill: the cotton candy family's play queue) is what lies
/// behind the box, blurred and darkened, with the theme's own fill laid
/// over it, as TIDAL draws it. Where the filter is drawn, the panel's look
/// carries the filter and the theme's fill as written (TIDAL 2.46.0's
/// computed `background-color`); it folded the brightness into the fill
/// before, a darker and more opaque colour than TIDAL's. Without the GPU
/// passes the fold stays.
#[test]
fn a_drawn_backdrop_filter_lies_under_the_themes_own_fill() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE_PANEL;
    let css = r#"/* {"name": "Glass queue"} */
#playQueueSidebar { background-color: rgba(200, 100, 50, 0.5); backdrop-filter: blur(.5em) brightness(.5) saturate(1.5); }
"#;
    let o = overlay("glass-queue.css", css, &queue_frame(), QUEUE_PANEL);
    let look = |gpu: bool| {
        let _gpu = backdrop::force(gpu);
        pd::test_overlay(Some((*o).clone()));
        let look = pd::anchor_look(QUEUE_PANEL).expect("the panel's look");
        pd::test_overlay(None);
        look
    };
    let drawn = look(true);
    let filter = drawn.backdrop.as_ref().expect("the backdrop filter");
    assert!((filter.blur - 7.0).abs() < 1e-3, "{filter:?}");
    assert_eq!(filter.rows.len(), 2, "brightness and saturate: {filter:?}");
    assert_eq!(
        drawn.fill,
        Some(egui::Color32::from_rgba_unmultiplied(200, 100, 50, 128)),
        "the theme's fill as it is"
    );
    let folded = look(false);
    assert_eq!(folded.backdrop, None);
    let fill = folded.fill.expect("the folded fill");
    assert!(
        fill.a() > 128 && fill != egui::Color32::from_rgba_unmultiplied(200, 100, 50, 128),
        "without the GPU passes the brightness is folded in: {fill:?}"
    );
}

/// The filter is drawn before anything of the box: a paint callback ahead
/// of the panel's fill, so the fill goes over the filtered window and not
/// under it. A box with no backdrop filter adds no callback.
#[test]
fn the_backdrop_is_painted_before_the_boxs_fill() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE_PANEL;
    let css = r#"/* {"name": "Glass order"} */
#playQueueSidebar { background-color: rgba(10, 200, 120, 0.4); backdrop-filter: blur(6px); }
"#;
    let o = overlay("glass-order.css", css, &queue_frame(), QUEUE_PANEL);
    let painted = |gpu: bool| {
        let _gpu = backdrop::force(gpu);
        pd::test_overlay(Some((*o).clone()));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let r = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(300.0, 600.0));
            pd::paint_part(
                ui,
                QUEUE_PANEL,
                r,
                egui::CornerRadius::same(12),
                egui::Color32::from_gray(30),
            );
        });
        out.textures_delta.clear();
        pd::test_overlay(None);
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Callback(_) => Some("backdrop"),
                egui::Shape::Rect(r)
                    if r.fill == egui::Color32::from_rgba_unmultiplied(10, 200, 120, 102) =>
                {
                    Some("fill")
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(painted(true), ["backdrop", "fill"]);
    assert!(!painted(false).contains(&"backdrop"));
}

/// A see-through box (no fill of its own) with a drawn backdrop filter
/// shows the filtered window first, in place of the stand-ins used where it
/// is not drawn (the page's picture blurred, the brightness as a veil):
/// the cotton candy family's title bar, `background-color: transparent;
/// backdrop-filter: blur(3px)`.
#[test]
fn a_see_through_box_shows_the_filtered_window() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::MAIN_CONTENT;
    let css = r#"/* {"name": "Glass content"} */
.mainContent { background-color: rgba(0, 0, 0, 0); backdrop-filter: blur(3px) brightness(.8); }
"#;
    let o = overlay(
        "glass-content.css",
        css,
        &dom_anchors::sample_frame(),
        MAIN_CONTENT,
    );
    let layers = |gpu: bool| {
        let _gpu = backdrop::force(gpu);
        pd::test_overlay(Some((*o).clone()));
        let layers = pd::box_layers(&o.hosts[MAIN_CONTENT], &pd::vars(), [2137.0, 1183.0]);
        pd::test_overlay(None);
        layers
    };
    let drawn = layers(true);
    assert!(
        matches!(drawn.first(), Some(pd::BoxLayer::Backdrop(b)) if (b.blur - 3.0).abs() < 1e-3 && b.rows.len() == 1),
        "{drawn:?}"
    );
    assert!(
        !drawn
            .iter()
            .any(|l| matches!(l, pd::BoxLayer::Frost(_) | pd::BoxLayer::Veil(..))),
        "{drawn:?}"
    );
    let stand_in = layers(false);
    assert!(
        matches!(stand_in.as_slice(), [pd::BoxLayer::Frost(r), pd::BoxLayer::Veil(..)] if *r == 3.0),
        "{stand_in:?}"
    );
}

/// TIDAL 2.46.0's back and forward pill is `div._container_bb78f1d
/// [data-test=navigation-arrows]` with `button._button_6808704` buttons.
/// A theme naming it exactly (the cotton candy family's
/// `[class="_container_bb78f1d"] { transform: translateY(-7em) }`, which
/// lifts it out of TIDAL's window) reached nothing: the anchor carried an
/// older build's `_navigationArrows_` class.
#[test]
fn the_navigation_arrows_are_tidals_pill() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{NAV_ARROWS, NAV_BACK};
    let css = r#"/* {"name": "Lifted arrows"} */
[class="_container_bb78f1d"] { transform: translateY(-5em); }
[data-test="navigation-arrows"] > button._button_6808704 { color: rgb(1, 2, 3); }
"#;
    let o = overlay(
        "lifted-arrows.css",
        css,
        &dom_anchors::sample_frame(),
        NAV_BACK,
    );
    pd::test_overlay(Some((*o).clone()));
    let pill = pd::anchor_look(NAV_ARROWS);
    let back = pd::anchor_look(NAV_BACK);
    pd::test_overlay(None);
    assert_eq!(
        pill.map(|l| l.translate),
        Some(egui::vec2(0.0, -70.0)),
        "the pill moves up 5em"
    );
    assert_eq!(
        back.and_then(|l| l.text),
        Some(egui::Color32::from_rgb(1, 2, 3))
    );
}

fn page_frame(page_id: &str) -> dom_anchors::Frame {
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.page_id = Some(page_id.into());
        page.actions = vec![dom_anchors::HeaderAction {
            test: "share-button".into(),
            label: "Share".into(),
            rect: Some([600.0, 220.0, 48.0, 48.0]),
            icon: Some([614.0, 224.0, 20.0, 20.0]),
            caption: Some([600.0, 250.0, 48.0, 14.0]),
            checked: None,
        }];
    }
    frame
}

/// The header's controls are each page's own modules in TIDAL 2.46.0: an
/// album's captioned buttons sit in `div._smallButtons_5c1ea50`, a
/// playlist's in `div._smallButtons_1da13ba._actionSmallButtons_6ce346a`,
/// and the pills in a box of their own (`div._container_84abbc3`,
/// `_container_5a08754`). A theme's rule for the album's box (the cotton
/// candy family's dark captions) coloured a playlist's captions too, which
/// stay TIDAL's there.
#[test]
fn the_header_controls_are_each_pages_own() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{PAGE_HEADER_CONTROLS, PAGE_PLAY};
    let css = r#"/* {"name": "Page captions"} */
[class="_smallButtons_5c1ea50"] { & [data-wave-color="textDefault"] { color: rgb(9, 8, 7) !important; } }
._actionContainer_334aa3b ._container_5a08754 > [data-test="play-all"] { color: rgb(1, 2, 3); }
[data-test="header-controls"] { opacity: 0.99; }
"#;
    let label = dom_anchors::keys::page_action_label("share-button");
    let looks = |page: &str| {
        let o = overlay(
            &format!("page-captions-{page}.css"),
            css,
            &page_frame(page),
            PAGE_HEADER_CONTROLS,
        );
        pd::test_overlay(Some((*o).clone()));
        let caption = pd::anchor_look(&label).and_then(|l| l.text);
        let play = pd::anchor_look(PAGE_PLAY).and_then(|l| l.text);
        pd::test_overlay(None);
        (caption, play)
    };
    let album = looks("album");
    let playlist = looks("playlist");
    assert_eq!(album, (Some(egui::Color32::from_rgb(9, 8, 7)), None));
    assert_eq!(playlist, (None, Some(egui::Color32::from_rgb(1, 2, 3))));
}

/// `em` in a box's corners is the box's own text size: TIDAL's medium
/// market buttons (the header's Play and Shuffle) are 16 points, so the
/// cotton candy family's `.9em` corners are 14.4 there, not 12.6.
#[test]
fn em_corners_follow_the_boxs_own_text_size() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::PAGE_PLAY;
    let css = r#"/* {"name": "Em pills"} */
[data-test="header-controls"] [data-test="play-all"] { border-radius: .5em; background-color: rgb(200, 0, 100); }
"#;
    let o = overlay("em-pills.css", css, &dom_anchors::sample_frame(), PAGE_PLAY);
    pd::test_overlay(Some((*o).clone()));
    let look = pd::anchor_look(PAGE_PLAY);
    let radius = pd::radius(PAGE_PLAY);
    pd::test_overlay(None);
    assert_eq!(
        look.and_then(|l| l.corners),
        Some(egui::CornerRadius::same(8))
    );
    assert_eq!(radius, Some(8.0));
}

/// A library row's art is the row's first child in TIDAL 2.46.0 (`a.
/// _contentItem_ > div._thumbnail_f82abfa > img._artworkImage_5bde5a4`): a
/// theme's rule for the open row's art reaches it (the cotton candy
/// family's `._contentItem_684cbbe._selected_ae83ba5 { ._thumbnail_f82abfa
/// img { filter: none } }` leaves the open playlist's art untinted while
/// the others are sepia). The art sat beside the rows, so it reached none.
#[test]
fn a_library_rows_art_is_in_the_row() {
    use dom_anchors::keys::{thumb, thumb_img};
    let css = r#"/* {"name": "Row art"} */
[id="sidebar"] ._thumbnail_f82abfa img { filter: grayscale(1); }
._contentItem_684cbbe._selected_ae83ba5 ._thumbnail_f82abfa img { filter: none; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.rows = vec![
        (Some([16.0, 400.0, 208.0, 56.0]), false),
        (Some([16.0, 456.0, 208.0, 56.0]), true),
    ];
    frame.sidebar.thumbs = vec![
        Some([24.0, 404.0, 48.0, 48.0]),
        Some([24.0, 460.0, 48.0, 48.0]),
    ];
    let o = overlay("row-art.css", css, &frame, &thumb_img(1));
    let filter = |key: &str| {
        o.hosts
            .get(key)
            .and_then(|h| h.style.filter.clone())
            .unwrap_or_default()
    };
    assert_eq!(filter(&thumb_img(0)), "grayscale(1)");
    assert_eq!(filter(&thumb_img(1)), "none");
    assert!(o.hosts.contains_key(&thumb_img(0)) && !thumb(0).is_empty());
}

/// The queue's word stand-ins sit in TIDAL 2.46.0's scrolling list
/// (`div._playQueueItemsContainer_01c88fe[data-test=playqueue] >
/// div._playQueueItems_84488b2`), under which
/// the cotton candy family nests its rules for the queue's artist lines
/// (`[class="_playQueueItems_84488b2"] { ._artistText_73966a1... { transform:
/// translatey(-.1em) } }`); with no box of that class they reached nothing.
#[test]
fn the_queue_words_sit_in_tidals_scrolling_list() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE_ROLE_SECONDARY;
    let css = r#"/* {"name": "Queue list"} */
[class="_playQueueItems_84488b2"] { ._artistText_73966a1._text20_1lyag_291 { transform: translateY(-.5em); color: rgb(3, 4, 5); } }
"#;
    let o = overlay("queue-list.css", css, &queue_frame(), QUEUE_ROLE_SECONDARY);
    pd::test_overlay(Some((*o).clone()));
    let look = pd::anchor_look(QUEUE_ROLE_SECONDARY);
    pd::test_overlay(None);
    let look = look.expect("the artist line's look");
    assert_eq!(look.translate, egui::vec2(0.0, -7.0));
    assert_eq!(look.text, Some(egui::Color32::from_rgb(3, 4, 5)));
}

/// TIDAL 2.46.0's navigation block is `div#sidebar-content.
/// _fixedNavigation_00842ee` holding the navigation links themselves, and
/// the playlists sit apart in `div._listSection_26d6dae > div._container_
/// 4454af7 > ... > nav[data-test=sidebar-scrollable-nav]`. The cotton candy
/// family's `[class="_fixedNavigation_00842ee"] { transform: translateY(
/// -.5em) }` lifts the rows, TidalCustomTheme's `#sidebar-content >
/// a[data-test="sidebar-feed"] > span` reaches the Feed link's badge;
/// neither reached the anchors, which nested a second box in the content.
#[test]
fn the_sidebar_content_is_tidals_navigation_block() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{LIB_NAV, SIDEBAR_CONTENT, lib_row, sidebar_item};
    let css = r#"/* {"name": "Nav block"} */
[class="_fixedNavigation_00842ee"] { transform: translateY(-.5em); }
#sidebar-content > a[data-test="sidebar-music"] { color: rgb(7, 8, 9); }
#sidebar > ._listSection_26d6dae nav[data-test="sidebar-scrollable-nav"] > a { color: rgb(9, 8, 7); }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.rows = vec![(Some([16.0, 400.0, 208.0, 56.0]), false)];
    let tree = dom_anchors::build(&frame);
    let parent = |key: &str| {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.parent.clone())
    };
    assert_eq!(parent(&lib_row(0)).as_deref(), Some(LIB_NAV));
    let music = sidebar_item("sidebar-music");
    let o = overlay("nav-block.css", css, &frame, &lib_row(0));
    pd::test_overlay(Some((*o).clone()));
    let content = pd::anchor_look(SIDEBAR_CONTENT);
    let link = pd::anchor_look(&music);
    let row = pd::anchor_look(&lib_row(0));
    pd::test_overlay(None);
    assert_eq!(content.map(|l| l.translate), Some(egui::vec2(0.0, -7.0)));
    assert_eq!(
        link.and_then(|l| l.text),
        Some(egui::Color32::from_rgb(7, 8, 9))
    );
    assert_eq!(
        row.and_then(|l| l.text),
        Some(egui::Color32::from_rgb(9, 8, 7))
    );
}

/// An album's title is TIDAL 2.46.0's `h2.wave-text-title-bold.
/// _titleText_7494646` in the header's `div._detailContainer_5b36b6c`, a
/// playlist's `h2..._title_c9bc1b1` in `_detailContainer_105eeff`, each in
/// the header box. The anchor was the page's `h1._pageTitle_` beside it, so
/// the cotton candy family's rules for those titles (`._title_c9bc1b1 {
/// font-size: 30px }`, `[class="_dataContainer_66f4f40"] { [data-wave-color
/// ="textDefault"] { transform: translateY(-.4em) } }`, 12.8 of the title's
/// 32 points) reached nothing. A playlist's header box is
/// `[data-test=header-background]`, under which the family nests its rules
/// for the playlist's title.
#[test]
fn an_album_and_playlist_title_is_the_headers_own() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::PAGE_TITLE;
    let css = r#"/* {"name": "Header titles"} */
[class="_dataContainer_66f4f40"] { [data-wave-color="textDefault"] { transform: translateY(-.4em); } }
._detailContainer_5b36b6c > ._titleText_7494646 { color: rgb(11, 12, 13); }
[data-test="header-background"] { ._title_c9bc1b1 { font-size: 30px !important; padding-top: .3em; } }
"#;
    let look = |page: &str| {
        let mut frame = dom_anchors::sample_frame();
        if let Some(p) = frame.page.as_mut() {
            p.page_id = Some(page.into());
            p.title = Some("Fragments".into());
            p.title_rect = Some([560.0, 120.0, 300.0, 40.0]);
            p.data = Some([240.0, 92.0, 1040.0, 300.0]);
        }
        let o = overlay(
            &format!("header-titles-{page}.css"),
            css,
            &frame,
            PAGE_TITLE,
        );
        pd::test_overlay(Some((*o).clone()));
        let look = pd::anchor_look(PAGE_TITLE).expect("the title's look");
        pd::test_overlay(None);
        look
    };
    let album = look("album");
    assert_eq!(album.translate, egui::vec2(0.0, -12.8));
    assert_eq!(album.text, Some(egui::Color32::from_rgb(11, 12, 13)));
    let playlist = look("playlist");
    assert_eq!(playlist.font_size, Some(30.0));
    assert_eq!(playlist.padding.map(|p| p[0]), Some(9.0));
    assert_eq!(playlist.text, None);
}

/// A theme's `border-image` paints a box's border with its picture in
/// place of the colour: the cotton candy family's track rows (`border-
/// style: solid none none none; border-image: linear-gradient(to right,
/// #0000 2%, <line> 20%, <line> 80%, #0000 100%) 1`) have a line that
/// fades in and out, as in TIDAL; the cascade dropped the property, so the
/// line was drawn solid end to end.
#[test]
fn a_border_image_paints_the_rows_line() {
    use crate::ui::plugin_dom as pd;
    use crate::ui::theme_gradient::color_at;
    let css = r#"/* {"name": "Row lines"} */
._tableRow_299a426 { border-style: solid none none none; border-width: 2px; border-image: linear-gradient(to right, #0000 10%, rgb(200, 0, 0) 50%, #0000 90%) 1; }
"#;
    let row = dom_anchors::keys::row("album:55", 1);
    let o = overlay("row-lines.css", css, &dom_anchors::sample_frame(), &row);
    pd::test_overlay(Some((*o).clone()));
    let line = pd::border_image(&row);
    pd::test_overlay(None);
    let line = line.expect("the row's border image");
    assert!((line.angle - 90.0).abs() < 1e-3, "{line:?}");
    assert_eq!(color_at(&line, 0.5), egui::Color32::from_rgb(200, 0, 0));
    assert_eq!(color_at(&line, 0.05).a(), 0);
    assert_eq!(color_at(&line, 0.95).a(), 0);
}

/// TIDAL 2.46.0's title bar sits in its main container, under the
/// `#wimp > div[data-market-theme]` box round the app's shell:
/// `#wimp > div > div.has-footer-player > div._mainContainer_6519c30.
/// _hasPlayer_57557c6 > div._bar_5b9ac59._smallText_a636ed6`. The cotton
/// candy family names it by that chain (its frosted title bar) and by its
/// two classes exactly; the anchor sat beside the main container with one
/// class, so neither reached it.
#[test]
fn the_title_bar_is_in_tidals_main_container() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::BAR;
    let css = r#"/* {"name": "Title bar chain"} */
#wimp > div > div > div._mainContainer_6519c30._hasPlayer_57557c6 > div._bar_5b9ac59._smallText_a636ed6 { background-color: rgb(1, 2, 3); }
[class="_bar_5b9ac59 _smallText_a636ed6"] { opacity: 0.5; }
"#;
    let o = overlay("title-bar.css", css, &dom_anchors::sample_frame(), BAR);
    pd::test_overlay(Some((*o).clone()));
    let look = pd::anchor_look(BAR);
    pd::test_overlay(None);
    let look = look.expect("the title bar's look");
    assert_eq!(look.fill, Some(egui::Color32::from_rgb(1, 2, 3)));
    assert_eq!(look.opacity, Some(0.5));
}

/// TIDAL 2.46.0's play queue panel has a half-point border of its own
/// (`._container_b3e8f28 { border: .5px solid rgba(255, 255, 255, .1) }`):
/// a theme that sets only its style and colour (the cotton candy family's
/// `border-style: solid; border-color: ...`) recolours that line. The
/// cascade had no width for it, so the theme's colour went nowhere; with
/// no theme TIDAL's own line stays the native one.
#[test]
fn a_theme_recolours_the_queue_panels_own_border() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE_PANEL;
    let css = r#"/* {"name": "Queue line"} */
#playQueueSidebar { border-style: solid; border-color: rgb(9, 99, 199); }
"#;
    let o = overlay("queue-line.css", css, &queue_frame(), QUEUE_PANEL);
    let layers = pd::box_layers(&o.hosts[QUEUE_PANEL], &o.vars, [2137.0, 1183.0]);
    assert!(
        layers.iter().any(|l| matches!(l, pd::BoxLayer::Border(w, c, _) if (*w - 0.5).abs() < 1e-3 && *c == egui::Color32::from_rgb(9, 99, 199))),
        "{layers:?}"
    );
    let plain = r#"/* {"name": "Queue plain"} */
#playQueueSidebar { opacity: 0.99; }
"#;
    let o = overlay("queue-plain.css", plain, &queue_frame(), QUEUE_PANEL);
    assert_eq!(
        o.hosts[QUEUE_PANEL].style.border, None,
        "TIDAL's own line is native"
    );
}
