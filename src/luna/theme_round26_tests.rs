//! Luna themes round 26: what round 25 left open, fixed in the general CSS
//! mapping and proved with made-up themes (their own variable names and
//! colours, TIDAL's classes and anchors only).

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

/// TIDAL gives each kind of home shortcut its own link class: the cotton
/// candy family paints `._link_0c5cc7e > ._shortcutItem_` (an artist's
/// tile) in its second colour, so TIDAL's artist tiles are blue beside the
/// user's pink playlists. Every tile carried a playlist's class, so all of
/// them took the first colour. Mixes and albums carry their own ids.
#[test]
fn shortcut_tiles_carry_their_kinds_link_class_and_id() {
    use dom_anchors::keys::tile_item;
    let css = r#"/* {"name": "Tile kinds"} */
:root { --sorbet-one: rgb(210, 40, 90); --sorbet-two: rgb(30, 90, 210); }
._disableUnderline_8e517ad._link_86815a7 > ._shortcutItem_6c8e7b4 { background-color: var(--sorbet-one); }
._disableUnderline_8e517ad._link_0c5cc7e > ._shortcutItem_6c8e7b4 { background-color: var(--sorbet-two); }
#mix-shortcut { background-color: rgb(1, 160, 70); }
"#;
    let tile = |x: f32, owned, kind| dom_anchors::Tile {
        rect: Some([x, 100.0, 300.0, 60.0]),
        image: Some([x + 8.0, 108.0, 44.0, 44.0]),
        owned,
        kind,
    };
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.feed = true;
        page.tiles = vec![
            tile(240.0, true, dom_anchors::TileKind::Playlist),
            tile(548.0, false, dom_anchors::TileKind::Artist),
            tile(856.0, false, dom_anchors::TileKind::Mix),
        ];
    }
    let (luna, o) = overlay_with(
        "tile-kinds.css",
        css,
        dom_anchors::build(&frame),
        &tile_item(2),
    );
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    let fills: Vec<_> = (0..3)
        .map(|i| crate::ui::plugin_dom::fill(&tile_item(i)))
        .collect();
    crate::ui::plugin_dom::test_overlay(None);
    luna.shutdown();
    assert_eq!(fills[0], Some(egui::Color32::from_rgb(210, 40, 90)), "own");
    assert_eq!(
        fills[1],
        Some(egui::Color32::from_rgb(30, 90, 210)),
        "artist"
    );
    assert_eq!(fills[2], Some(egui::Color32::from_rgb(1, 160, 70)), "mix");
}

/// TIDAL fills Explore's pills with `--wave-color-opacity-contrast-fill-thin`
/// (20 % white, the grey over its black window): under a theme that paints
/// a light window the pills are a paler shade of it (the cotton candy
/// family), a theme's own token for it shows (Rose Pine), and a theme's rule
/// on the pill (`div[data-test=page-links-cloud-row] > a`, TIDAL's
/// `._button_3357ce6`) wins. Tidalfast drew them opaque charcoal under every
/// theme. Without a theme nothing changes.
#[test]
fn explore_pills_take_tidals_thin_fill_or_the_themes() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom as pd;
    let rect = |x: f32| egui::Rect::from_min_size(egui::pos2(x, 200.0), egui::vec2(90.0, 46.0));
    let row = egui::Rect::from_min_size(egui::pos2(300.0, 200.0), egui::vec2(900.0, 46.0));
    // The anchors Tidalfast notes for one row of two pills.
    let noted = || {
        let mut frame = dom_anchors::sample_frame();
        if let Some(page) = frame.page.as_mut() {
            page.cloud_rows = vec![Some([300.0, 200.0, 900.0, 46.0])];
            page.cloud_pills = vec![
                (0, Some([300.0, 200.0, 90.0, 46.0])),
                (0, Some([410.0, 200.0, 90.0, 46.0])),
            ];
        }
        dom_anchors::build(&frame)
    };
    let style = |css: &str, name: &str, key: &str| {
        let (luna, o) = overlay_with(name, css, noted(), key);
        let palette = pd::themed(&Palette::dark(), &o);
        pd::test_overlay(Some((*o).clone()));
        pd::test_page_frame();
        pd::cloud_row(row);
        let pills = [
            pd::cloud_pill(rect(300.0), &palette),
            pd::cloud_pill(rect(410.0), &palette),
        ];
        pd::test_overlay(None);
        luna.shutdown();
        (palette, pills)
    };

    let light = r#"/* {"name": "Pale window"} */
#main { background-color: rgb(250, 190, 240); color: rgb(20, 10, 30); }
"#;
    let (palette, pills) = style(light, "pale-window.css", dom_anchors::keys::MAIN);
    let pill = pills[0].expect("styled under a theme");
    assert_eq!(pill.fill, Some(egui::Color32::from_white_alpha(51)));
    assert_eq!(pill.color, Some(palette.text), "the page's words");

    let token = r#"/* {"name": "Own token"} */
:root { --wave-color-opacity-contrast-fill-thin: #e0c0f033; }
"#;
    let (_, pills) = style(token, "own-token.css", "");
    assert_eq!(
        pills[0].and_then(|p| p.fill),
        Some(egui::Color32::from_rgba_unmultiplied(
            0xe0, 0xc0, 0xf0, 0x33
        ))
    );

    let rule = r#"/* {"name": "Pill rule"} */
#main { background-color: rgb(8, 0, 20); }
:root { --lollipop: 18, 6, 60; }
._button_3357ce6 { background-color: rgb(var(--lollipop)); }
[data-test="page-links-cloud-row"] > a:nth-child(2) { background: none; }
"#;
    let (_, pills) = style(rule, "pill-rule.css", &dom_anchors::keys::cloud_btn(1));
    assert_eq!(
        pills[0].and_then(|p| p.fill),
        Some(egui::Color32::from_rgb(18, 6, 60))
    );
    assert_eq!(
        pills[1].and_then(|p| p.fill),
        Some(egui::Color32::TRANSPARENT),
        "a rule clearing the second pill"
    );

    // No theme: the pill keeps Tidalfast's own grey.
    pd::test_overlay(Some(DomOverlay::default()));
    pd::test_page_frame();
    pd::cloud_row(row);
    assert_eq!(pd::cloud_pill(rect(300.0), &Palette::dark()), None);
    pd::test_overlay(None);
}

/// TIDAL's play queue draws the playing song in a theme's rule for the
/// queue's titles (the cotton candy family's white) or else in the theme's
/// accent token, on a dark window too (Rose Pine's foam, Tidalify's red,
/// Minimalist Black's green); Tidalfast kept its own cyan.
#[test]
fn the_queues_playing_song_follows_the_theme() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom::{SidePanel, test_overlay, themed, tidal_panel_text};
    let token = r#"/* {"name": "Queue token"} */
:root { --wave-color-solid-accent-fill: #ff7f11; }
"#;
    let (luna, o) = overlay_with("queue-token.css", token, dom_anchors::sample_tree(), "");
    let palette = themed(&Palette::dark(), &o);
    test_overlay(Some((*o).clone()));
    let queue = tidal_panel_text(palette, SidePanel::Queue);
    let sidebar = tidal_panel_text(palette, SidePanel::Sidebar);
    test_overlay(None);
    luna.shutdown();
    assert_eq!(queue.accent, egui::Color32::from_rgb(0xff, 0x7f, 0x11));
    assert_eq!(sidebar.accent, palette.accent, "the sidebar keeps TIDAL's");

    let rule = r#"/* {"name": "Queue titles"} */
:root { --wave-color-solid-accent-fill: #101010; }
#main { background-color: rgb(240, 200, 230); }
[class="_playQueueItemsContainer_01c88fe"] { ._marketText_1lyag_1._medium20_1lyag_192 { color: rgb(255, 250, 240); } }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([930.0, 92.0, 350.0, 600.0]), false));
    let (luna, o) = overlay_with("queue-titles.css", rule, dom_anchors::build(&frame), "");
    let palette = themed(&Palette::dark(), &o);
    test_overlay(Some((*o).clone()));
    let queue = tidal_panel_text(palette, SidePanel::Queue);
    test_overlay(None);
    luna.shutdown();
    assert_eq!(queue.accent, egui::Color32::from_rgb(255, 250, 240));
}

/// TIDAL's navigation rows keep their own dim words and icon under a
/// theme's catch-all `* { color }` (Meteor Night's white: TIDAL shows the
/// inactive rows dimmed), while a rule naming the words (one class, as
/// TidalCustomTheme's `[class^="_marketText_"]`) still colours them.
/// Tidalfast took the catch-all on the words and drew every row white.
#[test]
fn a_catch_all_colour_leaves_the_navigation_rows_words() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{sidebar_item_icon, sidebar_item_label};
    let row = || dom_anchors::SidebarItem {
        test: "sidebar-explore".into(),
        label: "Explore".into(),
        href: Some("/explore".into()),
        selected: false,
        rect: Some([16.0, 128.0, 208.0, 40.0]),
    };
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.items.push(row());
    let label = sidebar_item_label("sidebar-explore");
    let icon = sidebar_item_icon("sidebar-explore");
    let catch_all = r#"/* {"name": "Starlight"} */
:root { --starlight: rgb(240, 250, 10); }
* { color: var(--starlight); }
"#;
    // The words' stand-in takes the catch-all: the theme has reached the tree.
    let (luna, o) = overlay_with(
        "starlight.css",
        catch_all,
        dom_anchors::build(&frame),
        dom_anchors::keys::THEME_TEXT,
    );
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let words = pd::nav_style(&label).and_then(|s| s.color);
    let ink = pd::sidebar_icon(&icon);
    pd::test_overlay(None);
    assert_eq!(words, None, "the row's own words");
    assert_eq!(ink, None, "the row's own icon");

    let named = r#"/* {"name": "Named words"} */
* { color: rgb(240, 250, 10); }
[class^="_marketText_"] { color: rgb(120, 10, 40); }
"#;
    let (luna, o) = overlay_with("named-words.css", named, dom_anchors::build(&frame), &label);
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let words = pd::nav_style(&label).and_then(|s| s.color);
    pd::test_overlay(None);
    assert_eq!(words, Some(egui::Color32::from_rgb(120, 10, 40)));
}

/// TIDAL draws an album's, playlist's or mix's title as an `h2` in its
/// header and an artist's name as `h2[data-test=artist-name]`, not as the
/// page's `h1`: Lunallena's lime `h1` leaves them white in TIDAL, and
/// Giorgeous' `h2 { font-size: 30px; text-transform: uppercase; color }`
/// sets them in large green capitals (its artist name at 50px). Tidalfast
/// read the `h1` for them.
#[test]
fn header_titles_are_tidals_h2_not_the_pages_h1() {
    use crate::ui::plugin_dom::{self as pd, Title};
    let white = egui::Color32::WHITE;
    let page_heading = r#"/* {"name": "Lime pages"} */
:root { --lime-zest: rgb(150, 255, 100); }
h1 { font-size: 30px !important; color: var(--lime-zest) !important; }
"#;
    let (luna, o) = overlay_with(
        "lime-pages.css",
        page_heading,
        dom_anchors::sample_tree(),
        dom_anchors::keys::THEME_H1,
    );
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let page = pd::headline_color(white);
    let header = pd::title_color(Title::Header, white);
    let artist = pd::title_color(Title::ArtistName, white);
    let size = pd::title_font(Title::Header, 32.0).size;
    pd::test_overlay(None);
    assert_eq!(
        page,
        egui::Color32::from_rgb(150, 255, 100),
        "the page's h1"
    );
    assert_eq!(header, white, "an album's title");
    assert_eq!(artist, white, "an artist's name");
    assert_eq!(size, 32.0);

    let headings = r#"/* {"name": "Moss capitals"} */
:root { --moss: rgb(190, 250, 40); }
h2 { font-size: 30px !important; text-transform: uppercase !important; color: var(--moss) !important; }
h2[data-test='artist-name'] { font-size: 50px !important; }
"#;
    let (luna, o) = overlay_with(
        "moss-capitals.css",
        headings,
        dom_anchors::sample_tree(),
        dom_anchors::keys::THEME_ARTIST_NAME,
    );
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let color = pd::title_color(Title::Header, white);
    let case = pd::title_case(Title::Header, "Late night focus").into_owned();
    let size = pd::title_font(Title::Header, 32.0).size;
    let artist = pd::title_font(Title::ArtistName, 32.0).size;
    let artist_case = pd::title_case(Title::ArtistName, "Bonobo").into_owned();
    pd::test_overlay(None);
    assert_eq!(color, egui::Color32::from_rgb(190, 250, 40));
    assert_eq!(case, "LATE NIGHT FOCUS");
    assert_eq!(size, 30.0);
    assert_eq!(artist, 50.0);
    assert_eq!(artist_case, "BONOBO");
}

/// The play queue's title is TIDAL's market heading text, not a section's
/// `headerContainer > h2`: Lunallena's `[class*="heading"]` makes it lime
/// and its 30px rule for section headings leaves it small in TIDAL;
/// Giorgeous' green `h2` colours it. Tidalfast drew it in the panel's white
/// at the section headings' size.
#[test]
fn the_queue_title_takes_its_own_rules() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE_TITLE;
    let css = r#"/* {"name": "Citrus headings"} */
:root { --citrus: rgb(170, 255, 120); }
[class*="headerContainer"] > h2 { font-size: 30px !important; color: var(--citrus); }
[class*="heading"] { color: var(--citrus); }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([930.0, 92.0, 350.0, 600.0]), false));
    frame.queue_title = Some(("Play queue".into(), Some([946.0, 110.0, 120.0, 26.0])));
    let (luna, o) = overlay_with("citrus.css", css, dom_anchors::build(&frame), QUEUE_TITLE);
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let color = pd::queue_title_color();
    let size = pd::themed_font(QUEUE_TITLE, crate::theme::bold(19.0), 700).size;
    pd::test_overlay(None);
    assert_eq!(color, Some(egui::Color32::from_rgb(170, 255, 120)));
    assert_eq!(size, 19.0, "TIDAL's size, not the section headings'");
}

/// Abyss Neptune boxes TIDAL's back and forward buttons
/// (`[data-test="navigation-arrows"] > button { background-color; color;
/// border-radius }`, grey when `:disabled`): TIDAL shows two light squares
/// with dark chevrons. The arrows had no button anchors, so Tidalfast kept
/// its dark pill.
#[test]
fn the_back_and_forward_buttons_take_a_themes_box() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{NAV_BACK, NAV_FORWARD};
    let css = r#"/* {"name": "Pebbles"} */
:root { --pebble: rgb(240, 240, 235); }
[data-test="navigation-arrows"] > button { background-color: var(--pebble) !important; color: rgb(5, 5, 5) !important; border-radius: 5px; }
[data-test="navigation-arrows"] > button:disabled { background-color: rgb(200, 200, 200) !important; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.shell.nav_arrows = Some([256.0, 14.0, 70.0, 36.0]);
    frame.shell.nav_disabled = [false, true];
    let (luna, o) = overlay_with("pebbles.css", css, dom_anchors::build(&frame), NAV_FORWARD);
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let back = pd::nav_style(NAV_BACK).expect("styled");
    let forward = pd::nav_style(NAV_FORWARD).expect("styled");
    pd::test_overlay(None);
    assert_eq!(back.fill, Some(egui::Color32::from_rgb(240, 240, 235)));
    assert_eq!(back.radius, Some(5.0));
    assert_eq!(back.color, Some(egui::Color32::from_rgb(5, 5, 5)));
    assert_eq!(
        forward.fill,
        Some(egui::Color32::from_rgb(200, 200, 200)),
        "disabled"
    );
}
