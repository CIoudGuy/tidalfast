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
    assert_eq!(
        filter.blur,
        backdrop::Backdrop::parse("blur(9px)").unwrap().blur
    );
    assert_eq!(
        own,
        Some(egui::Color32::from_rgba_unmultiplied(10, 20, 30, 64))
    );
    // TIDAL's own filter where the theme leaves the variable alone.
    let css = r#"/* {"name": "Clear glass 2"} */
._player_8c80ae6:after { background: transparent; }
"#;
    let o = overlay_when(
        "clear-glass-2.css",
        css,
        &dom_anchors::sample_frame(),
        after,
    );
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

/// An album's rows and heading row lie in the page's flow, each in a box
/// (`[data-test=media-table] > div > div > div` in TIDAL's album table): a
/// theme's border on that box pushes what follows down, as Abyss Neptune's
/// does to a pitch of 50. The rows stayed 48 apart and the heading had no box
/// a rule could reach.
#[test]
fn a_border_on_an_albums_row_boxes_moves_the_rows() {
    use crate::ui::plugin_dom as pd;
    let frame = dom_anchors::sample_frame();
    let list = frame.lists[0].id.clone();
    assert!(list.starts_with("album:"), "{list}");
    let header = format!("{}:header", dom_anchors::keys::list(&list));
    let css = r#"/* {"name": "Row boxes"} */
._nativeRowWrapper_20e35be > div { border: 1px solid rgb(7, 8, 9) !important; }
"#;
    let o = overlay_when("row-boxes.css", css, &frame, |o| {
        o.hosts.contains_key(&header)
    });
    pd::test_overlay(Some((*o).clone()));
    let ink = egui::Color32::from_rgb(7, 8, 9);
    assert_eq!(
        crate::ui::widgets::table_header_border(Some(&list)),
        Some(([1.0; 4], ink, 0.0))
    );
    let row = crate::ui::widgets::row_wrapper_borders(&list, 0);
    assert!(row.contains(&([1.0; 4], ink, 0.0)), "{row:?}");
    pd::test_overlay(None);
}

/// `aside#sidebar` is TIDAL 2.47.0's `_sidebar_4927556 _newSidebar_f871cc1`.
/// A theme's rule for an older build's `_sidebar_` module matches nothing
/// there and must not reach the sidebar here either; both of TIDAL's own do.
#[test]
fn only_tidals_own_sidebar_module_reaches_the_sidebar() {
    use dom_anchors::keys::SIDEBAR_NAV;
    let css = r#"/* {"name": "Old sidebar"} */
._sidebar_54f18f3 { background: rgb(1, 2, 3); }
._sidebar_4927556 { color: rgb(4, 5, 6); }
._newSidebar_f871cc1 { border-radius: 3px; }
"#;
    let o = overlay_when(
        "old-sidebar.css",
        css,
        &dom_anchors::sample_frame(),
        |o: &DomOverlay| {
            o.hosts
                .get(SIDEBAR_NAV)
                .is_some_and(|h| h.style.color.is_some())
        },
    );
    let host = o.hosts.get(SIDEBAR_NAV).unwrap();
    assert_eq!(host.style.background_color, None);
    assert!(host.style.border_radius.is_some());
}

/// TIDAL's "All playlists" heading is a box of its own, in the black of
/// `--market-core-fill-black-color`, which a sidebar a theme colours shows;
/// a rule for the box wins, and a light sidebar has no such black.
#[test]
fn the_library_headings_box_has_tidals_black() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::SIDEBAR;
    let colored = |o: &DomOverlay| o.hosts.get(SIDEBAR).is_some_and(|h| !h.style.is_empty());
    let css = r#"/* {"name": "Coloured sidebar"} */
._sidebarWrapper_2f256ce { background: rgb(30, 40, 50); }
"#;
    let o = overlay_when(
        "colored-sidebar.css",
        css,
        &dom_anchors::sample_frame(),
        colored,
    );
    pd::test_overlay(Some((*o).clone()));
    assert_eq!(pd::library_heading_box(true), Some(egui::Color32::BLACK));
    assert_eq!(pd::library_heading_box(false), None);
    let css = r#"/* {"name": "Heading box"} */
._sidebarWrapper_2f256ce { background: rgb(30, 40, 50); }
._sectionHeader_ca54638 { background: rgb(7, 8, 9); }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.heading = Some([10.0, 100.0, 160.0, 22.0]);
    let heading = |o: &DomOverlay| {
        o.hosts
            .get(dom_anchors::keys::LIB_HEADING)
            .is_some_and(|h| !h.style.is_empty())
    };
    let o = overlay_when("heading-box.css", css, &frame, heading);
    pd::test_overlay(Some((*o).clone()));
    assert_eq!(
        pd::library_heading_box(true),
        Some(egui::Color32::from_rgb(7, 8, 9))
    );
    pd::test_overlay(None);
}

/// `#footerPlayer` is the box that holds the bar's parts: a theme's
/// `transform` moves them, its `width` and border give them other room
/// (they stayed where they were, with only the bar's look moved). In TIDAL
/// the left of what the bar holds stays put here: 2 px left, 2 px of border.
#[test]
fn a_theme_moves_the_bars_parts_with_its_box() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::FOOTER;
    let css = r#"/* {"name": "Moved bar"} */
#footerPlayer {
    width: 100.5% !important;
    transform: translate(-2px, 2px);
    border: 2px solid rgba(1, 2, 3, 0.1);
}
"#;
    let o = overlay_when(
        "moved-bar.css",
        css,
        &dom_anchors::sample_frame(),
        |o: &DomOverlay| o.hosts.get(FOOTER).is_some_and(|h| h.style.width.is_some()),
    );
    pd::test_overlay(Some((*o).clone()));
    let view = egui::vec2(1000.0, 800.0);
    let bar = egui::Rect::from_min_max(egui::pos2(18.0, 712.0), egui::pos2(982.0, 800.0));
    let (outer, inner) = pd::footer_box(view)
        .expect("the theme's box")
        .place(bar, 18.0);
    assert_eq!(outer.min, egui::pos2(-2.0, 714.0));
    assert!((outer.width() - 1005.0).abs() < 0.01, "{outer:?}");
    assert_eq!(inner.min, egui::pos2(18.0, 714.0));
    assert!((inner.right() - 983.0).abs() < 0.01, "{inner:?}");
    pd::test_overlay(None);
    assert_eq!(pd::footer_box(view), None);
}

/// TIDAL's previous, play and next stand in wrappers of their own in
/// `._centerControls_`, and themes reach their glyphs through those (the
/// Cotton Candy Nights family's exact-class selectors). The anchors had the
/// buttons straight in the controls, so such a rule reached nothing and the
/// glyphs kept the button's colour.
#[test]
fn a_themes_rule_on_the_transports_wrappers_reaches_its_glyphs() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{BAR_ICONS, NEXT, PLAY, PREVIOUS};
    let css = r#"/* {"name": "Transport wrappers"} */
[class="_player_8c80ae6 _notFullscreen_60c5a14"] {
    [class="_playbackButton_c00b950 _playButtonInner_03d539f"],
    [class="_skipButtonWrapper_dc2a3e5"],
    [class="_skipButtonWrapper_dc2a3e5 _previousWrapper_17b7a1c"] {
        ._icon_77f3f89 { fill: rgb(21, 22, 23) !important; }
    }
}
._centerControls_527ef93 > div > button { color: rgb(1, 2, 3); }
"#;
    let icon = BAR_ICONS.iter().find(|(b, ..)| *b == NEXT).unwrap().1;
    let o = overlay_when(
        "wrappers.css",
        css,
        &dom_anchors::sample_frame(),
        |o: &DomOverlay| o.hosts.get(icon).is_some_and(|h| h.style.fill.is_some()),
    );
    pd::test_overlay(Some((*o).clone()));
    let ink = egui::Color32::from_rgb(21, 22, 23);
    for button in [PREVIOUS, PLAY, NEXT] {
        assert_eq!(pd::bar_icon(button), Some(ink), "{button}");
    }
    for button in [PREVIOUS, NEXT] {
        assert_eq!(
            pd::color(button),
            Some(egui::Color32::from_rgb(1, 2, 3)),
            "{button}"
        );
    }
    pd::test_overlay(None);
}
