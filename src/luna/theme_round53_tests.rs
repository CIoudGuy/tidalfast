//! Round 53: what the parity checker and TIDAL 2.47.0's captured pages
//! showed at narrow windows and on the player bar. Proved with made-up
//! themes (their own colours, TIDAL's classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

/// Installs `css` as a theme over `frame`'s anchors and returns the overlay
/// once `ready` holds for it.
fn overlay_when(
    name: &str,
    css: &str,
    frame: &dom_anchors::Frame,
    ready: impl Fn(&DomOverlay) -> bool,
) -> Arc<DomOverlay> {
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
        if o.css_bytes > 0 && ready(&o) {
            break o;
        }
        assert!(Instant::now() < end, "{name} never took");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    o
}

/// TIDAL's bar glass is `._player_::after`, with the page blurred through it
/// by TIDAL's own `backdrop-filter`. A theme that only makes its colour
/// clear keeps that filter (Tidal Light's see-through bar): the bar was flat,
/// its fill over the window's colour. One that sets the filter too, or a
/// colour for the glass, is not this.
#[test]
fn a_clear_bar_glass_keeps_tidals_blur() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::FOOTER;
    let _drawn = backdrop::force(true);
    let after = |o: &DomOverlay| o.hosts.get(FOOTER).is_some_and(|h| h.after.is_some());
    let css = r#"/* {"name": "Clear glass"} */
:root { --frosted-glass-nav-blur: blur(9px); }
._player_8c80ae6 { background: rgba(10, 20, 30, 0.25); }
._player_8c80ae6:after { background: transparent; box-shadow: none; }
"#;
    let o = overlay_when("clear-glass.css", css, &dom_anchors::sample_frame(), after);
    pd::test_overlay(Some((*o).clone()));
    let (filter, own) = pd::bar_glass_backdrop().expect("TIDAL's blur stays");
    assert_eq!(filter.blur, backdrop::Backdrop::parse("blur(9px)").unwrap().blur);
    assert_eq!(
        own,
        Some(egui::Color32::from_rgba_unmultiplied(10, 20, 30, 64))
    );
    // TIDAL's own filter where the theme leaves the variable alone.
    let css = r#"/* {"name": "Clear glass 2"} */
._player_8c80ae6:after { background: transparent; }
"#;
    let o = overlay_when("clear-glass-2.css", css, &dom_anchors::sample_frame(), after);
    pd::test_overlay(Some((*o).clone()));
    let (filter, _) = pd::bar_glass_backdrop().expect("TIDAL's blur stays");
    assert_eq!(
        *filter,
        backdrop::Backdrop::parse("blur(20px) saturate(180%)").unwrap()
    );
    for (name, css) in [
        (
            "no-filter.css",
            r#"/* {"name": "No filter"} */
._player_8c80ae6:after { background: transparent; backdrop-filter: none; }
"#,
        ),
        (
            "coloured.css",
            r#"/* {"name": "Coloured glass"} */
._player_8c80ae6:after { background: rgb(1, 2, 3); }
"#,
        ),
        (
            "hidden.css",
            r#"/* {"name": "Hidden glass"} */
._player_8c80ae6:after { background: transparent; display: none; }
"#,
        ),
    ] {
        let o = overlay_when(name, css, &dom_anchors::sample_frame(), after);
        pd::test_overlay(Some((*o).clone()));
        assert!(pd::bar_glass_backdrop().is_none(), "{name}");
    }
    pd::test_overlay(None);
}
