//! Round 56: what the parity checker showed once its replay lays the demo's
//! own cover in TIDAL's header art box (`--img`), so the box compares.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::theme_round41_tests as r41;
use super::*;
use crate::ui::plugin_dom as pd;

/// The page's shapes in a 2137 x 1183 window under `overlay` (none: Luna's
/// look left out), after the frames a page needs to settle.
fn shapes(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    overlay: Option<DomOverlay>,
) -> Vec<egui::epaint::ClippedShape> {
    pd::test_overlay(None);
    pd::test_overlay(overlay);
    let mut shapes = Vec::new();
    for _ in 0..4 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2137.0, 1183.0),
                )),
                ..Default::default()
            },
            |ui| {
                app.refresh_frame_now();
                pd::test_viewport([2137.0, 1183.0]);
                pd::test_page_frame_for(&app.page().clone());
                crate::ui::show(app, ui);
                pd::end_frame(ui.ctx());
                app.clear_frame_now();
            },
        );
        output.textures_delta.clear();
        shapes = output.shapes;
    }
    pd::test_overlay(None);
    shapes
}

/// The header art's textured mesh, as wide as the page: its box and the
/// part of the cover it shows (its texture coordinates).
fn header_art(shapes: &[egui::epaint::ClippedShape]) -> Option<(egui::Rect, egui::Rect)> {
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
        .map(|m| {
            let pos =
                egui::Rect::from_points(&m.vertices.iter().map(|v| v.pos).collect::<Vec<_>>());
            let uv = egui::Rect::from_points(&m.vertices.iter().map(|v| v.uv).collect::<Vec<_>>());
            (pos, uv)
        })
        .filter(|(pos, _)| pos.width() >= 1000.0 && pos.height() >= 300.0)
        .max_by(|a, b| a.0.area().total_cmp(&b.0.area()))
}

/// TIDAL 2.47.0's playlist header art box sets no `background-position`
/// (`._dataContainer_9f47346::before { background-size: cover }`), so the
/// cover shows from its top, where an album's box (`50% center`) shows its
/// middle: under the replay's own cover, TIDAL's playlist header was the
/// sky of the demo's landscape and Tidalfast's the trees below it.
#[test]
fn a_playlists_header_art_shows_the_covers_top() {
    let shown = |page: &str| {
        let (ctx, mut app) = r41::demo_app(page, None);
        let _ = shapes(&ctx, &mut app, None);
        crate::ui::hold_cover_backdrop_for_test(&mut app, &ctx);
        let s = shapes(&ctx, &mut app, None);
        app.backend.shutdown();
        header_art(&s).unwrap_or_else(|| panic!("{page}: the header art is drawn"))
    };
    let (pos, uv) = shown("playlist:pl0");
    assert!(
        uv.min.y.abs() < 1e-3,
        "the cover's top at the box's top: {pos:?} {uv:?}"
    );
    assert!(uv.height() < 0.5, "cut to cover the box: {uv:?}");
    let (_, uv) = shown("album:alb0");
    assert!(
        (uv.center().y - 0.5).abs() < 1e-3,
        "an album's shows its middle: {uv:?}"
    );
}

/// Installs `css` as a theme over `frame`'s anchors and returns the overlay
/// once `ready` holds for it.
fn overlay_when(
    name: &str,
    css: &str,
    frame: &dom_anchors::Frame,
    ready: impl Fn(&DomOverlay) -> bool,
) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
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
    loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && ready(&o) {
            return o;
        }
        assert!(Instant::now() < end, "{name} never took");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// TIDAL 2.47.0's market corner tokens (`[data-market-theme] {
/// --market-core-radius-100: 6px }`, ...) are there for a theme's rules:
/// Lunallena's `button[aria-label="Play queue"]`, Giorgeous' and Sunken
/// Tidal's `[data-type="search-field"]` are rounded 6 by
/// `var(--market-core-radius-100)`. The token was unknown, so the corner
/// was dropped and TIDAL's own stayed.
#[test]
fn a_market_corner_token_rounds_a_themes_box() {
    use dom_anchors::keys::FOOTER;
    let frame = dom_anchors::sample_frame();
    let css = "/* {\"name\": \"Market corners\"} */ ._player_8c80ae6 { border-radius: var(--market-core-radius-100); outline-color: rgb(1, 2, 3); }";
    let o = overlay_when("market-corners.css", css, &frame, |o| {
        o.hosts.contains_key(FOOTER)
    });
    let radius = o.hosts[FOOTER].style.border_radius.clone();
    assert_eq!(radius.as_deref().map(str::trim), Some("6px"));
}

/// A caption's colour token: TIDAL 2.47.0 colours `[data-wave-color=
/// "textDefault"]` by `--wave-color-text-main`, so a theme's text token reaches a
/// playlist's Share caption, which would otherwise take its button's
/// colour (the Cotton Candy Nights family). A theme without the token
/// leaves the caption to Tidalfast.
#[test]
fn a_themes_text_token_colours_a_caption() {
    use super::theme_round38_tests::page_frame;
    let label = dom_anchors::keys::page_action_label("share-button");
    let caption = |name: &str, css: &str| {
        let o = overlay_when(name, css, &page_frame("playlist"), |o| {
            o.hosts.contains_key(dom_anchors::keys::FOOTER)
        });
        pd::test_overlay(Some((*o).clone()));
        let text = pd::anchor_look(&label).and_then(|l| l.text);
        pd::test_overlay(None);
        text
    };
    let with = caption(
        "text-token.css",
        "/* {\"name\": \"Text token\"} */ :root { --wave-color-text-main: rgb(9, 8, 7); } button { color: rgb(200, 201, 202); } ._player_8c80ae6 { color: rgb(1, 2, 3); }",
    );
    assert_eq!(with, Some(egui::Color32::from_rgb(9, 8, 7)));
    let without = caption(
        "no-text-token.css",
        "/* {\"name\": \"No text token\"} */ ._player_8c80ae6 { color: rgb(1, 2, 3); }",
    );
    assert_eq!(without, None);
}

/// TIDAL 2.47.0 multiplies an album's header art by its base tokens
/// (`._dataContainer_66f4f40::before { background-image: linear-gradient(
/// var(--wave-color-opacity-base-fill-thick) 0, var(--wave-color-solid-
/// base-fill) 100%), var(--img); background-blend-mode: multiply }`): a
/// theme's navy base (the Catppuccin flavours') tints the cover from its top
/// to its foot, where Tidalfast laid TIDAL's own black over it (the
/// checker's replay, under the demo's cover). With no theme, black as before.
#[test]
fn an_albums_header_art_takes_the_themes_base_tokens() {
    let corners = |css: Option<&str>| {
        let (ctx, mut app) = r41::demo_app("album:alb0", None);
        let plain = r41::drawn(
            &ctx,
            &mut app,
            DomOverlay {
                version: 1,
                ..DomOverlay::default()
            },
        );
        let overlay = css.map(|css| {
            let mut o = r41::styled("navy-base.css", css, &plain, dom_anchors::keys::MAIN);
            o.version = 2;
            o
        });
        crate::ui::hold_cover_backdrop_for_test(&mut app, &ctx);
        let s = shapes(&ctx, &mut app, overlay);
        app.backend.shutdown();
        let (pos, _) = header_art(&s).expect("the header art is drawn");
        let mut colours = (None, None);
        for c in &s {
            if let egui::Shape::Mesh(m) = &c.shape {
                for v in &m.vertices {
                    if v.pos == pos.left_top() {
                        colours.0 = Some(v.color);
                    }
                    if v.pos == pos.left_bottom() {
                        colours.1 = Some(v.color);
                    }
                }
            }
        }
        colours
    };
    let css = "/* {\"name\": \"Navy base\"} */ :root { --wave-color-opacity-base-fill-thick: rgba(30, 30, 46, 0.6); --wave-color-solid-base-fill: rgb(30, 30, 46); } #main { background-color: rgb(0, 0, 0); }";
    assert_eq!(
        corners(Some(css)),
        (
            Some(egui::Color32::from_rgb(120, 120, 130)),
            Some(egui::Color32::from_rgb(30, 30, 46))
        )
    );
    assert_eq!(
        corners(None),
        (
            Some(egui::Color32::from_gray(101)),
            Some(egui::Color32::BLACK)
        )
    );
}

/// A theme's right border on TIDAL's sidebar wrapper (Abyss Neptune's
/// `[data-test="main-layout-sidebar-wrapper"] { border-right: 1px solid }`)
/// lies inside the wrapper's 240 and narrows the navigation in it, whose
/// edge is TIDAL's divider: the divider stands one point left, beside the
/// border (the checker's replay: divider at 238, border at 239). Tidalfast
/// drew both in the last column.
#[test]
fn a_border_on_the_sidebar_wrapper_moves_the_divider_in() {
    let divider = |css: Option<&str>| {
        let (ctx, mut app) = r41::demo_app("album:alb0", None);
        let plain = r41::drawn(
            &ctx,
            &mut app,
            DomOverlay {
                version: 1,
                ..DomOverlay::default()
            },
        );
        let overlay = css.map(|css| {
            let mut o = r41::styled("wrapper-edge.css", css, &plain, dom_anchors::keys::SIDEBAR);
            o.version = 2;
            o
        });
        let s = shapes(&ctx, &mut app, overlay);
        app.backend.shutdown();
        let hairline = egui::Color32::from_white_alpha(20);
        s.iter()
            .find_map(|c| match &c.shape {
                egui::Shape::LineSegment { points, stroke }
                    if points[0].x == points[1].x
                        && (points[1].y - points[0].y).abs() > 500.0
                        && stroke.color == hairline
                        && points[0].x < 400.0 =>
                {
                    Some(points[0].x)
                }
                _ => None,
            })
            .expect("the sidebar's divider")
    };
    let plain = divider(None);
    let css = "/* {\"name\": \"Wrapper edge\"} */ [data-test=\"main-layout-sidebar-wrapper\"] { border-right: rgb(25, 25, 25) 1px solid; }";
    assert_eq!(divider(Some(css)), plain - 1.0);
}

/// A theme's own picture layers on TIDAL's header art box take the place of
/// TIDAL's gradient and cover (`background-image: linear-gradient(...),
/// var(--img)`): a gradient alone (Cotton Candy Nights Single's clear one on
/// `div._dataContainer_66f4f40::before`) leaves no cover, and TIDAL shows the
/// page's black there. Tidalfast still laid the cover.
#[test]
fn a_themes_gradient_on_the_header_art_box_leaves_no_cover() {
    let (ctx, mut app) = r41::demo_app("album:alb0", None);
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let css = "/* {\"name\": \"Clear art\"} */ div._dataContainer_66f4f40::before { background-image: linear-gradient(0deg, rgba(1, 2, 3, 0), rgba(1, 2, 3, 0)); } div._dataContainer_66f4f40 { scrollbar-width: none; }";
    let mut o = r41::styled("clear-art.css", css, &plain, dom_anchors::keys::PAGE_DATA);
    o.version = 2;
    crate::ui::hold_cover_backdrop_for_test(&mut app, &ctx);
    let s = shapes(&ctx, &mut app, Some(o));
    app.backend.shutdown();
    assert!(
        header_art(&s).is_none(),
        "no cover under the theme's gradient"
    );
}
