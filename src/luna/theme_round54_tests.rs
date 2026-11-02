//! Round 54: what the parity checker and TIDAL 2.47.0's captured pages
//! showed after round 53. Proved with made-up themes (their own colours,
//! TIDAL's classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

/// Installs `css` as a theme over `frame`'s anchors and returns the overlay
/// once `ready` holds for it.
fn overlay_when(
    name: &str,
    css: &str,
    frame: &dom_anchors::Frame,
    ready: impl Fn(&DomOverlay) -> bool,
) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
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
        if o.css_bytes > 0 && ready(&o) {
            break o;
        }
        assert!(Instant::now() < end, "{name} never took");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    o
}

/// The overlay over `frame` with no theme installed, its anchors synced.
fn plain_overlay(name: &str, frame: &dom_anchors::Frame) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
    let data = temp_dir(name);
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    for _ in 0..5 {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(30));
    }
    let o = luna.dom_overlay();
    luna.shutdown();
    o
}

/// The maintainer's decision: TIDAL's frosted glass under the search pill
/// (`backdrop-filter: var(--frosted-glass-nav-blur)` in TIDAL's own rule)
/// is drawn under a theme, and with no theme the pill stays Tidalfast's
/// own, no filter at all. A theme's token and a theme's `none` are its own.
#[test]
fn the_search_pills_blur_comes_with_a_theme_alone() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::SEARCH_FIELD;
    let _drawn = backdrop::force(true);
    let frame = dom_anchors::sample_frame();
    let filter = |o: &DomOverlay| {
        pd::test_overlay(Some(o.clone()));
        let got = pd::anchor_look(SEARCH_FIELD).and_then(|l| l.backdrop.clone());
        pd::test_overlay(None);
        got
    };
    let any = |o: &DomOverlay| o.hosts.contains_key(dom_anchors::keys::FOOTER);
    let o = overlay_when(
        "pill-plain.css",
        "/* {\"name\": \"Pill plain\"} */\n._player_8c80ae6 { color: rgb(1, 2, 3); }\n",
        &frame,
        any,
    );
    assert_eq!(
        filter(&o).as_deref(),
        backdrop::Backdrop::parse("blur(20px) saturate(180%)").as_ref(),
    );
    let o = overlay_when(
        "pill-token.css",
        "/* {\"name\": \"Pill token\"} */\n:root { --frosted-glass-nav-blur: blur(9px); }\n._player_8c80ae6 { color: rgb(1, 2, 3); }\n",
        &frame,
        any,
    );
    assert_eq!(
        filter(&o).as_deref(),
        backdrop::Backdrop::parse("blur(9px)").as_ref(),
    );
    let o = overlay_when(
        "pill-none.css",
        "/* {\"name\": \"Pill none\"} */\n[data-type=\"search-field\"] { backdrop-filter: none !important; }\n._player_8c80ae6 { color: rgb(1, 2, 3); }\n",
        &frame,
        any,
    );
    assert_eq!(filter(&o), None);
    let o = plain_overlay("pill-no-theme", &frame);
    assert_eq!(filter(&o), None, "no theme, no filter");
}

/// The pill's filter is painted once, and under its fill: a paint callback
/// ahead of the fill, as the bar's and the queue's are, with the pill's own
/// fill and with a fill of the theme's.
#[test]
fn the_search_pills_blur_is_painted_under_its_fill() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    let _drawn = backdrop::force(true);
    let frame = dom_anchors::sample_frame();
    for (name, rules) in [
        ("pill-painted.css", ""),
        (
            "pill-filled.css",
            ".searchFieldWithinSearchFieldWithPopover { background-color: rgba(1, 2, 3, 0.5); }",
        ),
    ] {
        let css = format!(
            "/* {{\"name\": \"{name}\"}} */ ._player_8c80ae6 {{ color: rgb(1, 2, 3); }} {rules}"
        );
        let o = overlay_when(name, &css, &frame, |o| {
            o.hosts.contains_key(dom_anchors::keys::FOOTER)
        });
        pd::test_overlay(Some((*o).clone()));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let r = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(280.0, 36.0));
            let fill = egui::Color32::from_rgba_unmultiplied(9, 9, 9, 191);
            pd::paint_search_pill(ui, r, 18.0, fill);
        });
        out.textures_delta.clear();
        pd::test_overlay(None);
        let order: Vec<bool> = out
            .shapes
            .iter()
            .map(|c| matches!(c.shape, egui::epaint::Shape::Callback(_)))
            .collect();
        assert_eq!(order.first(), Some(&true), "{name}: {order:?}");
        assert_eq!(order.iter().filter(|c| **c).count(), 1, "{name}: {order:?}");
        assert!(order.len() > 1, "{name}: {order:?}");
    }
}

/// A theme that keeps TIDAL's bar glass over its own bar colour: the glass
/// filters that colour (`blur(20px) saturate(180%)`) before its `#282828bf`
/// goes over it, so a blue bar stays bluer than the plain mix. The filter is
/// painted between the box's own look and the glass, once.
#[test]
fn tidals_bar_glass_filters_the_bars_own_colour() {
    use crate::ui::backdrop;
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::FOOTER;
    let _drawn = backdrop::force(true);
    let css =
        "/* {\"name\": \"Glass kept\"} */ ._player_8c80ae6 { background-color: rgb(0, 80, 200); }";
    let o = overlay_when("glass-kept.css", css, &dom_anchors::sample_frame(), |o| {
        o.hosts.contains_key(FOOTER)
    });
    pd::test_overlay(Some((*o).clone()));
    assert!(pd::footer_glass());
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 500.0), egui::vec2(800.0, 96.0));
        pd::paint_host_layers_under_glass(ui, FOOTER, r, pd::TIDAL_BAR_GLASS);
    });
    out.textures_delta.clear();
    pd::test_overlay(None);
    let order: Vec<&str> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Callback(_) => Some("filter"),
            egui::Shape::Rect(r) if r.fill == pd::TIDAL_BAR_GLASS => Some("glass"),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["filter", "glass"]);
}

/// TIDAL colours the search pill in a rule three classes deep, so a theme's
/// lighter rule for it does not reach it (`._container_f1be359 { background
/// }`), a rule as deep or deeper does. The theme's rule won here.
#[test]
fn a_lighter_rule_for_the_search_pill_loses_to_tidals() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::SEARCH_FIELD;
    let frame = dom_anchors::sample_frame();
    let fill = |name: &str, rules: &str| {
        let css = format!("/* {{\"name\": \"{name}\"}} */ {rules}");
        let o = overlay_when(name, &css, &frame, |o| o.hosts.contains_key(SEARCH_FIELD));
        pd::test_overlay(Some((*o).clone()));
        let got = pd::anchor_look(SEARCH_FIELD).and_then(|l| l.fill);
        pd::test_overlay(None);
        got
    };
    let tidal = egui::Color32::from_rgba_unmultiplied(40, 40, 40, 191);
    assert_eq!(
        fill(
            "pill-light.css",
            "._container_f1be359 { background-color: rgb(1, 2, 3); }"
        ),
        Some(tidal)
    );
    assert_eq!(
        fill(
            "pill-deep.css",
            "._container_cb5cbe4 ._searchFieldWithPopover_267e090 .searchFieldWithinSearchFieldWithPopover { background-color: rgb(1, 2, 3); }"
        ),
        Some(egui::Color32::from_rgb(1, 2, 3))
    );
}

/// TIDAL colours most of its parts by a class or tag of their own, so a
/// universal rule (`* { color }`) or a tag rule reaches only what has no
/// colour of its own there. The anchors had none of those rules, so it won
/// on every word and button. As heavy a rule of the theme's still reaches
/// them, and the sheet's rules stand with no theme at all as they were.
#[test]
fn a_universal_colour_loses_to_tidals_own_colours() {
    use dom_anchors::keys::{ARTIST, CURRENT_TIME, MUTE, SEARCH_FIELD};
    let css = "/* {\"name\": \"Everything white\"} */ * { color: rgb(9, 8, 7); }";
    let o = overlay_when("universal.css", css, &dom_anchors::sample_frame(), |o| {
        o.hosts.contains_key(SEARCH_FIELD)
    });
    let color = |o: &DomOverlay, key: &str| o.hosts.get(key).and_then(|h| h.style.color.clone());
    for key in [ARTIST, CURRENT_TIME, MUTE] {
        assert_eq!(color(&o, key), None, "{key}");
    }
    assert!(
        color(&o, SEARCH_FIELD).is_some_and(|c| c.contains("9, 8, 7")),
        "a part with no colour of its own in TIDAL takes it"
    );
    let css = "/* {\"name\": \"Words\"} */ ._marketText_1lyag_1 { color: rgb(9, 8, 7); }";
    let o = overlay_when("words.css", css, &dom_anchors::sample_frame(), |o| {
        o.hosts.contains_key(ARTIST)
    });
    assert!(color(&o, ARTIST).is_some_and(|c| c.contains("9, 8, 7")));
}

/// `data-test-page-name` is `#main`'s own attribute in TIDAL, so a theme's
/// `[data-test-page-name="album"] { margin-top }` takes the place of its
/// `[data-test="main"] { margin-top }` on that page. It stood on the page's
/// box inside: `#main` kept the other margin and the page stood 10 low.
#[test]
fn the_pages_name_stands_on_main() {
    use dom_anchors::keys::MAIN;
    let mut frame = dom_anchors::sample_frame();
    let page = frame.page.get_or_insert_default();
    page.page_id = Some("album".into());
    let css = "/* {\"name\": \"Two margins\"} */\n\
        [data-test=\"main\"] { margin-top: 66px; }\n\
        [data-test-page-name=\"album\"] { margin-top: 56px; }\n";
    let o = overlay_when("two-margins.css", css, &frame, |o| {
        o.hosts.contains_key(MAIN)
    });
    assert_eq!(o.hosts[MAIN].style.margin_top.as_deref(), Some("56px"),);
}

/// The "All playlists" heading's black box is drawn on the open sidebar, not
/// only on the rail, and its words keep TIDAL's own grey under a rule for
/// the box: the box's colour does not reach words with a colour of their own.
#[test]
fn the_open_sidebar_draws_the_headings_box_and_its_own_grey() {
    use super::theme_round41_tests::{boxes, demo_app, drawn, painted_at};
    use dom_anchors::keys::{LIB_HEADING, LIB_HEADING_TITLE};
    let css = "/* {\"name\": \"Coloured sidebar 2\"} */\n\
        ._sidebarWrapper_2f256ce { background: rgb(30, 40, 50); }\n\
        ._sectionHeader_ca54638 { color: rgb(240, 10, 10); }\n";
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.heading = Some([16.0, 300.0, 160.0, 22.0]);
    let o = overlay_when("coloured-sidebar-2.css", css, &frame, |o| {
        o.hosts.contains_key(LIB_HEADING)
    });
    let (ctx, mut app) = demo_app("album", Some("queue"));
    let tree = drawn(&ctx, &mut app, (*o).clone());
    let title = boxes(&tree)[LIB_HEADING_TITLE];
    let at = [[title[0] + 140.0, title[1] + 2.0]];
    let got = painted_at(&ctx, &mut app, (*o).clone(), &at);
    assert_eq!(got, [egui::Color32::BLACK]);
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    let ink = crate::ui::plugin_dom::library_heading(
        egui::Rect::from_min_size(egui::pos2(16.0, 300.0), egui::vec2(160.0, 22.0)),
        egui::Color32::WHITE,
    );
    crate::ui::plugin_dom::test_overlay(None);
    assert_eq!(ink, None, "TIDAL's grey ({LIB_HEADING})");
}

/// `#mainHeader` lies over the page in TIDAL (`position: fixed`), so a header
/// a theme pulls up into a tab (Lunallena's `margin-top: -80px`) leaves the
/// page where it was. The page followed the tab up, 45 points on a collection.
#[test]
fn a_header_pulled_into_a_tab_leaves_the_page() {
    use super::theme_round41_tests::{plain_and_themed, top};
    use dom_anchors::keys::{HEADER, PAGE_TITLE};
    let css = r#"/* {"name": "Tab"} */
#mainHeader {
    background-color: rgba(50, 50, 50, 0.5);
    margin-top: -80px;
    width: 30vw;
    left: 50%;
    transform: translateX(-50%);
    border-radius: 0 0 20px 20px;
    padding-top: 45px;
}
"#;
    let (plain, tab) = plain_and_themed("tab.css", css, HEADER);
    assert_eq!(top(&tab, PAGE_TITLE), top(&plain, PAGE_TITLE));
}
