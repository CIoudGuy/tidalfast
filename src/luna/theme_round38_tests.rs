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
