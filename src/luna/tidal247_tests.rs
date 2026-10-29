//! TIDAL 2.47.0: what changed in TIDAL's page since 2.46.0 that themes and
//! plugins see. Proved with made-up themes and plugins (their own values,
//! TIDAL's classes and anchors only).

use serde_json::json;

use super::dom_anchors::{self, keys};
use super::dom_tests::{finish, results_until, start};
use super::theme_round38_tests::{overlay, page_frame};

/// TIDAL 2.47.0 puts an album header's pills and captioned buttons in a box
/// of their own: `div[data-test=header-controls]._actionContainer_5bb1c14 >
/// div._primaryActions_097c7f3 > div._container_84abbc3`, `>
/// div._smallButtons_5c1ea50` (2.46.0 had both straight in the controls).
/// A theme's child rules through the new box reach them, and a rule for
/// 2.46.0's direct children matches nothing, as in TIDAL now. A playlist's
/// header has no such box.
#[test]
fn an_albums_header_actions_sit_in_their_own_box() {
    use crate::ui::plugin_dom as pd;
    use keys::{PAGE_HEADER_CONTROLS, PAGE_PLAY, PAGE_SHUFFLE};
    let css = r#"/* {"name": "Album actions"} */
._actionContainer_5bb1c14 > ._primaryActions_097c7f3 > ._container_84abbc3 > [data-test="play-all"] { color: rgb(1, 2, 3); }
[data-test="header-controls"] > ._container_84abbc3 > [data-test="shuffle-all"] { color: rgb(4, 5, 6); }
._primaryActions_097c7f3 > ._smallButtons_5c1ea50 [data-wave-color="textDefault"] { color: rgb(7, 8, 9) !important; }
._actionContainer_334aa3b > ._container_5a08754 > [data-test="shuffle-all"] { color: rgb(10, 11, 12); }
[data-test="header-controls"] { opacity: 0.99; }
"#;
    let label = keys::page_action_label("share-button");
    let looks = |page: &str| {
        let o = overlay(
            &format!("album-actions-{page}.css"),
            css,
            &page_frame(page),
            PAGE_HEADER_CONTROLS,
        );
        pd::test_overlay(Some((*o).clone()));
        let text = |key: &str| pd::anchor_look(key).and_then(|l| l.text);
        let found = (text(PAGE_PLAY), text(PAGE_SHUFFLE), text(&label));
        pd::test_overlay(None);
        found
    };
    let rgb = egui::Color32::from_rgb;
    assert_eq!(
        looks("album"),
        (Some(rgb(1, 2, 3)), None, Some(rgb(7, 8, 9)))
    );
    assert_eq!(looks("playlist"), (None, Some(rgb(10, 11, 12)), None));
    let tree = dom_anchors::build(&page_frame("album"));
    let parent = |key: &str| {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.parent.clone())
    };
    assert_eq!(
        parent(keys::PAGE_PRIMARY_ACTIONS).as_deref(),
        Some(PAGE_HEADER_CONTROLS)
    );
    assert_eq!(
        parent(keys::PAGE_SMALL_BUTTONS).as_deref(),
        Some(keys::PAGE_PRIMARY_ACTIONS)
    );
}

/// The new box is a flex box in TIDAL 2.47.0's sheet (`._primaryActions_
/// 097c7f3 { display: flex }`): a plugin reading it gets `flex`, as in
/// TIDAL, not the `block` of a class TIDAL's sheet does not know.
#[test]
fn an_albums_primary_actions_compute_flex() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[data-test="header-controls"] > ._primaryActions_097c7f3', (el) => {
	if (R.found) return;
	R.found = [getComputedStyle(el).display, el.querySelector('[data-test="play-all"]') !== null];
});
"##,
    );
    luna.sync_dom(dom_anchors::build(&page_frame("album")));
    let r = results_until(&luna, &dir, 20, |r| !r["found"].is_null());
    assert_eq!(r["found"], json!(["flex", true]), "{r}");
    finish(luna, dir);
}
