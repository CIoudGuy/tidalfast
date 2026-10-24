//! Plugin windows' pages laid out as a browser lays them out: made-up pages
//! load in a page runtime of their own, and the boxes the window draws for
//! them are checked against what CSS gives (Chromium's numbers).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use egui::{Pos2, Rect, vec2};

use super::{PageScope, plugin_dom, plugin_slots, prepare};
use crate::luna::dom::VNode;
use crate::luna::page::{Load, PageHub, PageWindows, WindowSpec};

fn ids(n: &VNode, out: &mut Vec<u64>) {
    out.push(n.id);
    for c in &n.children {
        ids(c, out);
    }
}

fn named(n: &VNode, out: &mut Vec<(String, u64)>) {
    if let Some((_, v)) = n.attrs.iter().find(|(k, _)| k == "id") {
        out.push((v.clone(), n.id));
    }
    for c in &n.children {
        named(c, out);
    }
}

/// Loads `html` as a plugin window's page, draws it in a window of `size`
/// for a few passes, and gives each element with an `id` its border box
/// (`[x, y, width, height]`).
pub(super) fn boxes(html: &str, size: [f32; 2]) -> HashMap<String, [f32; 4]> {
    let windows = Arc::new(PageWindows::default());
    let mut hub = PageHub::new(windows.clone());
    let (tx, _rx) = channel();
    hub.create("Probe", 2, WindowSpec::default(), tx).unwrap();
    hub.load(
        "Probe",
        2,
        Load {
            url: "data:text/html,".into(),
            html: html.into(),
            preload: None,
        },
    )
    .unwrap();
    let w = windows.all().into_iter().next().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let overlay = loop {
        let o = w.overlay();
        let has_body = o
            .injected
            .iter()
            .find(|i| i.anchor.is_empty())
            .and_then(|i| i.nodes.first())
            .is_some_and(|root| {
                root.children
                    .iter()
                    .any(|c| c.tag == "body" && !c.children.is_empty())
            });
        if has_body {
            break o;
        }
        assert!(Instant::now() < end, "the page never showed: {o:?}");
        std::thread::sleep(Duration::from_millis(10));
    };
    hub.stop_all();
    let mut o = (*overlay).clone();
    let root = o
        .injected
        .iter()
        .find(|i| i.anchor.is_empty())
        .unwrap()
        .nodes[0]
        .clone();
    let mut all = Vec::new();
    ids(&root, &mut all);
    all.sort_unstable();
    o.measure = all;
    let o = Arc::new(o);
    let (root, canvas) = prepare(&o).unwrap();
    let ctx = egui::Context::default();
    let size = vec2(size[0], size[1]);
    let mut rects = Vec::new();
    for _ in 0..8 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..egui::RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let scope = PageScope {
                overlay: o.clone(),
                vars: Arc::new(o.vars.clone()),
                viewport: [size.x, size.y],
                ..PageScope::default()
            };
            let rect = ui.max_rect();
            let ((), scope) = plugin_dom::with_page(scope, || {
                plugin_slots::page(ui, &root, &canvas, rect);
            });
            rects = scope.node_rects;
        });
        out.textures_delta.clear();
    }
    let mut names = Vec::new();
    named(&root, &mut names);
    names
        .into_iter()
        .filter_map(|(name, id)| {
            rects
                .iter()
                .find(|(n, _)| *n == id)
                .map(|(_, r)| (name, *r))
        })
        .collect()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0
}

/// A flex row with `justify-content: space-between` puts its first item at
/// its start and its last at its end; white space between them is no item
/// (Chromium: the header's two 28-pixel buttons at 0 and 272 of 300).
#[test]
fn a_spaced_row_puts_its_items_at_both_ends() {
    let b = boxes(
        r#"<!doctype html><html><head><style>
        *{margin:0;padding:0;box-sizing:border-box}
        .row{display:flex;justify-content:space-between;align-items:center;width:300px}
        button{width:28px;height:28px;border:none}
        .time{display:flex;justify-content:space-between;width:300px;font-size:10px}
        </style></head><body>
        <div class="row" id="row"><button id="a">a</button> <button id="b">b</button></div>
        <div class="time" id="time"><span id="s1">1:57</span> <span id="s2">3:00</span></div>
        </body></html>"#,
        [400.0, 200.0],
    );
    let row = b["row"];
    assert!(near(b["a"][0], row[0]), "{b:?}");
    assert!(near(b["b"][0] + 28.0, row[0] + 300.0), "{b:?}");
    let time = b["time"];
    assert!(near(b["s1"][0], time[0]), "{b:?}");
    assert!(
        near(b["s2"][0] + b["s2"][2], time[0] + 300.0),
        "the last span ends at the row's end: {b:?}"
    );
}

/// In a flex row an item is as wide as what it holds unless it grows: a
/// column that centres its cover is the cover's width and sits at the
/// row's start, and the `flex: 1` item beside it takes the rest (Chromium:
/// cover 0..100, info 116..500 of a 500-pixel row with a 16-pixel gap).
#[test]
fn a_rows_items_take_their_own_width_and_a_growing_one_the_rest() {
    let b = boxes(
        r#"<!doctype html><html><head><style>
        *{margin:0;padding:0;box-sizing:border-box}
        .player{display:flex;align-items:center;gap:16px;width:500px}
        .album{display:flex;flex-direction:column;align-items:center;flex-shrink:0}
        .cover{width:100px;height:100px;border-radius:50%;background:#f60}
        .info{flex:1;display:flex;flex-direction:column;gap:12px;min-width:0}
        </style></head><body>
        <div class="player" id="player"><div class="album" id="album"><div class="cover" id="cover"></div></div>
        <div class="info" id="info"><div id="title" style="height:69px">Title</div><div id="artist" style="height:69px">Artist</div></div></div>
        </body></html>"#,
        [600.0, 300.0],
    );
    let player = b["player"];
    assert!(near(b["album"][0], player[0]), "{b:?}");
    assert!(near(b["album"][2], 100.0), "{b:?}");
    assert!(near(b["cover"][0], player[0]), "{b:?}");
    assert!(near(b["info"][0], player[0] + 116.0), "{b:?}");
    assert!(near(b["info"][2], 384.0), "{b:?}");
    // `align-items: center`: the 100-pixel cover sits in the middle of the
    // 150-pixel row its details make.
    assert!(near(b["player"][3], 150.0), "{b:?}");
    assert!(near(b["album"][1], player[1] + 25.0), "{b:?}");
    assert!(near(b["info"][1], player[1]), "{b:?}");
}

/// A page's document holds only the page: none of TIDAL's own elements
/// (its icon sprite) sits in its body, so the first box starts at the top
/// and the page cannot find them (a 20-pixel sprite box pushed miniplayer's
/// panel down).
#[test]
fn a_pages_body_holds_only_the_page() {
    let b = boxes(
        r#"<!doctype html><html><head><style>*{margin:0}</style></head><body>
        <div id="first" style="height:10px"></div>
        <div id="found"></div>
        <script>document.getElementById("found").textContent =
            String(document.getElementById("svg-sprite"));</script>
        </body></html>"#,
        [200.0, 100.0],
    );
    assert!(!b.contains_key("svg-sprite"), "{b:?}");
    assert_eq!(b["first"][1], 0.0, "{b:?}");
}

/// A page's own `@font-face` (an icon font a linked sheet declares) is
/// loaded and drawn in its window: the `::before` glyph of an element in
/// that family at weight 900 is drawn in the family's bold face, not in the
/// app's (Font Awesome's icons were blank boxes).
#[test]
fn a_pages_web_font_draws_its_text() {
    use crate::luna::dom::{FontSource, Injected, Position, Style};
    use base64::Engine;
    let inter = base64::engine::general_purpose::STANDARD.encode(fastframe_fonts::INTER);
    let glyph = VNode {
        id: 13,
        tag: "::before".into(),
        children: vec![VNode {
            id: 14,
            tag: "#text".into(),
            text: Some("Z".into()),
            ..VNode::default()
        }],
        ..VNode::default()
    };
    let icon = VNode {
        id: 12,
        tag: "i".into(),
        style: Style {
            font_family: Some("\"Page Icons\"".into()),
            font_weight: Some("900".into()),
            ..Style::default()
        },
        children: vec![glyph],
        ..VNode::default()
    };
    let body = VNode {
        id: 11,
        tag: "body".into(),
        children: vec![icon],
        ..VNode::default()
    };
    let html = VNode {
        id: 10,
        tag: "html".into(),
        children: vec![body],
        ..VNode::default()
    };
    let overlay = Arc::new(crate::luna::dom::DomOverlay {
        version: 1,
        fonts: vec![FontSource {
            family: "Page Icons".into(),
            src: format!("url(data:font/ttf;base64,{inter})"),
            weight: Some("900".into()),
        }],
        injected: vec![Injected {
            anchor: String::new(),
            position: Position::Append,
            nodes: vec![html],
        }],
        ..crate::luna::dom::DomOverlay::default()
    });
    let (root, canvas) = prepare(&overlay).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut families = Vec::new();
    for _ in 0..300 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(200.0, 100.0))),
            ..egui::RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            plugin_dom::page_overlay(77, &overlay);
            plugin_dom::fonts_tick(ui.ctx());
            let scope = PageScope {
                overlay: overlay.clone(),
                viewport: [200.0, 100.0],
                ..PageScope::default()
            };
            let rect = ui.max_rect();
            let ((), _) = plugin_dom::with_page(scope, || {
                plugin_slots::page(ui, &root, &canvas, rect);
            });
        });
        out.textures_delta.clear();
        families = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Z" => {
                    Some(t.galley.job.sections[0].format.font_id.family.clone())
                }
                _ => None,
            })
            .collect();
        if families
            .iter()
            .any(|f| matches!(f, egui::FontFamily::Name(n) if n.starts_with("Page Icons")))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    plugin_dom::keep_pages(&[]);
    assert!(
        families
            .iter()
            .any(|f| matches!(f, egui::FontFamily::Name(n) if n.starts_with("Page Icons"))),
        "{families:?}"
    );
}

/// A box of a definite height lays out what it holds in that height: a
/// child's `height: 100%` is the box's 6 pixels (a progress bar's fill),
/// not the window's, and the bar stays 6 pixels tall (it grew to the
/// window's 380). What scripts read of a box is its border box, without
/// its margins, as `getBoundingClientRect` gives it.
#[test]
fn a_definite_height_holds_its_childrens_share_and_boxes_leave_out_margins() {
    let b = boxes(
        r#"<body style="margin:0"><div id="col" style="display:flex;flex-direction:column;gap:12px;width:300px"><div id="t" style="height:18px">T</div><div id="pc"><div id="bar" style="width:100%;height:6px;overflow:hidden;margin:4px 0 8px 10px;background:#000"><div id="fill" style="height:100%;width:50%;background:red"></div></div><div id="time" style="height:14px">0:00</div></div></div></body>"#,
        [700.0, 380.0],
    );
    // `width: 100%` is the column's 300 pixels, after the 10-pixel margin.
    assert_eq!(b["bar"], [10.0, 34.0, 300.0, 6.0], "{b:?}");
    assert_eq!(b["fill"], [10.0, 34.0, 150.0, 6.0], "{b:?}");
    assert_eq!(b["time"][1], 48.0, "under the bar and its margin: {b:?}");
}

/// A flex column held to its `max-height` is that tall once what it holds
/// would run past it, and its growing item takes what is left; a growing
/// item in that one with `overflow-y: auto` is held to its share and
/// scrolls (lyrics-everywhere's full lyrics under its player). Chromium
/// at 700 by 380: the container 348 tall at 16, the middle 240 from 100,
/// the player 90 from 100, the lyrics 126 from 214.
#[test]
fn a_column_held_to_its_max_height_shares_it_and_its_scroller_scrolls() {
    let lines: String = (0..20)
        .map(|i| format!(r#"<div style="height:40px">line {i}</div>"#))
        .collect();
    let html = format!(
        r#"<!doctype html><html><head><style>
        *{{margin:0;padding:0;box-sizing:border-box}}
        body{{height:100vh;display:flex;flex-direction:column;overflow:hidden}}
        .c{{margin:16px;padding:24px;min-height:200px;max-height:calc(100vh - 32px);display:flex;flex-direction:column;width:calc(100vw - 32px)}}
        .h{{height:40px;margin-bottom:20px}}
        .m{{display:flex;flex-direction:column;gap:24px;flex:1;min-height:0}}
        .p{{height:90px}}
        .l{{flex:1;overflow-y:auto;padding:24px}}
        </style></head><body><div class="c" id="c"><div class="h" id="h"></div><div class="m" id="m"><div class="p" id="p"></div><div class="l" id="l">{lines}</div></div></div></body></html>"#
    );
    let b = boxes(&html, [700.0, 380.0]);
    assert_eq!(b["c"], [16.0, 16.0, 668.0, 348.0], "{b:?}");
    assert_eq!(b["h"], [40.0, 40.0, 620.0, 40.0], "{b:?}");
    assert_eq!(b["m"], [40.0, 100.0, 620.0, 240.0], "{b:?}");
    assert_eq!(b["p"], [40.0, 100.0, 620.0, 90.0], "{b:?}");
    assert_eq!(b["l"], [40.0, 214.0, 620.0, 126.0], "{b:?}");
}

/// A column with less than its `max-height` to hold keeps its own height:
/// its growing item gets nothing more (Chromium: 100 tall, 16 + 24 + 20 +
/// 40 of words).
#[test]
fn a_column_under_its_max_height_is_as_tall_as_what_it_holds() {
    let b = boxes(
        r#"<!doctype html><html><head><style>
        *{margin:0;padding:0;box-sizing:border-box}
        .c{max-height:300px;display:flex;flex-direction:column;width:200px}
        .h{height:40px}
        .m{flex:1;min-height:0;overflow-y:auto}
        </style></head><body><div class="c" id="c"><div class="h" id="h"></div><div class="m" id="m"><div style="height:60px"></div></div></div></body></html>"#,
        [400.0, 400.0],
    );
    assert_eq!(b["c"], [0.0, 0.0, 200.0, 100.0], "{b:?}");
    assert_eq!(b["m"], [0.0, 40.0, 200.0, 60.0], "{b:?}");
}

/// Grid tracks `auto 1fr auto`: the `auto` tracks are as wide as what they
/// hold and the `1fr` track takes the rest (miniplayer's taskbar progress
/// row: its bar was a third of the row).
#[test]
fn a_grids_auto_tracks_fit_their_items_and_its_fr_track_takes_the_rest() {
    let b = boxes(
        r#"<!doctype html><html><head><style>
        *{margin:0;padding:0;box-sizing:border-box}
        .r{display:grid;grid-template-columns:auto 1fr auto;align-items:center;gap:6px;width:250px;font-size:10px}
        .bar{height:3px;background:#fff}
        </style></head><body><div class="r" id="r"><span id="a">0:00</span><div class="bar" id="bar"></div><span id="z">-0:00</span></div></body></html>"#,
        [400.0, 200.0],
    );
    let (a, bar, z) = (b["a"], b["bar"], b["z"]);
    assert!(a[2] > 5.0 && a[2] < 40.0, "{b:?}");
    assert!(
        near(bar[0], a[2] + 6.0),
        "the bar after the first time: {b:?}"
    );
    assert!(
        near(z[0] + z[2], 250.0),
        "the last time at the row's end: {b:?}"
    );
    assert!(near(bar[0] + bar[2] + 6.0, z[0]), "the bar up to it: {b:?}");
    assert!(near(bar[3], 3.0), "{b:?}");
}

/// Grid items fill their cells and hold absolutely placed children in
/// their own boxes: each preset is a 192 by 80 cell, 16 apart both ways,
/// and its name sits at its foot, 12 in from each side of the padding box
/// (lyrics-everywhere's colour presets; Chromium: name 15 to 177, its foot
/// at 69).
#[test]
fn a_grids_cells_hold_their_absolutely_placed_names() {
    let b = boxes(
        r#"<!doctype html><html><head><style>
        *{margin:0;padding:0;box-sizing:border-box}
        .g{display:grid;grid-template-columns:repeat(2,1fr);gap:16px;width:400px}
        .p{height:80px;position:relative;border:3px solid transparent;overflow:hidden}
        .n{position:absolute;bottom:8px;left:12px;right:12px;text-align:center;font-size:14px}
        </style></head><body><div class="g" id="g"><div class="p" id="p1"><div class="n" id="n1">Cosmic Purple</div></div><div class="p" id="p2"><div class="n" id="n2">Pink Passion</div></div><div class="p" id="p3"><div class="n" id="n3">Ocean Blue</div></div></div></body></html>"#,
        [700.0, 380.0],
    );
    assert_eq!(b["p1"], [0.0, 0.0, 192.0, 80.0], "{b:?}");
    assert_eq!(b["p2"], [208.0, 0.0, 192.0, 80.0], "{b:?}");
    assert_eq!(b["p3"], [0.0, 96.0, 192.0, 80.0], "{b:?}");
    let n = b["n1"];
    assert!(near(n[0], 15.0) && near(n[0] + n[2], 177.0), "{b:?}");
    assert!(near(n[1] + n[3], 69.0), "{b:?}");
}
