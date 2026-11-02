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

/// TIDAL's `span.context-menu` is there while a menu is open, in the shell
/// before the main container. Empty, it takes no room; a side of its own, as
/// a rule like `[class*="menu"] { border-right }` gives it, opens a line of
/// 20 above the sidebar and the page (Edge on the capture). Top and bottom
/// edges alone do not, nor does a rule that leaves it no box.
#[test]
fn a_side_on_the_menus_empty_span_opens_a_line_above_the_shell() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{CONTEXT_MENU_LINE, FOOTER, MAIN_CONTAINER, SHELL};
    let frame = dom_anchors::sample_frame();
    let tree = dom_anchors::build(&frame);
    let index = |key: &str| tree.nodes.iter().position(|n| n.key == key).unwrap();
    assert!(index(CONTEXT_MENU_LINE) < index(MAIN_CONTAINER));
    assert_eq!(
        tree.nodes[index(CONTEXT_MENU_LINE)].parent.as_deref(),
        Some(SHELL)
    );
    let mut closed = frame.clone();
    closed.context_menu = None;
    assert!(
        !dom_anchors::build(&closed)
            .nodes
            .iter()
            .any(|n| n.key == CONTEXT_MENU_LINE)
    );

    for (name, rule, line) in [
        (
            "line-right.css",
            "[class*=\"menu\"] { border-right: 1px solid rgba(1, 2, 3, 0.5); }",
            20.0,
        ),
        ("line-pad.css", ".context-menu { padding-left: 3px; }", 20.0),
        (
            "line-top.css",
            ".context-menu { border-top: 1px solid rgb(1, 2, 3); padding-top: 5px; }",
            0.0,
        ),
        (
            "line-gone.css",
            ".context-menu { display: none; border-right: 1px solid rgb(1, 2, 3); }",
            0.0,
        ),
        (
            "line-fill.css",
            ".context-menu { background-color: rgb(1, 2, 3); }",
            0.0,
        ),
    ] {
        let css = format!(
            "/* {{\"name\": \"{name}\"}} */ ._player_8c80ae6 {{ color: rgb(1, 2, 3); }} {rule}"
        );
        let o = overlay_when(name, &css, &frame, |o| {
            o.hosts.contains_key(FOOTER) && o.hosts.contains_key(CONTEXT_MENU_LINE)
        });
        pd::test_overlay(Some((*o).clone()));
        let got = pd::context_menu_line();
        pd::test_overlay(None);
        assert_eq!(got, line, "{name}");
    }
}

/// A box a theme gives a row's title is the words' box, not the cell's: as
/// wide as the words and its border, 20 high and its border, in the middle
/// of the row, and the words stand inside the border.
#[test]
fn a_box_on_a_rows_title_is_as_wide_as_its_words() {
    use crate::ui::plugin_dom as pd;
    let frame = dom_anchors::sample_frame();
    let list = frame.lists.first().unwrap();
    let key = dom_anchors::keys::row_cell(&list.id, list.rows[0].index, "title-text");
    let css = "/* {\"name\": \"Title box\"} */ [class*=\"active\"] { background: rgb(9, 8, 7) !important; border: 1px solid rgb(1, 2, 3) !important; }";
    let ready = |o: &DomOverlay| o.hosts.contains_key(&key);
    let o = overlay_when("title-box.css", css, &frame, ready);
    pd::test_overlay(Some((*o).clone()));
    let ctx = egui::Context::default();
    let font = egui::FontId::proportional(14.0);
    let (mut width, mut text_left, mut right) = (0.0, 0.0, 0.0);
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let cell = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(400.0, 56.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(cell)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        let white = egui::Color32::WHITE;
        width = child
            .painter()
            .layout_no_wrap("A title".to_owned(), font.clone(), white)
            .size()
            .x;
        right = pd::paint_words_box(&mut child, &key, "A title", font.clone(), white);
        text_left = child.cursor().left();
    });
    out.textures_delta.clear();
    pd::test_overlay(None);
    assert_eq!(right, 1.0);
    assert_eq!(text_left, 101.0);
    let boxes: Vec<egui::Rect> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) => Some(r.rect),
            _ => None,
        })
        .collect();
    let b = boxes.first().copied().expect("the box is painted");
    assert_eq!(b.left(), 100.0, "{boxes:?}");
    assert!((b.width() - (width + 2.0)).abs() < 0.01, "{boxes:?}");
    assert_eq!(b.height(), 22.0, "{boxes:?}");
    assert_eq!(b.center().y, 78.0, "{boxes:?}");
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
