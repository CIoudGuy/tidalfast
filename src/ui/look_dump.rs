//! A report of every anchor's resolved look, for comparing a theme in
//! Tidalfast with the same theme on TIDAL's page in a browser
//! (`--demo-dump-looks`). Development builds only: nothing here is drawn.
//!
//! For each node of the anchor tree last sent to the runtime: its key,
//! parent, tag, attributes and box (to find the same element in TIDAL's
//! page), the cascade's resolved style for it (the overlay's values, as
//! written), and the [`AnchorLook`] native parts paint from, with colours as
//! unmultiplied `[r, g, b, a]` and lengths in points.

use egui::Color32;
use serde_json::{Map, Value, json};

use super::anchor_look::{AnchorLook, look_of};
use super::{on, with};
use crate::luna::dom::HostTree;

/// The report for the tree last sent to the runtime, or `None` before one
/// was sent.
pub fn dump_looks() -> Option<Value> {
    let tree = with(|s| s.sent.clone())?;
    Some(looks_report(&tree))
}

/// The report for `tree` under the overlay in place now.
pub fn looks_report(tree: &HostTree) -> Value {
    let luna = on();
    let nodes: Vec<Value> = tree
        .nodes
        .iter()
        .map(|n| {
            let mut o = Map::new();
            o.insert("key".into(), json!(n.key));
            o.insert("parent".into(), json!(n.parent));
            o.insert("tag".into(), json!(n.tag));
            o.insert(
                "attrs".into(),
                Value::Object(n.attrs.iter().map(|(k, v)| (k.clone(), json!(v))).collect()),
            );
            if let Some(r) = &n.rect {
                o.insert("rect".into(), json!(r));
            }
            if luna {
                with(|s| {
                    let Some(h) = s.overlay.hosts.get(&n.key) else {
                        return;
                    };
                    if !h.classes.is_empty() {
                        o.insert("classes_added".into(), json!(h.classes));
                    }
                    if !h.removed.is_empty() {
                        o.insert("classes_removed".into(), json!(h.removed));
                    }
                    if !h.style.is_empty() {
                        o.insert("style".into(), set_only(&h.style));
                    }
                    if let Some(b) = &h.before {
                        o.insert("before".into(), set_only(b));
                    }
                    if let Some(a) = &h.after {
                        o.insert("after".into(), set_only(a));
                    }
                    if h.hidden() {
                        o.insert("hidden".into(), json!(true));
                    }
                    if let Some(look) = look_of(h, &s.vars, s.frame.viewport) {
                        o.insert("look".into(), look_json(&look));
                    }
                });
            }
            Value::Object(o)
        })
        .collect();
    json!({
        "luna": luna,
        "path": tree.path,
        "viewport": tree.viewport,
        "light": tree.light,
        "nodes": nodes,
    })
}

/// A style's set properties only.
fn set_only(style: &crate::luna::dom::Style) -> Value {
    match json!(style) {
        Value::Object(o) => Value::Object(o.into_iter().filter(|(_, v)| !v.is_null()).collect()),
        other => other,
    }
}

fn rgba(c: Color32) -> Value {
    json!(c.to_srgba_unmultiplied())
}

/// The look's set fields, by the CSS property each comes from.
fn look_json(l: &AnchorLook) -> Value {
    let mut o = Map::new();
    if let Some(c) = l.fill {
        o.insert("background-color".into(), rgba(c));
    }
    if l.layers {
        o.insert("layers".into(), json!(true));
    }
    if let Some(c) = l.corners {
        // CSS order: top-left, top-right, bottom-right, bottom-left.
        o.insert("border-radius".into(), json!([c.nw, c.ne, c.se, c.sw]));
    }
    if let Some((widths, c)) = l.sides {
        o.insert("border-width".into(), json!(widths));
        o.insert("border-color".into(), rgba(c));
    }
    if let Some(s) = l.shadow {
        o.insert(
            "box-shadow".into(),
            json!({ "offset": s.offset, "blur": s.blur, "spread": s.spread, "color": rgba(s.color) }),
        );
    }
    if let Some((c, blur, spread)) = l.glow {
        o.insert(
            "box-shadow-inset".into(),
            json!({ "blur": blur, "spread": spread, "color": rgba(c) }),
        );
    }
    if let Some(v) = l.opacity {
        o.insert("opacity".into(), json!(v));
    }
    if let Some(c) = l.text {
        o.insert("color".into(), rgba(c));
    }
    if l.text_inherits {
        o.insert("color-inherits".into(), json!(true));
    }
    if let Some(c) = l.icon {
        o.insert("fill".into(), rgba(c));
    }
    if let Some(v) = l.font_size {
        o.insert("font-size".into(), json!(v));
    }
    if let Some(v) = l.font_weight {
        o.insert("font-weight".into(), json!(v));
    }
    if let Some(v) = l.uppercase {
        o.insert("text-transform".into(), json!(v));
    }
    if let Some(p) = l.padding {
        o.insert("padding".into(), json!(p));
    }
    if let Some(v) = l.scale {
        o.insert("scale".into(), json!(v));
    }
    if l.translate != egui::Vec2::ZERO {
        o.insert("translate".into(), json!([l.translate.x, l.translate.y]));
    }
    Value::Object(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::luna::dom::{DomOverlay, HostOverride, Style};
    use crate::luna::dom_anchors::{self, keys};

    /// Each anchor comes out with what finds it in TIDAL's page (tag and
    /// attributes) and with the look a theme gives it, resolved; an anchor
    /// no rule reaches has no look.
    #[test]
    fn the_report_holds_each_anchors_markup_and_resolved_look() {
        let tree = dom_anchors::build(&dom_anchors::sample_frame());
        let mut overlay = DomOverlay::default();
        overlay.vars.insert("--pill".into(), "#102030".into());
        overlay.hosts.insert(
            keys::FOOTER.into(),
            HostOverride {
                style: Style {
                    background_color: Some("var(--pill)".into()),
                    border_radius: Some("1rem".into()),
                    color: Some("rgb(250, 240, 230)".into()),
                    font_weight: Some("700".into()),
                    ..Style::default()
                },
                ..HostOverride::default()
            },
        );
        super::super::test_overlay(Some(overlay));
        let report = looks_report(&tree);
        super::super::test_overlay(None);

        assert_eq!(report["luna"], json!(true));
        let nodes = report["nodes"].as_array().expect("nodes");
        let node = |key: &str| {
            nodes
                .iter()
                .find(|n| n["key"] == json!(key))
                .unwrap_or_else(|| panic!("no node {key}"))
        };
        let footer = node(keys::FOOTER);
        assert_eq!(footer["attrs"]["id"], json!("footerPlayer"));
        let look = &footer["look"];
        assert_eq!(look["background-color"], json!([16, 32, 48, 255]));
        assert_eq!(look["border-radius"], json!([14, 14, 14, 14]));
        assert_eq!(look["color"], json!([250, 240, 230, 255]));
        assert_eq!(look["font-weight"], json!(700));
        assert_eq!(footer["style"]["background-color"], json!("var(--pill)"));
        assert!(footer["style"].get("color").is_some());
        assert!(
            footer["style"].get("padding").is_none(),
            "only the properties a rule sets"
        );
        assert!(node(keys::PLAY).get("look").is_none());
    }
}
