//! Round 58: what the parity checker showed against TIDAL 2.47.0's captures,
//! proved with made-up themes (their own colours, TIDAL's classes and
//! anchors only).

use super::dom_anchors;

/// TIDAL 2.47.0 declares its title bar's height once, `:root {
/// --os-title-bar-height: 30px }`, and a theme's rules read it: a page box
/// kept `calc(100svh - var(--os-title-bar-height))` high (Lunallena's and
/// Giorgeous' `#main`) is 30 short of the window. The token was unknown,
/// so the declaration did not resolve and the box's height was lost.
#[test]
fn tidals_title_bar_height_token_sizes_a_themes_page_box() {
    use dom_anchors::keys::MAIN;
    let css = "/* {\"name\": \"Title bar token\"} */ #main { max-height: calc(100svh - var(--os-title-bar-height)); outline-color: rgb(1, 2, 3); }";
    let o = super::theme_round38_tests::overlay(
        "title-bar-token.css",
        css,
        &dom_anchors::sample_frame(),
        MAIN,
    );
    let max = o.hosts[MAIN].style.max_height.clone();
    assert_eq!(max.as_deref().map(str::trim), Some("calc(100svh - 30px)"));
}

