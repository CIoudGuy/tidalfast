//! Round 50: what the parity checker's comparisons found. Proved with
//! made-up themes (their own values, TIDAL's classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::*;
use crate::ui::plugin_dom as pd;

/// Installs `css` as a theme over `tree` and returns the overlay once the
/// theme reached it and gave `key` a colour.
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
        if o.css_bytes > 0 && o.hosts.get(key).is_some_and(|h| h.style.color.is_some()) {
            break o;
        }
        assert!(Instant::now() < end, "{key:?} was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    (luna, o)
}

/// The alpha of each dot (a filled circle) `key`'s box layers paint at
/// `time` seconds, with motion on or `reduced`.
fn dots_at(o: &DomOverlay, key: &str, times: &[f64], reduced: bool) -> Vec<Vec<u8>> {
    let ctx = egui::Context::default();
    crate::motion::apply(&ctx, reduced);
    pd::test_overlay(None);
    pd::test_overlay(Some(o.clone()));
    let mut seen = Vec::new();
    for &time in times {
        let mut out = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 300.0),
                )),
                ..Default::default()
            },
            |ui| {
                let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
                pd::paint_host_layers(ui, key, rect);
            },
        );
        out.textures_delta.clear();
        let mut alphas = Vec::new();
        for s in &out.shapes {
            if let egui::Shape::Circle(c) = &s.shape {
                alphas.push(c.fill.a());
            }
        }
        seen.push(alphas);
    }
    pd::test_overlay(None);
    seen
}

/// A custom property `@property` registers with a type and an
/// `initial-value` has that value where no rule sets it, and an animation
/// eases it between its keyframes (Cotton Candy Nights' and Astronaut Ice
/// Cream's stars: `hsla(56, 100%, 90%, var(--star1))` with `--star1`
/// registered at `0%` and `@keyframes blink` taking it from 5 % to 100 %
/// and back over ten seconds). Tidalfast read the unset variable as
/// nothing, so the stars were opaque and never twinkled; TIDAL with the
/// animation stopped (the parity checker) shows none.
#[test]
fn a_registered_property_starts_at_its_initial_value_and_its_animation_eases_it() {
    use dom_anchors::keys::MAIN_CONTAINER;
    let css = "/* {\"name\": \"Stars\"} */
@property --star1 { syntax: '<percentage>'; initial-value: 0% ; inherits: false; }
@keyframes blink { 0% { --star1: 5%; } 50% { --star1: 100%; } 100% { --star1: 5%; } }
._mainContainer_6519c30 {
  color: rgb(1, 2, 3);
  animation: blink 10s linear infinite;
  background-image: radial-gradient(circle at 50% 50%, hsla(56, 100%, 90%, var(--star1)) 3px, transparent 0);
}
";
    let (luna, o) = overlay_with("stars.css", css, dom_anchors::sample_tree(), MAIN_CONTAINER);
    luna.shutdown();
    assert_eq!(
        o.vars.get("@property --star1").map(String::as_str),
        Some("0%"),
        "the registration reaches the painter"
    );
    // With the animation stopped (as the checker settles TIDAL's page) the
    // initial value: no dot shows.
    let still = pd::box_layers(&o.hosts[MAIN_CONTAINER], &o.vars, [400.0, 300.0]);
    let alphas: Vec<u8> = still
        .iter()
        .filter_map(|l| match l {
            pd::BoxLayer::Paint(pd::PageLayer::Dot { color, .. }) => Some(color.a()),
            _ => None,
        })
        .collect();
    assert_eq!(alphas, vec![0], "{still:?}");
    // Running: 5 % at the start, 52.5 % a quarter in, 100 % half way, and
    // a quarter into the next turn on the way back.
    let seen = dots_at(&o, MAIN_CONTAINER, &[10.0, 12.5, 15.0, 22.5], false);
    let want = [0.05, 0.525, 1.0, 0.525].map(|a: f32| (a * 255.0).round() as i32);
    assert_eq!(seen.len(), 4);
    for (got, want) in seen.iter().zip(want) {
        assert!(
            matches!(got.as_slice(), [a] if (i32::from(*a) - want).abs() <= 2),
            "{seen:?}, want {want}"
        );
    }
    // Less motion holds the start.
    let held = dots_at(&o, MAIN_CONTAINER, &[1.0, 6.0], true);
    assert!(
        held.iter()
            .all(|a| matches!(a.as_slice(), [a] if (i32::from(*a) - 13).abs() <= 1)),
        "{held:?}"
    );
}

/// TIDAL 2.47.0 colours its library rows and My Collection's links by
/// class (`._contentItem_684cbbe { color: var(--market-core-text-20-color)
/// }`, `._submenuItem_3d0e609`, the open one's `_selected_` / `_active_`),
/// so Cotton Candy Nights' `a { color }` leaves them grey there while it
/// colours other links. Tidalfast gave them the theme's colour. A rule as
/// heavy as TIDAL's still colours them.
#[test]
fn a_lighter_link_rule_leaves_library_rows_and_collection_links_tidals_colour() {
    use dom_anchors::keys::{MAIN_CONTAINER, lib_row, sidebar_item};
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.rows = vec![
        (Some([16.0, 400.0, 208.0, 56.0]), false),
        (Some([16.0, 456.0, 208.0, 56.0]), true),
    ];
    frame.sidebar.items.push(dom_anchors::SidebarItem {
        test: "sidebar-collection-menu".into(),
        label: "Collection".into(),
        href: Some("/my-collection".into()),
        selected: false,
        rect: Some([16.0, 300.0, 208.0, 40.0]),
    });
    frame.sidebar.collection = None;
    frame.sidebar.submenu = ["albums", "tracks"]
        .iter()
        .enumerate()
        .map(|(i, page)| dom_anchors::SubmenuItem {
            test: format!("sidebar-collection-{page}"),
            label: page.to_string(),
            href: format!("/my-collection/{page}"),
            active: i == 0,
            rect: Some([16.0, 600.0 + 40.0 * i as f32, 208.0, 40.0]),
        })
        .collect();
    let tree = dom_anchors::build(&frame);
    let colours = |name: &str, css: &str| {
        let (luna, o) = overlay_with(name, css, tree.clone(), MAIN_CONTAINER);
        luna.shutdown();
        let keys = [
            lib_row(0),
            lib_row(1),
            sidebar_item("sidebar-collection-albums"),
            sidebar_item("sidebar-collection-tracks"),
        ];
        keys.map(|k| o.hosts.get(&k).and_then(|h| h.style.color.clone()))
    };
    let light = colours(
        "light-links.css",
        "/* {\"name\": \"Light links\"} */
._mainContainer_6519c30 { color: rgb(1, 2, 3); }
a { color: rgb(241, 232, 255); }
",
    );
    assert_eq!(light, [None, None, None, None]);
    let heavy = colours(
        "heavy-links.css",
        "/* {\"name\": \"Heavy links\"} */
._mainContainer_6519c30 { color: rgb(1, 2, 3); }
._contentItem_684cbbe, ._submenuItem_3d0e609 { color: rgb(241, 232, 255); }
",
    );
    // The open row and page keep TIDAL's own heavier rule.
    let want = Some("rgb(241, 232, 255)".to_string());
    assert_eq!(heavy, [want.clone(), None, None, want]);
}
