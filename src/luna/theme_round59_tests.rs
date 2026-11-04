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
