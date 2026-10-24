//! Round 34: TIDAL 2.46.0's playing track row, its other search fields, the
//! sidebar's create menu and a playlist's panel round its filter, captured
//! under themes. Proved with made-up themes (their own colours, TIDAL's
//! classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

fn themed_luna(name: &str, css: &str) -> Luna {
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert(format!("/{name}"), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir(name);
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    luna.install_theme(&format!("{base}/{name}"));
    luna
}

fn wait(
    luna: &Luna,
    tree: &super::dom::HostTree,
    want: &dyn Fn(&DomOverlay) -> bool,
) -> Arc<DomOverlay> {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && want(&o) {
            return o;
        }
        assert!(Instant::now() < end, "the overlay never got there");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// The overlay `css` gives `frame`'s tree, once `key` is styled.
fn overlay_of(frame: &dom_anchors::Frame, name: &str, css: &str, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
    let luna = themed_luna(name, css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(key));
    luna.shutdown();
    o
}

fn overlay_for(name: &str, css: &str, key: &str) -> Arc<DomOverlay> {
    overlay_of(&dom_anchors::sample_frame(), name, css, key)
}

const WINDOW: egui::Color32 = egui::Color32::from_rgb(10, 10, 10);

/// TIDAL 2.46.0 marks the playing row of a track list
/// `div._tableRow_._tableRowIsPlaying_a59b9fa` and its title
/// `span._titleText_._textHigh_bbc1075.table--inactive-title` (captured,
/// paused and playing): rules naming them reach them (TidalCustomTheme's
/// `[class*=_tableRowIsPlaying_]`, the cotton candy family's
/// `._textHigh_bbc1075`). Tidalfast's row had a made-up class and its title
/// neither, so those rules missed it.
#[test]
fn a_themes_rules_for_the_playing_row_reach_it() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Lit row"} */
[class*=_tableRowIsPlaying_] { background-color: rgb(20, 60, 90); }
._textHigh_bbc1075 { color: rgb(250, 120, 40); }
"#;
    let list = "album:55";
    let row = dom_anchors::keys::row(list, 0);
    let o = overlay_for("lit-row.css", css, &row);
    let fill = |i: usize| {
        o.hosts
            .get(&dom_anchors::keys::row(list, i))
            .and_then(|h| h.style.background_color.clone())
    };
    assert!(
        fill(0).is_some_and(|c| c.contains("20, 60, 90")),
        "{:?}",
        fill(0)
    );
    assert_eq!(fill(1), None);
    pd::test_overlay(Some((*o).clone()));
    let playing = pd::playing_title(list, 0, WINDOW);
    let idle = pd::playing_title(list, 1, WINDOW);
    pd::test_overlay(None);
    assert_eq!(playing, Some(egui::Color32::from_rgb(250, 120, 40)));
    assert_eq!(idle, None);
}

/// The playing title keeps TIDAL's `table--inactive-title`, so a theme's
/// `[class*="active"]` colours it as the other titles (Catppuccin Mocha
/// Blue's blue playing title in TIDAL 2.46.0, captured). Tidalfast left the
/// class off the playing title and drew it in the accent.
#[test]
fn the_playing_title_keeps_tidals_title_class() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Active blue"} */
[class*="active"] { color: rgb(30, 100, 240) !important; }
"#;
    let list = "album:55";
    let key = dom_anchors::keys::row_cell(list, 0, "title-text");
    let o = overlay_for("active-blue.css", css, &key);
    pd::test_overlay(Some((*o).clone()));
    let playing = pd::playing_title(list, 0, WINDOW);
    pd::test_overlay(None);
    assert_eq!(playing, Some(egui::Color32::from_rgb(30, 100, 240)));
}

/// A catch-all `* { color }` (Meteor Night's white) leaves the playing
/// title to TIDAL's own rule for its quality class, the accent, while the
/// other titles take it (TIDAL 2.46.0 under Meteor Night, captured).
#[test]
fn a_catch_all_colour_leaves_the_playing_title_in_the_accent() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Snow rows"} */
* { color: rgb(240, 240, 250); }
"#;
    let list = "album:55";
    let key = dom_anchors::keys::row_cell(list, 1, "title-text");
    let o = overlay_for("snow-rows.css", css, &key);
    pd::test_overlay(Some((*o).clone()));
    let playing = pd::playing_title(list, 0, WINDOW);
    let idle = pd::row_text(list, 1, WINDOW).and_then(|t| t.title);
    pd::test_overlay(None);
    assert_eq!(playing, None);
    assert_eq!(idle, Some(egui::Color32::from_rgb(240, 240, 250)));
}

/// TIDAL's rule for the playing title is the accent token: a theme that
/// sets the token colours it (Tidalify's red, Minimalist Black's green in
/// TIDAL 2.46.0, captured). Tidalfast drew it in its own cyan there, where
/// the queue's playing song already took the theme's accent.
#[test]
fn a_themes_accent_colours_the_playing_title() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Red accent"} */
:root { --wave-color-solid-accent-fill: rgb(255, 1, 1); }
"#;
    let list = "album:55";
    let key = dom_anchors::keys::row_cell(list, 0, "title-text");
    let o = overlay_for("red-accent.css", css, &key);
    pd::test_overlay(Some((*o).clone()));
    let playing = pd::playing_title(list, 0, WINDOW);
    pd::test_overlay(None);
    assert_eq!(playing, Some(egui::Color32::from_rgb(255, 1, 1)));
}

/// A page's track list sits in its panel's `div._tableContainer_556e9aa`
/// (TIDAL 2.46.0), so rules nested in that container reach the rows: the
/// cotton candy family's `[class="_tableContainer_556e9aa"] { a { color } }`
/// gives the artist links its words' colour (Cotton Candy Nights' lilac in
/// TIDAL, captured). Tidalfast's rows sat beside the container.
#[test]
fn a_track_list_sits_in_its_table_container() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Container words"} */
[class="_tableContainer_556e9aa"] {
a { color: rgb(216, 218, 255); }
._textHigh_bbc1075 { color: rgb(216, 218, 255) !important; }
}
"#;
    let mut frame = dom_anchors::sample_frame();
    let page = frame.page.get_or_insert_with(dom_anchors::Page::default);
    page.page_id = Some("album".into());
    page.list_panel = Some([260.0, 264.0, 1000.0, 400.0]);
    let list = "album:55";
    let key = dom_anchors::keys::row_cell(list, 0, "title-text");
    let o = overlay_of(&frame, "container-words.css", css, &key);
    pd::test_overlay(Some((*o).clone()));
    let artist = pd::row_text(list, 1, WINDOW).and_then(|t| t.artist);
    let playing = pd::playing_title(list, 0, WINDOW);
    pd::test_overlay(None);
    assert_eq!(artist, Some(egui::Color32::from_rgb(216, 218, 255)));
    assert_eq!(playing, Some(egui::Color32::from_rgb(216, 218, 255)));
}
