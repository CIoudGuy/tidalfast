//! Round 57: what the parity checker showed against TIDAL 2.47.0's captures,
//! proved with made-up themes (their own colours, TIDAL's classes and
//! anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

/// Installs `css` as a theme over `tree` and returns the overlay once the
/// theme reached it and `key` is styled.
fn overlay_with(
    name: &str,
    css: &str,
    tree: super::dom::HostTree,
    key: &str,
) -> (Luna, Arc<DomOverlay>) {
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert(format!("/{name}"), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir(name);
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    luna.install_theme(&format!("{base}/{name}"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && (key.is_empty() || o.hosts.contains_key(key)) {
            break o;
        }
        assert!(Instant::now() < end, "{key:?} was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    (luna, o)
}

/// TIDAL declares `--playQueueWidth` as its open play queue's 350 points and
/// `0px` while the queue is closed (its captures: 350 on the album and menu
/// pages, queue open, 0 on the playlist and collection). A theme that sizes
/// a box by it (the cotton candy family's Home card, `calc(88vw -
/// var(--sidebarWidth) - (var(--playQueueWidth) * .9))`) narrows it while the
/// queue is open. Tidalfast declared 0 always, so the open queue changed
/// nothing.
#[test]
fn a_box_sized_by_the_queues_width_narrows_while_the_queue_is_open() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Queue sized card"} */
._feed_412931c { background-color: #fff0; border-style: solid; border-width: 2px; border-color: rgb(255, 150, 240);
    min-width: calc(88vw - var(--sidebarWidth) - (var(--playQueueWidth)*.9)) !important;
    max-width: calc(88vw - var(--sidebarWidth) - (var(--playQueueWidth)*.9)) !important; }
"#;
    let width = |open: bool| {
        let mut frame = dom_anchors::sample_frame();
        frame.viewport = [2137.0, 1183.0];
        frame.sidebar.width = 240.0;
        frame.queue_sidebar = Some(open);
        frame.queue_panel = open.then_some((Some([1787.0, 60.0, 334.0, 900.0]), false));
        if let Some(p) = frame.page.as_mut() {
            p.feed = true;
        }
        let (luna, o) = overlay_with(
            "queue-card.css",
            css,
            dom_anchors::build(&frame),
            dom_anchors::keys::PAGE,
        );
        pd::test_overlay(Some((*o).clone()));
        pd::note(|f| f.viewport = [2137.0, 1183.0]);
        let w = pd::feed_card()
            .and_then(|c| c.width)
            .expect("the card's width");
        pd::test_overlay(None);
        luna.shutdown();
        w
    };
    let closed = width(false);
    assert!((closed - (0.88 * 2137.0 - 240.0)).abs() < 1.0, "{closed}");
    let open = width(true);
    assert!(
        (open - (0.88 * 2137.0 - 240.0 - 350.0 * 0.9)).abs() < 1.0,
        "{open}"
    );
}

/// The colours at the top-left and bottom-left corners of the header art's
/// textured mesh (as wide as the page), `None` when it is not drawn.
fn header_art_corners(
    shapes: &[egui::epaint::ClippedShape],
) -> Option<(egui::Color32, egui::Color32)> {
    fn meshes<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::Mesh>) {
        match shape {
            egui::Shape::Mesh(m) => out.push(m),
            egui::Shape::Vec(v) => v.iter().for_each(|s| meshes(s, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    for s in shapes {
        meshes(&s.shape, &mut all);
    }
    let mesh = all
        .into_iter()
        .filter(|m| m.texture_id != egui::TextureId::default() && !m.vertices.is_empty())
        .filter(|m| {
            let pos =
                egui::Rect::from_points(&m.vertices.iter().map(|v| v.pos).collect::<Vec<_>>());
            pos.width() >= 1000.0 && pos.height() >= 300.0
        })
        .max_by_key(|m| m.vertices.len())?;
    let pos = egui::Rect::from_points(&mesh.vertices.iter().map(|v| v.pos).collect::<Vec<_>>());
    let at = |p: egui::Pos2| mesh.vertices.iter().find(|v| v.pos == p).map(|v| v.color);
    Some((at(pos.left_top())?, at(pos.left_bottom())?))
}

/// A theme's `background-image: var(--img)` on TIDAL's header art box (Abyss
/// Neptune's `[class^="_dataContainer_"]::before`, with `filter: blur(10px)
/// brightness(0.4)`) is the cover alone: TIDAL's gradient goes, so no veil
/// darkens it towards the foot, and the brightness darkens the whole box to
/// 40 %. Tidalfast took a picture value without `url(` for no cover and
/// drew none.
#[test]
fn the_covers_own_variable_on_the_header_art_box_is_the_cover_alone() {
    use super::theme_round41_tests as r41;
    let corners = |css: &str| {
        let (ctx, mut app) = r41::demo_app("album:alb0", None);
        let plain = r41::drawn(
            &ctx,
            &mut app,
            DomOverlay {
                version: 1,
                ..DomOverlay::default()
            },
        );
        let mut o = r41::styled("own-art.css", css, &plain, dom_anchors::keys::PAGE_DATA);
        o.version = 2;
        crate::ui::hold_cover_backdrop_for_test(&mut app, &ctx);
        let s = super::theme_round56_tests::shapes(&ctx, &mut app, Some(o));
        app.backend.shutdown();
        header_art_corners(&s)
    };
    let css = "/* {\"name\": \"Own art\"} */ [class^=\"_dataContainer_\"]::before { background-image: var(--img); filter: brightness(0.4); } [class^=\"_dataContainer_\"] { scrollbar-width: none; }";
    assert_eq!(
        corners(css),
        Some((egui::Color32::from_gray(102), egui::Color32::from_gray(102))),
        "the cover at 40 % from top to foot"
    );
    let css = "/* {\"name\": \"Dim art\"} */ [class^=\"_dataContainer_\"]::before { filter: brightness(0.5); } [class^=\"_dataContainer_\"] { scrollbar-width: none; }";
    assert_eq!(
        corners(css),
        Some((egui::Color32::from_gray(51), egui::Color32::BLACK)),
        "TIDAL's veil, then half as bright"
    );
}

/// TIDAL 2.47.0 holds an album's header box and its controls in
/// `div._headerContainer_e0cf17f` (a playlist's `_headerContainer_2aeeef7`),
/// and the Cotton Candy Nights family nests its caption colour in it
/// (`[class^="_headerContainer_e0cf17f"] { [data-wave-color="textDefault"]
/// { color } }`): the header's Add and Share captions take it in TIDAL. The
/// box was not in Tidalfast's markup, so the theme's plain caption colour
/// stayed.
#[test]
fn the_header_container_holds_the_captions_a_theme_nests_in_it() {
    use dom_anchors::keys;
    let css = r#"/* {"name": "Nested caption"} */
[data-wave-color="textDefault"] { color: rgb(9, 9, 9) !important; }
[class^="_headerContainer_e0cf17f"] { [data-wave-color="textDefault"] { color: rgb(215, 217, 255) !important; } }
"#;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.data = Some([240.0, 92.0, 1040.0, 300.0]);
        page.actions = vec![dom_anchors::HeaderAction {
            test: "favorite-button".into(),
            label: "Add".into(),
            rect: Some([600.0, 220.0, 48.0, 48.0]),
            icon: Some([614.0, 224.0, 20.0, 20.0]),
            caption: Some([610.0, 248.0, 28.0, 16.0]),
            checked: Some(false),
        }];
    }
    let label = keys::page_action_label("favorite-button");
    let (luna, o) = overlay_with(
        "nested-caption.css",
        css,
        dom_anchors::build(&frame),
        &label,
    );
    luna.shutdown();
    let color = o.hosts.get(&label).and_then(|h| h.style.color.clone());
    assert_eq!(
        color.as_deref(),
        Some("rgb(215, 217, 255)"),
        "{:?}",
        o.hosts.get(&label)
    );
}

/// TIDAL 2.47.0 multiplies a playlist's header art by its market black
/// (`._dataContainer_9f47346::before { background-image: linear-gradient(
/// color-mix(in sRGB, var(--market-core-fill-black-color) 40%, transparent)
/// 0, var(--market-core-fill-black-color) 100%), var(--img, none) }`), so
/// Sunken Tidal's `rgb(30, 30, 30)` keeps a grey's worth of the cover down
/// its full-height box, where Tidalfast ended it in TIDAL's own black.
#[test]
fn a_playlists_header_art_takes_the_themes_market_black() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Grey black"} */
:root { --market-core-fill-black-color: rgb(30, 30, 30) !important; }
#main { background-color: rgb(1, 1, 1); }
"#;
    let (luna, o) = overlay_with(
        "grey-black.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::MAIN,
    );
    pd::test_overlay(Some((*o).clone()));
    let shade = pd::playlist_art_multiply();
    pd::test_overlay(None);
    luna.shutdown();
    // 60 % of the cover plus 40 % of it times 30/255 at the top; 30/255 at
    // the foot.
    assert_eq!(
        shade,
        Some((egui::Color32::from_gray(165), egui::Color32::from_gray(30)))
    );
    assert_eq!(
        pd::playlist_art_multiply(),
        None,
        "TIDAL's black without a theme"
    );
}

/// The box across of the header art's textured mesh (the cover laid on
/// the page's top, at least 1000 wide), `None` when it is not drawn.
fn header_art_span(shapes: &[egui::epaint::ClippedShape]) -> Option<(f32, f32)> {
    fn meshes<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::Mesh>) {
        match shape {
            egui::Shape::Mesh(m) => out.push(m),
            egui::Shape::Vec(v) => v.iter().for_each(|s| meshes(s, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    for s in shapes {
        meshes(&s.shape, &mut all);
    }
    all.into_iter()
        .filter(|m| m.texture_id != egui::TextureId::default() && !m.vertices.is_empty())
        .map(|m| egui::Rect::from_points(&m.vertices.iter().map(|v| v.pos).collect::<Vec<_>>()))
        .filter(|r| r.width() >= 1000.0 && r.height() >= 300.0)
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .map(|r| (r.left().round(), r.width().round()))
}

/// TIDAL's `#main` fills the content row's flex slot: the window less the
/// sidebar and the play queue wrapper's margin box. A theme that pulls the
/// wrapper left by more than its width (Lunallena's `margin-left:
/// calc(var(--playQueueWidth) * -1 - 480px)`, the wrapper reading TIDAL's
/// 350 with the queue open and 0 closed) widens `main` past the window:
/// 2377 in a 2137 window with a 240 sidebar, open or closed (captured
/// under the theme), and the header art box, `main`'s width, scales and
/// centres the cover on that. Tidalfast laid the box as wide as the
/// window.
#[test]
fn a_queue_wrapper_pulled_left_widens_the_header_art_box_past_the_window() {
    use super::theme_round41_tests as r41;
    let css = r#"/* {"name": "Wide main"} */
#main { min-width: 100vw; left: calc(var(--sidebarWidth) * -1); --playQueueWidth: 0px; }
[class*="playQueueWrapper"] { margin-left: calc(var(--playQueueWidth) * -1 - 480px); }
[class^="_dataContainer_"]::before { height: 100%; }
"#;
    let span = |show: Option<&str>| {
        let (ctx, mut app) = r41::demo_app("album:alb0", show);
        let plain = r41::drawn(
            &ctx,
            &mut app,
            DomOverlay {
                version: 1,
                ..DomOverlay::default()
            },
        );
        let mut o = r41::styled("wide-main.css", css, &plain, dom_anchors::keys::MAIN);
        o.version = 2;
        crate::ui::hold_cover_backdrop_for_test(&mut app, &ctx);
        let s = super::theme_round56_tests::shapes(&ctx, &mut app, Some(o));
        app.backend.shutdown();
        header_art_span(&s)
    };
    assert_eq!(span(Some("queue")), Some((0.0, 2377.0)), "queue open");
    assert_eq!(span(None), Some((0.0, 2377.0)), "queue closed");
}
