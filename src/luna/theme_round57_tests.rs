//! Round 57: what the parity checker showed against TIDAL 2.47.0's captures,
//! proved with made-up themes (their own colours, TIDAL's classes and
//! anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

/// Installs `css` as a theme over `tree` and returns the overlay once the
/// theme reached it and `key` is styled.
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
        if o.css_bytes > 0 && (key.is_empty() || o.hosts.contains_key(key)) {
            break o;
        }
        assert!(Instant::now() < end, "{key:?} was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    (luna, o)
}

/// TIDAL declares `--playQueueWidth` as its open play queue's 350 points and
/// `0px` while the queue is closed (its captures: 350 on the album and menu
/// pages, queue open, 0 on the playlist and collection). A theme that sizes
/// a box by it (the cotton candy family's Home card, `calc(88vw -
/// var(--sidebarWidth) - (var(--playQueueWidth) * .9))`) narrows it while the
/// queue is open. Tidalfast declared 0 always, so the open queue changed
/// nothing.
#[test]
fn a_box_sized_by_the_queues_width_narrows_while_the_queue_is_open() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Queue sized card"} */
._feed_412931c { background-color: #fff0; border-style: solid; border-width: 2px; border-color: rgb(255, 150, 240);
    min-width: calc(88vw - var(--sidebarWidth) - (var(--playQueueWidth)*.9)) !important;
    max-width: calc(88vw - var(--sidebarWidth) - (var(--playQueueWidth)*.9)) !important; }
"#;
    let width = |open: bool| {
        let mut frame = dom_anchors::sample_frame();
        frame.viewport = [2137.0, 1183.0];
        frame.sidebar.width = 240.0;
        frame.queue_sidebar = Some(open);
        frame.queue_panel = open.then_some((Some([1787.0, 60.0, 334.0, 900.0]), false));
        if let Some(p) = frame.page.as_mut() {
            p.feed = true;
        }
        let (luna, o) = overlay_with(
            "queue-card.css",
            css,
            dom_anchors::build(&frame),
            dom_anchors::keys::PAGE,
        );
        pd::test_overlay(Some((*o).clone()));
        pd::note(|f| f.viewport = [2137.0, 1183.0]);
        let w = pd::feed_card()
            .and_then(|c| c.width)
            .expect("the card's width");
        pd::test_overlay(None);
        luna.shutdown();
        w
    };
    let closed = width(false);
    assert!((closed - (0.88 * 2137.0 - 240.0)).abs() < 1.0, "{closed}");
    let open = width(true);
    assert!(
        (open - (0.88 * 2137.0 - 240.0 - 350.0 * 0.9)).abs() < 1.0,
        "{open}"
    );
}
