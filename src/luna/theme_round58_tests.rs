//! Round 58: what the parity checker showed against TIDAL 2.47.0's captures,
//! proved with made-up themes (their own colours, TIDAL's classes and
//! anchors only).

use super::dom_anchors;

/// TIDAL 2.47.0 declares its title bar's height once, `:root {
/// --os-title-bar-height: 30px }`, and a theme's rules read it: a page box
/// kept `calc(100svh - var(--os-title-bar-height))` high (Lunallena's and
/// Giorgeous' `#main`) is 30 short of the window. The token was unknown,
/// so the declaration did not resolve and the box's height was lost.
#[test]
fn tidals_title_bar_height_token_sizes_a_themes_page_box() {
    use dom_anchors::keys::MAIN;
    let css = "/* {\"name\": \"Title bar token\"} */ #main { max-height: calc(100svh - var(--os-title-bar-height)); outline-color: rgb(1, 2, 3); }";
    let o = super::theme_round38_tests::overlay(
        "title-bar-token.css",
        css,
        &dom_anchors::sample_frame(),
        MAIN,
    );
    let max = o.hosts[MAIN].style.max_height.clone();
    assert_eq!(max.as_deref().map(str::trim), Some("calc(100svh - 30px)"));
}

/// The box (top and height) of the header art's textured mesh (the cover
/// laid on the page's top, at least 1000 wide), `None` when it is not drawn.
fn header_art_down(shapes: &[egui::epaint::ClippedShape]) -> Option<(f32, f32)> {
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
        .map(|r| (r.top().round(), r.height().round()))
}

/// The header art box is TIDAL's `#main`'s, down as across: below its top
/// margin and as high as the theme makes it. Lunallena macOS's `#main {
/// height: 100svh; max-height: 100svh; margin-top: 100px }` puts TIDAL's
/// box 100 lower and the window's height high, 1183 (captured: 130 from
/// the window's top, under the 30-point title bar), where Tidalfast laid it
/// at the page's top as high as the page. Its regular build's `max-height:
/// calc(100svh - var(--os-title-bar-height))` keeps it 30 shorter, the
/// height the page has.
#[test]
fn the_header_art_box_is_the_page_boxs_own_down() {
    use super::dom::DomOverlay;
    use super::theme_round41_tests as r41;
    let down = |css: Option<&str>| {
        let (ctx, mut app) = r41::demo_app("album:alb0", None);
        let plain = r41::drawn(
            &ctx,
            &mut app,
            DomOverlay {
                version: 1,
                ..DomOverlay::default()
            },
        );
        let o = css.map(|css| {
            let mut o = r41::styled("main-down.css", css, &plain, dom_anchors::keys::MAIN);
            o.version = 2;
            o
        });
        crate::ui::hold_cover_backdrop_for_test(&mut app, &ctx);
        let s = super::theme_round56_tests::shapes(&ctx, &mut app, o);
        app.backend.shutdown();
        header_art_down(&s).expect("the header art is drawn")
    };
    let (top, _) = down(None);
    let tall = r#"/* {"name": "Main down"} */
#main { height: 100svh; max-height: 100svh; margin-top: 100px; }
[class^="_dataContainer_"]::before { height: 100%; }
"#;
    let (themed, height) = down(Some(tall));
    assert_eq!(themed - top, 100.0, "below main's top margin");
    // The demo's window is 1183 below TIDAL's 30-point title bar.
    assert_eq!(height, 1213.0, "TIDAL's whole window");
    let kept = r#"/* {"name": "Main kept"} */
#main { height: 100svh; max-height: calc(100svh - var(--os-title-bar-height)); margin-top: 0; }
[class^="_dataContainer_"]::before { height: 100%; }
"#;
    assert_eq!(down(Some(kept)), (top, 1183.0), "the page's height");
}

/// A theme's own `blur()` on a playlist's header art box other than
/// TIDAL's 50 (Abyss Neptune's `[class^="_dataContainer_"]::before {
/// background-image: var(--img); filter: blur(10px) brightness(0.4) }`)
/// blurs the cover by that much: TIDAL's box shows the playlist's picture
/// soft but whole, where Tidalfast laid its 50-point wash, a smear. The
/// cover is now blurred at the size the box shows it, as an album's is.
#[test]
fn a_themes_blur_on_a_playlists_header_art_blurs_the_cover_by_that_much() {
    use super::dom::DomOverlay;
    use super::theme_round41_tests as r41;
    let css = "/* {\"name\": \"Soft list art\"} */ [class^=\"_dataContainer_\"]::before { background-image: var(--img); filter: blur(10px) brightness(0.4); } [class^=\"_dataContainer_\"] { scrollbar-width: none; }";
    let (ctx, mut app) = r41::demo_app("playlist:pl0", None);
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = r41::styled(
        "soft-list-art.css",
        css,
        &plain,
        dom_anchors::keys::PAGE_DATA,
    );
    o.version = 2;
    crate::ui::hold_cover_backdrop_for_test(&mut app, &ctx);
    let _ = super::theme_round56_tests::shapes(&ctx, &mut app, Some(o.clone()));
    let held = app.softened_covers.hold_pending_blurs_for_test(&ctx);
    let s = super::theme_round56_tests::shapes(&ctx, &mut app, Some(o));
    assert_eq!(held.len(), 1, "one blurred copy asked for: {held:?}");
    let key = &held[0];
    let texture = app
        .softened_covers
        .held_texture_for_test(key)
        .expect("the copy is held");
    app.backend.shutdown();
    let sigma: f32 = key
        .strip_prefix("blur:")
        .and_then(|k| k.split_once(':'))
        .and_then(|(s, _)| s.parse().ok())
        .expect("the sigma in the key");
    assert!((1.0..=2.5).contains(&sigma), "{key}");
    fn meshes<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::Mesh>) {
        match shape {
            egui::Shape::Mesh(m) => out.push(m),
            egui::Shape::Vec(v) => v.iter().for_each(|s| meshes(s, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    for c in &s {
        meshes(&c.shape, &mut all);
    }
    assert!(
        all.iter().any(|m| m.texture_id == texture),
        "the blurred copy is drawn in the header art box"
    );
}

/// Without a height of the theme's, TIDAL's `#main` is as high as the
/// content row, down to the window's foot under a docked player bar
/// (`_scrollSlot_ _hasDockedPlayer`, `main` 1153 high in a 1183 window
/// with its 30-point title bar): Sunken Tidal's `[class*="dataContainer"]
/// :before { height: 100% }` box is that high (captured). Tidalfast took
/// the room above the bar, about 90 short.
#[test]
fn a_full_height_header_art_box_reaches_under_a_docked_player_bar() {
    use super::dom::DomOverlay;
    use super::theme_round41_tests as r41;
    let css = r#"/* {"name": "Full art"} */
[class*="dataContainer"]::before { height: 100% !important; }
[class^="_dataContainer_"] { scrollbar-width: none; }
"#;
    let (ctx, mut app) = r41::demo_app("album:alb0", None);
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = r41::styled("full-art.css", css, &plain, dom_anchors::keys::PAGE_DATA);
    o.version = 2;
    crate::ui::hold_cover_backdrop_for_test(&mut app, &ctx);
    let s = super::theme_round56_tests::shapes(&ctx, &mut app, Some(o));
    app.backend.shutdown();
    let (top, height) = header_art_down(&s).expect("the header art is drawn");
    assert_eq!(
        top + height,
        1183.0,
        "down to the window's foot: {top} + {height}"
    );
}
