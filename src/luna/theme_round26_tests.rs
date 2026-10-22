//! Luna themes round 26: what round 25 left open, fixed in the general CSS
//! mapping and proved with made-up themes (their own variable names and
//! colours, TIDAL's classes and anchors only).

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

/// TIDAL gives each kind of home shortcut its own link class: the cotton
/// candy family paints `._link_0c5cc7e > ._shortcutItem_` (an artist's
/// tile) in its second colour, so TIDAL's artist tiles are blue beside the
/// user's pink playlists. Every tile carried a playlist's class, so all of
/// them took the first colour. Mixes and albums carry their own ids.
#[test]
fn shortcut_tiles_carry_their_kinds_link_class_and_id() {
    use dom_anchors::keys::tile_item;
    let css = r#"/* {"name": "Tile kinds"} */
:root { --sorbet-one: rgb(210, 40, 90); --sorbet-two: rgb(30, 90, 210); }
._disableUnderline_8e517ad._link_86815a7 > ._shortcutItem_6c8e7b4 { background-color: var(--sorbet-one); }
._disableUnderline_8e517ad._link_0c5cc7e > ._shortcutItem_6c8e7b4 { background-color: var(--sorbet-two); }
#mix-shortcut { background-color: rgb(1, 160, 70); }
"#;
    let tile = |x: f32, owned, kind| dom_anchors::Tile {
        rect: Some([x, 100.0, 300.0, 60.0]),
        image: Some([x + 8.0, 108.0, 44.0, 44.0]),
        owned,
        kind,
    };
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.feed = true;
        page.tiles = vec![
            tile(240.0, true, dom_anchors::TileKind::Playlist),
            tile(548.0, false, dom_anchors::TileKind::Artist),
            tile(856.0, false, dom_anchors::TileKind::Mix),
        ];
    }
    let (luna, o) = overlay_with(
        "tile-kinds.css",
        css,
        dom_anchors::build(&frame),
        &tile_item(2),
    );
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    let fills: Vec<_> = (0..3)
        .map(|i| crate::ui::plugin_dom::fill(&tile_item(i)))
        .collect();
    crate::ui::plugin_dom::test_overlay(None);
    luna.shutdown();
    assert_eq!(fills[0], Some(egui::Color32::from_rgb(210, 40, 90)), "own");
    assert_eq!(
        fills[1],
        Some(egui::Color32::from_rgb(30, 90, 210)),
        "artist"
    );
    assert_eq!(fills[2], Some(egui::Color32::from_rgb(1, 160, 70)), "mix");
}
