//! Round 44: what the parity checker's comparisons found. Proved on the
//! demo's pages, with made-up themes (their own values, TIDAL's classes and
//! anchors only) where a theme is involved.

use super::dom::DomOverlay;
use super::dom_anchors::keys;
use super::theme_round41_tests as r41;

/// TIDAL 2.46.0 starts a page's content 24 points under its header (the
/// album's and playlist's cover, My Collection's title: `#main`'s 56-point
/// header room, then the header container's 24): the cover at 110 under a
/// header ending at 86 (TIDAL's captured pages replayed in Chromium).
/// Tidalfast's lay 23 under it, so everything on those pages, the song
/// rows and their lines too, sat a point high.
#[test]
fn a_pages_content_starts_24_under_the_header() {
    for (page, key) in [
        ("album:alb0", "hero-cover"),
        ("playlist:pl0", "hero-cover"),
        ("albums", keys::PAGE_TITLE),
    ] {
        let (ctx, mut app) = r41::demo_app(page, Some("quality-pill"));
        let tree = r41::drawn(
            &ctx,
            &mut app,
            DomOverlay {
                version: 1,
                ..DomOverlay::default()
            },
        );
        app.backend.shutdown();
        let boxes = r41::boxes(&tree);
        let header = boxes[keys::HEADER];
        let top = boxes
            .get(key)
            .unwrap_or_else(|| panic!("{page}: no box for {key}"));
        // My Collection's title is a 32-point line box; Tidalfast's box is
        // the 36-point glyph row centred on it.
        let start = if key == keys::PAGE_TITLE {
            top[1] + top[3] / 2.0 - 16.0
        } else {
            top[1]
        };
        let below = start - (header[1] + header[3]);
        assert!(
            (below - 24.0).abs() < 0.26,
            "{page}: {key} starts {below} under the header ({top:?}, the header {header:?})"
        );
    }
}
