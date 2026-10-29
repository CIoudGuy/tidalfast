//! Round 45: what the parity checker's comparisons found. Proved on the
//! demo's pages, with made-up themes (their own values, TIDAL's classes and
//! anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors::keys;
use super::theme_round41_tests as r41;
use super::*;

/// The watchdog stops a turn of the runtime's own that runs too long, and
/// the first cascade of a page under a large theme can (Cotton Candy
/// adaptive's 5,700 lines on a debug build or a busy machine). The stopped
/// overlay build left nothing asking for the overlay again, so a page
/// that then stayed as it was kept no theme at all: the album's list panel
/// and everything else bare, on some builds and not others. The build is
/// tried again and goes on from the cascades already worked out.
#[test]
fn an_overlay_build_the_watchdog_stops_is_built_again() {
    let (ctx, mut app) = r41::demo_app("album:alb0", Some("queue"));
    let tree = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    app.backend.shutdown();
    // Rules every element is matched against (no id, class, tag or
    // attribute to index them by, and each looks at every ancestor), quick
    // to read and parse but slow to match: the page's first cascade runs
    // far past the watchdog.
    let mut css = String::from(
        ".is-intersecting._occlusion_de6b9d3.ALBUM_ITEMS { background-color: rgb(1, 2, 3); }\n",
    );
    for i in 0..300 {
        css.push_str(&format!(
            ":is(.zz{i} *) {{ color: rgb(9, 9, {}); }}\n",
            i % 256
        ));
    }
    let name = "watchdog-theme.css";
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert(format!("/{name}"), css.into_bytes());
    let base = serve(files);
    let mut cfg = config(&temp_dir("round45-watchdog"));
    cfg.watchdog = Duration::from_millis(400);
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    // The page comes once and the theme after it: nothing changes after
    // the theme's sheet to ask for the overlay again.
    luna.sync_dom(tree);
    luna.sync(Duration::from_secs(5));
    luna.install_theme(&format!("{base}/{name}"));
    let end = Instant::now() + Duration::from_secs(60);
    let styled = loop {
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if let Some(h) = o
            .hosts
            .get(keys::LIST_PANEL)
            .filter(|h| !h.style.is_empty())
        {
            break h.style.clone();
        }
        if Instant::now() > end {
            break Default::default();
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stopped = luna
        .snapshot()
        .runtime_log
        .iter()
        .any(|l| l.message.contains("interrupted"));
    luna.shutdown();
    assert!(
        stopped,
        "the watchdog never stopped the build: the test proves nothing here"
    );
    assert_eq!(
        styled.background_color.as_deref(),
        Some("rgb(1, 2, 3)"),
        "the list panel never got the theme's style"
    );
}

/// The overlay a theme's `css` gives the demo's `page` once `done` holds.
fn page_overlay(
    name: &str,
    css: &str,
    page: &str,
    show: Option<&str>,
    done: impl Fn(&DomOverlay) -> bool,
) -> DomOverlay {
    let (ctx, mut app) = r41::demo_app(page, show);
    let tree = r41::drawn(
        &ctx,
        &mut app,
        DomOverlay {
            version: 1,
            ..DomOverlay::default()
        },
    );
    app.backend.shutdown();
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert(format!("/{name}"), css.as_bytes().to_vec());
    let base = serve(files);
    let luna = Luna::start(config(&temp_dir(name)), Arc::new(MockHost::default())).unwrap();
    luna.install_theme(&format!("{base}/{name}"));
    let end = Instant::now() + Duration::from_secs(30);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if done(&o) {
            break o;
        }
        assert!(Instant::now() < end, "{page}: the theme never got there");
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.shutdown();
    (*o).clone()
}

/// TIDAL 2.46.0's `main` holds a scroll sentinel before `div.mainContent`,
/// and the page's box is `mainContent`'s first child (captured). A theme's
/// `div:nth-child(1) > div > div { border-style: none }` (the cotton candy
/// nights colourways', meant for the song rows' lines) so never reaches
/// the list panel there, and the panel keeps the theme's 2-point border
/// (TIDAL's songs 2 lower for it). Tidalfast's page content was `main`'s
/// first child and its page box second, so the rule took the border off.
#[test]
fn a_div_chain_rule_reaches_the_list_panel_only_where_tidals_does() {
    let css = "div:nth-child(1) > div > div { border-style: none; }\n\
        ._items_8615603, .is-intersecting._occlusion_de6b9d3.ALBUM_ITEMS { border-style: solid; border-width: 2px; border-color: rgb(1, 2, 3); }\n";
    for page in ["playlist:pl0", "album:alb0"] {
        let o = page_overlay("div-chain.css", css, page, Some("queue"), |o| {
            o.hosts
                .get(keys::LIST_PANEL)
                .is_some_and(|h| h.style.border_color.is_some() || h.style.border.is_some())
        });
        let style = &o.hosts[keys::LIST_PANEL].style;
        assert_eq!(
            style.border.as_deref(),
            Some("2px solid rgb(1, 2, 3)"),
            "{page}: {style:?}"
        );
    }
}

/// TIDAL 2.46.0 has a dozen `_artwork_` modules, and a theme's rule for one
/// (the cotton candy family's `._artwork_b010eae { border-radius: 1em }`)
/// reaches that one alone: the bar's cover (`figure._artwork_941d1cf`) and
/// a playlist's header art (`button._artwork_1d717fe`) keep TIDAL's 2-point
/// corners. Tidalfast read any `_artwork_` hash as either of them.
#[test]
fn a_rule_for_one_artwork_module_reaches_that_one_alone() {
    let css = "._artwork_b010eae { border-radius: 1em; }\n\
        ._artwork_941d1cf { border-radius: 3px; }\n";
    let o = page_overlay("artwork.css", css, "playlist:pl0", None, |o| {
        o.hosts
            .get(keys::COVER)
            .is_some_and(|h| h.style.border_radius.is_some())
    });
    assert_eq!(
        o.hosts[keys::COVER].style.border_radius.as_deref(),
        Some("3px")
    );
    assert_eq!(
        o.hosts
            .get(keys::HERO_COVER)
            .and_then(|h| h.style.border_radius.as_deref()),
        None
    );
}
