//! Round 32: TIDAL 2.46.0's classes and token scopes for the queue's
//! artist line, the sidebar's playlist rows and the context menu's words,
//! and the native parts painted with their anchor's whole look. Proved with
//! made-up themes (their own colours, TIDAL's classes and anchors only).

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

/// The overlay `css` gives the sample tree, once `key` is styled.
fn overlay_for(name: &str, css: &str, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let luna = themed_luna(name, css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(key));
    luna.shutdown();
    o
}

/// A dark theme's rule for TIDAL's market text colours the queue's artist
/// names: TIDAL 2.46.0 draws them near white under Blur's
/// `[class*="_marketText_"] { color: #ffffffee !important }` (captured),
/// where Tidalfast kept its grey for every dark window. A catch-all
/// `* { color }` still leaves them grey, as TIDAL's own rule for the line
/// does in TIDAL (Meteor Night).
#[test]
fn a_dark_themes_market_text_rule_colours_the_queue_artists() {
    use crate::theme::Palette;
    use crate::ui::now_playing::queue_name_colors;
    use crate::ui::plugin_dom::{self as pd, SidePanel, themed, tidal_panel_text};
    let names = |name: &str, css: &str| {
        let o = overlay_for(name, css, dom_anchors::keys::QUEUE_ROLE_TEXT);
        pd::test_overlay(Some((*o).clone()));
        let queue = tidal_panel_text(themed(&Palette::dark(), &o), SidePanel::Queue);
        let colors = queue_name_colors(&queue);
        pd::test_overlay(None);
        colors
    };
    let pale = |c: egui::Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        r >= 248 && g >= 248 && b >= 250 && (230..=240).contains(&a)
    };
    let (plain, followed) = names(
        "frosted-words.css",
        r#"/* {"name": "Frosted words"} */
body { background-color: rgb(14, 12, 24) !important; }
[class*="_marketText_"] { color: rgba(250, 250, 255, .933) !important; }
"#,
    );
    assert!(pale(plain), "near white: {plain:?}");
    assert_eq!(followed, plain);
    let native = queue_name_colors(&Palette::dark());
    let (plain, _) = names(
        "snowdrift.css",
        r#"/* {"name": "Snowdrift"} */
body { background-color: rgb(14, 12, 24) !important; }
* { color: rgb(240, 240, 250); }
"#,
    );
    assert_eq!(plain, native.0, "a catch-all leaves TIDAL's grey");
}
