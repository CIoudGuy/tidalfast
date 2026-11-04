//! Round 56: what the parity checker showed once its replay lays the demo's
//! own cover in TIDAL's header art box (`--img`), so the box compares.

use super::dom::DomOverlay;
use super::dom_anchors;
use super::theme_round41_tests as r41;
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
