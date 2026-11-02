//! Round 55: what the parity checker and TIDAL 2.47.0's captured pages
//! showed after round 54. Proved with made-up themes (their own colours,
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

/// A side's shorthand is a border by itself, with none on the box before it
/// or after `border: none`: the sidebar's edge and the bar's top line of
/// several themes went undrawn. The side stands alone in the box's widths.
#[test]
fn a_sides_shorthand_alone_is_the_boxs_border() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::FOOTER;
    let frame = dom_anchors::sample_frame();
    for (name, rule, sides) in [
        (
            "side-top.css",
            "border-top: 2px solid rgb(1, 2, 3) !important;",
            Some([2.0, 0.0, 0.0, 0.0]),
        ),
        (
            "side-words.css",
            "border-right: rgb(1, 2, 3) 2px solid;",
            Some([0.0, 2.0, 0.0, 0.0]),
        ),
        (
            "side-after-none.css",
            "border: none; border-bottom: 2px solid rgb(1, 2, 3);",
            Some([0.0, 0.0, 2.0, 0.0]),
        ),
        (
            "side-spaced.css",
            "border-top: solid 2px rgb(1 2 3);",
            Some([2.0, 0.0, 0.0, 0.0]),
        ),
        ("side-none.css", "border-top: none;", None),
        ("side-zero.css", "border-top: 0 solid rgb(1, 2, 3);", None),
        ("side-nostyle.css", "border-top: 2px;", None),
    ] {
        let css = format!(
            "/* {{\"name\": \"{name}\"}} */ #footerPlayer {{ color: rgb(1, 2, 3); {rule} }}"
        );
        let o = overlay_when(name, &css, &frame, |o| o.hosts.contains_key(FOOTER));
        pd::test_overlay(Some((*o).clone()));
        let look = pd::anchor_look(FOOTER).expect("the bar's look");
        pd::test_overlay(None);
        let want = sides.map(|s| (s, egui::Color32::from_rgb(1, 2, 3)));
        assert_eq!(
            look.sides, want,
            "{name}: {:?}",
            o.hosts[FOOTER].style.border
        );
    }
}

/// Corners, a shadow and a colour that TIDAL sets by a class of its own are
/// out of reach of a rule of no weight (`* { ... }`) or for a tag, and within
/// reach of any rule that names the part.
#[test]
fn a_rule_of_no_weight_leaves_what_tidals_classes_set() {
    use dom_anchors::keys::{FOOTER, sidebar_item};
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.items.push(dom_anchors::SidebarItem {
        test: "sidebar-explore".into(),
        label: "Explore".into(),
        href: Some("/explore".into()),
        selected: false,
        rect: Some([16.0, 128.0, 208.0, 40.0]),
    });
    let row = sidebar_item("sidebar-explore");
    let css = "/* {\"name\": \"Square\"} */ * { border-radius: 0; box-shadow: none; } a { color: rgb(1, 2, 3); } #footerPlayer { color: rgb(1, 2, 3); }";
    let o = overlay_when("square.css", css, &frame, |o| o.hosts.contains_key(FOOTER));
    let style = o
        .hosts
        .get(&row)
        .map(|h| h.style.clone())
        .unwrap_or_default();
    assert_eq!(style.border_radius, None, "{row}");
    assert_eq!(style.color, None, "{row}");
    let css = "/* {\"name\": \"Named\"} */ [class*=\"navigationItem\"] { border-radius: 3px; color: rgb(1, 2, 3); }";
    let o = overlay_when("named.css", css, &frame, |o| o.hosts.contains_key(&row));
    let style = &o.hosts[&row].style;
    assert_eq!(style.border_radius.as_deref(), Some("3px"));
    assert_eq!(style.color.as_deref(), Some("rgb(1, 2, 3)"));
}
