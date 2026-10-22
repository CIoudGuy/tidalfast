//! Luna themes round 25: the comparison against the corrected TIDAL
//! captures (the cotton candy family's sidebar words, pressed queue button,
//! soft panels and adaptive Shuffle; Cotton Candy Twilight's list panel;
//! Tidal TUI's names), fixed in the general CSS mapping.

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

/// A sidebar with one library row.
fn frame_with_library_row() -> dom_anchors::Frame {
    let mut frame = dom_anchors::sample_frame();
    frame
        .sidebar
        .rows
        .push((Some([8.0, 300.0, 224.0, 52.0]), false));
    frame
}

/// The cotton candy colourways colour the sidebar's playlist names and
/// their second lines through TIDAL's market text classes
/// (`#sidebar ._marketText_1lyag_1._medium20_1lyag_192, ._text20_1lyag_291
/// { color: var(--SBTXT) }`, Baja Blast's green), and Tidal TUI's
/// `[class*="title"] { font-weight: var(--font-weight) }` (no value: TIDAL's
/// 500) makes the names medium. The library rows had no anchors for their
/// words, so the names stayed the panel's white, the second lines its grey
/// and the names semibold.
#[test]
fn library_rows_take_their_words_from_the_theme() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Sidebar words"} */
:root { --SBTXT: rgb(73, 120, 98); }
[id="sidebar"] {
._marketText_1lyag_1._text20_1lyag_291,._marketText_1lyag_1._medium20_1lyag_192 { color: var(--SBTXT) !important; }
}
[class*="title"] { font-weight: var(--font-weight) !important; }
"#;
    let tree = dom_anchors::build(&frame_with_library_row());
    let (luna, o) = overlay_with(
        "sidebar-words.css",
        css,
        tree,
        &dom_anchors::keys::lib_row_subtitle(0),
    );
    pd::test_overlay(Some((*o).clone()));
    let style = pd::library_row(
        egui::Rect::from_min_size(egui::pos2(8.0, 300.0), egui::vec2(224.0, 52.0)),
        false,
        false,
    )
    .expect("the row is styled");
    let green = Some(egui::Color32::from_rgb(73, 120, 98));
    assert_eq!(style.color, green, "the name");
    assert_eq!(style.secondary, green, "the second line");
    assert_eq!(style.title_weight, Some(500));
    pd::test_overlay(None);
    luna.shutdown();
}

/// TidalCustomTheme colours a paragraph of market text with its heading
/// blue (`[class^="_marketText_"][class*="_paragraph"] { color: #6f9bfb
/// !important }`): TIDAL's sidebar playlists have blue second lines under
/// dark red names. The second line's anchor was no paragraph, so it took
/// the dark red too.
#[test]
fn a_library_rows_second_line_is_a_paragraph() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Paragraphs"} */
[class^="_marketText_"] { color: #8a0022 !important; }
[class^="_marketText_"][class*="_paragraph"] { color: #6f9bfb !important; }
"#;
    let tree = dom_anchors::build(&frame_with_library_row());
    let (luna, o) = overlay_with(
        "paragraphs.css",
        css,
        tree,
        &dom_anchors::keys::lib_row_subtitle(0),
    );
    pd::test_overlay(Some((*o).clone()));
    let style = pd::library_row(
        egui::Rect::from_min_size(egui::pos2(8.0, 300.0), egui::vec2(224.0, 52.0)),
        false,
        false,
    )
    .expect("the row is styled");
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(style.color, Some(egui::Color32::from_rgb(0x8a, 0x00, 0x22)));
    assert_eq!(
        style.secondary,
        Some(egui::Color32::from_rgb(0x6f, 0x9b, 0xfb))
    );
}

/// Giorgeous and Sunken Tidal set `h2 { text-transform: uppercase
/// !important }`: TIDAL's section titles and the play queue's title are
/// capitals ("YOUR FAVORITE ARTISTS", "PLAY QUEUE"). Tidalfast drew them
/// as written. Page titles (TIDAL's `h1` and headline words) stay as they
/// are under a rule for `h2` alone.
#[test]
fn headings_take_a_themes_text_transform() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Capitals"} */
h2 { font-weight: 600 !important; text-transform: uppercase !important; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([900.0, 92.0, 350.0, 600.0]), false));
    frame.queue_title = Some(("Play queue".into(), Some([920.0, 110.0, 200.0, 26.0])));
    let (luna, o) = overlay_with(
        "capitals.css",
        css,
        dom_anchors::build(&frame),
        dom_anchors::keys::QUEUE_TITLE,
    );
    pd::test_overlay(Some((*o).clone()));
    let section = pd::subheadline_case("Your favorite artists").into_owned();
    let queue = pd::queue_title_case("Play queue").into_owned();
    let page = pd::headline_case("Albums").into_owned();
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(section, "YOUR FAVORITE ARTISTS");
    assert_eq!(queue, "PLAY QUEUE");
    assert_eq!(page, "Albums");
    assert_eq!(
        pd::subheadline_case("Your favorite artists"),
        "Your favorite artists"
    );
}

/// TIDAL marks the open queue's button `_activeButton_122ab25`, which the
/// cotton candy family paints as a rounded gradient square
/// (`border-radius: .5em; background-image: linear-gradient(...)`). The
/// anchor had no such class, so the button kept Tidalfast's grey disc.
#[test]
fn the_open_queues_button_is_tidals_active_button() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE;
    let css = r#"/* {"name": "Active button"} */
._activeButton_122ab25 { border-radius: .5em !important; background-color: #fff0;
    background-image: linear-gradient(90deg, rgb(255, 150, 240), rgb(200, 160, 255)); }
"#;
    let mut frame = dom_anchors::sample_frame();
    if let Some(player) = frame.player.as_mut() {
        player.queue_open = true;
    }
    let (luna, o) = overlay_with("active-button.css", css, dom_anchors::build(&frame), QUEUE);
    pd::test_overlay(Some((*o).clone()));
    assert!(pd::paints_box(QUEUE), "the pressed button's gradient");
    // .5em of the button's 14px text.
    assert_eq!(pd::radius(QUEUE), Some(7.0));
    pd::test_overlay(None);
    // Closed, the button is not active and keeps its native look.
    if let Some(player) = frame.player.as_mut() {
        player.queue_open = false;
    }
    let tree = dom_anchors::build(&frame);
    let queue = tree
        .nodes
        .iter()
        .find(|n| n.key == QUEUE)
        .expect("the button");
    assert!(
        !queue
            .attrs
            .iter()
            .any(|(k, v)| k == "class" && v.contains("_activeButton_122ab25"))
    );
    luna.shutdown();
}

/// The cotton candy family's panels glow inward (`box-shadow: inset 0 0
/// 6em ...` on the queue panel) and cast a soft shadow (`0 0 2em ...`).
/// Neither was drawn for a themed box.
#[test]
fn a_themed_panels_shadows_are_drawn() {
    use crate::ui::theme_layout::{inset_shadow, shadow};
    let vars = std::collections::BTreeMap::new();
    let viewport = [2137.0, 1183.0];
    let both = "inset 0px 0px 6em rgba(255, 255, 255, .4), 0px 0px 2em rgba(0, 0, 0, .2)";
    let (color, blur, spread) = inset_shadow(Some(both), &vars, viewport).expect("the glow");
    assert_eq!(
        color,
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 102)
    );
    assert_eq!((blur, spread), (96.0, 0.0));
    assert!(shadow(Some(both), &vars, viewport).is_some());
    assert!(inset_shadow(Some("0 0 2em black"), &vars, viewport).is_none());

    // Drawn: the queue panel with the glow paints more than without it.
    use crate::ui::plugin_dom as pd;
    let shapes = |css: &str, name: &str| {
        let mut frame = dom_anchors::sample_frame();
        frame.queue_panel = Some((Some([1795.0, 60.0, 334.0, 900.0]), false));
        let (luna, o) = overlay_with(
            name,
            css,
            dom_anchors::build(&frame),
            dom_anchors::keys::QUEUE_PANEL,
        );
        pd::test_overlay(Some((*o).clone()));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            pd::queue_panel(
                ui,
                egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(334.0, 600.0)),
                false,
            );
        });
        out.textures_delta.clear();
        pd::test_overlay(None);
        luna.shutdown();
        // A filled shape would lie inside the see-through box: an outer
        // shadow shows only outside it.
        let filled = out
            .shapes
            .iter()
            .any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill.a() > 0));
        (out.shapes.len(), filled)
    };
    let (plain, _) = shapes(
        r#"/* {"name": "Plain"} */ ._container_b3e8f28 { background-color: rgba(255, 103, 235, .6); }"#,
        "plain-panel.css",
    );
    let (bordered, _) = shapes(
        r#"/* {"name": "Border"} */ ._container_b3e8f28 { border: 2px solid white; }"#,
        "border-panel.css",
    );
    let (shadowed, filled) = shapes(
        r#"/* {"name": "Shadow"} */ ._container_b3e8f28 { border: 2px solid white; box-shadow: 0 0 2em rgba(0, 0, 0, .2); }"#,
        "shadow-panel.css",
    );
    assert!(shadowed > bordered + 10, "{bordered} -> {shadowed}");
    assert!(!filled, "nothing filled inside the see-through box");
    let (glowing, _) = shapes(
        &format!(
            r#"/* {{"name": "Glow"}} */ ._container_b3e8f28 {{ background-color: rgba(255, 103, 235, .6); box-shadow: {both}; }}"#
        ),
        "glow-panel.css",
    );
    assert!(glowing > plain + 10, "{plain} -> {glowing}");
}

/// `backdrop-filter: saturate(1.2) brightness(1.2)` multiplies what is
/// behind the box: over Cotton Candy Twilight's navy window TIDAL's track
/// list panel is a deeper navy, and the cotton candy family's feed card a
/// brighter pink. A white veil (10 %) turned them grey and greyer pink;
/// the veil is now the faintest that makes the window's filtered colour.
/// Darker stays a black veil.
#[test]
fn a_brighter_backdrop_makes_the_windows_filtered_colour() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE_PANEL;
    let veil = |filter: &str, name: &str| {
        let css = format!(
            r#"/* {{"name": "Brightness"}} */
:root {{ --wave-color-solid-base-fill: rgb(16, 4, 55); }}
._container_b3e8f28 {{ backdrop-filter: {filter}; }}
"#
        );
        let mut frame = dom_anchors::sample_frame();
        frame.queue_panel = Some((Some([1795.0, 60.0, 334.0, 900.0]), false));
        let (luna, o) = overlay_with(name, &css, dom_anchors::build(&frame), QUEUE_PANEL);
        pd::test_overlay(Some((*o).clone()));
        let layers = pd::box_layers(&o.hosts[QUEUE_PANEL], &pd::vars(), [2137.0, 1183.0]);
        pd::test_overlay(None);
        luna.shutdown();
        layers.iter().find_map(|l| match l {
            pd::BoxLayer::Fill(c, _) => Some(*c),
            _ => None,
        })
    };
    // Over the navy window the veil makes TIDAL's filtered navy: more
    // saturated and a little brighter, never grey.
    let tint = veil("saturate(1.2) brightness(1.2)", "brighter.css").expect("a veil");
    let window = [16.0, 4.0, 55.0];
    let a = f32::from(tint.a()) / 255.0;
    let [r, g, b, _] = tint.to_srgba_unmultiplied();
    let over = [r, g, b].map(f32::from);
    let shown: Vec<f32> = (0..3)
        .map(|i| over[i] * a + window[i] * (1.0 - a))
        .collect();
    let target = crate::css_filter::ColorFilter::parse("saturate(1.2) brightness(1.2)")
        .unwrap()
        .apply_color([16, 4, 55])
        .map(f32::from);
    for i in 0..3 {
        assert!((shown[i] - target[i]).abs() <= 2.0, "{shown:?} {target:?}");
    }
    assert!(target[2] > 70.0 && target[0] < 25.0, "{target:?}");
    assert!(a < 0.25, "the window's stars keep most of their own: {a}");
    assert_eq!(veil("blur(4px)", "blur-only.css"), None);
    assert_eq!(
        veil("brightness(.75)", "darker.css"),
        Some(egui::Color32::from_black_alpha(64))
    );
}

/// TIDAL's Shuffle pill keeps its white words under a theme that makes the
/// page's words black (the adaptive Cotton Candy in Light): only the
/// label's own rules recolour it. It was drawn in the page's black.
#[test]
fn the_shuffle_label_keeps_tidals_white_under_a_theme() {
    let mut palette = crate::theme::Palette::light();
    let (_, ink) = crate::ui::collection::native_pill(&palette, false, 0.0);
    assert_eq!(ink, palette.text, "without a theme, the app's own");
    palette.themed = true;
    palette.text = egui::Color32::BLACK;
    let (_, ink) = crate::ui::collection::native_pill(&palette, false, 0.0);
    assert_eq!(ink, egui::Color32::WHITE);
}

/// TIDAL heads the playlists with "All playlists" in its grey, which the
/// cotton candy family turns 64 % white (`[class="_sectionHeader_ca54638"]
/// ._titleText_1d0e44e { color }`); Rose Pine and Catppuccin, which name
/// neither, leave it grey. The heading had no anchors, so it stayed grey
/// under every theme (round 25's first pass made it white under every
/// theme, which TIDAL running Rose Pine does not).
#[test]
fn the_playlists_heading_takes_the_themes_colour_for_it() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::LIB_HEADING_TITLE;
    let r = egui::Rect::from_min_size(egui::pos2(16.0, 260.0), egui::vec2(160.0, 22.0));
    let heading = |css: &str, name: &str| {
        let mut frame = dom_anchors::sample_frame();
        frame.sidebar.heading = Some([16.0, 260.0, 160.0, 22.0]);
        let (luna, o) = overlay_with(name, css, dom_anchors::build(&frame), "");
        pd::test_overlay(Some((*o).clone()));
        let c = pd::library_heading(r, egui::Color32::WHITE);
        pd::test_overlay(None);
        luna.shutdown();
        c
    };
    assert_eq!(
        heading(
            r#"/* {"name": "Heading"} */ [class="_sectionHeader_ca54638"] { ._titleText_1d0e44e { color: rgba(255, 255, 255, 0.644) } }"#,
            "heading-cc.css",
        ),
        Some(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 164))
    );
    // With no value the rule leaves it the sidebar's white (Tidal TUI).
    assert_eq!(
        heading(
            r#"/* {"name": "Unset"} */ ._sectionHeader_ca54638 { color: var(--tui-border-inactive); }"#,
            "heading-tui.css",
        ),
        Some(egui::Color32::WHITE)
    );
    assert_eq!(
        heading(
            r#"/* {"name": "Own"} */ ._sectionHeader_ca54638 { color: rgb(1, 2, 3); }"#,
            "heading-own.css",
        ),
        Some(egui::Color32::from_rgb(1, 2, 3)),
        "the heading's box colours its words"
    );
    assert_eq!(
        heading(
            r#"/* {"name": "Tokens"} */ :root { --wave-color-solid-accent-fill: #c4a7e7; }"#,
            "heading-tokens.css",
        ),
        None,
        "TIDAL's grey"
    );
    let _ = LIB_HEADING_TITLE;
}

/// The cotton candy family draws Home's feed in a glass card
/// (`._feed_412931c`: a border, corners, a gradient, padding, and
/// `max-width: calc(88vw - var(--sidebarWidth) - (var(--playQueueWidth) *
/// .9))`), which narrows the feed in TIDAL. Nothing of it was drawn and
/// the feed kept its own width; TIDAL's `--playQueueWidth` (350) was not
/// declared, so the width could not resolve.
#[test]
fn homes_feed_card_takes_the_themes_width_and_padding() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Feed card"} */
._feed_412931c { background-color: #fff0; border-radius: 1em; border-style: solid; border-width: 2px;
    border-color: rgb(255, 150, 240);
    min-width: calc(88vw - var(--sidebarWidth) - (var(--playQueueWidth)*.9)) !important;
    max-width: calc(88vw - var(--sidebarWidth) - (var(--playQueueWidth)*.9)) !important;
    padding-top: 1.2em; padding-left: 2.2em; padding-right: 2.1em; padding-bottom: 0em; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.viewport = [2137.0, 1183.0];
    frame.sidebar.width = 240.0;
    if let Some(p) = frame.page.as_mut() {
        p.feed = true;
    }
    let (luna, o) = overlay_with(
        "feed-card.css",
        css,
        dom_anchors::build(&frame),
        dom_anchors::keys::PAGE,
    );
    pd::test_overlay(Some((*o).clone()));
    pd::note(|f| f.viewport = [2137.0, 1183.0]);
    let card = pd::feed_card().expect("the card");
    let width = card.width.expect("its width");
    assert!(
        (width - (0.88 * 2137.0 - 240.0 - 315.0)).abs() < 1.0,
        "{width}"
    );
    // In ems of the feed's 14-point text: TIDAL's card edge lies 2.2em
    // (30.8) left of the feed, measured on its capture.
    for (got, want) in card.padding.iter().zip([16.8, 29.4, 0.0, 30.8]) {
        assert!((got - want).abs() < 0.01, "{:?}", card.padding);
    }
    pd::test_overlay(None);
    assert_eq!(pd::feed_card(), None, "no card without a theme");
    luna.shutdown();
}

/// TIDAL's virtual list holds each track row two boxes down
/// (`div[k] > div > div._tableRow_`), so the cotton candy family's
/// `div:nth-child(1) > div > div { border-style: none }` takes the line off
/// the first row only: TIDAL draws no line over it, a line over every row
/// after. The rows were the table's own children, so every row kept its
/// line.
#[test]
fn only_the_first_track_row_loses_its_line() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::row;
    let css = r#"/* {"name": "Row lines"} */
div:nth-child(1) > div > div { border-style: none; }
._tableRow_299a426 { border-style: solid none none none; border-width: 2px; border-color: white; }
"#;
    let (luna, o) = overlay_with(
        "row-lines.css",
        css,
        dom_anchors::build(&dom_anchors::sample_frame()),
        &row("album:55", 1),
    );
    pd::test_overlay(Some((*o).clone()));
    let line = |i: usize| pd::outline_sides(&row("album:55", i), egui::Color32::RED);
    assert_eq!(line(0), None, "the first row");
    for i in [1, 2] {
        let (sides, color) = line(i).expect("a line over the row");
        assert_eq!(sides, [2.0, 0.0, 0.0, 0.0]);
        assert_eq!(color, egui::Color32::WHITE);
    }
    pd::test_overlay(None);
    luna.shutdown();
}

/// TIDAL draws the playing playlist's name in its accent, which is not its
/// white text class: a theme's rule on `._textWhite_` leaves it the accent,
/// while the cotton candy family's `#sidebar ._marketText_._medium20_`
/// colour reaches it (TIDAL under Cotton Candy draws it as pale as the
/// other names). Tidalfast drew it in the accent whatever the theme said.
#[test]
fn the_playing_playlists_name_takes_the_themes_name_colour() {
    use crate::ui::plugin_dom as pd;
    let row = egui::Rect::from_min_size(egui::pos2(8.0, 300.0), egui::vec2(224.0, 52.0));
    let name_colour = |css: &str, name: &str| {
        let mut frame = frame_with_library_row();
        frame.sidebar.playing = Some(0);
        let (luna, o) = overlay_with(
            name,
            css,
            dom_anchors::build(&frame),
            &dom_anchors::keys::lib_row_title(0),
        );
        pd::test_overlay(Some((*o).clone()));
        let style = pd::library_row(row, false, true);
        pd::test_overlay(None);
        luna.shutdown();
        style.and_then(|s| {
            s.title_color
                .or(s.title_inherits.then_some(egui::Color32::TRANSPARENT))
        })
    };
    assert_eq!(
        name_colour(
            r#"/* {"name": "Names"} */ [id="sidebar"] ._marketText_1lyag_1._medium20_1lyag_192 { color: rgb(255, 228, 250) !important; }"#,
            "playing-names.css",
        ),
        Some(egui::Color32::from_rgb(255, 228, 250))
    );
    assert_eq!(
        name_colour(
            r#"/* {"name": "White"} */ ._marketText_1lyag_1 { font-size: 15px; } ._textWhite_1lyag_319 { color: rgb(9, 0, 26); }"#,
            "playing-white.css",
        ),
        None,
        "the accent stays"
    );
    // With no value (`--SBTXT` undefined in Cotton Candy itself) the rule
    // still wins: the name takes the colour around it, not the accent.
    assert_eq!(
        name_colour(
            r#"/* {"name": "Unset"} */ [id="sidebar"] ._marketText_1lyag_1._medium20_1lyag_192 { color: var(--SBTXT) !important; }"#,
            "playing-unset.css",
        ),
        Some(egui::Color32::TRANSPARENT),
        "inherits"
    );
}

/// TIDAL rounds an artist's picture by its own class on it, which a theme's
/// plain `[class*="image"] { border-radius: 10px }` (Meteor Night) does not
/// beat: the pictures stay round there. Tidal TUI's `!important` rule with
/// no value squares them, and Giorgeous's deeper `[class*="_card_"]
/// [class*="_imageContainer_"] [class*="round"]` rule gives them its
/// corners. Tidalfast kept every portrait round.
#[test]
fn a_theme_beating_tidals_circle_squares_an_artists_picture() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{card, card_cover_wrapper};
    let portrait = |css: &str, name: &str| {
        let mut frame = dom_anchors::sample_frame();
        let page = frame.page.as_mut().expect("a page");
        page.cards = vec![dom_anchors::GridCard {
            rect: Some([300.0, 300.0, 160.0, 210.0]),
            artist: true,
            cover: Some([300.0, 300.0, 160.0, 160.0]),
            shelf: None,
        }];
        let (luna, o) = overlay_with(
            name,
            css,
            dom_anchors::build(&frame),
            &card_cover_wrapper(0),
        );
        pd::test_overlay(Some((*o).clone()));
        let style = pd::grid_card_cover(
            &card(0),
            egui::Rect::from_min_size(egui::pos2(300.0, 300.0), egui::vec2(160.0, 160.0)),
        );
        pd::test_overlay(None);
        luna.shutdown();
        style.and_then(|s| s.portrait)
    };
    assert_eq!(
        portrait(
            r#"/* {"name": "Plain"} */ img, [class*="image"] { border-radius: 10px; }"#,
            "portrait-plain.css",
        ),
        None,
        "the circle stays"
    );
    assert_eq!(
        portrait(
            r#"/* {"name": "Important"} */ [class*="image"], [class*="_imageContainer_"] img { border-radius: var(--border-radius) !important; }"#,
            "portrait-important.css",
        ),
        Some(0.0)
    );
    assert_eq!(
        portrait(
            r#"/* {"name": "Deep"} */ [class*="_card_"] [class*="_imageContainer_"] [class*="round"] { border-radius: 4px; }"#,
            "portrait-deep.css",
        ),
        Some(4.0)
    );
}

/// Tidal TUI labels its boxes with `::before` / `::after` words (`Tabs`
/// over the navigation, `Playing` over the player bar, `content: "Tabs"`).
/// The overlay dropped the words of an anchor's pseudo-element, so none
/// was drawn. A glyph on a button (`▶` on Play) still reaches the overlay;
/// the painter leaves those to the native icon.
#[test]
fn a_themes_pseudo_element_words_reach_the_overlay() {
    use dom_anchors::keys::{FIXED_NAV, PLAY};
    let css = r#"/* {"name": "Box labels"} */
[class*="_fixedNavigation_"] { color: var(--tui-border-inactive) !important; }
[class*="_fixedNavigation_"]::before { margin: -10px -8px; content: "Tabs"; position: absolute; color: inherit; }
[data-test="play"]::before { content: "▶"; font-size: 20px; }
"#;
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    // The navigation's box is its rows' (TIDAL's `Tabs` sits at its
    // corner), not the whole sidebar.
    let nav = tree.nodes.iter().find(|n| n.key == FIXED_NAV).expect("nav");
    assert_eq!(nav.rect, Some([8.0, 50.0, 224.0, 32.0]));
    // With the rows not drawn (now playing covers the window) it has no
    // box, so no label shows over the view.
    let mut covered = dom_anchors::sample_frame();
    covered.sidebar.items.clear();
    let covered = dom_anchors::build(&covered);
    let nav_box = covered
        .nodes
        .iter()
        .find(|n| n.key == FIXED_NAV)
        .and_then(|n| n.rect);
    assert_eq!(nav_box, None);
    let (luna, o) = overlay_with("box-labels.css", css, tree, FIXED_NAV);
    let before = o.hosts[FIXED_NAV].before.as_ref().expect("the label's box");
    assert_eq!(before.content.as_deref(), Some("Tabs"));
    assert_eq!(before.margin.as_deref(), Some("-10px -8px"));
    if let Some(play) = o.hosts.get(PLAY).and_then(|h| h.before.as_ref()) {
        assert_eq!(play.content.as_deref(), Some("▶"));
    }
    luna.shutdown();
}

/// TIDAL's progress fill is `[data-test=progress-indicator]`, which themes
/// colour by its background: Lunallena's and Giorgeous's 7 % white, and
/// Tidal TUI's `var(--tui-border-active)` with no value, which leaves it
/// none (TIDAL shows no fill there). The bar drew the theme's accent.
#[test]
fn the_progress_fill_takes_the_themes_background_for_it() {
    use crate::ui::player_bar::progress_fill;
    use crate::ui::plugin_dom as pd;
    let fill = |css: &str, name: &str| {
        let (luna, o) = overlay_with(
            name,
            css,
            dom_anchors::sample_tree(),
            dom_anchors::keys::PROGRESS_RANGE,
        );
        pd::test_overlay(Some((*o).clone()));
        let c = progress_fill();
        pd::test_overlay(None);
        luna.shutdown();
        c
    };
    assert_eq!(
        fill(
            r#"/* {"name": "Faint"} */ [data-test="progress-indicator"] { background-color: rgba(255, 255, 255, 0.07); }"#,
            "fill-faint.css",
        ),
        Some(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18))
    );
    assert_eq!(
        fill(
            r#"/* {"name": "None"} */ [data-test="progress-indicator"] { z-index: -1; background-color: var(--tui-border-active); }"#,
            "fill-none.css",
        )
        .map(|c| c.a()),
        Some(0)
    );
    assert_eq!(progress_fill(), None, "without a theme, the app's own");
}

/// TIDAL's volume popup is its raised surface with a range in its words'
/// colour: under Tidal Light a pale box with a dark range. Tidalfast drew
/// TIDAL's dark box and white range under every theme.
#[test]
fn the_volume_popup_takes_the_themes_surface() {
    use crate::ui::player_utility::volume_popup_colors;
    let dark = crate::theme::Palette::dark();
    assert_eq!(
        volume_popup_colors(&dark),
        (
            egui::Color32::from_rgb(0x24, 0x24, 0x29),
            egui::Color32::WHITE
        ),
        "without a theme, TIDAL's own"
    );
    let mut light = crate::theme::Palette::light();
    light.themed = true;
    light.own_surfaces = false;
    light.overlay = egui::Color32::from_rgb(0xf4, 0xf4, 0xf4);
    light.text = egui::Color32::BLACK;
    assert_eq!(
        volume_popup_colors(&light),
        (
            egui::Color32::from_rgb(0xf4, 0xf4, 0xf4),
            egui::Color32::BLACK
        )
    );
}

/// Lunallena sizes and colours TIDAL's page and section titles through
/// their elements (`h1 { font-size: 30px !important; color: var(--accent)
/// !important }`, `[class*="headerContainer"] > h2 { ... }`): TIDAL draws
/// them 30 points and pale green. The stand-ins for those titles had no
/// `h1` or header box round them, so they stayed white at their own size.
#[test]
fn page_and_section_titles_take_their_elements_size_and_colour() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Titles"} */
:root { --accent: rgb(216, 255, 144); }
h1 { font-size: 30px !important; color: var(--accent) !important; }
[class*="headerContainer"] > h2 { font-size: 30px !important; color: var(--accent); }
"#;
    let (luna, o) = overlay_with(
        "titles.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::THEME_H2,
    );
    pd::test_overlay(Some((*o).clone()));
    let green = egui::Color32::from_rgb(216, 255, 144);
    assert_eq!(pd::headline_font(27.0).size, 30.0);
    assert_eq!(pd::subheadline_font(19.0).size, 30.0);
    assert_eq!(pd::headline_color(egui::Color32::WHITE), green);
    assert_eq!(pd::subheadline_color(egui::Color32::WHITE), green);
    pd::test_overlay(None);
    assert_eq!(
        pd::headline_color(egui::Color32::WHITE),
        egui::Color32::WHITE
    );
    luna.shutdown();
}

/// TIDAL's shortcut tile has an `::after` box that is see-through at rest:
/// Tidal Light's `._shortcutItem_6c8e7b4::after { background:
/// linear-gradient(#0000, #fff) }` leaves its tiles flat grey in TIDAL.
/// Tidalfast painted the white gradient over every tile.
#[test]
fn a_shortcut_tiles_after_box_is_see_through_at_rest() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::tile_item;
    let css = r#"/* {"name": "Tiles"} */
._shortcutItem_6c8e7b4 { background-color: rgb(240, 240, 240) !important; }
._shortcutItem_6c8e7b4::after { background: linear-gradient(#0000, #fff); }
"#;
    let mut frame = dom_anchors::sample_frame();
    if let Some(p) = frame.page.as_mut() {
        p.feed = true;
        p.tiles = vec![dom_anchors::Tile {
            rect: Some([300.0, 200.0, 400.0, 56.0]),
            image: Some([300.0, 200.0, 56.0, 56.0]),
            owned: true,
        }];
    }
    let (luna, o) = overlay_with("tiles.css", css, dom_anchors::build(&frame), &tile_item(0));
    pd::test_overlay(Some((*o).clone()));
    let layers = pd::box_layers(&o.hosts[&tile_item(0)], &pd::vars(), [2137.0, 1183.0]);
    pd::test_overlay(None);
    luna.shutdown();
    assert!(
        !layers.iter().any(|l| matches!(l, pd::BoxLayer::Paint(_))),
        "no gradient over the tile"
    );
}

/// TIDAL 2.43.2 no longer paints its window, cards, menus, grey words or
/// accent from a theme's root `--wave-*` tokens: under Rose Pine and the
/// Catppuccin flavours its window stays black, its sidebar words white and
/// grey, its playing playlist teal, and only the page's words take the
/// theme's `--wave-color-solid-contrast-fill`. Its play queue still reads
/// `--wave-color-solid-base-fill`, a third of the way to a mid grey
/// (measured: Rose Pine rgb(38, 37, 45), Mocha rgb(42, 42, 52), Macchiato
/// rgb(46, 48, 61), Frappe rgb(53, 55, 68)). Tidalfast painted the whole
/// window in the theme's ramp.
#[test]
fn a_theme_of_root_tokens_alone_keeps_tidals_window() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::{self as pd, queue_fill_from, themed};
    let css = r#"/* {"name": "Rose tokens"} */
:root { --wave-color-solid-base-fill: #191724; --wave-color-solid-base-brighter: #26233a;
    --wave-color-solid-contrast-fill: #e0def4; --wave-color-solid-contrast-darker: #908caa;
    --wave-color-solid-accent-fill: #9ccfd8; }
"#;
    let (luna, o) = overlay_with("rose-tokens.css", css, dom_anchors::sample_tree(), "");
    let base = Palette::dark();
    let p = themed(&base, &o);
    assert_eq!(p.window, base.window);
    assert_eq!(p.surface, base.surface);
    assert_eq!(p.accent, base.accent);
    assert_eq!(p.secondary, base.secondary);
    assert_eq!(p.text, egui::Color32::from_rgb(0xe0, 0xde, 0xf4));
    assert!(p.tidal_dark(), "TIDAL's own Dark shades stay");
    pd::test_overlay(Some((*o).clone()));
    let fill = pd::token_queue_fill().expect("the panel reads the token");
    pd::test_overlay(None);
    luna.shutdown();
    let near = |a: egui::Color32, b: [u8; 3]| {
        let d = |x: u8, y: u8| (i16::from(x) - i16::from(y)).abs();
        d(a.r(), b[0]) <= 2 && d(a.g(), b[1]) <= 2 && d(a.b(), b[2]) <= 2
    };
    assert!(near(fill, [38, 37, 45]), "{fill:?}");
    let rgb = egui::Color32::from_rgb;
    assert!(near(queue_fill_from(rgb(0x1e, 0x1e, 0x2e)), [42, 42, 52]));
    assert!(near(queue_fill_from(rgb(0x24, 0x27, 0x3a)), [46, 48, 61]));
    assert!(near(queue_fill_from(rgb(0x30, 0x34, 0x46)), [53, 55, 68]));
    assert!(near(queue_fill_from(egui::Color32::BLACK), [22, 22, 22]));
}

/// Abyss Neptune clears TIDAL's Play and Shuffle pills
/// (`[data-test="play-all"] { color: var(--wave-color-solid-accent-fill)
/// !important; background-color: transparent !important }`): TIDAL shows
/// their words alone. A see-through fill with no picture was read as no
/// fill, so the native white and grey pills stayed.
#[test]
fn a_see_through_header_pill_draws_no_pill() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Clear pills"} */
:root { --wave-color-solid-accent-fill: #ffffff; }
button[data-test="play-all"], button[data-test="shuffle-all"] { color: black; background-color: var(--wave-color-solid-accent-fill); }
[data-test="play-all"], [data-test="shuffle-all"] { color: var(--wave-color-solid-accent-fill) !important; background-color: transparent !important; }
"#;
    let (luna, o) = overlay_with(
        "clear-pills.css",
        css,
        dom_anchors::build(&dom_anchors::sample_frame()),
        dom_anchors::keys::PAGE_PLAY,
    );
    pd::test_overlay(Some((*o).clone()));
    let pill = pd::header_pill(true).expect("a themed pill");
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(pill.fill.map(|c| c.a()), Some(0), "no pill");
    assert_eq!(pill.ink, Some(egui::Color32::WHITE));
}

/// TidalCustomTheme colours TIDAL's words by `[class^="_marketText_"] {
/// color: #8a0022 !important }`, which reaches the navigation rows' labels
/// in TIDAL (dark red on its pink sidebar). The labels had no anchor, so
/// they kept the sidebar's near-white, unreadable on the pink.
#[test]
fn navigation_labels_take_the_themes_colour_for_their_words() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::sidebar_item_label;
    let css = r#"/* {"name": "Words"} */
[class^="_marketText_"] { color: #8a0022 !important; }
"#;
    let label = sidebar_item_label("sidebar-music");
    let (luna, o) = overlay_with(
        "nav-words.css",
        css,
        dom_anchors::build(&dom_anchors::sample_frame()),
        &label,
    );
    pd::test_overlay(Some((*o).clone()));
    let color = pd::nav_style(&label).and_then(|s| s.color);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(color, Some(egui::Color32::from_rgb(0x8a, 0x00, 0x22)));
}

/// TIDAL lays its own glass (`#282828` at 75 %, the bar's `::before`) over
/// a theme's fill for `#footerPlayer`: Tidal Amoled's black bar shows
/// rgb(31, 31, 31) in TIDAL and Minimalist Black's `#181818` rgb(36, 36,
/// 36). Tidalfast drew the theme's fill alone. A theme that styles that
/// `::before` (the cotton candy family hides it) keeps its own colour.
#[test]
fn tidals_bar_glass_lies_over_a_themes_bar_fill() {
    use crate::ui::plugin_dom as pd;
    let glass = |css: &str, name: &str, fill: egui::Color32| {
        let (luna, o) = overlay_with(
            name,
            css,
            dom_anchors::sample_tree(),
            dom_anchors::keys::FOOTER,
        );
        pd::test_overlay(Some((*o).clone()));
        let c = pd::footer_under_glass(fill, egui::Color32::BLACK);
        pd::test_overlay(None);
        luna.shutdown();
        c
    };
    let near = |c: egui::Color32, v: u8| (i16::from(c.r()) - i16::from(v)).abs() <= 1;
    let black = egui::Color32::BLACK;
    assert!(near(
        glass(
            r#"/* {"name": "Amoled"} */ #footerPlayer { background: #000 !important; }"#,
            "bar-amoled.css",
            black,
        ),
        31
    ));
    let grey = egui::Color32::from_gray(0x18);
    assert!(near(
        glass(
            r#"/* {"name": "Minimal"} */ #footerPlayer { background-color: #181818 !important; }"#,
            "bar-minimal.css",
            grey,
        ),
        36
    ));
    assert_eq!(
        glass(
            r#"/* {"name": "Own glass"} */ #footerPlayer { background-color: #000; } ._player_8c80ae6:before { display: none; }"#,
            "bar-own.css",
            black,
        ),
        black
    );
}

/// TidalCustomTheme fills `#footerPlayer` pink (`#ff90ba` at 90 %) and
/// clears the bar's `::after` (`[class^="_player_"]:after { background:
/// none; backdrop-filter: none }`), where TIDAL's glass is: TIDAL shows
/// the bar pink, rgb(220, 126, 161). Tidalfast laid the glass over it and
/// drew a dark maroon bar, rgb(88, 63, 72).
#[test]
fn a_theme_that_clears_the_bars_after_keeps_its_fill() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Pink bar"} */
#footerPlayer { background-color: color-mix(in srgb, #ff90ba, transparent 10%) !important; background: none; }
[class^="_player_"]:after { background: none; backdrop-filter: none; }
"#;
    let (luna, o) = overlay_with(
        "bar-after.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::FOOTER,
    );
    let fill = egui::Color32::from_rgba_unmultiplied(0xff, 0x90, 0xba, 230);
    pd::test_overlay(Some((*o).clone()));
    let c = pd::footer_under_glass(fill, egui::Color32::BLACK);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(c, fill);
}

/// TidalCustomTheme fills `#sidebar` (the `aside` inside TIDAL's sidebar
/// wrapper) with its sidebar pink, `#f89cbf`, and leaves the wrapper
/// alone: TIDAL's sidebar is that pink. Tidalfast took only the wrapper's
/// fill and drew the sidebar in the page's paler pink.
#[test]
fn a_fill_on_the_sidebars_aside_colours_the_sidebar() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{SIDEBAR, SIDEBAR_NAV};
    let css = r#"/* {"name": "Aside fill"} */
#sidebar { background-color: color-mix(in srgb, #f89cbf, transparent 0%) !important; background-image: none; }
"#;
    let (luna, o) = overlay_with(
        "aside-fill.css",
        css,
        dom_anchors::sample_tree(),
        SIDEBAR_NAV,
    );
    pd::test_overlay(Some((*o).clone()));
    let fill = pd::surface(SIDEBAR, egui::Color32::BLACK);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(fill, egui::Color32::from_rgb(0xf8, 0x9c, 0xbf));
}

/// TidalCustomTheme colours every `svg` dark red (`svg { color: #8a0022
/// !important; fill: #8a0022 !important }`): TIDAL's navigation icons are
/// dark red. The rows' icons had no anchors and kept Tidalfast's
/// near-white.
#[test]
fn navigation_icons_take_a_themes_svg_colour() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Icons"} */
svg { color: #8a0022 !important; fill: #8a0022 !important; }
"#;
    let key = dom_anchors::keys::sidebar_item_icon("sidebar-music");
    let (luna, o) = overlay_with("nav-icons.css", css, dom_anchors::sample_tree(), &key);
    pd::test_overlay(Some((*o).clone()));
    let ink = pd::sidebar_icon(&key);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(ink, Some(egui::Color32::from_rgb(0x8a, 0x00, 0x22)));
}

/// TIDAL's play queue panel is `#playQueueSidebar`: TidalCustomTheme's
/// `#sidebar, #playQueueSidebar, ... { background-color:
/// var(--kl-sidebar-color) !important }` makes it `#f89cbf` in TIDAL
/// (Blur's glass and Abyss Neptune's lifted panel come from the same id).
/// The panel's anchor had no id, so Tidalfast drew it in the page's pale
/// pink.
#[test]
fn the_play_queue_panel_is_play_queue_sidebar() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE_PANEL;
    let css = r#"/* {"name": "Queue sidebar"} */
:root { --kl-sidebar-color: color-mix(in srgb, #f89cbf, transparent 0%); }
#sidebar, #playQueueSidebar { background-color: var(--kl-sidebar-color) !important; background-image: none; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([1795.0, 60.0, 334.0, 900.0]), false));
    let (luna, o) = overlay_with(
        "queue-sidebar.css",
        css,
        dom_anchors::build(&frame),
        QUEUE_PANEL,
    );
    pd::test_overlay(Some((*o).clone()));
    let fill = pd::fill(QUEUE_PANEL);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(fill, Some(egui::Color32::from_rgb(0xf8, 0x9c, 0xbf)));
}

/// TIDAL colours a mix's title by `--track-vibrant-color`, which Abyss
/// Neptune sets itself (`--track-vibrant-color: white !important`): TIDAL
/// shows the title white there. Tidalfast drew it in the mix's own colour.
#[test]
fn a_themes_vibrant_colour_is_the_mix_titles() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Vibrant"} */
:root { --track-vibrant-color: white !important; }
"#;
    let (luna, o) = overlay_with("vibrant.css", css, dom_anchors::sample_tree(), "");
    pd::test_overlay(Some((*o).clone()));
    let c = pd::theme_var_color("--track-vibrant-color");
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(c, Some(egui::Color32::WHITE));
    assert_eq!(pd::theme_var_color("--track-vibrant-color"), None);
}

/// TIDAL 2.43.2 keeps its teal accent even under a theme that paints a dark
/// window and names an accent token: Black Glow's grey and Minimalist
/// Black's green leave TIDAL's playing playlist teal. Tidalfast drew it in
/// the token's colour. On a light window the token stands.
#[test]
fn a_painted_themes_accent_token_leaves_tidals_accent() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::themed;
    let css = r#"/* {"name": "Green"} */
:root { --wave-color-solid-accent-fill: #22aa55 !important; }
body { background-color: #000000 !important; }
"#;
    let (luna, o) = overlay_with("green-accent.css", css, dom_anchors::sample_tree(), "");
    let base = Palette::dark();
    let p = themed(&base, &o);
    luna.shutdown();
    assert_eq!(p.accent, base.accent);
    // A light window's theme draws TIDAL's accent in its own (the cotton
    // candy family's black playing row): the token stands.
    let css = r#"/* {"name": "Light accent"} */
:root { --wave-color-solid-accent-fill: #39afa5 !important; }
body { background-color: #ffffff !important; }
"#;
    let (luna, o) = overlay_with("light-accent.css", css, dom_anchors::sample_tree(), "");
    let p = themed(&base, &o);
    luna.shutdown();
    assert_eq!(p.accent, egui::Color32::from_rgb(0x39, 0xaf, 0xa5));
}

/// TIDAL sets `--img` inline on the album and playlist header's box and
/// paints its cover from it in `::before`. Abyss Neptune's
/// `[class^="_dataContainer_"]::before { background-image: var(--img) }`
/// keeps that cover in TIDAL (blurred), but Tidalfast had no value for the
/// variable, so the declaration was `unset` (no picture) and the art went.
#[test]
fn a_themes_img_variable_keeps_the_header_art() {
    use dom_anchors::keys::PAGE_DATA;
    let css = r#"/* {"name": "Header img"} */
[class^="_dataContainer_"]::before { background-image: var(--img); filter: blur(10px) brightness(0.4); }
"#;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.data = Some([240.0, 92.0, 1040.0, 300.0]);
    }
    let (luna, o) = overlay_with("header-img.css", css, dom_anchors::build(&frame), PAGE_DATA);
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    let removed = crate::ui::plugin_dom::before_image_removed(PAGE_DATA);
    crate::ui::plugin_dom::test_overlay(None);
    luna.shutdown();
    assert!(!removed, "the cover stays");
}

/// TIDAL's volume popup is filled with `--wave-color-solid-base-brighter`:
/// under Rose Pine its `#26233a` (measured rgb(38, 35, 58)), under Black
/// Glow and Abyss Neptune black, under Tidal Light `#eaebeb`. Tidalfast drew
/// TIDAL's own dark grey under a theme that keeps TIDAL's surfaces.
#[test]
fn the_volume_popup_takes_the_themes_brighter_base() {
    use crate::ui::player_utility::volume_popup_colors;
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Rose popup"} */
:root { --wave-color-solid-base-fill: #191724; --wave-color-solid-base-brighter: #26233a; }
"#;
    let (luna, o) = overlay_with("rose-popup.css", css, dom_anchors::sample_tree(), "");
    let palette = pd::themed(&crate::theme::Palette::dark(), &o);
    pd::test_overlay(Some((*o).clone()));
    let (fill, ink) = volume_popup_colors(&palette);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(fill, egui::Color32::from_rgb(0x26, 0x23, 0x3a));
    assert_eq!(ink, egui::Color32::WHITE);
}

/// TIDAL 2.43.2's navigation rows are `_navigationItem_4e9deae` only: the
/// older `_sidebarItem_730d27e` is not in its class list, so Abyss
/// Neptune's `[class^="_sidebarItem"] > span { color: ... }` reaches nothing
/// and TIDAL's Music label stays white. The anchors carried both modules,
/// so Tidalfast greyed the labels.
#[test]
fn an_older_sidebar_item_class_misses_the_navigation_rows() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::sidebar_item_label;
    let css = r#"/* {"name": "Old rows"} */
[class^="_sidebarItem"] > span { color: rgb(128, 128, 128); }
[class^="_marketText_"] { font-size: 15px; }
"#;
    let label = sidebar_item_label("sidebar-music");
    let (luna, o) = overlay_with(
        "old-rows.css",
        css,
        dom_anchors::build(&dom_anchors::sample_frame()),
        &label,
    );
    pd::test_overlay(Some((*o).clone()));
    let color = pd::nav_style(&label).and_then(|s| s.color);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(color, None, "the label keeps TIDAL's white");
}

/// Black Glow paints `body, html { background-color: #000 !important }`,
/// TIDAL's own black, and names a black token ramp. TIDAL's window is
/// unchanged by that fill and its tiles, queue, bar and menus keep TIDAL's
/// greys (tile about 20, queue 22, menus 45), but the fill counted as the
/// theme painting the window, so the black ramp repainted every surface.
#[test]
fn a_body_fill_of_tidals_own_black_keeps_tidals_surfaces() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::themed;
    let css = r#"/* {"name": "Black body"} */
:root { --wave-color-solid-base-fill: #000000; --wave-color-solid-base-bright: #000000;
    --wave-color-solid-base-brighter: #000000; --wave-color-solid-base-brightest: #181818;
    --wave-color-solid-contrast-fill: #e4e4e4; }
body, html { background-color: #000000 !important; }
"#;
    let (luna, o) = overlay_with("black-body.css", css, dom_anchors::sample_tree(), "");
    let base = Palette::dark();
    let p = themed(&base, &o);
    luna.shutdown();
    assert_eq!(p.surface, base.surface);
    assert_eq!(p.overlay, base.overlay);
    assert!(p.themed && p.own_surfaces, "TIDAL's own surfaces");
    // A body fill of another colour still paints the window (Catppuccin
    // Mocha Blue's base, the single-file Cotton Candy Nights' purple).
    let css = r#"/* {"name": "Purple body"} */
:root { --wave-color-solid-base-brighter: #20103a; }
body { background-color: rgb(2, 0, 10) !important; }
"#;
    let (luna, o) = overlay_with("purple-body.css", css, dom_anchors::sample_tree(), "");
    let p = themed(&base, &o);
    luna.shutdown();
    assert_eq!(p.window, egui::Color32::from_rgb(2, 0, 10));
    assert_eq!(p.surface, egui::Color32::from_rgb(0x20, 0x10, 0x3a));
}

/// TIDAL's library rows are links (`a`), so Black Glow's `a, button {
/// background: none !important }` clears the selected playlist's pill in
/// TIDAL. The row's anchor was a `div`, so Tidalfast kept the pill.
#[test]
fn a_rule_for_links_reaches_the_library_rows() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "No link fills"} */
a, button { background: none !important; }
._title_8b245d1 { font-size: 15px; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame
        .sidebar
        .rows
        .push((Some([8.0, 300.0, 224.0, 52.0]), true));
    let (luna, o) = overlay_with(
        "no-link-fills.css",
        css,
        dom_anchors::build(&frame),
        &dom_anchors::keys::lib_row_title(0),
    );
    pd::test_overlay(Some((*o).clone()));
    let fill = pd::library_row(
        egui::Rect::from_min_size(egui::pos2(8.0, 300.0), egui::vec2(224.0, 52.0)),
        true,
        false,
    )
    .and_then(|s| s.fill);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(fill, Some(egui::Color32::TRANSPARENT), "no pill");
}

/// TIDAL paints a track row's hover on its cells, rounding the first and
/// the last. Tidal TUI's `[data-test="tracklist-row"] div:first-child,
/// ... div:last-child { border-radius: var(--border-radius) !important }`
/// (no value: 0) squares them, so its hovered rows are square in TIDAL.
/// Tidalfast rounded the row's ends by 5 whatever the theme said.
#[test]
fn a_themes_cell_corners_shape_the_row_hover() {
    use crate::ui::plugin_dom as pd;
    use crate::ui::widgets::row_fill_corners;
    use dom_anchors::keys::row_cell;
    let css = r#"/* {"name": "Square cells"} */
[data-test="tracklist-row"] div:first-child,
[data-test="tracklist-row"] div:last-child { border-radius: var(--border-radius) !important; }
"#;
    let (luna, o) = overlay_with(
        "square-cells.css",
        css,
        dom_anchors::build(&dom_anchors::sample_frame()),
        &row_cell("album:55", 1, "context"),
    );
    pd::test_overlay(Some((*o).clone()));
    let corners = row_fill_corners("album:55", 1);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(corners, egui::CornerRadius::ZERO);
    assert_eq!(
        row_fill_corners("album:55", 1),
        egui::CornerRadius::same(5),
        "without a theme, TIDAL's 5"
    );
}

/// Tidal TUI labels the page's box `Main` with
/// `[class*="_hasDockedPlayer_"]:not([class*="_nowPlayingVisible_"])::before
/// { content: "Main" }`: TIDAL draws it at the page's corner beside the
/// sidebar, and not over the now-playing view. No anchor carried TIDAL's
/// `_hasDockedPlayer_b3ed021`, so the label was never drawn.
#[test]
fn the_pages_box_is_tidals_docked_player_box() {
    use dom_anchors::keys::MAIN;
    let css = r#"/* {"name": "Main label"} */
[class*="_hasDockedPlayer_"]::before { padding: 0 3px; z-index: 92; }
[class*="_hasDockedPlayer_"]:not([class*="_nowPlayingVisible_"])::before { margin: 64px 5px; content: "Main"; position: absolute; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.now_playing = None;
    let (luna, o) = overlay_with("main-label.css", css, dom_anchors::build(&frame), MAIN);
    let before = o.hosts[MAIN].before.as_ref().expect("the label's box");
    assert_eq!(before.content.as_deref(), Some("Main"));
    luna.shutdown();
    // Over the now-playing view the box is `_nowPlayingVisible_`: no label.
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let main = tree.nodes.iter().find(|n| n.key == MAIN).expect("main");
    let class = main
        .attrs
        .iter()
        .find(|(k, _)| k == "class")
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    assert!(class.contains("_hasDockedPlayer_b3ed021"), "{class}");
    assert!(class.contains("_nowPlayingVisible_"), "{class}");
}

/// Tidal Light colours TIDAL's Collection chevron and the playlists'
/// "+" and sort icons black (`._arrowRight_f57b400, ._icon_77f3f89 {
/// color: var(--icon-solid) !important }`), as TIDAL shows them. The icons
/// had no anchors, so they stayed Tidalfast's grey.
#[test]
fn the_sidebars_chevron_and_heading_icons_take_the_themes_colour() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{LIB_CREATE_ICON, LIB_SORT_ICON, sidebar_item_arrow};
    let css = r#"/* {"name": "Black icons"} */
:root { --icon-solid: #000; }
._arrowRight_f57b400, ._icon_77f3f89 { color: var(--icon-solid) !important; fill: var(--icon-solid) !important; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.items.push(dom_anchors::SidebarItem {
        test: "sidebar-collection-menu".into(),
        label: "Collection".into(),
        href: Some("/my-collection".into()),
        selected: false,
        rect: Some([16.0, 248.0, 208.0, 40.0]),
    });
    frame.sidebar.heading = Some([16.0, 320.0, 208.0, 40.0]);
    let arrow = sidebar_item_arrow("sidebar-collection-menu");
    let (luna, o) = overlay_with("black-icons.css", css, dom_anchors::build(&frame), &arrow);
    pd::test_overlay(Some((*o).clone()));
    let colors = [
        pd::color(&arrow),
        pd::color(LIB_CREATE_ICON),
        pd::color(LIB_SORT_ICON),
    ];
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(colors, [Some(egui::Color32::BLACK); 3]);
}

/// TIDAL's navigation rows have their colour on their class, so a theme's
/// rule for a tag does not reach them: Synthwave '77's `button { color:
/// magenta }` leaves the Collection row (TIDAL's only navigation `button`)
/// white like the other rows, while a rule for the row's class still
/// colours it. Tidalfast drew Collection magenta.
#[test]
fn a_tag_rule_leaves_the_navigation_rows_colour() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::sidebar_item;
    let collection = || dom_anchors::SidebarItem {
        test: "sidebar-collection-menu".into(),
        label: "Collection".into(),
        href: Some("/my-collection".into()),
        selected: false,
        rect: Some([16.0, 248.0, 208.0, 40.0]),
    };
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.items.push(collection());
    let row = sidebar_item("sidebar-collection-menu");
    // The arrow's rule only says when the theme reached the tree.
    let css = r#"/* {"name": "Tag ink"} */
:root { --tag-ink: rgb(225, 0, 255); }
button { color: var(--tag-ink); }
._arrowRight_f57b400 { opacity: 0.9; }
"#;
    let arrow = dom_anchors::keys::sidebar_item_arrow("sidebar-collection-menu");
    let (luna, o) = overlay_with("tag-ink.css", css, dom_anchors::build(&frame), &arrow);
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let tag = pd::nav_style(&row).and_then(|s| s.color);
    pd::test_overlay(None);
    assert_eq!(tag, None, "the row keeps its own colour");
    let css = r#"/* {"name": "Class ink"} */
[class*="_navigationItem_"] { color: rgb(225, 0, 255); }
"#;
    let (luna, o) = overlay_with("class-ink.css", css, dom_anchors::build(&frame), &row);
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let class = pd::nav_style(&row).and_then(|s| s.color);
    pd::test_overlay(None);
    assert_eq!(class, Some(egui::Color32::from_rgb(225, 0, 255)));
}

/// TIDAL's virtual list holds each track row in two boxes
/// (`[data-test="media-table"] > div > div[k] > div > div._tableRow_`).
/// Abyss Neptune lines the inner one (`> div > div > div { border: 1px
/// solid rgb(25, 25, 25) }`) and its macOS variant rounds the outer one
/// (`> div > div { border-radius: 8px; border: 1px solid ... }`); TIDAL
/// draws both round each row. Tidalfast read a border on the row only.
#[test]
fn a_themes_border_on_a_rows_wrappers_draws_round_the_row() {
    use crate::ui::plugin_dom as pd;
    use crate::ui::widgets::row_wrapper_borders;
    use dom_anchors::keys::row_cell;
    let css = r#"/* {"name": "Row wrappers"} */
:root { --ultra-thin: #fffafa1a; }
[data-test="media-table"]>div>div>div { border: 1px solid rgb(25, 25, 25) !important; }
[data-test="media-table"] > div > div { border-radius: 8px; border: 1px solid var(--ultra-thin) !important; }
"#;
    let (luna, o) = overlay_with(
        "row-wrappers.css",
        css,
        dom_anchors::build(&dom_anchors::sample_frame()),
        &row_cell("album:55", 1, "inner"),
    );
    pd::test_overlay(Some((*o).clone()));
    let borders = row_wrapper_borders("album:55", 1);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(
        borders,
        vec![
            (
                [1.0; 4],
                egui::Color32::from_rgba_unmultiplied(255, 250, 250, 0x1a),
                8.0
            ),
            ([1.0; 4], egui::Color32::from_rgb(25, 25, 25), 0.0),
        ]
    );
    assert!(row_wrapper_borders("album:55", 1).is_empty(), "no theme");
}

/// TIDAL's own CSS colours the header pills, not their labels, so Meteor
/// Night's catch-all `* { color: var(--text-primary) }` (white) reaches
/// the Play label: TIDAL shows its white Play pill with the words white on
/// white (b10, b12). Tidalfast took a label colour only with a theme's
/// fill and drew the words dark.
#[test]
fn a_catch_all_colour_reaches_a_header_pills_label() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::PAGE_PLAY_LABEL;
    let css = r#"/* {"name": "Catch-all"} */
:root { --text-primary: #ffffff; }
* { color: var(--text-primary); }
"#;
    let (luna, o) = overlay_with(
        "catch-all.css",
        css,
        dom_anchors::build(&dom_anchors::sample_frame()),
        PAGE_PLAY_LABEL,
    );
    pd::test_overlay(Some((*o).clone()));
    let pill = pd::header_pill(true);
    pd::test_overlay(None);
    luna.shutdown();
    let pill = pill.expect("the label is themed");
    assert_eq!(pill.fill, None, "TIDAL's own white pill");
    assert_eq!(pill.ink, Some(egui::Color32::WHITE));
}

/// Meteor Night's glass `[class*="card"] { background; border;
/// border-radius: 12px }` matches TIDAL's `._cardContainer_`, so TIDAL
/// draws a rounded glass box round each whole shelf row, under its cards
/// (its b01 and b13). Tidalfast gave the row an anchor but never painted
/// it. Without a theme the shelf paints nothing more.
#[test]
fn a_themes_box_on_a_shelf_row_is_drawn_under_its_cards() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Shelf glass"} */
[class*="card"] { background: rgba(10, 15, 30, 0.3) !important; border: 1px solid #ffffff14 !important; border-radius: 12px; }
"#;
    let mut frame = dom_anchors::sample_frame();
    if let Some(p) = frame.page.as_mut() {
        p.feed = true;
        p.shelves = vec![Some([260.0, 300.0, 1000.0, 260.0])];
    }
    let key = dom_anchors::keys::shelf(0);
    let (luna, o) = overlay_with("shelf-glass.css", css, dom_anchors::build(&frame), &key);
    let glass = egui::Color32::from_rgba_unmultiplied(10, 15, 30, 77);
    let marker = egui::Color32::from_rgb(1, 2, 3);
    let draw = || {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::widgets::shelf(
                ui,
                &crate::theme::Palette::dark(),
                "glass-shelf",
                "Shelf",
                |ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(100.0, 100.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 0.0, marker);
                },
            );
        });
        let mut out = out;
        out.textures_delta.clear();
        // The fills in drawing order.
        let mut fills = Vec::new();
        let mut stack: Vec<egui::Shape> = out.shapes.into_iter().map(|c| c.shape).rev().collect();
        while let Some(shape) = stack.pop() {
            match shape {
                egui::Shape::Vec(inner) => stack.extend(inner.into_iter().rev()),
                egui::Shape::Rect(r) => fills.push(r.fill),
                _ => {}
            }
        }
        fills
    };
    pd::test_overlay(Some((*o).clone()));
    let themed = draw();
    pd::test_overlay(None);
    luna.shutdown();
    let at = |fills: &[egui::Color32], c| fills.iter().position(|f| *f == c);
    let card = at(&themed, marker).expect("the card");
    let row = at(&themed, glass).expect("the row's glass");
    assert!(row < card, "the glass is under the card");
    assert_eq!(at(&draw(), glass), None, "without a theme");
}

/// The sidebar's and the play queue's words take what a theme's rules give
/// TIDAL's market text in each panel, never a variable by its name: a root
/// `--text-default` that no rule reads leaves TIDAL's white (Tidalfast
/// read that one theme's variable), a bare `._marketText_` rule recolours
/// both panels, and a rule scoped to TIDAL's queue rows
/// (`[class="_playQueueItemsContainer_01c88fe"]`, Sugarcoat's white queue
/// over its dark sidebar names) reaches the queue only.
#[test]
fn panel_words_follow_market_text_rules_not_a_variable_name() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::{SidePanel, test_overlay, themed, tidal_panel_text};
    use dom_anchors::keys::{QUEUE_ROLES, QUEUE_WRAPPER};
    let rgb = egui::Color32::from_rgb;
    let named = r#"/* {"name": "Named only"} */
:root { --text-default: #000; --text-secondary: #000000b3; }
#main { background-color: rgb(32, 48, 64); }
"#;
    let (luna, o) = overlay_with("named-only.css", named, dom_anchors::sample_tree(), "");
    let palette = themed(&Palette::dark(), &o);
    assert!(palette.themed);
    test_overlay(Some((*o).clone()));
    let sidebar = tidal_panel_text(palette, SidePanel::Sidebar);
    test_overlay(None);
    luna.shutdown();
    assert_eq!(sidebar.text, egui::Color32::WHITE);
    assert_eq!(sidebar.secondary, egui::Color32::from_white_alpha(140));

    let scoped = r#"/* {"name": "Scoped words"} */
:root { --plum: rgb(20, 0, 40); }
#main { background-color: rgb(32, 48, 64); }
._marketText_1lyag_1 { color: var(--plum); }
[class="_playQueueItemsContainer_01c88fe"] { ._marketText_1lyag_1._medium20_1lyag_192 { color: rgb(250, 240, 230); } }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([930.0, 92.0, 350.0, 600.0]), false));
    let tree = dom_anchors::build(&frame);
    let roles = tree
        .nodes
        .iter()
        .find(|n| n.key == QUEUE_ROLES)
        .expect("queue roles");
    assert_eq!(
        roles.parent.as_deref(),
        Some(QUEUE_WRAPPER),
        "in the open panel"
    );
    let (luna, o) = overlay_with("scoped-words.css", scoped, tree, "");
    let palette = themed(&Palette::dark(), &o);
    test_overlay(Some((*o).clone()));
    let sidebar = tidal_panel_text(palette, SidePanel::Sidebar);
    let queue = tidal_panel_text(palette, SidePanel::Queue);
    test_overlay(None);
    luna.shutdown();
    assert_eq!(sidebar.text, rgb(20, 0, 40));
    assert_eq!(sidebar.secondary, rgb(20, 0, 40));
    assert_eq!(queue.text, rgb(250, 240, 230));
    assert_eq!(queue.secondary, rgb(20, 0, 40));

    // A catch-all colour loses to TIDAL's own grey on a queue row's artist
    // line (Meteor Night's `* { color: white }`).
    let catch_all = r#"/* {"name": "Catch-all"} */
#main { background-color: rgb(32, 48, 64); }
* { color: rgb(200, 100, 0); }
"#;
    let (luna, o) = overlay_with("catch-all.css", catch_all, dom_anchors::sample_tree(), "");
    let palette = themed(&Palette::dark(), &o);
    test_overlay(Some((*o).clone()));
    let queue = tidal_panel_text(palette, SidePanel::Queue);
    test_overlay(None);
    luna.shutdown();
    assert_eq!(queue.secondary, egui::Color32::from_white_alpha(140));
}

/// A theme's `::before` / `::after` words and its `--track-vibrant-color`
/// are read whatever they say: a made-up theme with its own words (single
/// quotes, an `::after`) and colour gets them as Tidal TUI's `Tabs` and
/// `Main` and Abyss Neptune's white do.
#[test]
fn pseudo_words_and_vibrant_colour_are_not_one_themes() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{FIXED_NAV, MAIN};
    let css = r#"/* {"name": "Own words"} */
:root { --track-vibrant-color: rgb(12, 200, 99); }
[class*="_fixedNavigation_"]::after { content: 'Zone A'; position: absolute; }
[class*="_hasDockedPlayer_"]:not([class*="_nowPlayingVisible_"])::before { content: "Seite " "1"; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.now_playing = None;
    let (luna, o) = overlay_with("own-words.css", css, dom_anchors::build(&frame), MAIN);
    let after = o.hosts[FIXED_NAV]
        .after
        .as_ref()
        .expect("the nav's ::after");
    assert_eq!(after.content.as_deref(), Some("Zone A"));
    let before = o.hosts[MAIN].before.as_ref().expect("main's ::before");
    assert_eq!(before.content.as_deref(), Some("Seite 1"));
    pd::test_overlay(Some((*o).clone()));
    let vibrant = pd::theme_var_color("--track-vibrant-color");
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(vibrant, Some(egui::Color32::from_rgb(12, 200, 99)));
}

/// TIDAL 2.43.2 fills its window, the page and the sidebar, with its own
/// `--market-core-fill-black-color`: Sunken Tidal's `* { --market-core-fill-
/// black-color: rgb(30, 30, 30) }` makes TIDAL's page and sidebar grey (30)
/// while its play queue (22), menus and hovers keep TIDAL's own colours.
/// Tidalfast ignored the variable and drew the window black.
#[test]
fn tidals_window_variable_paints_the_window() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::themed;
    let css = r#"/* {"name": "Grey well"} */
:root { --well-edge: solid 1px rgb(255 255 255 / 10%); }
* { --market-core-fill-black-color: rgb(30, 30, 30) !important; }
"#;
    let (luna, o) = overlay_with("grey-well.css", css, dom_anchors::sample_tree(), "");
    luna.shutdown();
    let base = Palette::dark();
    let p = themed(&base, &o);
    assert_eq!(p.window, egui::Color32::from_gray(30));
    assert_eq!(p.surface, base.surface, "TIDAL's cards stay");
    assert_eq!(p.secondary, base.secondary, "TIDAL's grey words stay");
    assert!(p.tidal_dark(), "TIDAL's own Dark shades stay");
    assert_eq!(p.menu_bg(), base.menu_bg());
    // TIDAL's own black there changes nothing.
    let css = "/* {\"name\": \"Black well\"} */\n* { --market-core-fill-black-color: #000; }\n";
    let (luna, o) = overlay_with("black-well.css", css, dom_anchors::sample_tree(), "");
    luna.shutdown();
    assert_eq!(themed(&base, &o), base);
}

/// TIDAL 2.43.2's backdrop layers of the now playing view carry more than
/// their plain class: the cotton candy family and Sugarcoat hide
/// `[class="now-playing-background-image"]` and its siblings by the exact
/// attribute, which leaves TIDAL's blurred cover as without a theme (the
/// cover's dark colours behind white words), while Tidal Light's
/// `.now-playing-background-image { filter }` reaches it. Tidalfast's
/// stand-ins had the one class, so the view took the pale window and its
/// white words could not be read.
#[test]
fn exact_class_rules_leave_tidals_now_playing_backdrop() {
    use dom_anchors::keys::{NP_BG_BLACK, NP_BG_GRADIENT, NP_BG_IMAGE};
    let css = r#"/* {"name": "Pale view"} */
[class="now-playing-black-bg"] { display: none; }
[class="now-playing-background-image"] { display: none !important; }
[class="now-playing-gradient-overlay"] { display: none !important; }
.now-playing-gradient-overlay { background-color: rgb(250, 240, 245); }
"#;
    let (luna, o) = overlay_with(
        "pale-view.css",
        css,
        dom_anchors::sample_tree(),
        NP_BG_GRADIENT,
    );
    luna.shutdown();
    for key in [NP_BG_BLACK, NP_BG_IMAGE, NP_BG_GRADIENT] {
        assert!(!o.hidden(key), "{key}");
    }
    assert!(
        o.hosts[NP_BG_GRADIENT].style.background_color.is_some(),
        "a class rule reaches the layer"
    );
}

/// Lunallena's visible sidebar build pulls `#main` under the sidebar card
/// and keeps it as wide as the window (`left: calc(var(--sidebarWidth) *
/// -1); padding-left: 50px; min-width: 100vw`): TIDAL centres the album
/// across the whole window, under the open play queue too (its rows at
/// 361, beside the card). Tidalfast still kept the queue's room beside the
/// page, so the album's column was centred in what the panel leaves and
/// started under the card ("Playlists" read "lists").
#[test]
fn a_page_as_wide_as_the_window_runs_under_the_queue() {
    use crate::ui::plugin_dom as pd;
    use crate::ui::queue::reserve_in;
    let css = r#"/* {"name": "Wide page"} */
:root { --rail: 240px; }
#main { left: calc(var(--rail) * -1); padding-left: 50px; min-width: 100vw; }
"#;
    let (luna, o) = overlay_with(
        "wide-page.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::MAIN,
    );
    luna.shutdown();
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2137.0, 1183.0));
    let reserve = || {
        let ctx = egui::Context::default();
        let mut out = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(window),
                ..Default::default()
            },
            |ui| out = Some(reserve_in(ui.ctx(), true, window)),
        );
        output.textures_delta.clear();
        out.expect("the frame ran")
    };
    assert!(
        reserve() > 300.0,
        "without a theme the panel keeps its room"
    );
    pd::test_overlay(Some((*o).clone()));
    let themed = reserve();
    pd::test_overlay(None);
    assert_eq!(themed, 0.0);
}

/// A theme's `[data-wave-color=textSecondary] { color }` reaches in TIDAL
/// only the few words that carry the attribute: under TIDAL's own window
/// (Lunallena's accent green, Synthwave '77's magenta) its card subtitles,
/// tabs, the bar's artist line and the filter placeholder stay grey.
/// Tidalfast made the colour every grey word's. Where the theme paints the
/// window the colour still stands for the grey words.
#[test]
fn a_text_secondary_rule_leaves_tidals_grey_words() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::themed;
    let css = r#"/* {"name": "Lime notes"} */
:root { --lime-note: #c4f06a; }
[data-wave-color=textSecondary] { color: var(--lime-note); }
"#;
    let (luna, o) = overlay_with("lime-notes.css", css, dom_anchors::sample_tree(), "");
    luna.shutdown();
    let base = Palette::dark();
    assert_eq!(themed(&base, &o).secondary, base.secondary);
    let css = r#"/* {"name": "Lime window"} */
:root { --lime-note: #c4f06a; }
body { background-color: #101820; }
[data-wave-color=textSecondary] { color: var(--lime-note); }
"#;
    let (luna, o) = overlay_with("lime-window.css", css, dom_anchors::sample_tree(), "");
    luna.shutdown();
    assert_eq!(
        themed(&base, &o).secondary,
        egui::Color32::from_rgb(0xc4, 0xf0, 0x6a)
    );
}

/// A plugin that colours every word (cover-colored-ui's `:root { --its-own:
/// <cover colour> } span { color: var(--its-own) !important }`) colours them
/// in TIDAL over any theme's text tokens. The palette read the colour only
/// under cover-colored-ui's own variable name, and a plugin calling it
/// anything else lost to the theme's `--wave-color-solid-contrast-fill`.
/// The text roles' stand-ins take the rule, whatever its variable is called.
#[test]
fn a_rule_colouring_every_word_outranks_the_text_tokens() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::themed;
    let tint = egui::Color32::from_rgb(200, 120, 40);
    let css = r#"/* {"name": "Tint of the day"} */
:root {
    --wave-color-solid-contrast-fill: #e0e0e0;
    --tint-of-the-day: rgb(200, 120, 40);
}
span { color: var(--tint-of-the-day) !important; }
"#;
    let (luna, o) = overlay_with(
        "tint-of-the-day.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::THEME_TEXT,
    );
    luna.shutdown();
    let base = Palette::dark();
    let p = themed(&base, &o);
    assert_eq!(p.text, tint);
    // TIDAL's own window keeps its grey words.
    assert_eq!(p.secondary, base.secondary);
    // Where the theme paints the window, the grey words take it too.
    let css = r#"/* {"name": "Tint over a window"} */
:root {
    --wave-color-solid-contrast-fill: #e0e0e0;
    --wave-color-text-secondary: #888888;
    --tint-of-the-day: rgb(200, 120, 40);
}
body { background-color: #101820; }
span { color: var(--tint-of-the-day) !important; }
"#;
    let (luna, o) = overlay_with(
        "tint-over-a-window.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::THEME_SECONDARY,
    );
    luna.shutdown();
    let p = themed(&base, &o);
    assert_eq!(p.text, tint);
    assert_eq!(p.secondary, tint);
}

/// TIDAL's backdrop layers of the now playing view (the black, the blurred
/// cover, the gradient) are boxes inside `#nowPlaying`, so a theme's
/// opaque `#nowPlaying { background-image }` lies under the cover and never
/// shows while the cover does (Cotton Candy: Calavera paints the view a pale
/// pink gradient with dots, and TIDAL still shows the cover's dark colours
/// behind its white words). Tidalfast painted the view's layers over the
/// cover, so the view was a pale page with unreadable words. Where a theme
/// hides the cover too, the view's own layers are what shows.
#[test]
fn a_themes_now_playing_paint_lies_under_tidals_cover() {
    use crate::ui::plugin_dom as pd;
    let pale = egui::Color32::from_rgb(255, 177, 229);
    let marker = egui::Color32::from_rgb(1, 2, 3);
    // The fills a frame of the view's backdrop paints, in drawing order,
    // the cover standing in as one marker fill.
    let draw = || {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
            crate::ui::now_playing::backdrop_layers(ui, rect, rect, |ui| {
                ui.painter().rect_filled(rect, 0.0, marker);
            });
        });
        out.textures_delta.clear();
        let mut fills = Vec::new();
        let mut stack: Vec<egui::Shape> = out.shapes.into_iter().map(|c| c.shape).rev().collect();
        while let Some(shape) = stack.pop() {
            match shape {
                egui::Shape::Vec(inner) => stack.extend(inner.into_iter().rev()),
                egui::Shape::Rect(r) => fills.push(r.fill),
                egui::Shape::Mesh(m) => fills.extend(m.vertices.first().map(|v| v.color)),
                _ => {}
            }
        }
        fills
    };
    let css = r#"/* {"name": "Pale view paint"} */
#nowPlaying, .now-playing-background-container {
    background-image: linear-gradient(180deg, rgb(255, 177, 229), rgb(255, 177, 229)) !important;
}
[class="now-playing-background-image"] { display: none !important; }
"#;
    let (luna, o) = overlay_with(
        "pale-view-paint.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::NOW_PLAYING,
    );
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let themed = draw();
    pd::test_overlay(None);
    assert_eq!(themed, vec![marker], "the cover alone: {themed:?}");
    // A theme that hides the cover by its class shows the view's own paint.
    let css = r#"/* {"name": "Pale view no cover"} */
#nowPlaying { background-image: linear-gradient(180deg, rgb(255, 177, 229), rgb(255, 177, 229)) !important; }
.now-playing-background-image { display: none !important; }
"#;
    let (luna, o) = overlay_with(
        "pale-view-no-cover.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::NOW_PLAYING,
    );
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let bare = draw();
    pd::test_overlay(None);
    assert!(!bare.contains(&marker), "no cover: {bare:?}");
    assert!(bare.contains(&pale), "the view's own paint: {bare:?}");
}

/// TIDAL's album, playlist and mix header art is the header box's
/// `::before`, 450 points of the cover under black fading to solid black.
/// The box holds the whole page under the header, so a theme's
/// `[class*="dataContainer"]:before { height: 100% }` (Sunken Tidal) lays
/// the dimmed cover behind every row and the page under the header is
/// near black where the theme's window is grey. Tidalfast kept the 450, so
/// the rows lay on the theme's grey. Without a theme the box keeps 450.
#[test]
fn a_themes_height_on_the_header_art_box_reaches_down_the_page() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::PAGE_DATA;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.data = Some([240.0, 92.0, 1040.0, 300.0]);
    }
    let page = egui::Rect::from_min_size(egui::pos2(0.0, 30.0), egui::vec2(1600.0, 1000.0));
    assert_eq!(crate::ui::cover_backdrop_rect(page).height(), 450.0);
    let css = r#"/* {"name": "Tall header art"} */
[class*="dataContainer"]:before { height: 100% !important; }
"#;
    let (luna, o) = overlay_with("tall-art.css", css, dom_anchors::build(&frame), PAGE_DATA);
    pd::test_overlay(Some((*o).clone()));
    let full = pd::before_height(PAGE_DATA, 2400.0);
    // A page shorter than the window still has the box reach its foot.
    let rect = crate::ui::cover_backdrop_rect(page);
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(full, Some(2400.0));
    assert_eq!(rect.min, page.min);
    assert!(rect.height() >= page.height(), "{rect:?}");
    let css = r#"/* {"name": "Fixed header art"} */
[class*="dataContainer"]:before { height: 600px; }
"#;
    let (luna, o) = overlay_with("fixed-art.css", css, dom_anchors::build(&frame), PAGE_DATA);
    pd::test_overlay(Some((*o).clone()));
    let fixed = crate::ui::cover_backdrop_rect(page).height();
    pd::test_overlay(None);
    luna.shutdown();
    assert_eq!(fixed, 600.0);
}

/// A theme that paints a light window restyles the play queue's artist
/// lines and TIDAL's accent with its own colours (Tidal Light: its 70 %
/// black words and dark teal), so TIDAL's queue shows readable names on
/// the light card. Tidalfast drew them in the now-playing view's pale grey
/// and a followed artist in its bright cyan, almost invisible on white.
/// Under a dark window, and without a theme, the queue keeps those.
#[test]
fn queue_artist_names_take_a_light_themes_words_and_accent() {
    use crate::theme::Palette;
    use crate::ui::now_playing::queue_name_colors;
    use crate::ui::plugin_dom::{self as pd, SidePanel, themed, tidal_panel_text};
    let words = egui::Color32::from_rgb(67, 67, 67);
    let teal = egui::Color32::from_rgb(0x2a, 0x9d, 0x8f);
    let css = r#"/* {"name": "Pale page"} */
:root { --wave-color-solid-accent-fill: #2a9d8f !important; }
body { background-color: #ffffff !important; }
[class*="_marketText_"][class*="_text20_"] { color: rgb(67, 67, 67) !important; }
"#;
    let (luna, o) = overlay_with(
        "pale-page.css",
        css,
        dom_anchors::sample_tree(),
        dom_anchors::keys::QUEUE_ROLE_SECONDARY,
    );
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let light = tidal_panel_text(themed(&Palette::light(), &o), SidePanel::Queue);
    pd::test_overlay(None);
    assert_eq!(queue_name_colors(&light), (words, teal));
    // A dark window keeps TIDAL's grey and cyan, as does no theme.
    let native = queue_name_colors(&Palette::dark());
    assert_eq!(queue_name_colors(&Palette::light()), native);
    let css = r#"/* {"name": "Dark page"} */
:root { --wave-color-solid-accent-fill: #2a9d8f !important; }
body { background-color: #101010 !important; }
"#;
    let (luna, o) = overlay_with("dark-page.css", css, dom_anchors::sample_tree(), "");
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let dark = tidal_panel_text(themed(&Palette::dark(), &o), SidePanel::Queue);
    pd::test_overlay(None);
    assert_eq!(queue_name_colors(&dark), native);
}
