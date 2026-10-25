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
