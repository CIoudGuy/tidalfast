//! The shared anchor look (`crate::ui::plugin_dom::anchor_look`): a native
//! part reads its TIDAL anchor's whole box from the theme cascade, in the
//! state the anchor is in, and paints it. Proved with made-up themes (their
//! own variable names and colours, TIDAL's classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::{DomEvent, DomOverlay, DomTarget};
use super::dom_anchors;
use super::*;

/// Installs `css` as a theme over `tree`; returns the runtime and a wait
/// for an overlay that satisfies `want`.
fn themed(name: &str, css: &str) -> Luna {
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

/// The sidebar with a selected Music row and an idle Explore row.
fn two_nav_rows() -> super::dom::HostTree {
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.items.push(dom_anchors::SidebarItem {
        test: "sidebar-explore".into(),
        label: "Explore".into(),
        href: Some("/view/pages/explore".into()),
        selected: false,
        rect: Some([8.0, 90.0, 224.0, 32.0]),
    });
    dom_anchors::build(&frame)
}

/// The navigation rows' look follows their state: the selected row takes
/// the `_selected_` rule's glow and fill, the idle one its own rule, and
/// under the pointer its `:hover` rule's fill, corners and words, as the
/// cotton candy family paints them.
#[test]
fn navigation_rows_take_their_states_whole_look() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::sidebar_item;
    let css = r#"/* {"name": "Glowing pills"} */
:root { --lagoon: 40, 170, 200; }
._navigationItem_4e9deae { background-color: rgba(var(--lagoon), .3); }
._navigationItem_4e9deae:hover { background-color: rgb(250, 240, 250); border-radius: 1em; color: rgb(var(--lagoon)); }
._navigationItem_4e9deae._selected_29027d1 {
  box-shadow: inset 0px 0px .9em color-mix(in srgb, white 25%, rgb(var(--lagoon)) 75%);
  background-color: rgb(230, 250, 255);
}
"#;
    let luna = themed("glowing-pills.css", css);
    let tree = two_nav_rows();
    let music = sidebar_item("sidebar-music");
    let explore = sidebar_item("sidebar-explore");
    let o = wait(&luna, &tree, &|o| {
        o.hosts.contains_key(&music) && o.hosts.contains_key(&explore)
    });
    pd::test_overlay(Some((*o).clone()));
    let selected = pd::anchor_look(&music).expect("the selected row's look");
    let idle = pd::anchor_look(&explore).expect("the idle row's look");
    assert_eq!(selected.fill, Some(egui::Color32::from_rgb(230, 250, 255)));
    let (glow, blur, _) = selected.glow.expect("the selected row's glow");
    assert!(glow.b() > glow.r(), "the theme's colour: {glow:?}");
    assert!((blur - 14.4).abs() < 0.5, ".9em: {blur}");
    assert!(idle.glow.is_none(), "only the selected row glows");
    assert_eq!(idle.corners, None);
    // The pointer on the idle row: its `:hover` look.
    luna.dom_event(DomEvent {
        target: DomTarget::Host(explore.clone()),
        kind: "hover".into(),
        data: serde_json::Value::Null,
    });
    let hovered = wait(&luna, &tree, &|o| {
        o.hosts
            .get(&explore)
            .is_some_and(|h| h.style.border_radius.is_some())
    });
    pd::test_overlay(Some((*hovered).clone()));
    let hover = pd::anchor_look(&explore).expect("the hovered row's look");
    assert_eq!(hover.fill, Some(egui::Color32::from_rgb(250, 240, 250)));
    assert_eq!(hover.corners, Some(egui::CornerRadius::same(14)));
    assert_eq!(hover.text, Some(egui::Color32::from_rgb(40, 170, 200)));
    pd::test_overlay(None);
    luna.shutdown();
}

/// A border on some sides of a box (`border-style: solid none none none`,
/// a line along the top) is drawn on those sides only; it was drawn round
/// the whole box.
#[test]
fn a_border_on_some_sides_is_drawn_on_those() {
    use crate::ui::plugin_dom::{BoxLayer, box_layers};
    let css = r#"/* {"name": "Top line"} */
._container_b3e8f28 { border-style: solid none none none; border-width: 2px; border-color: rgb(200, 30, 90); }
"#;
    let luna = themed("top-line.css", css);
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([1795.0, 60.0, 334.0, 900.0]), false));
    let tree = dom_anchors::build(&frame);
    let o = wait(&luna, &tree, &|o| {
        o.hosts.contains_key(dom_anchors::keys::QUEUE_PANEL)
    });
    let h = &o.hosts[dom_anchors::keys::QUEUE_PANEL];
    let layers = box_layers(h, &o.vars, [2137.0, 1183.0]);
    luna.shutdown();
    assert!(
        !layers.iter().any(|l| matches!(l, BoxLayer::Border(..))),
        "no border round the box: {layers:?}"
    );
    assert!(
        layers.iter().any(|l| matches!(
            l,
            BoxLayer::Sides(sides, c) if *sides == [2.0, 0.0, 0.0, 0.0] && *c == egui::Color32::from_rgb(200, 30, 90)
        )),
        "the top line: {layers:?}"
    );
}

/// A theme's menu box beyond one colour (a border round the menu, a
/// gradient on the popover round it; Catppuccin Mocha Blue's and
/// Giorgeous' outlined menus) is laid into every popup menu's frame; the menus kept a
/// flat frame of the palette's menu colour. Without such a box the frame
/// stays as it was.
#[test]
fn a_themes_menu_box_is_laid_into_the_menus_frame() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Framed menus"} */
:root { --dusk: rgb(20, 30, 90); --dawn: rgb(230, 80, 160); }
[class*="_contextMenu_"][role="menu"] { border: 2px solid rgb(120, 180, 250); background-color: transparent; }
._container_d3060bb._visible_4bed23e { background-image: linear-gradient(180deg, var(--dusk), var(--dawn)); }
"#;
    let luna = themed("framed-menus.css", css);
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let o = wait(&luna, &tree, &|o| {
        o.hosts.contains_key(dom_anchors::keys::THEME_MENU)
    });
    pd::test_overlay(Some((*o).clone()));
    let palette = crate::theme::Palette::dark();
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    // Two passes: an area is laid out unseen on its first.
    let mut out = egui::FullOutput::default();
    for _ in 0..2 {
        out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            egui::Area::new(egui::Id::new("menu"))
                .fixed_pos(egui::pos2(100.0, 100.0))
                .show(&ctx, |ui| {
                    crate::ui::widgets::menu_frame(&palette).show(ui, |ui| {
                        ui.allocate_exact_size(egui::vec2(200.0, 120.0), egui::Sense::hover());
                    });
                });
            pd::end_frame(&ctx);
        });
        out.textures_delta.clear();
    }
    pd::test_overlay(None);
    luna.shutdown();
    let mut flat = Vec::new();
    fn walk(s: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            other => out.push(other.clone()),
        }
    }
    for c in &out.shapes {
        walk(&c.shape, &mut flat);
    }
    let mark = flat
        .iter()
        .any(|s| matches!(s, egui::Shape::Rect(r) if pd::mark_opacity(r.fill).is_some()));
    assert!(!mark, "the mark is replaced");
    // The menu fades in: colours are the theme's times its opacity.
    let blue_border = flat.iter().any(|s| {
        matches!(s, egui::Shape::Rect(r) if r.stroke.width == 2.0
            && r.stroke.color.b() > r.stroke.color.g()
            && r.stroke.color.g() > r.stroke.color.r())
    });
    assert!(blue_border, "the menu's border is drawn");
    // Dusk at the top, dawn (redder) at the foot.
    let gradient = flat.iter().any(|s| match s {
        egui::Shape::Mesh(m) => {
            m.vertices.iter().any(|v| v.color.b() > v.color.r())
                && m.vertices.iter().any(|v| v.color.r() > v.color.g() * 2)
        }
        _ => false,
    });
    assert!(gradient, "the popover's gradient is drawn");
}
