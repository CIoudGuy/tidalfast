//! Plugins compared with TidaLuna in batch 4 (CoverTheme,
//! StarryNight, NoPlaylistBackgroundImage and the rest): what their store
//! builds do to TIDAL's markup, held against Tidalfast's anchors.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

/// Applies `css` as a theme to the sample frame `edit` changed, and returns
/// the overlay once `key` is styled.
fn styled(
    name: &str,
    css: &str,
    edit: impl FnOnce(&mut dom_anchors::Frame),
    key: &str,
) -> DomOverlay {
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert(format!("/{name}.css"), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir(name);
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let mut frame = dom_anchors::sample_frame();
    edit(&mut frame);
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/{name}.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(key) {
            break o;
        }
        assert!(Instant::now() < end, "the CSS never reached {key}");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
    (*o).clone()
}

/// NoPlaylistBackgroundImage's second rule: the compact header pinned over
/// a scrolled album or playlist (TIDAL's `div._smallHeader_` with
/// `_showHeader_`) loses the blurred cover its `::before` paints from
/// `--img` and takes a flat 30 % grey instead. Before, Tidalfast's pinned
/// bar was no anchor, so the cover stayed.
#[test]
fn no_playlist_background_image_tints_the_pinned_compact_header() {
    let css = r#"/* {"name": "NoPlaylistBackgroundImage"} */
[class^="_dataContainer_"]:before{--img:none!important;background-image:none!important}
[class^="_smallHeader_"]:before{--img:none!important;background-color:color-mix(in srgb,#242429,transparent 70%)!important;filter:none!important}
"#;
    let key = dom_anchors::keys::PAGE_SMALL_HEADER;
    let o = styled(
        "noplaylistbg",
        css,
        |frame| {
            if let Some(page) = frame.page.as_mut() {
                page.small_header = Some([240.0, 0.0, 1040.0, 72.0]);
            }
        },
        key,
    );
    let before = o.hosts[key].before.as_ref().expect("the header's art box");
    assert_eq!(before.background_image.as_deref(), Some("none"));
    let layers = crate::ui::plugin_dom::box_layers(
        &o.hosts[key],
        &std::collections::BTreeMap::new(),
        [0.0; 2],
    );
    assert!(
        matches!(layers.as_slice(), [crate::ui::plugin_dom::BoxLayer::Fill(c, _)] if c.a() > 60 && c.a() < 90),
        "{layers:?}"
    );
}
