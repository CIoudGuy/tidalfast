//! Round 43: the parity checker's one-window run and what its comparisons
//! found. Proved with made-up themes (their own values, TIDAL's classes and
//! anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

/// What of an overlay makes the look: everything but its counters.
fn look(o: &DomOverlay) -> serde_json::Value {
    let mut v = serde_json::to_value(o).unwrap();
    if let Some(m) = v.as_object_mut() {
        m.remove("version");
        m.remove("theme_version");
    }
    v
}

/// The overlay Luna gives `tree` once the demo's themes are in and `done`
/// holds, or the last one after 20 seconds.
fn until(
    luna: &Luna,
    themes: &mut crate::demo::DemoThemes,
    tree: &super::dom::HostTree,
    done: impl Fn(&DomOverlay) -> bool,
) -> Arc<DomOverlay> {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let installing = themes.tick(luna);
        let o = luna.dom_overlay();
        if (!installing && done(&o)) || Instant::now() > end {
            return o;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// [`serve`], with the file at `slow` sent `delay` late.
fn serve_slowly(files: Files, slow: &'static str, delay: Duration) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let files = files.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                if path == slow {
                    std::thread::sleep(delay);
                }
                let body = files
                    .lock()
                    .unwrap()
                    .get(&path)
                    .cloned()
                    .unwrap_or_default();
                let head = format!(
                    "HTTP/1.1 200 OK
Content-Length: {}
Connection: close

",
                    body.len()
                );
                let _ = stream.write_all(&[head.into_bytes(), body].concat());
            });
        }
    });
    format!("http://{addr}")
}

/// A theme with a little of everything a theme leaves in the overlay: a
/// variable, the page's fill, an anchor's box, a face, keyframes running on
/// an anchor and a `:hover` rule.
const FIRST: &str = r#"/* {"name": "First made-up theme"} */
@font-face { font-family: "First Face"; src: url("first-face.woff2"); }
@keyframes first-pulse { from { opacity: 0.2; } to { opacity: 1; } }
:root { --first-gap: 7px; --wave-color-solid-base-fill: rgb(40, 10, 60); }
body { background-color: rgb(40, 10, 60); }
#main { background-color: rgb(10, 200, 30); }
#sidebar { border-radius: 9px; font-family: "First Face"; }
#footerPlayer { animation: first-pulse 2s infinite; }
#main [data-test="tracklist-row"]:hover { color: rgb(250, 0, 0); }
"#;

/// Another theme, in two files as the cotton candy family ships: a base
/// that reads its colours from variables it does not set, and a small
/// colourway that sets them.
fn second_base() -> String {
    let reads: String = (1..=8)
        .map(|i| {
            format!("#sidebar [data-test=\"sidebar-item-{i}\"] {{ color: var(--second-{i}); }}\n")
        })
        .collect();
    format!(
        "/* {{\"name\": \"Second made-up theme\"}} */\n#main {{ background-color: var(--second-1); }}\n{reads}/* {} */\n",
        "padding ".repeat(400)
    )
}

fn second_colours() -> String {
    let set: String = (1..=8)
        .map(|i| format!("  --second-{i}: rgb({i}, 2, 3);\n"))
        .collect();
    format!("/* Second made-up theme, its colours */\n:root {{\n{set}}}\n")
}

/// The parity checker shoots every theme in one window, one after another
/// (a dev script's `theme` control, `crate::demo::DemoThemes`): after one
/// theme and then another, the overlay is the second theme's as a window
/// with only that theme has it, nothing of the first left (its variables,
/// page fill, boxes, faces, keyframes, `:hover`); after `none`, it is a
/// window's with no theme. The second comes in two files, a base and its
/// colourway, which go in one after another: the colourway applies over
/// its base, whichever file's download ends first.
#[test]
fn a_theme_switched_to_leaves_nothing_of_the_one_before() {
    let files: Files = Arc::default();
    for (name, css) in [
        ("/first.css", FIRST.to_string()),
        ("/second/base.css", second_base()),
        ("/second/theme.css", second_colours()),
    ] {
        files.lock().unwrap().insert(name.into(), css.into_bytes());
    }
    // The base is slow to arrive (156 KB for the cotton candy family's,
    // beside a 7 KB colourway): the colourway's download ends first.
    let base = serve_slowly(files, "/second/base.css", Duration::from_millis(400));
    let first = vec![format!("{base}/first.css")];
    let second = vec![
        format!("{base}/second/base.css"),
        format!("{base}/second/theme.css"),
    ];
    assert!(crate::luna::themes::is_colourway_of(
        &second_colours(),
        &second_base()
    ));
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let start =
        |tag: &str| Luna::start(config(&temp_dir(tag)), Arc::new(MockHost::default())).unwrap();
    let themed = |o: &DomOverlay, var: &str| o.css_bytes > 0 && o.vars.contains_key(var);
    // The colourway over its base: the base's rule takes the colourway's
    // colour.
    let over_base = |o: &DomOverlay| {
        themed(o, "--second-1")
            && o.hosts
                .get(dom_anchors::keys::MAIN)
                .is_some_and(|h| h.style.background_color.is_some())
    };

    // What a window with the second theme alone, and one with none, has.
    let alone = start("r43-second-alone");
    let mut themes = crate::demo::DemoThemes::default();
    themes.replace(&alone, second.clone());
    let second_alone = until(&alone, &mut themes, &tree, over_base);
    alone.shutdown();
    assert!(
        over_base(&second_alone),
        "the colourway applies over its base"
    );
    let bare = start("r43-none");
    let mut no_themes = crate::demo::DemoThemes::default();
    // Settled once the sample tree's anchors are in the overlay.
    let none_alone = until(&bare, &mut no_themes, &tree, |o| !o.hosts.is_empty());
    bare.shutdown();

    // The first theme, then the second in its place, then none.
    let luna = start("r43-first-then-second");
    let mut themes = crate::demo::DemoThemes::default();
    themes.replace(&luna, first);
    let first_on = until(&luna, &mut themes, &tree, |o| themed(o, "--first-gap"));
    assert!(
        themed(&first_on, "--first-gap") && first_on.hover && !first_on.fonts.is_empty(),
        "the first theme applies"
    );
    themes.replace(&luna, second);
    let switched = until(&luna, &mut themes, &tree, |o| {
        look(o) == look(&second_alone)
    });
    themes.replace(&luna, Vec::new());
    let cleared = until(&luna, &mut themes, &tree, |o| look(o) == look(&none_alone));
    let installed: Vec<String> = luna
        .snapshot()
        .themes
        .iter()
        .map(|t| t.url.clone())
        .collect();
    luna.shutdown();

    assert_eq!(look(&switched), look(&second_alone));
    assert_eq!(look(&cleared), look(&none_alone));
    assert!(installed.is_empty(), "themes left installed: {installed:?}");
}

/// TIDAL's My Collection card is as tall as what it holds: its cover, its
/// lines (title, subtitle, meta) and its padding round them. TIDAL 2.46.0's
/// grid row (`div._staticCell_d3a8e6a`) is 334 points tall under a
/// 232-point cover, the card in it 305.5 with no theme and 301 under the
/// cotton candy family's `padding: .6em` (Chromium, the parity checker's
/// replay). Tidalfast's card filled the row, so a theme's fill for the card
/// (the cotton candy family's glass cards) ran 29 points below TIDAL's.
#[test]
fn a_collection_cards_box_ends_with_what_it_holds() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let fill = egui::Color32::from_rgb(250, 160, 200);
    let css = "/* {\"name\": \"Padded cards\"} */\n[data-test^=\"grid-item\"] { background-color: rgb(250, 160, 200); padding: 8px; }\n";
    let (ctx, mut app) = r41::demo_app("albums", Some("many-albums"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let rect = |tree: &super::dom::HostTree, key: &str| {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.rect)
            .unwrap_or_else(|| panic!("no box for {key}"))
    };
    // With no theme: the cover, then 74 points of lines (TIDAL: 305.5
    // under a 232-point cover).
    let card = rect(&plain, &keys::card(0));
    let cover = rect(&plain, &keys::card_cover(0));
    let plain_lines = card[3] - cover[3];
    let mut o = r41::styled("padded-cards.css", css, &plain, &keys::card(0));
    o.version = 2;
    let padded = r41::drawn(&ctx, &mut app, o.clone());
    let card = rect(&padded, &keys::card(0));
    let cover = rect(&padded, &keys::card_cover(0));
    // Below the card's foot, inside its grid row: the page, not the fill.
    let x = card[0] + 20.0;
    let colours = r41::painted_at(
        &ctx,
        &mut app,
        o,
        &[[x, card[1] + card[3] - 3.0], [x, card[1] + card[3] + 6.0]],
    );
    app.backend.shutdown();
    assert!(
        (plain_lines - 74.0).abs() < 0.5,
        "lines under the cover: {plain_lines}"
    );
    assert!(
        (card[3] - (8.0 + cover[3] + 74.0 + 8.0)).abs() < 0.5,
        "card {card:?}, cover {cover:?}"
    );
    assert_eq!(colours[0], fill, "the card's foot is filled");
    assert_ne!(colours[1], fill, "the row below the card is not");
}

/// TIDAL's player bar card (`._trackContent_`: Sugarcoat's and the cotton
/// candy singles' rounded box round the song's words, heart and "…") is a
/// flex row's box: it starts where the words do with no theme, its padding
/// and border move the words in, and down it holds TIDAL's lines (the
/// title's 22, the artists' 20, the source's 2 + 18, 1 apart: 64) with its
/// padding and border round them, centred in the bar. Under Sugarcoat
/// (`padding: .28em; padding-left: .78em; border: 3px`) TIDAL 2.46.0's card
/// is 77.8 points tall and its words 14 points right of where they are
/// with no theme (Chromium, the parity checker's replay); Tidalfast's card
/// was the words' glyph rows with the padding round them and the border
/// inside it (62 tall), its words where they were.
#[test]
fn a_bar_cards_box_holds_tidals_lines_in_its_padding_and_border() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let edge = egui::Color32::from_rgb(10, 200, 30);
    let css = "/* {\"name\": \"Bar card\"} */\n[class*=\"_trackContent_\"] { background-color: rgb(250, 160, 200); padding: 4px 4px 4px 11px; border: 3px solid rgb(10, 200, 30); }\n";
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("queue,quality-pill"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let rect = |tree: &super::dom::HostTree, key: &str| {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.rect)
            .unwrap_or_else(|| panic!("no box for {key}"))
    };
    let title_plain = rect(&plain, keys::TITLE);
    let footer = rect(&plain, keys::FOOTER);
    let mut o = r41::styled("bar-card.css", css, &plain, keys::TRACK_CONTENT);
    o.version = 2;
    let themed = r41::drawn(&ctx, &mut app, o.clone());
    let title = rect(&themed, keys::TITLE);
    // Down the card's left border, the bar's height through.
    let x = title_plain[0] + 1.5;
    let ys: Vec<f32> = (0..footer[3] as usize)
        .map(|i| footer[1] + i as f32 + 0.5)
        .collect();
    let points: Vec<[f32; 2]> = ys.iter().map(|y| [x, *y]).collect();
    let colours = r41::painted_at(&ctx, &mut app, o, &points);
    app.backend.shutdown();
    let close = |c: &egui::Color32| {
        c.to_array()[..3]
            .iter()
            .zip(edge.to_array())
            .all(|(a, b)| a.abs_diff(b) <= 3)
    };
    let edged: Vec<f32> = ys
        .iter()
        .zip(&colours)
        .filter(|(_, c)| close(c))
        .map(|(y, _)| *y)
        .collect();
    let (top, foot) = (edged.first().copied(), edged.last().copied());
    let height = foot.zip(top).map(|(f, t)| f - t + 1.0);
    assert!(
        height.is_some_and(|h| (h - (64.0 + 2.0 * (4.0 + 3.0))).abs() <= 1.0),
        "the card's height: {height:?} (from {top:?} to {foot:?}, the bar {footer:?})"
    );
    assert!(
        ((title[0] - title_plain[0]) - 14.0).abs() < 0.5,
        "the words move in by the padding and border: {title:?} against {title_plain:?}"
    );
}

/// A theme's inset `box-shadow` on TIDAL's sidebar card (the cotton candy
/// family's white `inset 0 0 6em` glow on `div._sidebarWrapper_`) lies
/// over the card's fill along its edges and under what it holds, as
/// Chromium paints it: under Purple Paradise Punch TIDAL 2.46.0's card is
/// 15 levels lighter 5 points in from its left edge than at its middle (the
/// parity checker's replay). Tidalfast painted the card's fill alone.
#[test]
fn a_sidebar_cards_inset_glow_lies_along_its_edges() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let css = "/* {\"name\": \"Glowing sidebar\"} */\n[class*=\"_sidebarWrapper_\"] { background-color: rgb(40, 0, 60); box-shadow: inset 0 0 40px rgba(255, 255, 255, .6); }\n";
    let (ctx, mut app) = r41::demo_app("album:alb0", None);
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let sidebar = plain
        .nodes
        .iter()
        .find(|n| n.key == keys::SIDEBAR)
        .and_then(|n| n.rect)
        .expect("the sidebar's box");
    let mut o = r41::styled("glowing-sidebar.css", css, &plain, keys::SIDEBAR);
    o.version = 2;
    // 2 points in from the card's left edge, half way down: the fill,
    // rgb(40, 0, 60), under the white glow (TIDAL: 0.45 of its 60 %).
    let y = sidebar[1] + sidebar[3] / 2.0;
    let colours = r41::painted_at(&ctx, &mut app, o, &[[sidebar[0] + 2.0, y]]);
    app.backend.shutdown();
    let edge = colours[0];
    assert!(
        edge.g() >= 50 && edge.r() > 60,
        "the glow along the card's edge: {edge:?} (the fill alone is rgb(40, 0, 60))"
    );
}

/// Sunken Tidal's play queue: `[class*="_playQueueWrapper"] { margin: 0;
/// margin-top: var(--headerHeight); height: 100% }` and `#playQueueSidebar
/// { margin: 0; width: 100%; height: 100%; backdrop-filter: none }`. TIDAL
/// 2.46.0 lays the panel over the whole 350-point wrapper, from 56 under
/// the content row's top down past the player bar to the window's foot
/// (`aside#playQueueSidebar` 350 x 1153 at x 1787 of a 2137 window), and
/// paints its own grey glass over what is behind it with no blur, so the
/// page shows through (Chromium, the parity checker's replay). Tidalfast
/// kept TIDAL's 334-point panel 8 in from the window's edge, ending 20
/// above the bar, an opaque card under the glass.
#[test]
fn a_play_queue_a_theme_unboxes_runs_full_bleed_over_the_page() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let css = |page: &str| {
        format!(
            "/* {{\"name\": \"Full-bleed queue\"}} */\n:root {{ --wave-color-solid-base-fill: {page}; }}\n[class*=\"_hasPlayer\"] [class*=\"_playQueueWrapper\"] {{ margin: 0; height: 100%; margin-top: var(--headerHeight); }}\n#playQueueSidebar {{ border: 1px solid rgba(255, 255, 255, 0.1); border-radius: 0; backdrop-filter: none; width: 100%; height: 100%; margin: 0; }}\n"
        )
    };
    let mut boxes = Vec::new();
    let mut colours = Vec::new();
    for (name, page) in [
        ("full-bleed-red.css", "rgb(200, 0, 0)"),
        ("full-bleed-green.css", "rgb(0, 200, 0)"),
    ] {
        let (ctx, mut app) = r41::demo_app("album:alb0", Some("queue"));
        let plain = r41::drawn(
            &ctx,
            &mut app,
            DomOverlay {
                version: 1,
                ..DomOverlay::default()
            },
        );
        let mut o = r41::styled(name, &css(page), &plain, keys::QUEUE_PANEL);
        o.version = 2;
        let themed = r41::drawn(&ctx, &mut app, o.clone());
        let panel = themed
            .nodes
            .iter()
            .find(|n| n.key == keys::QUEUE_PANEL)
            .and_then(|n| n.rect)
            .expect("the queue panel's box");
        // Inside the panel, in its right padding half way down.
        let at = [panel[0] + panel[2] - 6.0, panel[1] + 400.0];
        colours.push(r41::painted_at(&ctx, &mut app, o, &[at])[0]);
        app.backend.shutdown();
        boxes.push(panel);
    }
    let panel = boxes[0];
    assert!(
        (panel[0] - (2137.0 - 350.0)).abs() < 0.5 && (panel[2] - 350.0).abs() < 0.5,
        "across the wrapper: {panel:?}"
    );
    assert!(
        panel[1] + panel[3] > 1183.0 - 88.0,
        "down past the player bar's top: {panel:?}"
    );
    let [red, green] = [colours[0], colours[1]];
    assert!(
        red.r() > green.r() + 60 && green.g() > red.g() + 60,
        "the page shows through the glass: {red:?} over red, {green:?} over green"
    );
}

/// TIDAL's play queue panel is 334 points wide (`width: 334px`) and its
/// margins move it in its 350-point wrapper: Abyss Neptune's `margin: 2px;
/// margin-right: -14px` lays it 2 in from the wrapper's left edge, still
/// 334 wide (TIDAL 2.46.0: x 1789 to 2123 in a 2137 window, the replay).
/// And where a theme fills TIDAL's `main` (Sunken Tidal's grey page), what
/// lies round the open panel is what lies behind `main`, TIDAL's black,
/// not the page's grey.
#[test]
fn a_queue_panels_margins_move_it_in_its_wrapper() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let css = "/* {\"name\": \"Moved queue\"} */\n#main { background-color: rgb(120, 120, 120); }\n#playQueueSidebar { margin: 2px; margin-right: -14px !important; background-color: rgba(0, 0, 0, 0.8) !important; }\n";
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("queue"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = r41::styled("moved-queue.css", css, &plain, keys::QUEUE_PANEL);
    o.version = 2;
    let themed = r41::drawn(&ctx, &mut app, o.clone());
    let panel = themed
        .nodes
        .iter()
        .find(|n| n.key == keys::QUEUE_PANEL)
        .and_then(|n| n.rect)
        .expect("the queue panel's box");
    // Right of the panel, in the wrapper's last 14 points, half way down:
    // what lies behind `main`.
    let colours = r41::painted_at(&ctx, &mut app, o, &[[2133.0, panel[1] + 400.0]]);
    app.backend.shutdown();
    assert!(
        (panel[0] - (2137.0 - 350.0 + 2.0)).abs() < 0.5 && (panel[2] - 334.0).abs() < 0.5,
        "{panel:?}"
    );
    assert!(colours[0].r() < 40, "beside the panel: {:?}", colours[0]);
}

/// TIDAL's page (`main`) stops at the open play queue's column, where the
/// document's own fill shows: under a theme's base colour token
/// (Catppuccin Frappe's `--wave-color-solid-base-fill`) the player bar's
/// glass lies over the page's colour beside the column and over the lighter
/// base colour under it (TIDAL 2.46.0, the parity checker's replay of
/// Catppuccin Frappe and Macchiato: the bar lighter right of x 1787 with
/// the queue open). Tidalfast's bar was one colour across.
#[test]
fn the_bars_glass_lies_over_the_open_queues_column_as_tidals_does() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let css = "/* {\"name\": \"Base colour\"} */\n:root { --wave-color-solid-base-fill: rgb(48, 52, 70); }\n";
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("queue"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let footer = plain
        .nodes
        .iter()
        .find(|n| n.key == keys::FOOTER)
        .and_then(|n| n.rect)
        .expect("the bar's box");
    let o = styled_vars(
        "base-colour.css",
        css,
        &plain,
        "--wave-color-solid-base-fill",
    );
    // The palette the app makes of the theme's tokens: a themed dark one,
    // Catppuccin Frappe's page.
    app.palette = crate::theme::Palette {
        themed: true,
        own_surfaces: false,
        window: egui::Color32::from_rgb(35, 38, 52),
        ..crate::theme::Palette::dark()
    };
    // Low in the bar, clear of its words and buttons.
    let y = footer[1] + footer[3] - 4.0;
    let colours = r41::painted_at(&ctx, &mut app, o, &[[1000.0, y], [2120.0, y]]);
    app.backend.shutdown();
    let [page, column] = [colours[0], colours[1]];
    assert!(
        column.r() > page.r() && column.b() >= page.b() + 3,
        "the bar over the page {page:?}, over the queue's column {column:?}"
    );
}

/// The overlay a theme's `css` gives `tree` once it sets the variable
/// `var`.
fn styled_vars(name: &str, css: &str, tree: &super::dom::HostTree, var: &str) -> DomOverlay {
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert(format!("/{name}"), css.as_bytes().to_vec());
    let base = serve(files);
    let luna = Luna::start(config(&temp_dir(name)), Arc::new(MockHost::default())).unwrap();
    luna.install_theme(&format!("{base}/{name}"));
    let mut none = crate::demo::DemoThemes::default();
    let o = until(&luna, &mut none, tree, |o| o.vars.contains_key(var));
    luna.shutdown();
    let mut o = (*o).clone();
    o.version = 2;
    o
}

/// TIDAL's song list moves with its module box (`.ALBUM_ITEMS`): the box's
/// own top padding and border lie above the list in the flow, and its
/// `transform` moves the box and the list together. Sugarcoat's
/// `translateY(-1.1em)` over `padding-top: 1.4em` and a 2-point border lays
/// the album's songs 6.2 points lower than with no theme in TIDAL 2.46.0
/// (the parity checker's replay: the list's header at 526, not 519), its
/// box 15.4 higher. Tidalfast kept the list where it was and laid the box
/// round it.
#[test]
fn a_song_lists_box_moves_the_list_as_tidals_flow_does() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let css = "/* {\"name\": \"Moved list\"} */\n.is-intersecting._occlusion_de6b9d3._defaultStyles_a1b9da9.ALBUM_ITEMS.tidal-ui__occlusion { transform: translateY(-1.1em); padding-top: 1.4em; border-style: solid; border-width: 2px; border-color: rgb(255, 255, 255); background-color: rgba(255, 0, 0, 0.2); }\n";
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("queue"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = r41::styled("moved-list.css", css, &plain, keys::LIST_PANEL);
    o.version = 2;
    let moved = r41::drawn(&ctx, &mut app, o);
    app.backend.shutdown();
    let top = |tree: &super::dom::HostTree, key: &str| {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.rect)
            .map(|r| r[1])
            .unwrap_or_else(|| panic!("no box for {key}"))
    };
    let header = "list:album:alb0:header";
    let (before, after) = (top(&plain, header), top(&moved, header));
    assert!(
        ((after - before) - (19.6 + 2.0 - 15.4)).abs() < 0.6,
        "the list's header from {before} to {after}"
    );
    let panel = top(&moved, keys::LIST_PANEL);
    assert!(
        (panel - (before - 15.4)).abs() < 0.6,
        "the box at {panel}, the list's place {before}"
    );
}

/// A playlist's songs move with their box too: the filter field and the
/// table lie in TIDAL's `div._items_8615603`, so its padding, border and
/// `transform` move the field and the table, and the table's own
/// `transform` (`div._tableContainer_556e9aa`) moves the table in it. The
/// cotton candy family's box (`padding: 11.2px 12.6px 0`, a 2-point border,
/// `translateY(-.75em)`) and table (`translateY(-1.6em)`) lay the field 2.7
/// lower and the table's header 19.7 higher than with no theme in TIDAL
/// 2.46.0 (the parity checker's replay: field 476.7, header 534.3).
#[test]
fn a_playlists_filter_and_table_move_with_their_boxes() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let css = "/* {\"name\": \"Moved playlist\"} */\n._items_8615603 { padding: 11.2px 12.6px 0; border-style: solid; border-width: 2px; border-color: rgb(255, 255, 255); transform: translateY(-10.5px); background-color: rgba(255, 0, 0, 0.2); }\n._tableContainer_556e9aa { transform: translateY(-1.6em); }\n";
    let (ctx, mut app) = r41::demo_app("playlist:pl0", None);
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = r41::styled("moved-playlist.css", css, &plain, keys::LIST_PANEL_INNER);
    o.version = 2;
    let moved = r41::drawn(&ctx, &mut app, o);
    app.backend.shutdown();
    let top = |tree: &super::dom::HostTree, key: &str| {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.rect)
            .map(|r| r[1])
            .unwrap_or_else(|| panic!("no box for {key}"))
    };
    let field = keys::PAGE_FILTER_BOX;
    let header = "list:playlist:pl0:header";
    let moved_by = |key: &str| top(&moved, key) - top(&plain, key);
    assert!(
        (moved_by(field) - 2.7).abs() < 0.6,
        "the field moved {}",
        moved_by(field)
    );
    assert!(
        (moved_by(header) - (2.7 - 22.4)).abs() < 0.6,
        "the table's header moved {}",
        moved_by(header)
    );
}

/// TIDAL 2.46.0's My Collection page lays a theme's boxes round the grid
/// of cards: the page box's `min-width`, `margin` and `transform`
/// (`div._defaultStyles_a1b9da9._container_d43f21d`), the grid's margins,
/// `transform`, `min-width`, padding, border and gaps
/// (`section._grid_97562f5`, its top margin collapsing with the filter
/// field's 36), and each card's cell (`div._staticCell_d3a8e6a`) with its
/// padding, `max-width` and `max-height` in its track, its negative
/// margins and its `transform`, as tall as a 232-point card's cell for its
/// width. The numbers are Chromium's for these rules on TIDAL's captured
/// page (the parity checker's replay): the page 1811.5 wide at x 280.7 and
/// 20 higher, eight 223.4-point tracks, cells 201.1 x 276.8 that start
/// 18 in and 12 down in the grid, rows 309.6 apart, the card 10 in the
/// cell. Tidalfast laid six 232-point cards in TIDAL's own column and
/// left the cells out of its tree.
#[test]
fn a_collection_grids_page_grid_and_cell_boxes_lay_out_as_tidals() {
    use super::theme_round41_tests as r41;
    use dom_anchors::keys;
    let css = "/* {\"name\": \"Collection boxes\"} */\n\
        [data-track--page-id=\"mycollection_albums\"] { margin: auto; min-width: calc(96vw - var(--sidebarWidth)) !important; transform: translateX(-2px) translateY(4px); }\n\
        ._grid_97562f5 { margin: 20px; transform: translateX(-20px); min-width: 100%; padding: 10px; border: 2px solid rgb(1, 2, 3); row-gap: 30px; column-gap: 0px; }\n\
        [class^=\"_staticCell_\"] { background-color: rgb(250, 160, 200); padding: 10px; max-width: 90%; max-height: 99%; margin-right: -20px; margin-bottom: -10px; transform: translateX(6px); }\n";
    let (ctx, mut app) = r41::demo_app("albums", Some("many-albums"));
    let plain = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let rect = |tree: &super::dom::HostTree, key: &str| {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.rect)
            .unwrap_or_else(|| panic!("no box for {key}"))
    };
    let plain_grid = rect(&plain, keys::CARD_GRID);
    let plain_cell = rect(&plain, &keys::card_cell(0));
    let plain_card = rect(&plain, &keys::card(0));
    let mut o = r41::styled("collection-boxes.css", css, &plain, &keys::card_cell(0));
    o.version = 2;
    let themed = r41::drawn(&ctx, &mut app, o.clone());
    let grid = rect(&themed, keys::CARD_GRID);
    let cell = rect(&themed, &keys::card_cell(0));
    let next = rect(&themed, &keys::card_cell(1));
    let below = rect(&themed, &keys::card_cell(8));
    let card = rect(&themed, &keys::card(0));
    let colour = r41::painted_at(&ctx, &mut app, o, &[[cell[0] + 4.0, cell[1] + 4.0]]);
    app.backend.shutdown();
    let near = |a: f32, b: f32| (a - b).abs() < 0.6;
    // With no theme: TIDAL's 1472-point column, the cell its track.
    assert!(
        near(plain_grid[0], 452.5) && near(plain_grid[2], 1472.0),
        "{plain_grid:?}"
    );
    assert!(
        near(plain_cell[2], 232.0) && near(plain_cell[3], 334.0) && near(plain_card[2], 232.0),
        "{plain_cell:?} {plain_card:?}"
    );
    assert!(
        near(grid[0], 280.73) && near(grid[2], 1811.52) && near(grid[1], plain_grid[1] - 20.0),
        "grid {grid:?}, no theme {plain_grid:?}"
    );
    assert!(
        near(cell[0], grid[0] + 18.0)
            && near(cell[1], grid[1] + 12.0)
            && near(cell[2], 201.09)
            && near(cell[3], 276.77),
        "cell {cell:?} in {grid:?}"
    );
    assert!(near(next[0] - cell[0], 223.44), "next cell {next:?}");
    assert!(
        near(below[0], cell[0]) && near(below[1] - cell[1], 309.56),
        "the second row's cell {below:?}"
    );
    assert!(
        near(card[0], cell[0] + 10.0) && near(card[1], cell[1] + 10.0) && near(card[2], 181.09),
        "card {card:?}"
    );
    assert_eq!(
        colour[0],
        egui::Color32::from_rgb(250, 160, 200),
        "the cell's fill"
    );
}
