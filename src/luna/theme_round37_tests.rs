//! Round 37: differences the parity checker found between TIDAL 2.46.0's
//! computed styles under a theme and Tidalfast's resolved anchor looks.
//! Proved with made-up themes (their own colours, TIDAL's classes and
//! anchors only).

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

fn overlay(name: &str, css: &str, frame: &dom_anchors::Frame, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(frame);
    let luna = themed_luna(name, css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(key));
    luna.shutdown();
    o
}

/// TIDAL's root defines its layout in variables (TIDAL 2.46.0:
/// `--miniPlayerHeight: 88px`, `--headerHeight: 56px`, ...), and themes
/// size their boxes by them: the cotton candy family's `.mainContent {
/// padding-bottom: var(--miniPlayerHeight) }` is 88 points in TIDAL. They
/// were undefined here, so the rule was dropped.
#[test]
fn tidals_layout_variables_are_defined() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::MAIN_CONTENT;
    let css = r#"/* {"name": "Layout vars"} */
.mainContent { opacity: 0.99; padding-bottom: var(--miniPlayerHeight); padding-top: calc(var(--headerHeight) / 4); }
"#;
    let o = overlay(
        "layout-vars.css",
        css,
        &dom_anchors::sample_frame(),
        MAIN_CONTENT,
    );
    pd::test_overlay(Some((*o).clone()));
    let look = pd::anchor_look(MAIN_CONTENT);
    pd::test_overlay(None);
    let padding = look
        .and_then(|l| l.padding)
        .expect("main content's padding");
    assert_eq!(padding[2], 88.0, "{padding:?}");
    assert_eq!(padding[0], 14.0, "{padding:?}");
}

/// TIDAL's own dark base steps (`--wave-color-solid-base-bright`,
/// `-brighter`, `-brightest`: #18181b, #242429, #303036 in TIDAL 2.46.0)
/// are what a theme reads with no palette of its own: the cotton candy
/// family's and Blur's `color-mix(in srgb, var(--wave-color-solid-base-
/// brighter), ...)` play queue is TIDAL's grey 36, not the surface 24.
#[test]
fn tidals_own_dark_base_steps_are_its_tokens() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE_PANEL;
    let mut frame = dom_anchors::sample_frame();
    frame.queue_panel = Some((Some([1000.0, 90.0, 334.0, 600.0]), false));
    let css = r#"/* {"name": "Brighter glass"} */
#playQueueSidebar { background: color-mix(in srgb, var(--wave-color-solid-base-brighter), transparent 75%); }
"#;
    let o = overlay("brighter.css", css, &frame, QUEUE_PANEL);
    pd::test_overlay(Some((*o).clone()));
    let _ = pd::themed_palette(crate::theme::Palette::dark());
    let look = pd::anchor_look(QUEUE_PANEL);
    pd::test_overlay(None);
    let fill = look.and_then(|l| l.fill).expect("the panel's fill");
    let [r, g, b, a] = fill.to_srgba_unmultiplied();
    assert!(
        (i32::from(r) - 36).abs() <= 1
            && (i32::from(g) - 36).abs() <= 1
            && (i32::from(b) - 41).abs() <= 1,
        "{fill:?}"
    );
    assert!((i32::from(a) - 64).abs() <= 1, "{fill:?}");
}

/// TIDAL 2.46.0's selected navigation row has exactly two classes
/// (`_navigationItem_4e9deae _selected_29027d1`, captured on the Collection
/// page), which the cotton candy family names whole: `[class=
/// "_navigationItem_4e9deae _selected_29027d1"] { ._icon_77f3f89 { fill } }`
/// colours the open row's icons. The anchor carried a third class, so the
/// rule missed it.
#[test]
fn the_selected_navigation_rows_class_list_is_tidals() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{sidebar_item, sidebar_item_arrow, sidebar_item_icon};
    let css = r#"/* {"name": "Whole class"} */
._navigationItem_4e9deae { opacity: 0.99; }
[class="_navigationItem_4e9deae _selected_29027d1"] { ._icon_77f3f89 { fill: rgb(10, 200, 100); } }
"#;
    let mut frame = dom_anchors::sample_frame();
    for item in &mut frame.sidebar.items {
        item.selected = false;
    }
    frame.sidebar.items.push(dom_anchors::SidebarItem {
        test: "sidebar-collection-menu".into(),
        label: "Collection".into(),
        href: Some("/my-collection".into()),
        selected: true,
        rect: Some([16.0, 248.0, 208.0, 40.0]),
    });
    let row = sidebar_item("sidebar-collection-menu");
    let o = overlay("whole-class.css", css, &frame, &row);
    pd::test_overlay(Some((*o).clone()));
    let inks = [
        pd::sidebar_icon(&sidebar_item_arrow("sidebar-collection-menu")),
        pd::sidebar_icon(&sidebar_item_icon("sidebar-collection-menu")),
    ];
    pd::test_overlay(None);
    assert_eq!(inks, [Some(egui::Color32::from_rgb(10, 200, 100)); 2]);
}

/// TIDAL 2.46.0's album and playlist headers are `_dataContainer_66f4f40`
/// and `_dataContainer_9f47346`, and its bar's title box is
/// `_titleContainer_2da23c0` (one of nine `_titleContainer_` modules). A
/// theme's hash from an older TIDAL build names none of them: the cotton
/// candy family's `[class="_dataContainer_108ce86"] { transform:
/// translateY(-6em) }` and `[class="_titleContainer_9fba00e"] { transform:
/// translateX(-.38em) }` move nothing in TIDAL, while here they lifted the
/// album header 84 points and moved the bar's title.
#[test]
fn an_older_tidal_builds_hash_names_neither_the_header_nor_the_bars_title() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::{PAGE_DATA, TITLE};
    let css = r#"/* {"name": "Old hashes"} */
[class="_dataContainer_108ce86"] { transform: translateY(-6em); }
[class="_dataContainer_66f4f40"] { opacity: 0.5; }
[class="_titleContainer_9fba00e"] { transform: translateX(-.38em); }
[class="_titleContainer_2da23c0"] { opacity: 0.5; }
"#;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.page_id = Some("album".into());
        page.data = Some([240.0, 92.0, 1040.0, 300.0]);
    }
    let tree = dom_anchors::build(&frame);
    let luna = themed_luna("old-hashes.css", css);
    let o = wait(&luna, &tree, &|o| {
        o.hosts.contains_key(PAGE_DATA) && o.hosts.contains_key(TITLE)
    });
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let looks = [pd::anchor_look(PAGE_DATA), pd::anchor_look(TITLE)];
    pd::test_overlay(None);
    for look in looks {
        let look = look.expect("a look");
        assert_eq!(look.opacity, Some(0.5));
        assert_eq!(look.translate, egui::Vec2::ZERO);
    }
}

/// A header's meta words are TIDAL 2.46.0's `span.wave-text-capital-demi
/// ._metaItem_ed501cd[data-wave-color=textSecondary]` (the release date's
/// anchor stands for them): the cotton candy family's `[class=
/// "_dataContainer_66f4f40"] { [data-wave-color=textSecondary] { color;
/// font-weight: 700 } }` makes the album's meta line dark and bold in
/// TIDAL, and the header paints its meta words in that look.
#[test]
fn a_headers_meta_words_take_the_rules_for_tidals_meta_items() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::PAGE_RELEASE;
    let css = r#"/* {"name": "Meta words"} */
[data-test="meta-release-date"] { opacity: 0.99; }
[class="_dataContainer_66f4f40"] { [data-wave-color=textSecondary] { color: rgb(10, 20, 30); font-weight: 700; } }
"#;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.page_id = Some("album".into());
        page.data = Some([240.0, 92.0, 1040.0, 300.0]);
        page.meta = Some(dom_anchors::MetaLine {
            rect: Some([240.0, 300.0, 400.0, 16.0]),
            release_date: "2019".into(),
        });
    }
    let tree = dom_anchors::build(&frame);
    let luna = themed_luna("meta-words.css", css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(PAGE_RELEASE));
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let style = crate::ui::collection::meta_style(&crate::theme::Palette::dark());
    pd::test_overlay(None);
    assert_eq!(style, (egui::Color32::from_rgb(10, 20, 30), 700));
}

/// TIDAL 2.46.0's queue button is the first of the bar's utility buttons
/// (`._utilityButtons_4d7aaf9 > button.withBackground[data-test=
/// play-queue-button]`, no `_moreContainer_`), so the cotton candy
/// family's `._utilityButtons_4d7aaf9 button { border-radius: .7em; color }`
/// rounds and colours it as the others; under a container TIDAL dropped,
/// the anchor kept TIDAL's grey.
#[test]
fn the_queue_button_is_one_of_the_bars_utility_buttons() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::QUEUE;
    let css = r#"/* {"name": "Utility buttons"} */
[data-test="play-queue-button"] { opacity: 0.99; }
._utilityButtons_4d7aaf9 button { border-radius: .7em; color: rgb(10, 20, 30); }
"#;
    let o = overlay(
        "utility-buttons.css",
        css,
        &dom_anchors::sample_frame(),
        QUEUE,
    );
    pd::test_overlay(Some((*o).clone()));
    let look = pd::anchor_look(QUEUE);
    pd::test_overlay(None);
    let look = look.expect("the queue button's look");
    assert_eq!(look.text, Some(egui::Color32::from_rgb(10, 20, 30)));
    assert_eq!(look.corners, Some(egui::CornerRadius::same(10)));
    // Under a theme that leaves it alone it keeps TIDAL's own disc: the
    // see-through look of a bare utility button (for plugin buttons that
    // copy its class) is not the queue button's.
    let css = "/* {\"name\": \"Bar only\"} */
._utilityButtons_4d7aaf9 { opacity: 0.99; }
";
    let o = overlay(
        "bar-only.css",
        css,
        &dom_anchors::sample_frame(),
        dom_anchors::keys::UTILITY_BUTTONS,
    );
    pd::test_overlay(Some((*o).clone()));
    let fill = pd::fill(QUEUE);
    pd::test_overlay(None);
    assert_eq!(fill, None);
}

/// The open playlist's row in the sidebar is TIDAL 2.46.0's
/// `_contentItem_684cbbe _selected_ae83ba5`, two classes, which the cotton
/// candy family names whole (`[class="_contentItem_684cbbe
/// _selected_ae83ba5"] { padding-bottom: 6px }`); the anchor carried a
/// third.
#[test]
fn the_open_playlists_row_class_list_is_tidals() {
    use crate::ui::plugin_dom as pd;
    use dom_anchors::keys::lib_row;
    let css = r#"/* {"name": "Whole row class"} */
._contentItem_684cbbe { opacity: 0.99; }
[class="_contentItem_684cbbe _selected_ae83ba5"] { padding-bottom: 6px; }
"#;
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.rows = vec![(Some([16.0, 400.0, 208.0, 56.0]), true)];
    let o = overlay("whole-row-class.css", css, &frame, &lib_row(0));
    pd::test_overlay(Some((*o).clone()));
    let look = pd::anchor_look(&lib_row(0));
    pd::test_overlay(None);
    let padding = look.and_then(|l| l.padding).expect("the row's padding");
    assert_eq!(padding[2], 6.0, "{padding:?}");
}
