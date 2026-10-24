//! Luna themes round 23: what the round 22 comparison left open (Tidal
//! TUI's headings, square pills and see-through menus, the cotton candy
//! family's pill icons, Cotton Candy Twilight's bar),
//! fixed in the general CSS mapping.

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

/// A page with TIDAL's Play and Shuffle pills.
fn frame_with_pills() -> dom_anchors::Frame {
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.play = Some(dom_anchors::HeaderButton {
            rect: Some([240.0, 300.0, 160.0, 48.0]),
            label: "Play".into(),
        });
        page.shuffle = Some(dom_anchors::HeaderButton {
            rect: Some([416.0, 300.0, 160.0, 48.0]),
            label: "Shuffle".into(),
        });
    }
    frame
}

/// Tidal TUI without its settings file: every `var(--font-size)`,
/// `var(--border-radius)` and `var(--tui-main)` names a variable nothing
/// defines, so in TIDAL each such declaration is `unset`. Its headings and
/// labels then inherit TIDAL's 14px body text, its Play and Shuffle are
/// square (`border-radius` back to 0) and its menus see-through
/// (`background-color` back to transparent, `[class*="Menu"]` takes the
/// shadow). Tidalfast left those declarations out, so the native 32 and 16
/// point titles and labels, round pills and the grey menu stayed. A
/// variable of a family TIDAL declares below the root (`--market-*`) is
/// not made `unset`: Tidalfast cannot know it is undefined there.
#[test]
fn an_undefined_variable_unsets_headings_pills_and_menus() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "TUI-like"} */
:root { --font-family: monospace; }
[class*="wave-text"], [class*="label"], button {
    font-size: var(--font-size) !important;
    font-weight: var(--font-weight) !important;
}
.wave-text-headline-bold {
    font-size: calc(var(--font-size) * var(--font-size-multiplier-header)) !important;
    letter-spacing: 6px;
}
[class*="medium"] { border-radius: var(--border-radius); }
[role="menu"] { background-color: var(--tui-main); border-radius: var(--border-radius); }
[class*="Menu"] { box-shadow: none !important; }
[class*="subheadline"] { background-color: var(--market-core-fill-black-color); }
"#;
    let tree = dom_anchors::build(&frame_with_pills());
    let (luna, o) = overlay_with(
        "tui-like.css",
        css,
        tree,
        dom_anchors::keys::PAGE_PLAY_LABEL,
    );
    pd::test_overlay(Some((*o).clone()));
    // Square pills with TIDAL's 14px, weight 500 labels.
    let play = pd::header_pill(true).expect("Play is styled");
    assert_eq!(play.radius, Some(0.0));
    assert_eq!(play.fill, None, "the pill keeps TIDAL's own colours");
    let label = pd::pill_font(true, crate::theme::semibold(16.0), 600);
    assert_eq!(label, crate::theme::medium(14.0));
    // Headings in the body's 14, the page title spaced as the theme asks.
    assert_eq!(pd::headline_font(32.0), crate::theme::medium(14.0));
    assert_eq!(pd::subheadline_font(19.0), crate::theme::medium(14.0));
    assert_eq!(pd::headline_spacing(), Some(6.0));
    // See-through, square menus without a shadow.
    let menu = pd::menu_look().expect("the menu box is styled");
    assert!(menu.clear);
    assert!(!menu.shadow);
    assert_eq!(menu.radius, Some(0.0));
    // TIDAL's own variable family is left alone.
    assert_eq!(
        o.hosts[dom_anchors::keys::THEME_SUBHEADLINE]
            .style
            .background_color,
        None
    );
    pd::test_overlay(None);
    luna.shutdown();
}

/// The cotton candy family fills the icon inside its pills apart from the
/// label (`._icon_77f3f89 > use { fill: ... }` nested in the pill's rule):
/// a pale glyph next to a black label in TIDAL, where Tidalfast drew the
/// icon in the label's colour.
#[test]
fn a_pills_icon_takes_the_fill_its_theme_gives_it() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Pill icons"} */
[data-test="header-controls"]{
._label_1d16h_1._label_1d16h_1{ color: rgb(0, 0, 0); }
._marketButton_1d16h_1._medium_1d16h_4._primary_1d16h_6{
    background-color: #00000000;
._icon_77f3f89 > use{fill: rgb(250, 200, 240) !important;}
    background-image: linear-gradient(var(--rotate), rgb(255, 0, 255), rgb(255, 0, 255));}
._marketButton_1d16h_1._medium_1d16h_4._secondary_1d16h_7{
    background-image: linear-gradient(var(--rotate), rgb(0, 0, 255), rgb(0, 0, 255));}
}
"#;
    let tree = dom_anchors::build(&frame_with_pills());
    let (luna, o) = overlay_with(
        "pill-icons.css",
        css,
        tree,
        dom_anchors::keys::PAGE_PLAY_ICON_USE,
    );
    pd::test_overlay(Some((*o).clone()));
    assert_eq!(
        pd::pill_icon(true),
        Some(egui::Color32::from_rgb(250, 200, 240))
    );
    assert_eq!(
        pd::pill_icon(false),
        None,
        "Shuffle's icon keeps its label's"
    );
    assert_eq!(
        pd::header_pill(true).and_then(|p| p.ink),
        Some(egui::Color32::BLACK)
    );
    pd::test_overlay(None);
    luna.shutdown();
}

/// Cotton Candy Twilight colours the bar's words and buttons dark on its
/// light bar: `._utilityButtons_ button`, the artist links, the heart's
/// sprite (`._button_c372e89 svg`) and the "..." in `._trackContent_`, the
/// clock's `._scrubberTime_`. The clock, the heart and the "..." had no
/// such classes, and the right-hand buttons the queue's and sound output's
/// buttons sit among in TIDAL took no colour; Tidal TUI's
/// `[data-test=next] { color: transparent }` hides the skip glyphs.
#[test]
fn the_bars_buttons_artist_and_clock_take_the_themes_colours() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::*;
    let css = r#"/* {"name": "Bar words"} */
._utilityButtons_4d7aaf9 button { color: rgb(10, 20, 30); }
[class="_trackContent_7b758a9"] { a { color: rgb(40, 50, 60); } }
._scrubberTime_f2b9e3c, ._scrubberTimeText_5696619 { color: rgb(70, 80, 90) !important; }
[data-test="next"], [data-test="previous"] { color: transparent !important; }
[class="_trackContent_7b758a9"] {
    ._contextMenuButton_9741d3a { color: rgb(10, 20, 30); }
    ._button_c372e89 svg { fill: rgb(10, 20, 30); }
}
"#;
    let (luna, o) = overlay_with(
        "bar-words.css",
        css,
        dom_anchors::sample_tree(),
        CURRENT_TIME,
    );
    pd::test_overlay(Some((*o).clone()));
    assert_eq!(
        pd::color(CURRENT_TIME),
        Some(egui::Color32::from_rgb(70, 80, 90))
    );
    assert_eq!(
        pd::color(&format!("{ARTIST}:0")),
        Some(egui::Color32::from_rgb(40, 50, 60))
    );
    let dark = Some(egui::Color32::from_rgb(10, 20, 30));
    assert_eq!(crate::ui::player_bar::utility_glyph(MUTE), dark);
    assert_eq!(crate::ui::player_bar::utility_glyph(QUEUE), dark);
    assert_eq!(pd::color(NEXT).map(|c| c.a()), Some(0));
    // The heart's sprite and the "..." beside it.
    assert_eq!(pd::icon_fill(FAVORITE_ICON, FAVORITE_ICON_USE), dark);
    assert_eq!(pd::color(TRACK_MORE), dark);
    pd::test_overlay(None);
    assert_eq!(crate::ui::player_bar::utility_glyph(MUTE), None);
    luna.shutdown();
}

/// The cotton candy family's track list panel paints `radial-gradient(...
/// rgb(47, 10, 61, calc(.1 * .7)), ...)`: an alpha of .07, as a browser
/// computes it. The `calc()` was not read, so the alpha was missing and
/// the gradient opaque: a solid dark (Nights) or pink panel where TIDAL's
/// is nearly clear.
#[test]
fn a_colours_calc_alpha_is_computed() {
    use crate::ui::plugin_slots::css::color;
    assert_eq!(
        color("rgb(47, 10, 61, calc(.1 * .7))"),
        Some(egui::Color32::from_rgba_unmultiplied(47, 10, 61, 18))
    );
    assert_eq!(
        color("rgba(255, 103, 235, calc(0.5 * .7))").map(|c| c.a()),
        Some(89)
    );
    assert_eq!(
        color("hsla(56, 100%, 90%, calc(1 / 2))").map(|c| c.a()),
        Some(128)
    );
    assert_eq!(
        color("rgb(1, 2, 3)"),
        Some(egui::Color32::from_rgb(1, 2, 3))
    );
}

/// `backdrop-filter: brightness(.75)` darkens what shows through a box,
/// never the box's own colour: Sugarcoat's opaque hot-pink sidebar is
/// TIDAL's `rgb(255, 144, 186)` exactly, and the cotton candy family's
/// see-through pink queue panel darkens only the page under it. A black
/// veil at 25 % was laid over the box, its fill included (the sidebar was
/// `rgb(180, 115, 136)`, the queue panel a dull mauve).
#[test]
fn a_backdrop_brightness_darkens_only_what_shows_through() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::*;
    let css = r#"/* {"name": "Veil"} */
._container_b3e8f28 { background-color: rgba(255, 103, 235, .6); backdrop-filter: blur(.8em) brightness(.75); }
._sidebarWrapper_2f256ce { background-color: rgb(255, 144, 186); backdrop-filter: brightness(.75); }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([1795.0, 60.0, 334.0, 900.0]), false));
    let (luna, o) = overlay_with("veil.css", css, dom_anchors::build(&frame), QUEUE_PANEL);
    pd::test_overlay(Some((*o).clone()));
    // The opaque box keeps its colour.
    assert_eq!(
        pd::fill(SIDEBAR),
        Some(egui::Color32::from_rgb(255, 144, 186))
    );
    // The see-through one: 60 % pink, then 40 % of the page at .75.
    let queue = pd::fill(QUEUE_PANEL).expect("a fill");
    assert_eq!(queue.a(), 179);
    let over_white = crate::motion::blend(
        egui::Color32::WHITE,
        opaque(queue),
        f32::from(queue.a()) / 255.0,
    );
    let [r, g, b, _] = over_white.to_array();
    // 0.6 * (255, 103, 235) + 0.3 * 255
    assert!(
        (228..=232).contains(&r) && (135..=141).contains(&g) && (216..=220).contains(&b),
        "{over_white:?}"
    );
    let layers = pd::box_layers(&o.hosts[QUEUE_PANEL], &o.vars, [2137.0, 1183.0]);
    assert!(
        !layers
            .iter()
            .any(|l| matches!(l, pd::BoxLayer::Veil(c, _) if c.r() == 0 && c.a() > 0)),
        "no veil over the fill: {layers:?}"
    );
    pd::test_overlay(None);
    luna.shutdown();
}

/// `c` without its alpha (the unmultiplied colour).
fn opaque(c: egui::Color32) -> egui::Color32 {
    let [r, g, b, _] = egui::Rgba::from(c).to_srgba_unmultiplied();
    egui::Color32::from_rgb(r, g, b)
}
