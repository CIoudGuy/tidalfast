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
