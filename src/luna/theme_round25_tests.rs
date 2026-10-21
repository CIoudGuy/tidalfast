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
