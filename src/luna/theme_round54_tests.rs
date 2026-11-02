//! Round 54: what the parity checker and TIDAL 2.47.0's captured pages
//! showed after round 53. Proved with made-up themes (their own colours,
//! TIDAL's classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

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
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && ready(&o) {
            break o;
        }
        assert!(Instant::now() < end, "{name} never took");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    o
}

/// The overlay over `frame` with no theme installed, its anchors synced.
fn plain_overlay(name: &str, frame: &dom_anchors::Frame) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
    let data = temp_dir(name);
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    for _ in 0..5 {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(30));
    }
    let o = luna.dom_overlay();
    luna.shutdown();
    o
}

/// The maintainer's decision: TIDAL's frosted glass under the search pill
/// (`backdrop-filter: var(--frosted-glass-nav-blur)` in TIDAL's own rule)
/// is drawn under a theme, and with no theme the pill stays Tidalfast's
/// own, no filter at all. A theme's token and a theme's `none` are its own.
#[test]
fn the_search_pills_blur_comes_with_a_theme_alone() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::SEARCH_FIELD;
    let _drawn = backdrop::force(true);
    let frame = dom_anchors::sample_frame();
    let filter = |o: &DomOverlay| {
        pd::test_overlay(Some(o.clone()));
        let got = pd::anchor_look(SEARCH_FIELD).and_then(|l| l.backdrop.clone());
        pd::test_overlay(None);
        got
    };
    let any = |o: &DomOverlay| o.hosts.contains_key(dom_anchors::keys::FOOTER);
    let o = overlay_when(
        "pill-plain.css",
        "/* {\"name\": \"Pill plain\"} */\n._player_8c80ae6 { color: rgb(1, 2, 3); }\n",
        &frame,
        any,
    );
    assert_eq!(
        filter(&o).as_deref(),
        backdrop::Backdrop::parse("blur(20px) saturate(180%)").as_ref(),
    );
    let o = overlay_when(
        "pill-token.css",
        "/* {\"name\": \"Pill token\"} */\n:root { --frosted-glass-nav-blur: blur(9px); }\n._player_8c80ae6 { color: rgb(1, 2, 3); }\n",
        &frame,
        any,
    );
    assert_eq!(
        filter(&o).as_deref(),
        backdrop::Backdrop::parse("blur(9px)").as_ref(),
    );
    let o = overlay_when(
        "pill-none.css",
        "/* {\"name\": \"Pill none\"} */\n[data-type=\"search-field\"] { backdrop-filter: none !important; }\n._player_8c80ae6 { color: rgb(1, 2, 3); }\n",
        &frame,
        any,
    );
    assert_eq!(filter(&o), None);
    let o = plain_overlay("pill-no-theme", &frame);
    assert_eq!(filter(&o), None, "no theme, no filter");
}

/// The pill's filter is painted once, and under its fill: a paint callback
/// ahead of the fill, as the bar's and the queue's are, with the pill's own
/// fill and with a fill of the theme's.
#[test]
fn the_search_pills_blur_is_painted_under_its_fill() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    let _drawn = backdrop::force(true);
    let frame = dom_anchors::sample_frame();
    for (name, rules) in [
        ("pill-painted.css", ""),
        (
            "pill-filled.css",
            ".searchFieldWithinSearchFieldWithPopover { background-color: rgba(1, 2, 3, 0.5); }",
        ),
    ] {
        let css = format!(
            "/* {{\"name\": \"{name}\"}} */ ._player_8c80ae6 {{ color: rgb(1, 2, 3); }} {rules}"
        );
        let o = overlay_when(name, &css, &frame, |o| {
            o.hosts.contains_key(dom_anchors::keys::FOOTER)
        });
        pd::test_overlay(Some((*o).clone()));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let r = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(280.0, 36.0));
            let fill = egui::Color32::from_rgba_unmultiplied(9, 9, 9, 191);
            pd::paint_search_pill(ui, r, 18.0, fill);
        });
        out.textures_delta.clear();
        pd::test_overlay(None);
        let order: Vec<bool> = out
            .shapes
            .iter()
            .map(|c| matches!(c.shape, egui::epaint::Shape::Callback(_)))
            .collect();
        assert_eq!(order.first(), Some(&true), "{name}: {order:?}");
        assert_eq!(order.iter().filter(|c| **c).count(), 1, "{name}: {order:?}");
        assert!(order.len() > 1, "{name}: {order:?}");
    }
}

/// A theme that keeps TIDAL's bar glass over its own bar colour: the glass
/// filters that colour (`blur(20px) saturate(180%)`) before its `#282828bf`
/// goes over it, so a blue bar stays bluer than the plain mix. The filter is
/// painted between the box's own look and the glass, once.
#[test]
fn tidals_bar_glass_filters_the_bars_own_colour() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::FOOTER;
    let _drawn = backdrop::force(true);
    let css =
        "/* {\"name\": \"Glass kept\"} */ ._player_8c80ae6 { background-color: rgb(0, 80, 200); }";
    let o = overlay_when("glass-kept.css", css, &dom_anchors::sample_frame(), |o| {
        o.hosts.contains_key(FOOTER)
    });
    pd::test_overlay(Some((*o).clone()));
    assert!(pd::footer_glass());
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 500.0), egui::vec2(800.0, 96.0));
        pd::paint_host_layers_under_glass(ui, FOOTER, r, pd::TIDAL_BAR_GLASS);
    });
    out.textures_delta.clear();
    pd::test_overlay(None);
    let order: Vec<&str> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Callback(_) => Some("filter"),
            egui::Shape::Rect(r) if r.fill == pd::TIDAL_BAR_GLASS => Some("glass"),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["filter", "glass"]);
}
