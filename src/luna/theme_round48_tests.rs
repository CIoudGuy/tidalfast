//! Round 48: what the parity checker's comparisons found. Proved on the
//! demo's pages, with made-up themes (their own values, TIDAL's classes and
//! anchors only).

use super::dom::DomOverlay;
use super::*;

/// Chromium paints what is inside a box with a `transform` on whole pixels
/// from the box, which keeps its own fractional place: under a
/// `translateY(-1.1em)` on the album's list box over a 1.4em top padding
/// and a 2-point border, TIDAL 2.47.0 lays the songs 97.19 below the box
/// and paints them, and their lines, 97 below it (the parity checker's
/// replay). Tidalfast drew them where the layout put them, a fraction of a
/// pixel off on every row.
#[test]
fn what_a_moved_list_box_holds_lies_on_whole_pixels_from_it() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let css = "/* {\"name\": \"Moved list\"} */\n.is-intersecting._occlusion_de6b9d3._defaultStyles_a1b9da9.ALBUM_ITEMS.tidal-ui__occlusion { transform: translateY(-1.1em); padding-top: 1.4em; border-style: solid; border-width: 2px; border-color: rgb(255, 255, 255); background-color: rgba(255, 0, 0, 0.2); }\n";
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("queue"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = r41::styled("moved-list.css", css, &plain, keys::LIST_PANEL);
    o.version = 2;
    let moved = r41::drawn(&ctx, &mut app, o);
    app.backend.shutdown();
    let top = |tree: &super::dom::HostTree, key: &str| {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.rect)
            .map(|r| r[1])
            .unwrap_or_else(|| panic!("no box for {key}"))
    };
    let header = "list:album:alb0:header";
    let before = top(&plain, header);
    let panel = top(&moved, keys::LIST_PANEL);
    // The box keeps its place (as `a_song_lists_box_moves_the_list_as_tidals_flow_does`).
    assert!(
        (panel - (before - 15.4)).abs() < 0.05,
        "the box at {panel}, the list's place {before}"
    );
    for key in [header, "row:album:alb0:0", "row:album:alb0:3"] {
        let below = top(&moved, key) - panel;
        assert!(
            (below - below.round()).abs() < 0.05,
            "{key} lies {below} below its box"
        );
    }
}
