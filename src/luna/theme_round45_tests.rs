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
