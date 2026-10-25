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
