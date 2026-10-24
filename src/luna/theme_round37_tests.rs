//! Round 37: differences the parity checker found between TIDAL 2.46.0's
//! computed styles under a theme and Tidalfast's resolved anchor looks.
//! Proved with made-up themes (their own colours, TIDAL's classes and
//! anchors only).

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

fn overlay(name: &str, css: &str, frame: &dom_anchors::Frame, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
    let luna = themed_luna(name, css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(key));
    luna.shutdown();
    o
}

/// TIDAL's root defines its layout in variables (TIDAL 2.46.0:
/// `--miniPlayerHeight: 88px`, `--headerHeight: 56px`, ...), and themes
/// size their boxes by them: the cotton candy family's `.mainContent {
/// padding-bottom: var(--miniPlayerHeight) }` is 88 points in TIDAL. They
/// were undefined here, so the rule was dropped.
#[test]
fn tidals_layout_variables_are_defined() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::MAIN_CONTENT;
    let css = r#"/* {"name": "Layout vars"} */
.mainContent { opacity: 0.99; padding-bottom: var(--miniPlayerHeight); padding-top: calc(var(--headerHeight) / 4); }
"#;
    let o = overlay(
        "layout-vars.css",
        css,
        &dom_anchors::sample_frame(),
        MAIN_CONTENT,
    );
    pd::test_overlay(Some((*o).clone()));
    let look = pd::anchor_look(MAIN_CONTENT);
    pd::test_overlay(None);
    let padding = look
        .and_then(|l| l.padding)
        .expect("main content's padding");
    assert_eq!(padding[2], 88.0, "{padding:?}");
    assert_eq!(padding[0], 14.0, "{padding:?}");
}
