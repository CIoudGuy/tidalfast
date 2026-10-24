//! A theme's preview: a miniature of the app as the theme paints it.
//!
//! The theme's CSS is read the way the DOM bridge applies it, approximated
//! without a browser: its rules (nesting, `@media (prefers-color-scheme)`
//! for the app's Dark or Light, `@supports`, `@layer`) are matched by their
//! last compound selector against the elements the bridge paints (the page,
//! main container, sidebar, `#main`, the player bar, TIDAL's text roles),
//! the cascade is source order with `!important` first, and the result
//! goes through the bridge's own code: the themed palette, the hosts' fills,
//! the page's gradients and pictures, and the floating bar and sidebar
//! card. Resolved off the UI thread, once per theme CSS, and kept with the
//! theme; painting it is a handful of shapes.

use std::collections::BTreeMap;

use egui::{Color32, CornerRadius, Rect, Stroke, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::luna::dom::{DomOverlay, HostOverride, Style};
use crate::luna::dom_anchors::keys;
use crate::theme::Palette;

use super::plugin_dom::{self, PageLayer};
use super::theme_layout;

/// What a preview paints, for one app theme (Dark or Light).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemePreview {
    pub dark: bool,
    pub window: Color32,
    /// The page's colour and picture layers (gradients, `url()`), bottom
    /// first, over the window.
    pub layers: Vec<PageLayer>,
    pub sidebar: Color32,
    pub main: Color32,
    pub bar: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub accent: Color32,
    pub hairline: Color32,
    /// A floating player bar: side and bottom gap, corner radius, in
    /// points of a 1400 x 900 window.
    pub bar_float: Option<[f32; 3]>,
    /// A sidebar card: top and bottom gap, corner radius.
    pub side_float: Option<[f32; 2]>,
    /// Whether any of the theme's rules apply in this app theme: false
    /// when every rule sits under the other `prefers-color-scheme` (the
    /// adaptive Cotton Candy files, Tidal Light), and in previews saved
    /// before this was recorded.
    #[serde(default)]
    pub styled: bool,
}

/// The preview in Dark and in Light.
pub type Previews = [ThemePreview; 2];

/// Both previews of a theme's CSS.
pub fn resolve_both(css: &str) -> Previews {
    [resolve(css, false), resolve(css, true)]
}

/// The preview in the app's Dark (`light` false) or Light.
pub fn resolve(css: &str, light: bool) -> ThemePreview {
    let (o, styled) = overlay(css, light);
    let base = if light {
        Palette::light()
    } else {
        Palette::dark()
    };
    let p = plugin_dom::themed(&base, &o);
    let mut vars = plugin_dom::wave_tokens(&p);
    vars.extend(o.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
    let host = |k: &str| o.hosts.get(k);
    let fill = |k: &str| host(k).and_then(|h| plugin_dom::host_fill(h, &vars));
    let (page_fill, mut layers) = plugin_dom::page_layers(&o, &vars);
    let mut window = page_fill.unwrap_or(p.window);
    if let Some(c) = fill(keys::MAIN_CONTAINER) {
        window = over(c, window);
    }
    // The main container's own gradients over the page's.
    if let Some(h) = host(keys::MAIN_CONTAINER) {
        layers.extend(
            plugin_dom::box_layers(h, &vars, VIEW)
                .into_iter()
                .filter_map(|l| match l {
                    plugin_dom::BoxLayer::Paint(l) => Some(l),
                    plugin_dom::BoxLayer::Window {
                        layer,
                        scale,
                        start,
                    } => Some(plugin_dom::window_at(&layer, scale, start)),
                    _ => None,
                }),
        );
    }
    let painted = !layers.is_empty();
    // As `plugin_dom::surface`: the sidebar and page show a painted page.
    let surface = |k: &str, native: Color32| match fill(k) {
        Some(c) => c,
        None if painted => native.gamma_multiply(0.35),
        None => native,
    };
    let bar_native = if p.tidal_dark() {
        Color32::from_rgb(30, 30, 30)
    } else {
        p.panel
    };
    let layout = theme_layout::resolve(
        host(keys::FOOTER),
        host(keys::SIDEBAR),
        host(keys::HEADER),
        host(keys::MAIN),
        &vars,
        VIEW,
    );
    ThemePreview {
        dark: p.dark,
        window,
        layers,
        sidebar: surface(keys::SIDEBAR, p.window),
        main: surface(keys::MAIN, p.window),
        bar: fill(keys::FOOTER).unwrap_or(bar_native),
        text: p.text,
        secondary: p.secondary,
        accent: p.accent,
        hairline: p.hairline(),
        bar_float: layout.player_bar.map(|f| {
            [
                f32::from(f.gaps.left),
                f32::from(f.gaps.bottom),
                f32::from(f.radius.nw.max(f.radius.ne)),
            ]
        }),
        side_float: layout.sidebar.map(|f| {
            [
                f32::from(f.gaps.top),
                f32::from(f.radius.ne.max(f.radius.se)),
            ]
        }),
        styled,
    }
}

/// The app theme a theme shows in when it shows in only one: `Some(true)`
/// for a theme whose rules all apply only in Light (`@media
/// (prefers-color-scheme: light)`), `Some(false)` for Dark only, `None`
/// when it shows in both (or in neither, or the previews do not say).
pub fn only_in(previews: &Previews) -> Option<bool> {
    match (previews[0].styled, previews[1].styled) {
        (false, true) => Some(true),
        (true, false) => Some(false),
        _ => None,
    }
}

/// The window the theme's lengths are read in.
const VIEW: [f32; 2] = [1400.0, 900.0];

fn over(c: Color32, under: Color32) -> Color32 {
    let a = f32::from(c.a()) / 255.0;
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    let [ur, ug, ub, _] = under.to_srgba_unmultiplied();
    let mix = |x: u8, y: u8| (f32::from(y) + (f32::from(x) - f32::from(y)) * a).round() as u8;
    Color32::from_rgb(mix(r, ur), mix(g, ug), mix(b, ub))
}

// ------------------------------------------------------------------ CSS

/// Text without `/* comments */`.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(a) = rest.find("/*") {
        out.push_str(&rest[..a]);
        match rest[a + 2..].find("*/") {
            Some(b) => rest = &rest[a + 2 + b + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Index of the `}` closing the block opened just before `from`.
fn block_end(s: &[u8], from: usize) -> usize {
    let (mut depth, mut i, mut quote) = (1usize, from, 0u8);
    while i < s.len() {
        let c = s[i];
        if quote != 0 {
            if c == b'\\' {
                i += 1;
            } else if c == quote {
                quote = 0;
            }
        } else {
            match c {
                b'"' | b'\'' => quote = c,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return i;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    s.len()
}

/// Whether an `@media` condition holds in the app's mode.
fn media_holds(cond: &str, light: bool) -> bool {
    let c = cond.to_ascii_lowercase().replace(' ', "");
    if c.contains("prefers-color-scheme:light") {
        return light;
    }
    if c.contains("prefers-color-scheme:dark") {
        return !light;
    }
    !c.starts_with("print")
}

/// Every style rule as (selector list, declarations), in source order.
fn rules(css: &str, light: bool, parent: Option<&str>, out: &mut Vec<(String, String)>) {
    let b = css.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let Some(open) = css[i..].find(['{', ';', '}']).map(|p| p + i) else {
            break;
        };
        if b[open] != b'{' {
            // A statement (`@import ...;`), a declaration of the parent
            // (handled by the caller) or a stray brace.
            i = open + 1;
            continue;
        }
        let prelude = css[i..open].trim();
        let end = block_end(b, open + 1);
        let body = &css[open + 1..end.min(css.len())];
        i = end + 1;
        if let Some(at) = prelude.strip_prefix('@') {
            let (name, cond) = at
                .split_once(|c: char| c.is_whitespace())
                .unwrap_or((at, ""));
            match name.to_ascii_lowercase().as_str() {
                "media" if media_holds(cond, light) => rules(body, light, parent, out),
                "supports" if !cond.trim_start().starts_with("not") => {
                    rules(body, light, parent, out)
                }
                "layer" | "scope" | "document" | "-moz-document" | "container" => {
                    rules(body, light, parent, out)
                }
                _ => {}
            }
            continue;
        }
        let selector = match parent {
            None => prelude.to_string(),
            Some(parent) => prelude
                .split(',')
                .flat_map(|child| {
                    let child = child.trim();
                    parent.split(',').map(move |p| {
                        let p = p.trim();
                        if child.contains('&') {
                            child.replace('&', p)
                        } else {
                            format!("{p} {child}")
                        }
                    })
                })
                .collect::<Vec<_>>()
                .join(", "),
        };
        // The body's own declarations: what is outside nested blocks.
        let mut decls = String::new();
        let bb = body.as_bytes();
        let mut k = 0;
        let mut start = 0;
        while k < bb.len() {
            match bb[k] {
                b'{' => {
                    // A nested rule: its prelude starts after the last `;`.
                    let pre_start = body[..k]
                        .rfind([';', '}'])
                        .map_or(start, |p| p + 1)
                        .max(start);
                    decls.push_str(&body[start..pre_start]);
                    let e = block_end(bb, k + 1);
                    let nested = &body[pre_start..(e + 1).min(body.len())];
                    rules(nested, light, Some(&selector), out);
                    k = e + 1;
                    start = k;
                    continue;
                }
                b'"' | b'\'' => {
                    let q = bb[k];
                    k += 1;
                    while k < bb.len() && bb[k] != q {
                        k += 1;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        decls.push_str(&body[start.min(body.len())..]);
        if !decls.trim().is_empty() {
            out.push((selector, decls));
        }
    }
}

/// `prop: value` pairs, with whether each is `!important`.
fn declarations(body: &str) -> Vec<(String, String, bool)> {
    super::plugin_slots::css::split_top(body, ';')
        .into_iter()
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            let k = k.trim();
            if k.is_empty() || k.contains(['{', '}', ' ']) {
                return None;
            }
            let v = v.trim();
            let important = v.to_ascii_lowercase().ends_with("important") && v.contains('!');
            let v = match v.rfind('!') {
                Some(p) if important => v[..p].trim(),
                _ => v,
            };
            let k = if k.starts_with("--") {
                k.to_string()
            } else {
                k.to_ascii_lowercase()
            };
            Some((k, v.to_string(), important))
        })
        .collect()
}

/// What a selector's last compound names, among the elements the preview
/// paints.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Root,
    Container,
    Sidebar,
    Main,
    Footer,
    Text,
    Secondary,
    Menu,
}

fn targets(selector: &str) -> Vec<Target> {
    let s = selector.trim().to_ascii_lowercase();
    // States and boxes TIDAL draws only sometimes.
    if s.contains(":hover")
        || s.contains(":active")
        || s.contains(":focus")
        || s.contains("::")
        || s.contains(":before")
        || s.contains(":after")
    {
        return Vec::new();
    }
    // The last compound: after the last combinator outside brackets.
    let mut depth = 0i32;
    let mut cut = 0;
    for (i, c) in s.char_indices() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ' ' | '>' | '+' | '~' if depth == 0 => cut = i + 1,
            _ => {}
        }
    }
    let last = s[cut..].trim();
    let has_id = |id: &str| {
        last.match_indices(id).any(|(i, _)| {
            !last[i + id.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
    };
    let mut out = Vec::new();
    if matches!(
        last,
        ":root" | "html" | "body" | "#wimp" | "*" | ":root:root"
    ) || last.starts_with(":root")
        || has_id("#wimp")
    {
        out.push(Target::Root);
    }
    if last.contains("_maincontainer") || last.contains("maincontainer") {
        out.push(Target::Container);
    }
    if has_id("#sidebar")
        || last.contains("_sidebarwrapper")
        || last.contains("main-layout-sidebar-wrapper")
        || last.contains("navigation sidebar")
    {
        out.push(Target::Sidebar);
    }
    if has_id("#main")
        || last.contains("[id^=\"main\"]")
        || last.contains("[id^=main]")
        || last.contains("._main_")
        || last.contains("[class^=\"_main_")
        || last.contains("data-test=\"main\"]")
        || last == "main"
    {
        out.push(Target::Main);
    }
    if has_id("#footerplayer") || last.contains("footer-player") || last.contains("_player_") {
        out.push(Target::Footer);
    }
    if last.contains("textdefault") {
        out.push(Target::Text);
    }
    if last.contains("textsecondary") {
        out.push(Target::Secondary);
    }
    if last.contains("_contextmenu") || last.contains("contextmenu") {
        out.push(Target::Menu);
    }
    out
}

/// One element's declarations in cascade order.
#[derive(Default)]
struct Cascade {
    normal: Map<String, Value>,
    important: Map<String, Value>,
}

impl Cascade {
    fn add(&mut self, k: &str, v: &str, important: bool) {
        // `background` sets the colour or the picture.
        let entries: Vec<(&str, &str)> = if k == "background" {
            let lower = v.to_ascii_lowercase();
            if lower.contains("gradient(") || lower.contains("url(") {
                vec![("background-image", v)]
            } else if lower.trim() == "none" || lower.trim() == "transparent" {
                vec![
                    ("background-color", "transparent"),
                    ("background-image", "none"),
                ]
            } else {
                vec![("background-color", v)]
            }
        } else {
            vec![(k, v)]
        };
        for (k, v) in entries {
            let map = if important {
                &mut self.important
            } else {
                &mut self.normal
            };
            map.insert(k.to_string(), json!(v));
        }
    }

    fn vars(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.normal
            .iter()
            .chain(self.important.iter())
            .filter(|(k, _)| k.starts_with("--"))
    }

    fn style(&self) -> Style {
        let mut m = self.normal.clone();
        m.extend(self.important.clone());
        m.retain(|k, _| !k.starts_with("--"));
        serde_json::from_value(Value::Object(m)).unwrap_or_default()
    }
}

/// The overlay the bridge would get from the theme, for the elements the
/// preview paints.
/// What the theme paints on the previewed elements, and whether any of its
/// rules apply in this app theme at all.
fn overlay(css: &str, light: bool) -> (DomOverlay, bool) {
    let mut list = Vec::new();
    rules(&strip_comments(css), light, None, &mut list);
    let styled = !list.is_empty();
    let mut by: BTreeMap<u8, Cascade> = BTreeMap::new();
    for (selector, body) in &list {
        let mut hit: Vec<Target> = selector.split(',').flat_map(targets).collect();
        hit.dedup();
        if hit.is_empty() {
            continue;
        }
        let decls = declarations(body);
        for t in hit {
            let c = by.entry(t as u8).or_default();
            for (k, v, important) in &decls {
                c.add(k, v, *important);
            }
        }
    }
    let mut o = DomOverlay {
        version: 1,
        ..DomOverlay::default()
    };
    if let Some(root) = by.get(&(Target::Root as u8)) {
        for (k, v) in root.vars() {
            if let Some(v) = v.as_str() {
                o.vars.insert(k.clone(), v.to_string());
            }
        }
        o.page = root.style();
    }
    for (t, key) in [
        (Target::Container, keys::MAIN_CONTAINER),
        (Target::Sidebar, keys::SIDEBAR),
        (Target::Main, keys::MAIN),
        (Target::Footer, keys::FOOTER),
        (Target::Text, keys::THEME_TEXT),
        (Target::Secondary, keys::THEME_SECONDARY),
        (Target::Menu, keys::THEME_MENU),
    ] {
        if let Some(c) = by.get(&(t as u8)) {
            o.hosts.insert(
                key.to_string(),
                HostOverride {
                    style: c.style(),
                    ..HostOverride::default()
                },
            );
        }
    }
    (o, styled)
}

// ---------------------------------------------------------------- painting

/// A miniature of the app in `rect`: the page (with the theme's gradients
/// and picture), the sidebar, a heading and rows with the playing one in
/// the accent, and the player bar with its progress; floating where the
/// theme floats them. `card` is the colour round the preview, for its
/// rounded corners.
pub fn paint(ui: &egui::Ui, rect: Rect, preview: &ThemePreview, card: Color32) {
    let rect = Rect::from_min_max(rect.min.round(), rect.max.round());
    let radius = 8.0;
    // A Ui of its own, clipped to the miniature, on the caller's layer.
    let mut child = egui::Ui::new(
        ui.ctx().clone(),
        ui.id()
            .with(("theme-preview", rect.min.x as i32, rect.min.y as i32)),
        egui::UiBuilder::new()
            .layer_id(ui.layer_id())
            .max_rect(rect),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let p = child.painter().clone();
    let (w, h) = (rect.width(), rect.height());
    // Lengths of a 1400-point window, at the miniature's scale (doubled so
    // a 14-point gap still shows).
    let s = (w / 700.0).max(0.1);
    p.rect_filled(rect, 0.0, preview.window);
    for layer in &preview.layers {
        plugin_dom::paint_layer(&child, rect, layer);
    }
    // The player bar.
    let bar_h = (h * 0.2).round().max(10.0);
    let (bar, bar_r) = match preview.bar_float {
        Some([side, bottom, r]) => {
            let side = (side * s).max(2.0).round();
            let bottom = (bottom * s).max(2.0).round();
            (
                Rect::from_min_max(
                    pos2(rect.left() + side, rect.bottom() - bottom - bar_h),
                    pos2(rect.right() - side, rect.bottom() - bottom),
                ),
                (r * s).clamp(2.0, bar_h / 2.0),
            )
        }
        None => (
            Rect::from_min_max(pos2(rect.left(), rect.bottom() - bar_h), rect.max),
            0.0,
        ),
    };
    let above = if preview.bar_float.is_some() {
        bar.top() - (rect.bottom() - bar.bottom())
    } else {
        bar.top()
    };
    // The sidebar.
    let side_w = (w * 0.24).round();
    let (side, side_r) = match preview.side_float {
        Some([top, r]) => {
            let gap = (top * s).max(2.0).round();
            (
                Rect::from_min_max(
                    pos2(rect.left(), rect.top() + gap),
                    pos2(rect.left() + side_w, above - gap),
                ),
                (r * s).clamp(2.0, 10.0),
            )
        }
        None => (
            Rect::from_min_max(rect.min, pos2(rect.left() + side_w, above)),
            0.0,
        ),
    };
    let main = Rect::from_min_max(pos2(side.right(), rect.top()), pos2(rect.right(), above));
    p.rect_filled(main, 0.0, preview.main);
    p.rect_filled(
        side,
        CornerRadius {
            nw: 0,
            sw: 0,
            ne: side_r as u8,
            se: side_r as u8,
        },
        preview.sidebar,
    );
    if preview.side_float.is_none() {
        p.vline(
            side.right() - 0.5,
            side.y_range(),
            Stroke::new(1.0, preview.hairline),
        );
    }
    let line = |x: f32, y: f32, len: f32, c: Color32, t: f32| {
        p.rect_filled(
            Rect::from_min_size(pos2(x, y).round(), vec2(len.max(2.0), t).round()),
            t / 2.0,
            c,
        );
    };
    // Sidebar entries: the selected one in the accent.
    let unit = (h / 14.0).clamp(3.0, 8.0);
    for k in 0..4 {
        let y = side.top() + unit * (1.6 + k as f32 * 1.5);
        if y + unit * 0.6 > side.bottom() - 2.0 {
            break;
        }
        let c = if k == 1 {
            preview.accent
        } else {
            preview.text.gamma_multiply(0.55)
        };
        line(
            side.left() + unit,
            y,
            side_w - unit * (2.5 + k as f32 * 0.5),
            c,
            unit * 0.55,
        );
    }
    // The page: heading, a subtitle, rows with the playing one lit.
    let x = main.left() + unit * 1.6;
    let mw = main.right() - x - unit * 1.6;
    line(
        x,
        main.top() + unit * 1.4,
        mw * 0.42,
        preview.text,
        unit * 0.9,
    );
    line(
        x,
        main.top() + unit * 2.8,
        mw * 0.25,
        preview.secondary,
        unit * 0.55,
    );
    for k in 0..4 {
        let y = main.top() + unit * (4.3 + k as f32 * 1.3);
        if y + unit > above - 2.0 {
            break;
        }
        let (c, c2) = if k == 0 {
            (preview.accent, preview.accent.gamma_multiply(0.6))
        } else {
            (
                preview.text.gamma_multiply(0.8),
                preview.secondary.gamma_multiply(0.8),
            )
        };
        line(x, y, mw * 0.36, c, unit * 0.5);
        line(x + mw * 0.55, y, mw * 0.25, c2, unit * 0.5);
    }
    // The bar: cover, title, progress in the accent, the play button.
    if let Some(sh) = preview.bar_float.map(|_| bar) {
        p.add(
            egui::Shadow {
                offset: [0, 1],
                blur: 6,
                spread: 0,
                color: Color32::from_black_alpha(50),
            }
            .as_shape(sh, bar_r),
        );
    }
    p.rect_filled(bar, bar_r, preview.bar);
    if preview.bar_float.is_none() {
        p.hline(
            bar.x_range(),
            bar.top() + 0.5,
            Stroke::new(1.0, preview.hairline),
        );
    }
    let cy = bar.center().y;
    let cover = (bar.height() * 0.56).round();
    let cover_rect = Rect::from_min_size(
        pos2(bar.left() + unit * 1.2, cy - cover / 2.0),
        vec2(cover, cover),
    );
    p.rect_filled(cover_rect, 2.0, preview.secondary.gamma_multiply(0.5));
    line(
        cover_rect.right() + unit * 0.6,
        cy - unit * 0.5,
        w * 0.12,
        preview.text,
        unit * 0.45,
    );
    line(
        cover_rect.right() + unit * 0.6,
        cy + unit * 0.25,
        w * 0.08,
        preview.secondary,
        unit * 0.4,
    );
    let track = Rect::from_center_size(
        pos2(bar.center().x + w * 0.04, cy + bar.height() * 0.22),
        vec2(w * 0.3, 2.0_f32.max(unit * 0.3)),
    );
    p.rect_filled(track, 1.0, preview.secondary.gamma_multiply(0.35));
    p.rect_filled(
        Rect::from_min_max(
            track.min,
            pos2(track.left() + track.width() * 0.45, track.bottom()),
        ),
        1.0,
        preview.accent,
    );
    p.circle_filled(
        pos2(bar.center().x + w * 0.04, cy - bar.height() * 0.12),
        (bar.height() * 0.17).max(2.5),
        preview.text,
    );
    // Rounded corners against the card.
    ui.painter().rect_stroke(
        rect.expand(radius / 2.0),
        radius * 1.5,
        Stroke::new(radius, card),
        egui::StrokeKind::Middle,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A theme with every rule under one `prefers-color-scheme` shows only
    /// in that mode; a rule outside it shows in both.
    #[test]
    fn a_theme_under_one_colour_scheme_shows_only_in_it() {
        let light = "@media (prefers-color-scheme: light) { :root { --a: 1; } .x { color: red; } }";
        assert_eq!(only_in(&resolve_both(light)), Some(true));
        let dark = "@media (prefers-color-scheme: dark) { .x { color: red; } }";
        assert_eq!(only_in(&resolve_both(dark)), Some(false));
        let both = format!("{light} .y {{ color: blue; }}");
        assert_eq!(only_in(&resolve_both(&both)), None);
        assert_eq!(only_in(&resolve_both(".y { color: blue; }")), None);
        // A preview saved before this was recorded says nothing.
        let mut old = resolve_both(light);
        old[1].styled = false;
        assert_eq!(only_in(&old), None);
    }

    #[test]
    fn rules_follow_nesting_media_and_importance() {
        let css = r#"
/* {"name": "x"} */
:root { --wave-color-solid-base-fill: #101018; --wave-color-solid-accent-fill: #ff00aa; }
@media (prefers-color-scheme: light) { :root { --wave-color-solid-base-fill: #f0f0f8; } }
[class="has-footer-player is-desktop-app"]{ ._player_8c80ae6 { max-width: 95vw !important; } [data-test=footer-player]{ bottom: 10px; border-radius: 1em; } }
#footerPlayer { background-color: #223344 !important; }
._player_8c80ae6 { background-color: #000000; }
[class*="_sidebarWrapper_"] { top: 2vh; border-radius: 0 20px 20px 0; background: #334455; }
body { background-color: var(--wave-color-solid-base-fill); background-image: linear-gradient(#000000, #ffffff); }
#footerPlayer:hover { background-color: #ffffff; }
"#;
        let dark = resolve(css, false);
        assert_eq!(dark.window, Color32::from_rgb(0x10, 0x10, 0x18));
        // TIDAL 2.43.2 keeps its accent whatever the token says.
        assert_eq!(dark.accent, Palette::dark().accent);
        assert_eq!(
            dark.bar,
            Color32::from_rgb(0x22, 0x33, 0x44),
            "!important wins, :hover is out"
        );
        assert_eq!(dark.sidebar, Color32::from_rgb(0x33, 0x44, 0x55));
        // `1em` corners of TIDAL's 14-point bar.
        assert_eq!(dark.bar_float, Some([35.0, 10.0, 14.0]));
        assert_eq!(dark.side_float, Some([18.0, 20.0]));
        assert!(matches!(dark.layers.as_slice(), [PageLayer::Linear { .. }]));
        let light = resolve(css, true);
        assert_eq!(light.window, Color32::from_rgb(0xf0, 0xf0, 0xf8));
        assert!(!light.dark);
    }

    #[test]
    fn a_theme_without_rules_previews_the_default_look() {
        let p = resolve("", false);
        assert_eq!(p.window, Palette::dark().window);
        assert_eq!(p.bar_float, None);
        assert_eq!(p.side_float, None);
        assert!(p.layers.is_empty());
    }

    /// A preview is the same theme in both modes when it only names one.
    #[test]
    fn previews_differ_by_theme() {
        let paint = "body{background-color:var(--wave-color-solid-base-fill)}";
        let a = resolve(
            &format!(":root{{--wave-color-solid-base-fill:#200020}}{paint}"),
            false,
        );
        let b = resolve(
            &format!(":root{{--wave-color-solid-base-fill:#002020}}{paint}"),
            false,
        );
        assert_ne!(a.window, b.window);
        // A theme of root tokens alone keeps TIDAL's window, as TIDAL 2.43.2
        // does (Rose Pine, the Catppuccin flavours).
        let tokens = resolve(":root{--wave-color-solid-base-fill:#200020}", false);
        assert_eq!(tokens.window, Palette::dark().window);
    }
}
