//! Round 40: page backgrounds painted as Chromium paints them, and what the
//! parity checker's pixel comparison found next. Proved with made-up themes
//! (their own values, TIDAL's classes and anchors only); the colours
//! expected are Chromium's, read from its screenshot of the same box.

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

fn overlay(name: &str, css: &str, frame: &dom_anchors::Frame, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
    let luna = themed_luna(name, css);
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(key) {
            break o;
        }
        assert!(Instant::now() < end, "the overlay never got there");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    o
}

/// What the theme paints on the anchor `key`'s box over `rect` (its
/// `background-image` layers), over black, at the centres of the pixels
/// `pixels`.
fn painted(o: &DomOverlay, key: &str, rect: egui::Rect, pixels: &[[f32; 2]]) -> Vec<[u8; 3]> {
    use crate::ui::plugin_dom as pd;
    pd::test_overlay(Some(o.clone()));
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(rect),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| pd::paint_host_layers(ui, key, rect));
    pd::test_overlay(None);
    out.textures_delta.clear();
    pixels
        .iter()
        .map(|[x, y]| {
            let c = pd::test_colour_at(
                &out.shapes,
                egui::pos2(x + 0.5, y + 0.5),
                egui::Color32::BLACK,
            );
            [c.r(), c.g(), c.b()]
        })
        .collect()
}

/// Each colour within `tolerance` levels of Chromium's.
fn assert_near(got: &[[u8; 3]], chromium: &[[u8; 3]], tolerance: u8) {
    let near = got
        .iter()
        .zip(chromium)
        .all(|(g, c)| g.iter().zip(c).all(|(a, b)| a.abs_diff(*b) <= tolerance));
    assert!(near, "Tidalfast {got:?}, Chromium {chromium:?}");
}

/// `radial-gradient(ellipse at center right, ...)` is centred on the middle
/// of the right edge: CSS reads two position keywords in either order
/// (Chromium's computed value is `at 100% 50%`). Tidalfast took the first
/// word for the x keyword and centred the glow on the box: the cotton
/// candy family and Sugarcoat stack three such layers on the main
/// container, so their page backgrounds had a glow in the middle and none
/// at the sides.
#[test]
fn a_radial_gradient_at_center_right_sits_on_the_right_edge() {
    let css = r#"/* {"name": "Side glows"} */
._mainContainer_6519c30 {
  background-image: radial-gradient(ellipse at center right, rgb(255, 0, 0), transparent 50%),
    radial-gradient(circle at center left, rgb(0, 0, 255), transparent 30%);
}"#;
    let key = dom_anchors::keys::MAIN_CONTAINER;
    let o = overlay("side-glows.css", css, &dom_anchors::sample_frame(), key);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 500.0));
    let pixels = [
        [990.0, 250.0],
        [700.0, 250.0],
        [500.0, 250.0],
        [10.0, 250.0],
        [990.0, 10.0],
    ];
    let chromium = [[251, 0, 0], [147, 0, 0], [74, 0, 0], [0, 0, 246], [0, 0, 0]];
    assert_near(&painted(&o, key, rect, &pixels), &chromium, 3);
}

/// A box inside the sidebar's wrapper paints its fill over the wrapper's,
/// as CSS paints a child over its parent: Sugarcoat lays a 60 % glass on
/// the wrapper and fills `aside#sidebar` in it with an opaque pink, and
/// TIDAL's sidebar is that pink. Tidalfast took the wrapper's fill alone
/// whenever the wrapper had one, and drew the paler glass.
#[test]
fn the_sidebars_aside_paints_over_the_wrappers_fill() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{SIDEBAR, SIDEBAR_NAV};
    let surface = |name: &str, css: &str| {
        let o = overlay(name, css, &dom_anchors::sample_frame(), SIDEBAR_NAV);
        pd::test_overlay(Some((*o).clone()));
        let c = pd::surface(SIDEBAR, egui::Color32::BLACK);
        pd::test_overlay(None);
        c
    };
    let opaque = surface(
        "glass-and-pink.css",
        r#"/* {"name": "Glass and pink"} */
._sidebarWrapper_2f256ce { background-color: rgba(255, 255, 255, 0.6); }
#sidebar { background-color: rgb(255, 144, 186); }
"#,
    );
    assert_eq!(opaque, egui::Color32::from_rgb(255, 144, 186));
    // Two see-through fills add up: half red over half blue.
    let both = surface(
        "half-and-half.css",
        r#"/* {"name": "Half and half"} */
._sidebarWrapper_2f256ce { background-color: rgba(0, 0, 255, 0.5); }
#sidebar { background-color: rgba(255, 0, 0, 0.5); }
"#,
    );
    let chromium = egui::Color32::from_rgba_premultiplied(128, 0, 64, 191);
    let near = both
        .to_array()
        .iter()
        .zip(chromium.to_array())
        .all(|(a, b)| a.abs_diff(b) <= 1);
    assert!(near, "Tidalfast {both:?}, Chromium {chromium:?}");
}

/// TIDAL's bar glass is the bar's `::after` (TIDAL 2.46.0: `background:
/// #282828bf`); its `::before` is the shadow above the bar. A theme that
/// hides the `::before` keeps the glass: Sunken Tidal's rgb(10, 10, 10)
/// bar shows rgb(31, 31, 31) in TIDAL, Lunallena's (which draws a line in
/// it) rgb(38, 38, 38). Tidalfast took any styled `::before` for the glass
/// and drew the themes' own colours, rgb(10, 10, 10) and rgb(27, 27, 27).
#[test]
fn a_bar_whose_before_a_theme_hides_keeps_tidals_glass() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Sunk bar"} */
#footerPlayer { background-color: rgb(10, 10, 10); }
[data-test="footer-player"]::before { display: none !important; }
"#;
    let o = overlay(
        "sunk-bar.css",
        css,
        &dom_anchors::sample_frame(),
        dom_anchors::keys::FOOTER,
    );
    pd::test_overlay(Some((*o).clone()));
    let bar = pd::footer_under_glass(egui::Color32::from_gray(10), egui::Color32::BLACK);
    pd::test_overlay(None);
    // TIDAL's glass over the theme's grey 10: 30 + 10 / 4.
    assert!(bar.r().abs_diff(32) <= 1, "{bar:?}");
}
