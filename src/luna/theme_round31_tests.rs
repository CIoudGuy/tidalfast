//! Round 31: anchors checked against TIDAL 2.46.0's own element tree and
//! computed styles, captured with each theme applied. Proved with made-up
//! themes (their own colours, TIDAL's classes and anchors only).

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

/// A track's title is the words of TIDAL's `span[data-test=table-cell-title]`
/// itself (`._titleText_ .table--inactive-title`), not a link: a theme's
/// `a { color }` leaves it alone and its `[class*="active"]` colours it, as
/// Catppuccin Mocha Blue's blue titles show in TIDAL. Tidalfast's title sat
/// in a link and took the theme's link colour.
#[test]
fn a_track_title_is_not_a_link() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Linky"} */
a { color: rgb(200, 160, 250) !important; }
[class*="active"] { color: rgb(30, 100, 240) !important; }
"#;
    let luna = themed_luna("linky.css", css);
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let list = "album:55";
    let title = dom_anchors::keys::row_cell(list, 1, "title-text");
    let o = wait(&luna, &tree, &|o| {
        o.hosts.get(&title).is_some_and(|h| h.style.color.is_some())
    });
    pd::test_overlay(Some((*o).clone()));
    let window = egui::Color32::from_rgb(10, 10, 10);
    let text = pd::row_text(list, 1, window).expect("the row's colours");
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(text.title, Some(egui::Color32::from_rgb(30, 100, 240)));
    // The artist is a link in TIDAL too.
    assert_eq!(text.artist, Some(egui::Color32::from_rgb(200, 160, 250)));
}
