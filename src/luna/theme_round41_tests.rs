//! Round 41: TIDAL's flow of the shell's boxes under a theme's margins,
//! paddings and positions. TIDAL 2.46.0 lays its window out as a column:
//! the 30-point title bar (`div._bar_5b9ac59`), then the content row
//! (`div._containerRow_`: the sidebar, the content area holding the
//! header, `main` and the play queue's wrapper). Tidalfast's window shows
//! that page from the title bar's foot down. A theme that pulls the row up
//! under the title bar (the cotton candy family's `margin-bottom: -2.5em`,
//! Blur's `position: absolute`) and moves the parts back down by their own
//! margins had every part 25 to 30 points lower here than in TIDAL. Proved
//! with made-up themes (their own values, TIDAL's classes and anchors
//! only), drawn by the app on the demo's album page; the moves expected are
//! Chromium's, measured on TIDAL 2.46.0's captured page under the same
//! rules (the parity checker's replay).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::{DomOverlay, HostTree};
use super::dom_anchors::keys;
use super::*;
use crate::ui::plugin_dom as pd;

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

/// The overlay a theme's `css` gives the anchors of `tree`, once it styles
/// the anchor `key`.
pub(super) fn styled(name: &str, css: &str, tree: &HostTree, key: &str) -> DomOverlay {
    let luna = themed_luna(name, css);
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.get(key).is_some_and(|h| !h.style.is_empty()) {
            break o;
        }
        assert!(Instant::now() < end, "the overlay never got there");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    (*o).clone()
}

/// The demo's `page` with `show` (the album with the play queue open, as
/// the parity checker shoots it).
pub(super) fn demo_app(page: &str, show: Option<&str>) -> (egui::Context, crate::app::App) {
    let root = crate::paths::unique_test_dir("round41-flow");
    let _ = std::fs::remove_dir_all(&root);
    let mut app = crate::app::App::new(
        &crate::backend::Waker::default(),
        crate::paths::AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        },
        crate::settings::Settings::default(),
        crate::app::AppOptions {
            media_controls: false,
            restore_sign_in: false,
            tray: false,
        },
    );
    app.reveal_theme_changes = false;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    app.attach(&ctx);
    app.motion.force(true);
    crate::motion::apply(&ctx, true);
    crate::demo::populate(&mut app);
    crate::demo::apply_flags(&mut app, Some(page), show);
    (ctx, app)
}

/// The tree the app sends once it has drawn the page under `overlay` a
/// few times.
pub(super) fn drawn(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    overlay: DomOverlay,
) -> HostTree {
    pd::test_overlay(None);
    pd::test_overlay(Some(overlay));
    for _ in 0..4 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2137.0, 1183.0),
                )),
                ..Default::default()
            },
            |ui| {
                // The pass as the app runs it, Luna's half left out (the
                // overlay is the test's).
                app.refresh_frame_now();
                pd::test_viewport([2137.0, 1183.0]);
                pd::test_page_frame_for(&app.page().clone());
                crate::ui::show(app, ui);
                pd::end_frame(ui.ctx());
                app.clear_frame_now();
            },
        );
        output.textures_delta.clear();
    }
    let tree = pd::test_sent_tree().expect("a tree was sent");
    pd::test_overlay(None);
    tree
}

/// The colours the app paints at `points` (over black) once it has drawn
/// the page under `overlay` a few times.
pub(super) fn painted_at(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    overlay: DomOverlay,
    points: &[[f32; 2]],
) -> Vec<egui::Color32> {
    pd::test_overlay(None);
    pd::test_overlay(Some(overlay));
    let mut shapes = Vec::new();
    for _ in 0..4 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(2137.0, 1183.0),
                )),
                ..Default::default()
            },
            |ui| {
                app.refresh_frame_now();
                pd::test_viewport([2137.0, 1183.0]);
                pd::test_page_frame_for(&app.page().clone());
                crate::ui::show(app, ui);
                pd::end_frame(ui.ctx());
                app.clear_frame_now();
            },
        );
        output.textures_delta.clear();
        shapes = output.shapes;
    }
    let colours = points
        .iter()
        .map(|[x, y]| pd::test_colour_at(&shapes, egui::pos2(*x, *y), egui::Color32::BLACK))
        .collect();
    pd::test_overlay(None);
    colours
}

/// The anchors' boxes in `tree`, by key.
pub(super) fn boxes(tree: &HostTree) -> BTreeMap<String, [f32; 4]> {
    tree.nodes
        .iter()
        .filter_map(|n| Some((n.key.clone(), n.rect?)))
        .collect()
}

/// The album page's boxes with Luna on and no theme, and under `css` (its
/// cascade over the app's own anchors, once it styles `key`).
pub(super) fn plain_and_themed(
    name: &str,
    css: &str,
    key: &str,
) -> (BTreeMap<String, [f32; 4]>, BTreeMap<String, [f32; 4]>) {
    plain_and_themed_on("album:alb0", Some("queue"), name, css, key)
}

/// [`plain_and_themed`] on the demo's `page` with `show`.
fn plain_and_themed_on(
    page: &str,
    show: Option<&str>,
    name: &str,
    css: &str,
    key: &str,
) -> (BTreeMap<String, [f32; 4]>, BTreeMap<String, [f32; 4]>) {
    let (ctx, mut app) = demo_app(page, show);
    let plain = drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = styled(name, css, &plain, key);
    o.version = 2;
    let themed = drawn(&ctx, &mut app, o);
    (boxes(&plain), boxes(&themed))
}

pub(super) fn top(boxes: &BTreeMap<String, [f32; 4]>, key: &str) -> f32 {
    boxes.get(key).unwrap_or_else(|| panic!("no box for {key}"))[1]
}

pub(super) fn near(got: f32, want: f32) -> bool {
    (got - want).abs() <= 1.0
}

/// The cotton candy family's "header removal": `[class*="_bar_"] {
/// margin-bottom: -2.5em }` (30 points of the bar's 12-point words) pulls
/// TIDAL's content row 30 points up, under the title bar. Every part of
/// the row moves with it: the sidebar's card, the header's controls, the
/// page and the play queue (TIDAL 2.46.0: the row's top from 30 to 0, the
/// sidebar wrapper's `top: 2.25em` then 31.5 under the window's top
/// instead of 61.5). Tidalfast drew the row where it was, 30 points low.
#[test]
fn a_title_bars_negative_margin_pulls_the_content_row_up() {
    let css = r#"/* {"name": "Row pull"} */
[class*="_bar_"] { margin-bottom: -2.5em; background: transparent; }
"#;
    let (plain, pulled) = plain_and_themed("row-pull.css", css, keys::BAR);
    for key in [
        keys::CONTAINER_ROW,
        keys::HEADER_BAR,
        "hero-cover",
        keys::PAGE_HEADER_CONTROLS,
        keys::QUEUE_PANEL,
    ] {
        let (was, now) = (top(&plain, key), top(&pulled, key));
        let want = if cfg!(target_os = "macos") {
            was
        } else {
            was - 30.0
        };
        assert!(near(now, want), "{key}: {was} -> {now}, want {want}");
    }
}

/// What the family then moves back down, each by its own rule: the
/// header's first box by `#mainHeader > * { margin-top: 2em }` (28 down),
/// the page by `#main { margin-top: -10px }` and `.mainContent {
/// margin-top: 2em }` (18 down), the play queue by its wrapper's
/// `transform: translateY(2.6em)` (36.4 down), and the album's Play row
/// by its controls' `margin-top: 34px` (TIDAL's own is 48). TIDAL 2.46.0's
/// header, page, queue and controls move so under these rules; Tidalfast
/// left them all in place.
#[test]
fn the_headers_pages_and_queues_own_margins_move_them() {
    let css = r#"/* {"name": "Row parts"} */
#mainHeader:not([data-test="header"]) > * { margin-top: 2em; }
#main { margin-top: -10px !important; }
.mainContent { margin-top: 2em; }
[class="_playQueueWrapper_cbf5aaa"] { transform: translateY(2.6em); }
[data-test="header-controls"] { margin-top: 34px; }
"#;
    let (plain, moved) = plain_and_themed("row-parts.css", css, keys::MAIN_CONTENT);
    let shift = |key: &str| top(&moved, key) - top(&plain, key);
    assert!(
        near(shift(keys::HEADER_BAR), 28.0),
        "header {}",
        shift(keys::HEADER_BAR)
    );
    assert!(
        near(shift("hero-cover"), 18.0),
        "page {}",
        shift("hero-cover")
    );
    assert!(
        near(shift(keys::QUEUE_PANEL), 36.4),
        "queue {}",
        shift(keys::QUEUE_PANEL)
    );
    // 18 with the page, 14 less room above the Play row.
    assert!(
        near(shift(keys::PAGE_HEADER_CONTROLS), 4.0),
        "controls {}",
        shift(keys::PAGE_HEADER_CONTROLS)
    );
    // The header lies over the page: the page keeps its place for it.
    assert!(near(shift(keys::CONTAINER_ROW), 0.0));
}

/// Blur and TidalCustomTheme take TIDAL's title bar out of the flow
/// (`position: absolute`): the content row starts at the window's top in
/// TIDAL 2.46.0, under the bar, and the content area's `padding-top:
/// 30px` holds the header (its first box in the flow) down where it was.
#[test]
fn a_title_bar_out_of_the_flow_lets_the_row_start_under_it() {
    let css = r#"/* {"name": "Bar out"} */
[class^="_bar_"] { position: absolute; z-index: 100; }
[class^="_contentArea"] { padding-top: 30px; }
"#;
    let (plain, out) = plain_and_themed("bar-out.css", css, keys::CONTENT_AREA);
    let shift = |key: &str| top(&out, key) - top(&plain, key);
    let row = if cfg!(target_os = "macos") {
        0.0
    } else {
        -30.0
    };
    assert!(
        near(shift(keys::CONTAINER_ROW), row),
        "row {}",
        shift(keys::CONTAINER_ROW)
    );
    assert!(
        near(shift("hero-cover"), row),
        "page {}",
        shift("hero-cover")
    );
    assert!(
        near(shift(keys::HEADER_BAR), row + 30.0),
        "header {}",
        shift(keys::HEADER_BAR)
    );
}

/// The family's `[class*="_navigationItem_"] { margin-bottom: .6em }`
/// spaces the sidebar's rows 48.4 points apart in TIDAL 2.46.0 (8.4 under
/// each 40-point row); Tidalfast kept them 40 apart.
#[test]
fn a_navigation_rows_margin_spaces_the_rows() {
    let css = r#"/* {"name": "Row margins"} */
[class*="_navigationItem_"] { margin-bottom: .6em; }
"#;
    let explore = keys::sidebar_item("sidebar-explore");
    let (plain, spaced) = plain_and_themed("nav-margins.css", css, &explore);
    let music = keys::sidebar_item("sidebar-music");
    let gap = |b: &BTreeMap<String, [f32; 4]>| top(b, &explore) - top(b, &music);
    assert!(near(gap(&plain), 40.0), "plain {}", gap(&plain));
    assert!(near(gap(&spaced), 48.4), "spaced {}", gap(&spaced));
}

/// The cotton candy singles' `._grid_97562f5 { padding: 1.3em .8em .9em
/// 1em }` on a collection page: TIDAL 2.46.0's grid keeps its width (its
/// `box-sizing: border-box`) and lays its six cards inside the padding,
/// 227.6 points wide and 14 in from the grid's left. Tidalfast laid the
/// cards as with no padding (232 wide, at the grid's edge) and grew the
/// theme's panel round them instead.
#[test]
fn a_card_grids_padding_lies_inside_it() {
    let css = r#"/* {"name": "Grid padding"} */
._grid_97562f5 { padding: 1.3em .8em .9em 1em; background-color: rgba(255, 255, 255, .2); }
"#;
    let (plain, padded) = plain_and_themed_on("albums", None, "grid.css", css, keys::CARD_GRID);
    let card = |b: &BTreeMap<String, [f32; 4]>, i: usize| {
        *b.get(&format!("card:{i}"))
            .unwrap_or_else(|| panic!("no card {i}"))
    };
    let grid = |b: &BTreeMap<String, [f32; 4]>| *b.get(keys::CARD_GRID).expect("the grid");
    assert!(
        near(card(&plain, 0)[2], 232.0),
        "plain {:?}",
        card(&plain, 0)
    );
    let [gx, gy, gw, _] = grid(&padded);
    assert!(near(gw, grid(&plain)[2]), "the grid keeps its width");
    let first = card(&padded, 0);
    assert!(
        near(first[0], gx + 14.0),
        "card {first:?} in grid {:?}",
        grid(&padded)
    );
    assert!(
        near(first[1], gy + 18.2),
        "card {first:?} in grid {:?}",
        grid(&padded)
    );
    // Six across what the padding leaves: (1472 - 25.2 - 5 x 16) / 6.
    let want = (gw - 14.0 - 11.2 - 5.0 * 16.0) / 6.0;
    assert!(near(first[2], want), "width {} want {want}", first[2]);
}

/// Blur's `[class^="_sidebar_"] { padding-top: 30px }` holds the
/// navigation 30 points down inside `aside#sidebar` (TIDAL 2.46.0: the
/// sidebar's box from the window's top under its bar taken out of the
/// flow, its rows where they were). Tidalfast left the rows where TIDAL's
/// own sidebar has them, so with the row pulled up they sat 30 too high.
#[test]
fn a_sidebars_top_padding_holds_its_rows_down() {
    let css = r#"/* {"name": "Sidebar padding"} */
[class^="_sidebar_"] { padding-top: 30px; }
"#;
    let (plain, padded) = plain_and_themed("sidebar-padding.css", css, keys::SIDEBAR_NAV);
    let music = keys::sidebar_item("sidebar-music");
    let shift = top(&padded, &music) - top(&plain, &music);
    assert!(near(shift, 30.0), "rows {shift}");
}

/// The cotton candy family's sidebar card is `top: 2.25em; height:
/// calc(100% - (var(--miniPlayerHeight) * 1.234))` of TIDAL's content
/// row (the window less its 30-point title bar): 1044.4 points tall in a
/// 1183-point TIDAL window, its foot 1075.9 under the row's top. Tidalfast
/// ended the card as far off the column's foot as its top is off the top.
/// Round 43: Tidalfast's window is TIDAL's content row (it shows TIDAL's
/// page from the title bar's foot down; the parity checker shoots it 1153
/// tall for TIDAL's 1183), so `100%` is the whole window here: the card
/// ended 30 points above TIDAL's.
#[test]
fn a_sidebar_cards_own_height_ends_it() {
    let css = r#"/* {"name": "Card height"} */
[class*="_bar_"] { margin-bottom: -2.5em; }
[class*="_sidebarWrapper_"] { top: 2.25em; height: calc(100% - (var(--miniPlayerHeight) * 1.234)); border-radius: 0 19px 19px 0; background-color: rgb(255, 0, 0); }
"#;
    let (ctx, mut app) = demo_app("album:alb0", Some("queue"));
    let plain = drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    let mut o = styled("card-height.css", css, &plain, keys::SIDEBAR);
    o.version = 2;
    let row = top(&boxes(&plain), keys::CONTAINER_ROW);
    // TIDAL's `--miniPlayerHeight` is 88; the row, pulled 30 up under the
    // title bar as the cotton candy family's is, is the 1183-point window.
    let foot = row - 30.0 + 31.5 + (1183.0 - 88.0 * 1.234);
    let colours = painted_at(
        &ctx,
        &mut app,
        o,
        &[[120.0, foot - 3.0], [120.0, foot + 4.0]],
    );
    let red = |c: egui::Color32| c.r() > 200 && c.g() < 60 && c.b() < 60;
    assert!(red(colours[0]), "inside the card's foot: {:?}", colours[0]);
    assert!(!red(colours[1]), "under it: {:?}", colours[1]);
}

/// TIDAL 2.46.0's track list panel is the list's module box (`.is-
/// intersecting...ALBUM_ITEMS`): the cotton candy family's `padding:
/// 11.2px 12.6px 0` and 2-point border lie round the songs, the glass 14.6
/// points out at the sides and 13.2 above them. Tidalfast laid the panel on
/// the list's own box. Round 43: the box stays where the list was and the
/// padding and border move the songs down in it, as in TIDAL's flow (see
/// `theme_round43_tests::a_song_lists_box_moves_the_list_as_tidals_flow_does`).
#[test]
fn a_list_panels_padding_and_border_lie_round_the_list() {
    let css = r#"/* {"name": "List panel"} */
.is-intersecting._occlusion_de6b9d3._defaultStyles_a1b9da9.ALBUM_ITEMS.tidal-ui__occlusion { padding: 11.2px 12.6px 0; border-style: solid; border-width: 2px; border-color: rgb(255, 255, 255); background-color: rgba(255, 0, 0, .2); }
"#;
    let (plain, padded) = plain_and_themed("list-panel.css", css, keys::LIST_PANEL);
    let list = *plain.get(keys::LIST_PANEL).expect("the list's panel");
    let panel = *padded.get(keys::LIST_PANEL).expect("the list's panel");
    assert!(
        near(panel[0], list[0] - 14.6),
        "left {panel:?} round {list:?}"
    );
    assert!(near(panel[1], list[1]), "top {panel:?} round {list:?}");
    assert!(
        near(panel[2], list[2] + 29.2),
        "width {panel:?} round {list:?}"
    );
}
