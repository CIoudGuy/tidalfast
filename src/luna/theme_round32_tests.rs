//! Round 32: TIDAL 2.46.0's classes and token scopes for the queue's
//! artist line, the sidebar's playlist rows and the context menu's words,
//! and the native parts painted with their anchor's whole look. Proved with
//! made-up themes (their own colours, TIDAL's classes and anchors only).

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

/// The overlay `css` gives the sample tree, once `key` is styled.
fn overlay_for(name: &str, css: &str, key: &str) -> Arc<DomOverlay> {
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    let luna = themed_luna(name, css);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(key));
    luna.shutdown();
    o
}

/// A dark theme's rule for TIDAL's market text colours the queue's artist
/// names: TIDAL 2.46.0 draws them near white under Blur's
/// `[class*="_marketText_"] { color: #ffffffee !important }` (captured),
/// where Tidalfast kept its grey for every dark window. A catch-all
/// `* { color }` still leaves them grey, as TIDAL's own rule for the line
/// does in TIDAL (Meteor Night).
#[test]
fn a_dark_themes_market_text_rule_colours_the_queue_artists() {
    use crate::theme::Palette;
    use crate::ui::now_playing::queue_name_colors;
    use crate::ui::plugin_dom::{self as pd, SidePanel, themed, tidal_panel_text};
    let names = |name: &str, css: &str| {
        let o = overlay_for(name, css, dom_anchors::keys::QUEUE_ROLE_TEXT);
        pd::test_overlay(Some((*o).clone()));
        let queue = tidal_panel_text(themed(&Palette::dark(), &o), SidePanel::Queue);
        let colors = queue_name_colors(&queue);
        pd::test_overlay(None);
        colors
    };
    let pale = |c: egui::Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        r >= 248 && g >= 248 && b >= 250 && (230..=240).contains(&a)
    };
    let (plain, followed) = names(
        "frosted-words.css",
        r#"/* {"name": "Frosted words"} */
body { background-color: rgb(14, 12, 24) !important; }
[class*="_marketText_"] { color: rgba(250, 250, 255, .933) !important; }
"#,
    );
    assert!(pale(plain), "near white: {plain:?}");
    assert_eq!(followed, plain);
    let native = queue_name_colors(&Palette::dark());
    let (plain, _) = names(
        "snowdrift.css",
        r#"/* {"name": "Snowdrift"} */
body { background-color: rgb(14, 12, 24) !important; }
* { color: rgb(240, 240, 250); }
"#,
    );
    assert_eq!(plain, native.0, "a catch-all leaves TIDAL's grey");
}

/// TIDAL 2.46.0's playlist names in the sidebar carry no `_textWhite_`
/// class (TIDAL 2.43.2's did), and their second lines carry
/// `_subtitle_c0fd197` (captured under five themes): a rule naming the
/// first leaves the names alone in TIDAL, one naming the second colours
/// the lines. Tidalfast kept 2.43's classes.
#[test]
fn sidebar_playlist_rows_have_tidal_2_46s_classes() {
    use crate::ui::plugin_dom as pd;
    let css = r#"/* {"name": "Old white"} */
._marketText_1lyag_1._textWhite_1lyag_319 { color: rgb(9, 0, 26); font-weight: 600; }
._subtitle_c0fd197._text20_1lyag_291 { color: rgb(120, 40, 160); }
"#;
    let mut frame = dom_anchors::sample_frame();
    let row = Some([8.0, 300.0, 224.0, 52.0]);
    frame.sidebar.rows = vec![(row, false)];
    let tree = dom_anchors::build(&frame);
    let luna = themed_luna("old-white.css", css);
    let line = dom_anchors::keys::lib_row_subtitle(0);
    let o = wait(&luna, &tree, &|o| o.hosts.contains_key(&line));
    luna.shutdown();
    pd::test_overlay(Some((*o).clone()));
    let r = egui::Rect::from_min_size(egui::pos2(8.0, 300.0), egui::vec2(224.0, 52.0));
    let style = pd::library_row(r, false, false);
    pd::test_overlay(None);
    let style = style.expect("the line's colour");
    assert_eq!(style.title_color, None, "no _textWhite_ on the name");
    assert_eq!(style.title_weight, None);
    assert_eq!(style.secondary, Some(egui::Color32::from_rgb(120, 40, 160)));
}

/// A theme that sets TIDAL's contrast token on the context menu alone
/// (Cotton Candy Nights: `._contextMenu_ { --wave-color-solid-contrast-fill:
/// #000 }` with its root's white, and `._actionTextInner_ { color: black }`
/// nested in it) has TIDAL 2.46.0 draw the menu's title, a submenu's
/// heading and the rows that open submenus in those dark colours (captured).
/// Tidalfast drew the title and those rows in the page's pale words, faint
/// on the theme's pink menu.
#[test]
fn a_menus_own_contrast_token_colours_its_title_and_submenu_rows() {
    use crate::theme::Palette;
    use crate::ui::{plugin_dom as pd, widgets};
    let css = r#"/* {"name": "Candy menu"} */
:root { --wave-color-solid-contrast-fill: rgb(250, 250, 250); }
._contextMenu_8606b2d {
  background-color: rgb(240, 170, 230);
  --wave-color-solid-contrast-fill: rgb(10, 0, 30) !important;
  ._actionTextInner_a14f37c { color: rgb(20, 0, 40) !important; }
}
"#;
    let o = overlay_for("candy-menu.css", css, dom_anchors::keys::THEME_MENU_TITLE);
    pd::test_overlay(Some((*o).clone()));
    let palette = Palette::dark();
    let title = pd::menu_words(palette.menu_bg());
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        widgets::menu_frame(&palette).show(ui, |ui| {
            ui.set_width(widgets::MENU_MIN_CONTENT);
            widgets::menu_submenu(ui, &palette, None, "Add to playlist", |_| {});
        });
    });
    out.textures_delta.clear();
    pd::test_overlay(None);
    assert_eq!(title, Some(egui::Color32::from_rgb(10, 0, 30)));
    let row = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Add to playlist" => {
                Some(t.galley.job.sections[0].format.color)
            }
            _ => None,
        })
        .expect("the row's words");
    assert_eq!(row, egui::Color32::from_rgb(20, 0, 40));
}

/// With no theme the menu's words keep the app's own colours.
#[test]
fn without_a_theme_the_menus_words_are_the_apps() {
    use crate::theme::Palette;
    use crate::ui::plugin_dom as pd;
    pd::test_overlay(None);
    let palette = Palette::dark();
    assert_eq!(pd::menu_words(palette.menu_bg()), None);
    assert_eq!(pd::menu_row_words(palette.menu_bg()), None);
}
