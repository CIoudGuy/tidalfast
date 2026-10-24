//! Round 36: differences the parity checker found between TIDAL 2.46.0's
//! computed styles under a theme and Tidalfast's resolved anchor looks.
//! Proved with made-up themes (their own colours, TIDAL's classes and
//! anchors only).

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

fn overlay_for(name: &str, css: &str, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
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

/// `em` in a theme's shadows and corners is the box's font size, TIDAL's
/// 14-point text (TIDAL 2.46.0 computes the cotton candy family's sidebar
/// `0 0 2em` glow 28 px and its `inset 0 0 6em` 84 px, its bar's `1em`
/// corners 14 px). Tidalfast measured them at 16, every glow 8/7 too wide.
#[test]
fn em_in_a_themes_shadows_and_corners_is_tidals_text_size() {
    use crate::ui::plugin_dom as pd;
    use crate::ui::theme_layout;
    let css = r#"/* {"name": "Em glow"} */
[class*="_sidebarWrapper_"] {
  box-shadow: 0 0 2em rgba(10, 20, 30, 0.5), inset 0 0 6em rgba(200, 210, 220, 0.3);
}
"#;
    let o = overlay_for("em-glow.css", css, dom_anchors::keys::SIDEBAR);
    pd::test_overlay(Some((*o).clone()));
    let look = pd::anchor_look(dom_anchors::keys::SIDEBAR);
    pd::test_overlay(None);
    let look = look.expect("the sidebar's look");
    assert_eq!(look.shadow.map(|s| s.blur), Some(28), "2em of 14");
    assert_eq!(look.glow.map(|g| g.1), Some(84.0), "6em of 14");

    let vars = std::collections::BTreeMap::new();
    let viewport = [2137.0, 1183.0];
    assert_eq!(
        theme_layout::radii(Some("1em"), &vars, viewport),
        egui::CornerRadius::same(14)
    );
}

/// TIDAL 2.46.0 marks the navigation rows' icons up as icons: `svg.
/// _icon_77f3f89._icon_b8b52c1`, and Collection's chevron `svg._icon_77f3f89
/// ._icon_b8b52c1._arrowRight_f57b400` (captured). Rules for TIDAL's icons
/// (the cotton candy family's nested `._navigationItem_ { ._icon_77f3f89 {
/// fill } }`) reached neither: the chevron carried `_arrowRight_` alone and
/// the row's icon no class.
#[test]
fn rules_for_tidals_icons_reach_the_navigation_rows_icons() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{sidebar_item_arrow, sidebar_item_icon};
    let css = r#"/* {"name": "Nested icons"} */
._navigationItem_4e9deae {
  background-color: rgb(30, 40, 50);
  ._icon_77f3f89 { fill: rgb(10, 200, 100); }
}
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.items.push(dom_anchors::SidebarItem {
        test: "sidebar-collection-menu".into(),
        label: "Collection".into(),
        href: Some("/my-collection".into()),
        selected: false,
        rect: Some([16.0, 248.0, 208.0, 40.0]),
    });
    let arrow = sidebar_item_arrow("sidebar-collection-menu");
    let icon = sidebar_item_icon("sidebar-collection-menu");
    let tree = dom_anchors::build(&frame);
    let luna = themed_luna("nested-icons.css", css);
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0
            && o.hosts
                .contains_key(&dom_anchors::keys::sidebar_item("sidebar-collection-menu"))
        {
            break o;
        }
        assert!(Instant::now() < end, "the overlay never got there");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let inks = [pd::sidebar_icon(&arrow), pd::sidebar_icon(&icon)];
    pd::test_overlay(None);
    assert_eq!(inks, [Some(egui::Color32::from_rgb(10, 200, 100)); 2]);
}
