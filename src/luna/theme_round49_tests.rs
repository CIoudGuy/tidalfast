//! Round 49: what the parity checker's comparisons found. Proved on the
//! demo's pages, with made-up themes (their own values, TIDAL's classes and
//! anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::*;

/// Installs `css` as a theme over `tree` and returns the overlay once the
/// theme reached it and gave `key` a colour.
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
        if o.css_bytes > 0 && o.hosts.get(key).is_some_and(|h| h.style.color.is_some()) {
            break o;
        }
        assert!(Instant::now() < end, "{key:?} was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    (luna, o)
}

/// TIDAL 2.47.0 gives its search pill no shadow by a rule three classes
/// deep (`:is(._container_cb5cbe4 ._searchFieldWithPopover_267e090)
/// .searchFieldWithinSearchFieldWithPopover { box-shadow: none }`), so a
/// theme's one-class rule for the pill's `._container_f1be359` (the
/// cotton candy family's drop shadow) leaves it without one, while a
/// theme rule as deep wins. Tidalfast drew the lighter rule's shadow.
#[test]
fn a_lighter_rule_does_not_give_the_search_pill_a_shadow() {
    use dom_anchors::keys::SEARCH_FIELD;
    let shadow = |css: &str, name: &str| {
        let (luna, o) = overlay_with(name, css, dom_anchors::sample_tree(), SEARCH_FIELD);
        let s = o.hosts[SEARCH_FIELD].style.box_shadow.clone();
        luna.shutdown();
        s.unwrap_or_default()
    };
    let light = shadow(
        "/* {\"name\": \"Light\"} */
._container_f1be359 { box-shadow: 0 4px 8px rgb(255, 0, 0); color: rgb(1, 2, 3); }
",
        "light-shadow.css",
    );
    assert!(!light.contains("255"), "{light}");
    let deep = shadow(
        "/* {\"name\": \"Deep\"} */
._container_f1be359 { color: rgb(1, 2, 3); }
:is(._container_cb5cbe4 ._searchFieldWithPopover_267e090) .searchFieldWithinSearchFieldWithPopover { box-shadow: 0 0 4px rgb(0, 0, 255); }
",
        "deep-shadow.css",
    );
    assert!(deep.contains("255"), "{deep}");
}

/// TIDAL 2.47.0 fills the open playlist's library row with its own
/// `--market-core-fill-30-color` (white at 15 % in dark mode) whatever
/// colours a theme gives the app, so on a theme's pink sidebar the row is
/// a lighter pink (TidalCustomTheme: 249, 170, 200 over 248, 156, 191).
/// Tidalfast filled it with a shade made from the theme's palette, darker
/// than the sidebar (212, 154, 189). A theme that sets the token still
/// gets its own colour.
#[test]
fn the_open_playlists_row_takes_tidals_veil_under_a_theme() {
    use super::theme_round41_tests as r41;
    use crate::luna::dom::HostTree;
    let (ctx, mut app) = r41::demo_app("playlist:pl0", None);
    // The palette a theme that paints the app pink makes.
    let mut palette = crate::theme::Palette::light();
    palette.window = egui::Color32::from_rgb(248, 156, 191);
    palette.panel = palette.window;
    palette.surface_active = egui::Color32::from_rgb(212, 154, 189);
    palette.themed = true;
    app.palette = palette;
    let plain = DomOverlay {
        version: 49_001,
        ..DomOverlay::default()
    };
    let tree: HostTree = r41::drawn(&ctx, &mut app, plain.clone());
    let row = |selected: bool| {
        tree.nodes
            .iter()
            .find(|n| {
                n.key.starts_with("lib-row:")
                    && n.key.matches(':').count() == 1
                    && n.attrs
                        .iter()
                        .any(|(k, v)| k == "class" && v.contains("_selected_"))
                        == selected
            })
            .and_then(|n| n.rect)
            .expect("a library row")
    };
    let open = row(true);
    let other = row(false);
    // Beside the words, at the row's right end.
    let at = |r: [f32; 4]| [r[0] + r[2] - 6.0, r[1] + r[3] / 2.0];
    let paint = |o: DomOverlay, app: &mut crate::app::App| {
        r41::painted_at(&ctx, app, o, &[at(open), at(other)])
    };
    let seen = paint(plain, &mut app);
    let veil = |under: egui::Color32, over: egui::Color32| {
        let a = f32::from(over.a()) / 255.0;
        let [r, g, b, _] = under.to_array();
        let [or, og, ob, _] = over.to_srgba_unmultiplied();
        let mix = |u: u8, o: u8| (f32::from(u) + (f32::from(o) - f32::from(u)) * a).round() as i32;
        [mix(r, or), mix(g, og), mix(b, ob)]
    };
    let near = |c: egui::Color32, want: [i32; 3]| {
        let [r, g, b, _] = c.to_array();
        (i32::from(r) - want[0]).abs() <= 2
            && (i32::from(g) - want[1]).abs() <= 2
            && (i32::from(b) - want[2]).abs() <= 2
    };
    let want = veil(seen[1], egui::Color32::from_white_alpha(38));
    assert!(
        near(seen[0], want),
        "the open row {:?}, the sidebar {:?}: want {want:?}",
        seen[0],
        seen[1]
    );
    // A theme's value for the token is the one drawn.
    let mut own = DomOverlay {
        version: 49_002,
        ..DomOverlay::default()
    };
    own.vars.insert(
        "--market-core-fill-30-color".into(),
        "rgb(10, 20, 30)".into(),
    );
    let seen = paint(own, &mut app);
    assert!(near(seen[0], [10, 20, 30]), "{:?}", seen[0]);
    app.backend.shutdown();
}
