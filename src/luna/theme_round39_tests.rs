//! Round 39: differences the parity checker's style and pixel comparisons
//! found between TIDAL 2.46.0 under a theme and Tidalfast. Proved with
//! made-up themes (their own values, TIDAL's classes and anchors only).

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

/// TIDAL 2.46.0's queue list sits in `div._playQueueItemsContainer_01c88fe
/// [data-test=playqueue]`, so the rules a theme nests under that container
/// reach the rows' titles and artist lines, and a relative size of an
/// artist link inside is of its line's (the cotton candy family's 94 %
/// weight-600 words, 95 % links: 12.5 px). Tidalfast's rows sat beside the
/// container, and kept TIDAL's 14 px and their weights.
#[test]
fn the_queue_rows_sit_in_tidals_items_container() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Small queue words"} */
[class="_playQueueItemsContainer_01c88fe"] {
  ._marketText_1lyag_1._medium20_1lyag_192, ._artistText_73966a1 { font-weight: 800; font-size: 50%; }
}
[class="_playQueueItems_84488b2"] { a._link_6a45a19._item_39605ae { font-size: 50%; } }
"#;
    let row = dom_anchors::keys::queue_item("next:0");
    let title = format!("{row}:title");
    let artists = format!("{row}:artists");
    let link = format!("{row}:artist:0");
    let o = overlay("small-queue-words.css", css, &queue_frame(), &title);
    pd::test_overlay(Some((*o).clone()));
    let got =
        [&title, &artists, &link].map(|k| pd::anchor_look(k).map(|l| (l.font_size, l.font_weight)));
    pd::test_overlay(None);
    assert_eq!(
        got,
        [
            Some((Some(7.0), Some(800))),
            Some((Some(7.0), Some(800))),
            Some((Some(3.5), None)),
        ]
    );
}
