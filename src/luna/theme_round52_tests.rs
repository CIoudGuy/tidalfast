//! Round 52: what the parity checker and TIDAL 2.47.0's captured pages
//! showed on the player bar. Proved with made-up themes (their own colours,
//! TIDAL's classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

/// Installs `css` as a theme over `frame`'s anchors and returns the overlay
/// once the theme gave `key` a colour.
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
        let styled =
            |h: &super::dom::HostOverride| h.style.color.is_some() || h.style.fill.is_some();
        if o.css_bytes > 0 && o.hosts.get(key).is_some_and(styled) {
            break o;
        }
        assert!(Instant::now() < end, "{key:?} was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    o
}

/// TIDAL's bar glyphs are `svg._icon_77f3f89 > use` in each button, and a
/// theme's `fill` on them beats the button's `color` (Tidal Light's
/// `._icon_77f3f89 { fill: ... !important }`, the bar's greys in black).
/// Tidalfast's buttons had no such child, so such a rule reached nothing.
#[test]
fn a_themes_icon_fill_reaches_the_bars_glyphs_over_the_buttons_colour() {
    use crate::ui::player_bar::utility_glyph;
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{BAR_ICONS, QUEUE, SHUFFLE, SOUND_OUTPUT};
    let css = r#"/* {"name": "Bar glyphs"} */
._utilityButtons_4d7aaf9 button { color: rgb(200, 200, 200); }
._shuffleButton_545c75a { color: rgb(200, 200, 200); }
._icon_77f3f89 { fill: rgb(10, 20, 30) !important; }
"#;
    let o = overlay_with(
        "bar-glyphs.css",
        css,
        &dom_anchors::sample_frame(),
        BAR_ICONS[0].1,
    );
    pd::test_overlay(Some((*o).clone()));
    let ink = egui::Color32::from_rgb(10, 20, 30);
    for (button, ..) in BAR_ICONS {
        assert_eq!(pd::bar_icon(button), Some(ink), "{button}");
    }
    assert_eq!(utility_glyph(QUEUE), Some(ink));
    assert_eq!(utility_glyph(SOUND_OUTPUT), Some(ink));
    assert_eq!(pd::bar_icon(SHUFFLE), Some(ink));
    pd::test_overlay(None);
    // With no fill the button's colour stands.
    let css = r#"/* {"name": "Button colour"} */
._utilityButtons_4d7aaf9 button { color: rgb(200, 100, 50); }
"#;
    let o = overlay_with(
        "button-colour.css",
        css,
        &dom_anchors::sample_frame(),
        QUEUE,
    );
    pd::test_overlay(Some((*o).clone()));
    assert_eq!(pd::bar_icon(QUEUE), None);
    assert_eq!(
        utility_glyph(QUEUE),
        Some(egui::Color32::from_rgb(200, 100, 50))
    );
    pd::test_overlay(None);
}
