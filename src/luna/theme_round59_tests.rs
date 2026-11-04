//! Round 59: what the parity checker showed against TIDAL 2.47.0's captures,
//! proved with made-up themes (their own colours, TIDAL's classes and
//! anchors only).

use super::dom_anchors;

/// TIDAL's header lies over `#main` (`position: fixed`, 56 high), whose
/// `padding-top: 56px` keeps its room. A theme that takes the room back
/// (Tidalify's `#main { margin-top: -30px; padding-top: 30px }`) lays the
/// page 56 higher, under the header (captured: the album's data container
/// at 54 against 110 with no theme). Tidalfast stopped the page at the
/// header's foot, 52 low. The header stays where it was.
#[test]
fn a_page_that_takes_back_the_headers_room_lies_under_the_header() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys::{HEADER, PAGE_DATA};
    let css = r#"/* {"name": "Page under header"} */
#main { margin-top: -30px; padding-top: 30px; outline-color: rgb(1, 2, 3); }
"#;
    let (plain, themed) =
        r41::plain_and_themed("page-under-header.css", css, dom_anchors::keys::MAIN);
    let (was, now) = (r41::top(&plain, PAGE_DATA), r41::top(&themed, PAGE_DATA));
    assert!(r41::near(now, was - 56.0), "the page 56 up: {was} -> {now}");
    assert!(
        r41::near(r41::top(&themed, HEADER), r41::top(&plain, HEADER)),
        "the header stays: {} -> {}",
        r41::top(&plain, HEADER),
        r41::top(&themed, HEADER)
    );
}

/// A corner radius in `%` is a share of the box's own size: a theme's
/// round 40-point navigation buttons (Lunallena's `[data-test=
/// "navigation-arrows"] button { width: 40px; height: 40px; border-radius:
/// 50% }`, captured 20 in TIDAL) and its 10-point progress knob (5). The
/// share was taken of nothing, so the buttons' fills were drawn square.
#[test]
fn a_corner_radius_in_percent_is_a_share_of_the_box() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{NAV_BACK, NAV_FORWARD, PROGRESS_KNOB};
    let css = r#"/* {"name": "Round arrows"} */
[data-test="navigation-arrows"] button { background-color: rgba(0, 0, 0, 0.1) !important; border-radius: 50%; width: 40px; height: 40px; }
[class*='_knob_'] { width: 10px; height: 10px; border-radius: 50%; }
[data-test="navigation-arrows"] button:disabled { border-radius: 10%; }
"#;
    let o = super::theme_round38_tests::overlay(
        "round-arrows.css",
        css,
        &dom_anchors::sample_frame(),
        NAV_BACK,
    );
    pd::test_overlay(Some((*o).clone()));
    let corners = |key| pd::anchor_look(key).and_then(|l| l.corners);
    let (back, forward, knob) = (
        corners(NAV_BACK),
        corners(NAV_FORWARD),
        corners(PROGRESS_KNOB),
    );
    pd::test_overlay(None);
    let round = |n| Some(egui::CornerRadius::same(n));
    assert!(
        [back, forward].contains(&round(20)),
        "half of 40: {back:?} {forward:?}"
    );
    assert_eq!(knob, round(5), "half of 10");
}
