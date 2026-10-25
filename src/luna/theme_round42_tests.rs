//! Round 42: what the parity checker's style and pixel comparisons found
//! after round 41. Proved with made-up themes (their own values, TIDAL's
//! classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;
use crate::ui::plugin_dom as pd;

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

/// The overlay a theme's `css` gives the anchors of `frame`, once it
/// styles the anchor `key`.
fn overlay(name: &str, css: &str, frame: &dom_anchors::Frame, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
    let luna = themed_luna(name, css);
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.get(key).is_some_and(|h| !h.style.is_empty()) {
            break o;
        }
        assert!(Instant::now() < end, "the overlay never got there");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    o
}

/// The open queue with one song to come, as the queue panel describes it.
fn queue_frame() -> dom_anchors::Frame {
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([1795.0, 60.0, 334.0, 900.0]), false));
    frame.queue_rows = vec![dom_anchors::QueueRow {
        id: "next:0".into(),
        priority: "priority_none",
        track_id: "77".into(),
        title: "Next Song".into(),
        artists: vec![dom_anchors::Link {
            id: "5".into(),
            name: "Artist Five".into(),
        }],
        removable: true,
        rect: Some([1800.0, 200.0, 320.0, 60.0]),
        cover_rect: Some([1808.0, 210.0, 40.0, 40.0]),
        title_rect: Some([1860.0, 210.0, 220.0, 20.0]),
        artist_rect: Some([1860.0, 230.0, 220.0, 20.0]),
        action_rect: Some([1990.0, 216.0, 28.0, 28.0]),
        ..dom_anchors::QueueRow::default()
    }];
    frame
}

/// TIDAL 2.46.0 has three `_cellContainer_` modules, each its own hash: a
/// collection card's cell (`article._cellContainer_19b23ad`), a queue
/// song's cell (`div._cellContainer_182d240`) and the cover inside it
/// (`div._cellContainer_c39825b[data-test=cell-cover]`). A theme's rule
/// for one of them reaches that one only (the cotton candy family's
/// `._cellContainer_182d240 { border-radius: .4em }` rounds the queue
/// song's cell, not its cover, whose corners stay square in TIDAL); the
/// selector engine matched any `_cellContainer_` whatever its hash.
#[test]
fn a_hashed_cell_rule_reaches_its_own_module_only() {
    let css = r#"/* {"name": "Round queue cells"} */
._cellContainer_182d240 { border-radius: 9px; }
"#;
    let row = dom_anchors::keys::queue_item("next:0");
    let (cell, cover) = (format!("{row}:cell"), format!("{row}:cover"));
    let o = overlay("round-queue-cells.css", css, &queue_frame(), &cell);
    pd::test_overlay(Some((*o).clone()));
    let got = [&cell, &cover].map(|k| pd::anchor_look(k).and_then(|l| l.corners));
    pd::test_overlay(None);
    assert_eq!(got, [Some(egui::CornerRadius::same(9)), None]);
}

/// TIDAL's open menu, `div._contextMenu_8606b2d[data-test=contextmenu]`,
/// is the whole box: the song's cover and title at the top, then the rows,
/// inside its 4-point padding (TIDAL 2.46.0, captured). Its anchor's box
/// was the rows' alone, so a plugin reading the menu's box, and the parity
/// checker laying TIDAL's menu where Tidalfast's is, missed the header.
#[test]
fn an_open_menus_box_holds_its_header_and_rows() {
    use super::theme_round41_tests as r41;
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("row-menu"));
    let tree = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    app.backend.shutdown();
    let rect = |key: &str| {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.rect)
            .unwrap_or_else(|| panic!("no box for {key}"))
    };
    let menu = rect(dom_anchors::keys::CONTEXT_MENU);
    let first = rect(&dom_anchors::keys::menu_item(0));
    let rows: Vec<[f32; 4]> = (0..)
        .map(dom_anchors::keys::menu_item)
        .map_while(|k| tree.nodes.iter().find(|n| n.key == k).and_then(|n| n.rect))
        .collect();
    let last = rows.last().copied().expect("rows");
    // The header (a 38-point cover in 16 points of padding) above the rows.
    assert!(
        first[1] - menu[1] >= 60.0,
        "header: menu {menu:?}, first row {first:?}"
    );
    // The frame's padding round the rows' sides and foot.
    assert!(
        (first[0] - menu[0] - 4.0).abs() < 0.5,
        "left: {menu:?} {first:?}"
    );
    assert!(
        ((menu[0] + menu[2]) - (first[0] + first[2]) - 4.0).abs() < 0.5,
        "right: {menu:?} {first:?}"
    );
    assert!(
        ((menu[1] + menu[3]) - (last[1] + last[3]) - 4.0).abs() < 0.5,
        "foot: {menu:?} {last:?}"
    );
    assert_eq!(rect(dom_anchors::keys::CONTEXT_MENU_BOX), menu);
}
/// TIDAL blurs a playlist's header art box as a whole (`._dataContainer_
/// 9f47346::before { filter: blur(50px) }`), so the box's black foot runs
/// on into the page below it and fades out over about 130 points (TIDAL
/// 2.46.0 under TidalCustomTheme's pink page: the parity checker's replay
/// and a real TIDAL screenshot). Tidalfast's ended at the box's foot. A
/// theme's own `filter` on the box takes TIDAL's place (`none`: a hard
/// foot, as before).
#[test]
fn a_playlists_header_box_fades_out_below_its_foot() {
    use super::theme_round41_tests as r41;
    let pink = egui::Color32::from_rgb(250, 160, 200);
    let first_pink = |name: &str, extra: &str| -> f32 {
        let css = format!(
            "/* {{\"name\": \"Pink page\"}} */\n#main {{ background-color: rgb(250, 160, 200); }}\n{extra}\n"
        );
        let (ctx, mut app) = r41::demo_app("playlist:pl0", None);
        let plain = r41::drawn(
            &ctx,
            &mut app,
            DomOverlay {
                version: 1,
                ..DomOverlay::default()
            },
        );
        let mut o = r41::styled(name, &css, &plain, dom_anchors::keys::MAIN);
        o.version = 2;
        crate::ui::hold_cover_backdrop_for_test(&mut app, &ctx);
        let points: Vec<[f32; 2]> = (0..100).map(|i| [1000.0, 300.0 + 5.0 * i as f32]).collect();
        let colours = r41::painted_at(&ctx, &mut app, o, &points);
        app.backend.shutdown();
        let close = |c: egui::Color32| {
            c.to_array()[..3]
                .iter()
                .zip(pink.to_array())
                .all(|(a, b)| a.abs_diff(b) <= 2)
        };
        // Down the page from inside the header box: the first point that is
        // the page's own pink.
        let dark = colours
            .iter()
            .position(|c| !close(*c))
            .expect("the header box is drawn");
        let pink_at = colours[dark..]
            .iter()
            .position(|c| close(*c))
            .expect("the page shows below the box");
        points[dark + pink_at][1]
    };
    let blurred = first_pink("pink-page.css", "");
    let hard = first_pink(
        "pink-page-hard.css",
        "[class^=\"_dataContainer_\"]::before { filter: none; }",
    );
    assert!(
        blurred - hard >= 90.0,
        "the foot fades out from {hard} to {blurred}"
    );
}

/// A theme's background on `#mainHeader` fills TIDAL's header box over the
/// page's top on every page (Giorgeous' dark glass band, TidalCustomTheme's
/// pink: TIDAL 2.46.0 under them, the parity checker's replay). Tidalfast
/// painted it only over a plugin's layer behind the page, so a page with no
/// header art showed no band.
#[test]
fn a_themes_header_fill_lies_over_the_pages_top() {
    use super::theme_round41_tests as r41;
    let green = egui::Color32::from_rgb(10, 200, 30);
    let css =
        "/* {\"name\": \"Green header\"} */\n#mainHeader { background-color: rgb(10, 200, 30); }\n";
    let (ctx, mut app) = r41::demo_app("albums", None);
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = r41::styled("green-header.css", css, &plain, dom_anchors::keys::HEADER);
    o.version = 2;
    let header = plain
        .nodes
        .iter()
        .find(|n| n.key == dom_anchors::keys::HEADER)
        .and_then(|n| n.rect)
        .expect("the header's box");
    let middle = [header[0] + header[2] / 2.0, header[1] + header[3] / 2.0];
    let below = [middle[0], header[1] + header[3] + 40.0];
    let got = r41::painted_at(&ctx, &mut app, o, &[middle, below]);
    app.backend.shutdown();
    assert_eq!(got[0], green, "the header's box");
    assert_ne!(got[1], green, "the page under it");
}
