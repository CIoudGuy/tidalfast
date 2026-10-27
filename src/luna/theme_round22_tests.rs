//! Luna themes round 22: themes compared view by view with TIDAL running
//! them (reference captures), and the general fixes that
//! comparison asked for.

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

/// TidalCustomTheme paints `#main` (the page) pink and names no colour
/// token; its dark red text reaches the palette through TIDAL's
/// `wave-text-*` classes. The page's text is read against the pink it is
/// drawn on: before, the window stayed TIDAL's black, the dark red failed
/// the contrast floor against it and the app's white text was kept, white
/// on pink.
#[test]
fn a_theme_painting_the_page_gives_the_window_its_colour_and_keeps_its_dark_text() {
    use crate::ui::plugin_dom::{contrast, themed};
    let css = r#"/* {"name": "Page colour"} */
:root {
    --kl-home-background-color: color-mix(in srgb, #fdb7e2, transparent 0%);
    --kl-text-color: #8a0022;
}
.wave-text-body-medium, .wave-text-headline-bold { color: var(--kl-text-color) !important; }
#main { background: var(--kl-home-background-color) !important; }
"#;
    let (luna, o) = overlay_with("page-colour.css", css, dom_anchors::sample_tree(), "");
    let p = themed(&crate::theme::Palette::dark(), &o);
    assert_eq!(p.window, egui::Color32::from_rgb(0xfd, 0xb7, 0xe2));
    assert!(!p.dark);
    assert_eq!(p.text, egui::Color32::from_rgb(0x8a, 0x00, 0x22));
    assert!(contrast(p.secondary, p.window) >= 2.0);
    luna.shutdown();
}

/// A declaration naming a variable nothing defines (no fallback) is invalid
/// at computed-value time, as in a browser: Tidal TUI's `outline: 14px
/// solid var(--tui-main)` and `border: var(--border-width)
/// var(--border-style)` (its settings file not installed) draw nothing in
/// TIDAL, where Tidalfast drew a grey frame round the page and the bar;
/// its `color` is inherited. TIDAL's `--wave-*` tokens and a gradient's
/// spinning angle stay for the app to resolve.
#[test]
fn a_declaration_naming_an_undefined_variable_is_dropped() {
    use dom_anchors::keys::MAIN;
    let css = r#"/* {"name": "Undefined"} */
:root { --known: 4px; }
#main {
    border: var(--border-width) var(--border-style);
    outline: 14px solid var(--tui-main);
    color: var(--tui-border-inactive);
    padding: var(--known);
    background-color: var(--wave-color-solid-base-brighter);
    background-image: linear-gradient(var(--rotate), red, blue);
}
@keyframes blink { from { --star1: 0; } to { --star1: 1; } }
.mainContent {
    animation: blink 10s infinite;
    background-image: radial-gradient(circle at 10% 20%, hsla(56, 100%, 90%, var(--star1)) .7px, transparent 0);
}
"#;
    let (luna, o) = overlay_with("undefined.css", css, dom_anchors::sample_tree(), MAIN);
    // A variable an animation sets is defined (Cotton Candy Twilight's
    // twinkling stars on its main container).
    assert!(
        o.hosts[dom_anchors::keys::MAIN_CONTENT]
            .style
            .background_image
            .as_deref()
            .is_some_and(|i| i.contains("var(--star1)"))
    );
    let main = &o.hosts[MAIN].style;
    assert_eq!(main.border, None);
    assert_eq!(main.outline, None);
    // Inherited: since round 25 said so (`inherit`), so a painter can tell
    // a rule that reached the colour from none (it still parses to no
    // colour of its own).
    assert_eq!(main.color.as_deref(), Some("inherit"));
    assert_eq!(main.padding.as_deref(), Some("4px"));
    assert_eq!(
        main.background_color.as_deref(),
        Some("var(--wave-color-solid-base-brighter)")
    );
    assert!(
        main.background_image
            .as_deref()
            .is_some_and(|i| i.contains("var(--rotate)"))
    );
    luna.shutdown();
}

/// `border-style`, `border-width` and `border-color` written apart (the
/// cotton candy family's track list and queue panels) make the border,
/// which was left out without the shorthand; and a border with no style
/// draws nothing, as in a browser (it was a grey line).
#[test]
fn border_longhands_make_a_border_and_a_border_needs_a_style() {
    use crate::ui::plugin_slots::css::border;
    use dom_anchors::keys::MAIN;
    let css = r#"/* {"name": "Longhands"} */
#main { border-style: solid; border-width: 2px; border-color: #123456; }
.mainContent { border-radius: .8em; border-style: solid none none none; border-width: 2px;
    border-image: linear-gradient(to right, #0000 1em, #0000 2%, rgb(1, 2, 3) 20%, rgb(1, 2, 3) 80%, #0000 100%) 1; }
"#;
    let (luna, o) = overlay_with("longhands.css", css, dom_anchors::sample_tree(), MAIN);
    assert_eq!(
        o.hosts[MAIN].style.border.as_deref(),
        Some("2px solid rgb(18, 52, 86)")
    );
    // One side only, coloured by its `border-image` (the family's line
    // over each track row; it was drawn as a whole grey box).
    let content = &o.hosts[dom_anchors::keys::MAIN_CONTENT].style;
    assert_eq!(content.border.as_deref(), Some("2px solid rgb(1, 2, 3)"));
    assert_eq!(content.border_width.as_deref(), Some("2px 0px 0px 0px"));
    luna.shutdown();
    assert_eq!(
        border("2px solid #123456"),
        Some((2.0, egui::Color32::from_rgb(0x12, 0x34, 0x56)))
    );
    assert_eq!(border(" "), None);
    assert_eq!(border("1px #fff"), None);
    assert_eq!(border("2px none #fff"), None);
    assert_eq!(border("solid red").map(|(w, _)| w), Some(3.0));
}

/// The open play queue is TIDAL's side panel `div._container_b3e8f28._isOpen_869124d`
/// around `div._playQueueWrapper_cbf5aaa`, and a theme's box for it (the
/// cotton candy family's tinted glass) is what the panel paints. The class
/// was on the now-playing view before, which the cotton candy family then
/// painted pink where TIDAL keeps the cover's colours.
#[test]
fn the_play_queue_is_tidals_side_panel_and_themes_reach_it() {
    use crate::ui::plugin_dom::{BoxLayer, box_layers};
    use dom_anchors::keys::*;
    let css = r#"/* {"name": "Queue"} */
._container_b3e8f28, ._container_b3e8f28._nowPlayingVisible_6285d60 {
    background-color: rgb(255, 103, 235);
    backdrop-filter: blur(.8em) brightness(.75);
}
[class="_playQueueWrapper_cbf5aaa"] { color: #fedcba; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([1795.0, 60.0, 334.0, 900.0]), false));
    frame.queue_title = Some(("Play queue".into(), Some([1811.0, 82.0, 120.0, 26.0])));
    let tree = dom_anchors::build(&frame);
    let (luna, o) = overlay_with("queue.css", css, tree, QUEUE_WRAPPER);
    assert!(
        !o.hosts.contains_key(NOW_PLAYING),
        "the now-playing view is not the queue's panel"
    );
    let panel = &o.hosts[QUEUE_PANEL];
    assert_eq!(
        panel.style.background_color.as_deref(),
        Some("rgb(255, 103, 235)")
    );
    assert_eq!(
        o.hosts[QUEUE_WRAPPER].style.color.as_deref(),
        Some("rgb(254, 220, 186)")
    );
    // The backdrop's brightness darkens only what shows through the
    // panel: under this opaque fill nothing does, so no veil is laid over
    // it (round 23: it darkened the fill itself).
    let layers = box_layers(panel, &o.vars, [2137.0, 1183.0]);
    assert!(
        !layers.iter().any(|l| matches!(
            l,
            BoxLayer::Veil(c, _) if *c == egui::Color32::from_black_alpha(64)
        )),
        "{layers:?}"
    );
    luna.shutdown();
}

/// TIDAL 2.43.2's header pills are `_marketButton_1d16h_1` buttons inside
/// `div[data-test=header-controls]`, and the cotton candy family nests its
/// rules for them there, with a nested rule for the icon before the
/// gradient: the pills take the gradient's colour (they stayed white).
#[test]
fn header_pills_inside_the_header_controls_take_a_nested_rules_gradient() {
    let css = r#"/* {"name": "Nested pills"} */
[data-test="header-controls-wrapper"],[data-test="header-controls"]{
._marketButton_1d16h_1._medium_1d16h_4._primary_1d16h_6{
    background-color: #00000000;
._icon_77f3f89 > use{fill: rgba(255, 255, 255, 0.6) !important;}
    background-image: linear-gradient(var(--rotate), rgb(255, 0, 255), rgb(255, 0, 255));
    border-radius: .9em;border: 0;}
._marketButton_1d16h_1._medium_1d16h_4._secondary_1d16h_7
{background-color: #00000000;
    background-image: linear-gradient(var(--rotate), rgb(0, 0, 255), rgb(0, 0, 255)); }
}
"#;
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
    let tree = dom_anchors::build(&frame);
    let (luna, o) = overlay_with(
        "nested-pills.css",
        css,
        tree,
        dom_anchors::keys::PAGE_SHUFFLE,
    );
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    let play = crate::ui::plugin_dom::header_pill(true).expect("Play is themed");
    assert_eq!(play.fill, Some(egui::Color32::from_rgb(255, 0, 255)));
    // Its corners are the theme's `.9em`, not TIDAL's full round.
    assert!(play.radius.is_some_and(|r| (12.0..=15.0).contains(&r)));
    let shuffle = crate::ui::plugin_dom::header_pill(false).expect("Shuffle is themed");
    assert_eq!(shuffle.fill, Some(egui::Color32::from_rgb(0, 0, 255)));
    crate::ui::plugin_dom::test_overlay(None);
    luna.shutdown();
}

/// The sidebar and the play queue keep TIDAL's own text colours under a
/// theme: TIDAL's white and 55 % white. The cotton candy family darkens
/// only the page's words (`#main { --wave-color-text-main: black }`), so
/// its sidebar and queue are white in TIDAL; they were the page's
/// near-black. Round 25: the root `--wave-*` tokens do not reach them
/// either (TIDAL running Rose Pine and Catppuccin, which set
/// `--wave-color-solid-contrast-fill` at the root, keeps its sidebar
/// names pure white and its nav labels 55 % white); round 22 took them
/// from those tokens. A theme's rules for TIDAL's market text (Tidal
/// Light's black) do recolour them. Without a theme the palette is the
/// app's, untouched.
#[test]
fn the_sidebar_and_queue_keep_tidals_text_colours_under_a_theme() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::{SidePanel, test_overlay, themed};
    let tidal_panel_text = |p| crate::ui::plugin_dom::tidal_panel_text(p, SidePanel::Sidebar);
    let page_only = r#"/* {"name": "Page only"} */
#main { --wave-color-text-main: black; background-color: #ffc0f0; }
.mainContent { --wave-color-solid-contrast-fill: black; }
"#;
    let (luna, o) = overlay_with("page-only.css", page_only, dom_anchors::sample_tree(), "");
    let palette = themed(&Palette::dark(), &o);
    assert!(palette.themed);
    test_overlay(Some((*o).clone()));
    let panel = tidal_panel_text(palette);
    assert_eq!(panel.text, egui::Color32::WHITE);
    assert_eq!(panel.secondary, egui::Color32::from_white_alpha(140));
    assert_eq!(panel.window, palette.window, "only the words change");
    // Without a theme nothing changes.
    assert_eq!(tidal_panel_text(Palette::dark()), Palette::dark());
    test_overlay(None);
    luna.shutdown();

    let root = r#"/* {"name": "Root tokens"} */
:root { --wave-color-solid-contrast-fill: #101010; --wave-color-solid-contrast-darker: #202020;
    --wave-color-text-secondary: #303030; --wave-color-solid-base-fill: #f0f0f0; }
"#;
    let (luna, o) = overlay_with("root-tokens.css", root, dom_anchors::sample_tree(), "");
    let palette = themed(&Palette::dark(), &o);
    test_overlay(Some((*o).clone()));
    let panel = tidal_panel_text(palette);
    assert_eq!(panel.text, egui::Color32::WHITE);
    assert_eq!(panel.secondary, egui::Color32::from_white_alpha(140));
    test_overlay(None);
    luna.shutdown();

    let light = r#"/* {"name": "Market text"} */
:root { --ink: #000; --ink-soft: #000000b3; --wave-color-solid-base-fill: #fff; }
body, [class^="_main"] { background-color: var(--wave-color-solid-base-fill) !important; }
._marketText_1lyag_1 { color: var(--ink) !important; }
._marketText_1lyag_1._text20_1lyag_291 { color: var(--ink-soft) !important; }
"#;
    let (luna, o) = overlay_with("market-text.css", light, dom_anchors::sample_tree(), "");
    let palette = themed(&Palette::dark(), &o);
    test_overlay(Some((*o).clone()));
    let panel = tidal_panel_text(palette);
    assert_eq!(panel.text, egui::Color32::BLACK);
    assert_eq!(
        panel.secondary,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0xb3)
    );
    test_overlay(None);
    luna.shutdown();
}

/// `--img: none` set on the album header's box itself (Tidal TUI's
/// `[class*="dataContainer"] { --img: none }`, no `::before` rule) takes
/// away the cover art TIDAL's own `::before` paints from that variable; the
/// wash was still drawn.
#[test]
fn img_none_on_the_header_box_removes_its_art() {
    use dom_anchors::keys::PAGE_DATA;
    let css = r#"/* {"name": "No art"} */
[class*="dataContainer"] { --img: none !important; }
[class*="nothingHere"]::before { content: ""; }
"#;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.data = Some([240.0, 92.0, 1040.0, 300.0]);
    }
    let (luna, o) = overlay_with("no-art.css", css, dom_anchors::build(&frame), PAGE_DATA);
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    assert!(crate::ui::plugin_dom::before_image_removed(PAGE_DATA));
    crate::ui::plugin_dom::test_overlay(None);
    luna.shutdown();
}

/// The cotton candy family leaves TIDAL's `._contextMenu_` see-through and
/// paints the popover round it (`[class="_container_d3060bb _visible_4bed23e"]`)
/// with a gradient: menus take that gradient's colour (they kept the
/// app's white), and the open context menu sits in that popover.
#[test]
fn a_menus_popover_gives_menus_their_colour() {
    let css = r#"/* {"name": "Menu glass"} */
[class="_container_d3060bb _visible_4bed23e"] {
    background-color: #0000;
    background-image: linear-gradient(var(--rotate),
        color-mix(in srgb, rgb(255, 0, 0) 50%, rgb(0, 0, 255) 50%),
        color-mix(in srgb, rgb(255, 0, 0) 50%, rgb(0, 0, 255) 50%)) !important;
}
._contextMenu_8606b2d { background-color: #0000; }
"#;
    let (luna, o) = overlay_with(
        "menu-glass.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::THEME_MENU_BOX,
    );
    let p = crate::ui::plugin_dom::themed(&crate::theme::Palette::dark(), &o);
    let [r, g, b, _] = p.overlay.to_array();
    assert!(
        (120..=135).contains(&r) && g < 8 && (120..=135).contains(&b),
        "{:?}",
        p.overlay
    );
    luna.shutdown();
}

/// Tidal TUI gives its monospace to TIDAL's text elements
/// (`[class*="wave-text"]`, `[class*="Text"]`, `button`), never to the
/// document: in TIDAL that is every word. The page's font is then the text
/// role's (`span.wave-text-body-medium`); before, it stayed the app's.
#[test]
fn a_font_given_to_tidals_text_elements_is_the_pages_font() {
    let css = r#"/* {"name": "Mono"} */
:root { --font-family: "JetBrains Mono", monospace; }
[class*="wave-text"], button { font-family: var(--font-family) !important; }
"#;
    let (luna, o) = overlay_with(
        "mono.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::THEME_TEXT,
    );
    assert_eq!(
        crate::ui::plugin_dom::page_font_family(&o),
        Some("\"JetBrains Mono\", monospace")
    );
    luna.shutdown();
}
