//! Round 51: what the parity checker's comparisons found. Proved with
//! made-up themes (their own values, TIDAL's classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

/// Installs `css` as a theme over `frame`'s anchors and returns the overlay
/// once the theme gave `key` a transform.
fn overlay_with(name: &str, css: &str, frame: &dom_anchors::Frame, key: &str) -> Arc<DomOverlay> {
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
        if o.css_bytes > 0 && o.hosts.get(key).is_some_and(|h| h.style.color.is_some()) {
            break o;
        }
        assert!(Instant::now() < end, "{key:?} was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    o
}

/// TIDAL 2.47.0 holds each song row of a list as
/// `div._nativeRowWrapper_20e35be > div[data-type=mediaItem] >
/// div._tableRow_` (captured). Rules nested in the wrapper reach the row's
/// cells (Cotton Candy Nights' `[class="_nativeRowWrapper_20e35be"] {
/// ._playButton_4942431 { transform: scale(.8) } }`), and
/// `[data-type="mediaItem"]` names the box round the row (Synthwave '77).
/// The row's boxes were bare, so neither reached. A playlist's rows stay
/// in TIDAL's virtual list boxes, which have neither.
#[test]
fn a_rule_in_the_row_wrapper_reaches_the_rows_cells() {
    let css = r#"/* {"name": "Row wrapper"} */
[class="_nativeRowWrapper_20e35be"] {
  ._notCircleIcon_dad7d82._iconButton_4b3ed40._noPadding_9cb3eea._playButton_4942431 { transform: scale(.8); color: rgb(1, 2, 3); }
}
[data-type="mediaItem"] > ._tableRow_299a426 { color: rgb(4, 5, 6); }
"#;
    let frame = dom_anchors::sample_frame();
    let play = dom_anchors::keys::row_cell("album:55", 0, "play");
    let o = overlay_with("row-wrapper.css", css, &frame, &play);
    let style = |key: &str| {
        o.hosts
            .get(key)
            .map(|h| h.style.clone())
            .unwrap_or_default()
    };
    let button = style(&play);
    assert_eq!(button.transform.as_deref(), Some("scale(.8)"));
    assert_eq!(
        style(&dom_anchors::keys::row("album:55", 0))
            .color
            .as_deref(),
        Some("rgb(4, 5, 6)")
    );
    let mut playlist = dom_anchors::sample_frame();
    playlist.lists[0].id = "playlist:9".into();
    let tree = dom_anchors::build(&playlist);
    let slot = format!("{}:slot", dom_anchors::keys::row("playlist:9", 0));
    let node = tree
        .nodes
        .iter()
        .find(|n| n.key == slot)
        .expect("the row's box");
    assert!(!node.attrs.iter().any(|(k, _)| k == "class"), "{node:?}");
}
