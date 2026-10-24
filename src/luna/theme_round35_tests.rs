//! Round 35: TIDAL 2.46.0's element tree where Tidalfast's anchors had a
//! box less or another class, captured under themes. Proved with made-up
//! themes (their own colours, TIDAL's classes and anchors only).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

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

fn wait(
    luna: &Luna,
    tree: &super::dom::HostTree,
    want: &dyn Fn(&DomOverlay) -> bool,
) -> Arc<DomOverlay> {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && want(&o) {
            return o;
        }
        assert!(Instant::now() < end, "the overlay never got there");
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn overlay_for(name: &str, css: &str, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let luna = themed_luna(name, css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(key));
    luna.shutdown();
    o
}

const WINDOW: egui::Color32 = egui::Color32::from_rgb(10, 10, 10);

/// TIDAL 2.46.0's track row holds its title two boxes down: `div.
/// _titleColumn_ac57455[data-test=table-row-title] > div._flexWrapper_9d7a784
/// ._titleCell_e18305f > span._titleText_51cccae` (captured). Rules that
/// name that path reach the title (TidalCustomTheme's playing title
/// `[class*=_tableRowIsPlaying_]>div>div>span`, the cotton candy family's
/// `._titleCell_._flexWrapper_ > ._titleText_` list colour); Tidalfast's
/// title sat one box higher, in a cell carrying `_titleCell_`, so they
/// missed it.
#[test]
fn rules_on_tidals_title_path_reach_the_title() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Deep titles"} */
[class*=_tableRowIsPlaying_]>div>div>span { color: rgb(240, 200, 30) !important; }
._titleCell_e18305f._flexWrapper_9d7a784 > .table--inactive-title._titleText_51cccae { color: rgb(90, 160, 230); }
[class^="_titleColumn_"] { padding-left: 3px; }
[data-test=tracklist-row] { padding-top: 1px; }
"#;
    let list = "album:55";
    let o = overlay_for("deep-titles.css", css, &dom_anchors::keys::row(list, 1));
    let cell = o.hosts.get(&dom_anchors::keys::row_cell(list, 1, "title"));
    assert!(
        cell.is_some(),
        "the title column takes `[class^=_titleColumn_]`"
    );
    pd::test_overlay(Some((*o).clone()));
    let playing = pd::playing_title(list, 0, WINDOW);
    let idle = pd::row_text(list, 1, WINDOW).and_then(|t| t.title);
    pd::test_overlay(None);
    assert_eq!(playing, Some(egui::Color32::from_rgb(240, 200, 30)));
    assert_eq!(idle, Some(egui::Color32::from_rgb(90, 160, 230)));
}

/// TIDAL's header art is `._dataContainer_::before`, absolutely placed in
/// `main`, the scroll container: a theme's `height: 100%` on it is `main`'s
/// own height, not the page's content (TIDAL 2.46.0 under Sunken Tidal,
/// Giorgeous and Lunallena: 1153 points in a 1183-point window, with 20
/// songs below the header). Tidalfast took the content's height, so the
/// cover's fade to black stretched down every row and the rows near the
/// top lay on a much brighter cover than TIDAL's.
#[test]
fn a_full_height_header_art_box_is_as_tall_as_the_page_box() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::PAGE_DATA;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.data = Some([240.0, 92.0, 1040.0, 300.0]);
    }
    let css = r#"/* {"name": "Tall art"} */
[class*="dataContainer"]:before { height: 100% !important; }
"#;
    let tree = dom_anchors::build(&frame);
    let luna = themed_luna("tall-art.css", css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(PAGE_DATA));
    luna.shutdown();
    let page = egui::Rect::from_min_size(egui::pos2(240.0, 30.0), egui::vec2(1600.0, 1000.0));
    pd::test_overlay(Some((*o).clone()));
    // The page's content runs 2400 points down.
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        pd::main_scroll(ui, 0.0, 2400.0, page);
    });
    output.textures_delta.clear();
    let rect = crate::ui::cover_backdrop_rect(page);
    pd::test_overlay(None);
    assert_eq!(rect.min, page.min);
    assert_eq!(rect.height(), page.height());
}
