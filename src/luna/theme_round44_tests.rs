//! Round 44: what the parity checker's comparisons found. Proved on the
//! demo's pages, with made-up themes (their own values, TIDAL's classes and
//! anchors only) where a theme is involved.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::{DomOverlay, HostTree};
use super::dom_anchors::keys;
use super::theme_round41_tests as r41;
use super::*;

/// The overlay a theme's `css` gives `tree` once it sets the variable
/// `var`.
fn styled_vars(name: &str, css: &str, tree: &HostTree, var: &str) -> DomOverlay {
    styled_until(name, css, tree, |o| o.vars.contains_key(var))
}

/// The overlay a theme's `css` gives `tree` once `done` holds.
fn styled_until(
    name: &str,
    css: &str,
    tree: &HostTree,
    done: impl Fn(&DomOverlay) -> bool,
) -> DomOverlay {
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert(format!("/{name}"), css.as_bytes().to_vec());
    let base = serve(files);
    let luna = Luna::start(config(&temp_dir(name)), Arc::new(MockHost::default())).unwrap();
    luna.install_theme(&format!("{base}/{name}"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if done(&o) {
            break o;
        }
        assert!(Instant::now() < end, "the theme never got there");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    let mut o = (*o).clone();
    o.version = 2;
    o
}

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

/// TIDAL 2.46.0 marks a playlist's header art up as its own
/// (`button._artwork_1d717fe > div._image_d7a36dc._shadow_32a27bc > ... >
/// img._resourceImage_4beb8bc`, captured), not as an album's
/// (`._coverArtContainer_2eafdf9 > ._coverArt_79d862c > ... > img`): a
/// theme's rule for the album's art leaves a playlist's with TIDAL's own
/// shadow (Cotton Candy Single's pink `0 16px 32px` shadow lies under the
/// album's cover only), and one for the playlist's classes reaches it (the
/// cotton candy family's `._shadow_32a27bc img`). Tidalfast gave a
/// playlist's art the album's classes.
#[test]
fn a_playlists_header_art_is_marked_up_as_tidals() {
    let css = "/* {\"name\": \"Art\"} */\n:is(._coverArtContainer_2eafdf9 ._coverArt_79d862c) img { box-shadow: 0 16px 32px 0 rgb(200, 80, 180); }\n._shadow_32a27bc img { box-shadow: 0 16px 32px 0 rgb(20, 200, 40); }\n";
    for (page, want) in [
        ("album:alb0", "rgb(200, 80, 180)"),
        ("playlist:pl0", "rgb(20, 200, 40)"),
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
        let o = r41::styled("art.css", css, &tree, keys::HERO_COVER_IMG);
        let shadow = o.hosts[keys::HERO_COVER_IMG]
            .style
            .box_shadow
            .clone()
            .unwrap_or_default();
        assert!(
            shadow.contains(want),
            "{page}: the header art's shadow is {shadow:?}, not the one for its own classes ({want})"
        );
    }
}

/// TIDAL fills its back and forward pill with its frosted chip,
/// `var(--frosted-glass-floating-surface)` (`#282828bf`), whatever colours
/// a theme gives the page: under TidalCustomTheme's pink page the pill is
/// TIDAL's dark glass (TIDAL 2.46.0's captured page replayed in Chromium),
/// and a theme that sets the token (Tidal Light's `#dbdbdb59`) fills it
/// with its own. Tidalfast filled the pill with a themed palette's surface
/// (TidalCustomTheme's light pink).
#[test]
fn the_back_and_forward_pill_is_tidals_frosted_chip() {
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("quality-pill"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let pill = r41::boxes(&plain)[keys::NAV_ARROWS];
    let light = crate::theme::Palette {
        themed: true,
        ..crate::theme::Palette::light()
    };
    let blend = |c: egui::Color32, under: egui::Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        let a = f32::from(a) / 255.0;
        let mix = |x: u8, y: u8| (f32::from(x) * a + f32::from(y) * (1.0 - a)).round() as u8;
        egui::Color32::from_rgb(mix(r, under.r()), mix(g, under.g()), mix(b, under.b()))
    };
    // Inside the pill above the back chevron, and just left of the pill.
    let points = [
        [pill[0] + 17.0, pill[1] + 5.0],
        [pill[0] - 4.0, pill[1] + 5.0],
    ];
    for (css, want) in [
        (
            ":root { --unrelated-token: 1px; }",
            egui::Color32::from_rgba_unmultiplied(40, 40, 40, 191),
        ),
        (
            ":root { --frosted-glass-floating-surface: #dbdbdb59; }",
            egui::Color32::from_rgba_unmultiplied(0xdb, 0xdb, 0xdb, 0x59),
        ),
    ] {
        let var = if css.contains("frosted") {
            "--frosted-glass-floating-surface"
        } else {
            "--unrelated-token"
        };
        let css = format!(
            "/* {{\"name\": \"Chip\"}} */
{css}
"
        );
        let o = styled_vars("chip.css", &css, &plain, var);
        app.palette = light;
        let colours = r41::painted_at(&ctx, &mut app, o, &points);
        let expected = blend(want, colours[1]);
        let near = colours[0]
            .to_array()
            .iter()
            .zip(expected.to_array())
            .all(|(a, b)| a.abs_diff(b) <= 3);
        assert!(
            near,
            "{var}: the pill is {:?} over {:?}, TIDAL's chip there {expected:?}",
            colours[0], colours[1]
        );
    }
    app.backend.shutdown();
}

/// A theme's `transform: translateX()` on TIDAL's search field and on the
/// account button's box moves them as Chromium does, keeping their room:
/// the cotton candy family's `translateX(var(--searchbuf))` (-5em, 70
/// points, in a window 2100 or more wide) and `translateX(var(--profbuff))`
/// (-1em) put TIDAL 2.46.0's field 70 points left of its wrapper and the
/// account button 14 left of its place (TIDAL's captured page replayed in
/// Chromium). Tidalfast read the moves and drew both where they were.
#[test]
fn a_moved_search_field_and_account_button_are_drawn_moved() {
    let css = "/* {\"name\": \"Moved header\"} */\n[class=\"_container_f1be359 searchFieldWithinSearchFieldWithPopover _searchField_b2eb7f7\"] { transform: translateX(-70px); }\n[class=\"_buttonContainer_d207640\"] { transform: translateX(-14px); }\n";
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("quality-pill"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = r41::styled("moved-header.css", css, &plain, keys::SEARCH_FIELD);
    o.version = 2;
    let themed = r41::drawn(&ctx, &mut app, o);
    app.backend.shutdown();
    let (before, after) = (r41::boxes(&plain), r41::boxes(&themed));
    for (key, want) in [(keys::SEARCH_FIELD, -70.0), (keys::PROFILE, -14.0)] {
        let moved = after[key][0] - before[key][0];
        assert!(
            (moved - want).abs() < 0.5,
            "{key} moved {moved}, not {want} ({:?} -> {:?})",
            before[key],
            after[key]
        );
    }
}

/// TIDAL 2.46.0's search field (`div._container_f1be359`) is its 280 x 36
/// pill, 1797 points from the left of a 2137-point window with the play
/// queue closed (TIDAL's captured page replayed in Chromium), and a theme's
/// box for it lies there. Tidalfast's anchor was the text box inside the
/// pill (200 x 19, 44 in from its left).
#[test]
fn the_search_fields_box_is_its_pill() {
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("quality-pill"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    app.backend.shutdown();
    let field = r41::boxes(&plain)[keys::SEARCH_FIELD];
    assert!(
        (field[0] - 1797.0).abs() < 0.5
            && (field[2] - 280.0).abs() < 0.5
            && (field[3] - 36.0).abs() < 0.5,
        "the search field's box: {field:?}"
    );
}

/// TIDAL shades what lies above its bar: `._player_8c80ae6::before` is a
/// gradient from clear to black at 15 %, from 80 points above the bar to 12
/// below it, over the page, the sidebar and the play queue. Under
/// TidalCustomTheme's pink page TIDAL 2.46.0's page is 16 levels darker
/// just above the bar than higher up (TIDAL's captured page replayed in
/// Chromium); a theme that hides the box (the cotton candy family's
/// `._player_8c80ae6:before { display: none }`) leaves the page as it is.
/// Tidalfast drew no shade.
#[test]
fn the_page_is_shaded_above_the_bar_as_tidals_is() {
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("quality-pill"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let footer = r41::boxes(&plain)[keys::FOOTER];
    let bar_top = footer[1];
    // Beside the page's column, clear of the sidebar and the list.
    let x = 262.0;
    let points = [[x, bar_top - 2.5], [x, bar_top - 150.0]];
    let light = crate::theme::Palette {
        themed: true,
        ..crate::theme::Palette::light()
    };
    for (css, shaded) in [
        (":root { --unrelated-token: 1px; }", true),
        ("._player_8c80ae6:before { display: none; }", false),
    ] {
        let css = format!("/* {{\"name\": \"Shade\"}} */\n{css}\n");
        let o = if shaded {
            styled_vars("shade.css", &css, &plain, "--unrelated-token")
        } else {
            styled_until("shade.css", &css, &plain, |o| {
                o.hosts
                    .get(keys::FOOTER)
                    .is_some_and(|h| h.before.is_some())
            })
        };
        app.palette = light;
        let colours = r41::painted_at(&ctx, &mut app, o, &points);
        let (near, far) = (colours[0], colours[1]);
        // 2.5 points above the bar: 15 % of the way 77.5 / 180 down.
        let alpha = 0.15 * (80.0 - 2.5) / 180.0;
        let want = if shaded {
            (f32::from(far.r()) * (1.0 - alpha)).round()
        } else {
            f32::from(far.r())
        };
        assert!(
            (f32::from(near.r()) - want).abs() <= 2.0,
            "shaded {shaded}: the page above the bar is {near:?}, higher up {far:?} (want red {want})"
        );
    }
    app.backend.shutdown();
}

/// TIDAL's filter pill on a My Collection page is its 40-point field in a
/// 1-point border, 42 in all; a theme's border changes its height, and
/// what follows moves with it: under the cotton candy family's `border: 0
/// !important` TIDAL 2.46.0's pill is 40 tall and the grid 2 points
/// higher (TIDAL's captured page replayed in Chromium). Tidalfast's pill
/// stayed 42.
#[test]
fn a_filter_pills_border_sets_its_height() {
    let css = "/* {\"name\": \"Pill\"} */\n._container_080b5dc { border: 0 !important; background-color: rgb(40, 80, 160); }\n";
    let (ctx, mut app) = r41::demo_app("albums", Some("quality-pill"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = r41::styled("pill.css", css, &plain, keys::PAGE_FILTER_BOX);
    o.version = 2;
    let themed = r41::drawn(&ctx, &mut app, o);
    app.backend.shutdown();
    let (before, after) = (r41::boxes(&plain), r41::boxes(&themed));
    let pill = (
        before[keys::PAGE_FILTER_BOX][3],
        after[keys::PAGE_FILTER_BOX][3],
    );
    assert!(
        (pill.0 - 42.0).abs() < 0.01 && (pill.1 - 40.0).abs() < 0.01,
        "the pill's height plain and with no border: {pill:?}"
    );
    let grid = after[keys::CARD_GRID][1] - before[keys::CARD_GRID][1];
    assert!((grid + 2.0).abs() < 0.01, "the grid moved {grid}, not 2 up");
}
