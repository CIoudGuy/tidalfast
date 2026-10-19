//! Shell geometry a Luna theme sets: a floating, rounded player bar
//! (Snusehunden's Tidaluna-Theme, Lunallena, the cotton candy family and
//! the Solo themes built on it) and a floating sidebar card.
//!
//! Only a theme the user applied changes the shell: with no theme (or
//! Luna off) nothing here is resolved and the bar and sidebar keep their
//! native geometry exactly. What a theme sets is read once per overlay
//! change and window size ([`super::plugin_dom::theme_layout`]), never per
//! frame.

use std::collections::BTreeMap;

use egui::{Color32, CornerRadius, Margin, Shadow};

use crate::luna::dom::{HostOverride, Style};

use super::plugin_slots::css;

/// The most a theme may move the bar or sidebar in from the window's edge.
const MAX_GAP: f32 = 64.0;

/// A shell box a theme lifts off the window's edges.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Floating {
    /// Room left between the box and the window's edges, in points.
    pub gaps: Margin,
    pub radius: CornerRadius,
    /// The theme's outer `box-shadow` (its first non-inset one).
    pub shadow: Shadow,
}

impl Floating {
    /// The gaps along the panel's own axis (top and bottom).
    pub fn vertical(&self) -> f32 {
        f32::from(self.gaps.top) + f32::from(self.gaps.bottom)
    }
}

/// What a theme sets on the shell's geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeLayout {
    /// `#footerPlayer`: inset from the sides (`width`/`max-width` under the
    /// window's), lifted (`bottom`), rounded, shadowed.
    pub player_bar: Option<Floating>,
    /// `[class*=_sidebarWrapper_]`: moved down (`top`) to a card the same
    /// distance off the bottom, rounded, shadowed.
    pub sidebar: Option<Floating>,
    /// `[class*=_sidebarWrapper_]`'s negative `left`: how far the sidebar
    /// slides out of its place to the left, in points (Lunallena's, which
    /// slides back in under the pointer through its `:hover` rule).
    pub sidebar_shift: f32,
    /// `#main`'s negative `left` under a `min-width`/`width` as wide as the
    /// window: how far the page reaches left under the sidebar, in points
    /// (Lunallena's `left: calc(var(--sidebarWidth) * -1)`, which lays the
    /// page under the whole sidebar and leaves the sidebar floating over it).
    pub main_reach: f32,
    /// `#mainHeader` lifted above the page as a tab (Snusehunden,
    /// Lunallena), which their `:hover` rules slide down.
    pub header: Option<HeaderTab>,
}

impl ThemeLayout {
    pub fn is_empty(&self) -> bool {
        self.player_bar.is_none()
            && self.sidebar.is_none()
            && self.sidebar_shift == 0.0
            && self.main_reach == 0.0
            && self.header.is_none()
    }
}

/// `#mainHeader` as a tab over the page's top: a box `width` wide, placed
/// by `left` and `transform: translateX()`, pulled up by a negative
/// `margin-top` so that only its foot shows, with its `padding` above and
/// beside the controls. It takes no room from the page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeaderTab {
    /// The tab's width, in points.
    pub width: f32,
    /// `left` as a fraction of the page's width, plus points.
    pub left_fraction: f32,
    pub left: f32,
    /// `translateX()` as a fraction of the tab's width, plus points.
    pub shift_fraction: f32,
    pub shift: f32,
    /// `margin-top`: negative, how far the tab is pulled up.
    pub margin_top: f32,
    /// [top, right, bottom, left] room round the controls.
    pub padding: [f32; 4],
    pub radius: CornerRadius,
}

impl HeaderTab {
    /// The tab's box in a page `page` wide whose top is at `page.top()`,
    /// for controls `content` points tall. Part of it is above the page.
    pub fn rect(&self, page: egui::Rect, content: f32) -> egui::Rect {
        let [top, right, bottom, left] = self.padding;
        let width = self.width.max(left + right + 1.0);
        let x = page.left()
            + self.left_fraction * page.width()
            + self.left
            + self.shift_fraction * width
            + self.shift;
        egui::Rect::from_min_size(
            egui::pos2(x, page.top() + self.margin_top),
            egui::vec2(width, top + content + bottom),
        )
    }

    /// Where the controls go inside the tab's box `rect`.
    pub fn content(&self, rect: egui::Rect) -> egui::Rect {
        let [top, right, bottom, left] = self.padding;
        egui::Rect::from_min_max(
            rect.min + egui::vec2(left, top),
            rect.max - egui::vec2(right, bottom),
        )
    }
}

/// The layout the theme's rules on the player bar and sidebar ask for, in a
/// window `viewport` points wide and high.
pub fn resolve(
    footer: Option<&HostOverride>,
    sidebar: Option<&HostOverride>,
    header: Option<&HostOverride>,
    main: Option<&HostOverride>,
    vars: &BTreeMap<String, String>,
    viewport: [f32; 2],
) -> ThemeLayout {
    let sidebar = sidebar.filter(|h| !h.hidden());
    ThemeLayout {
        player_bar: footer
            .filter(|h| !h.hidden())
            .and_then(|h| player_bar(&h.style, vars, viewport)),
        sidebar: sidebar.and_then(|h| side_card(&h.style, vars, viewport)),
        sidebar_shift: sidebar.map_or(0.0, |h| side_shift(&h.style, vars, viewport)),
        main_reach: main
            .filter(|h| !h.hidden())
            .map_or(0.0, |h| main_reach(&h.style, vars, viewport)),
        header: header
            .filter(|h| !h.hidden())
            .and_then(|h| header_tab(&h.style, vars, viewport)),
    }
}

/// A negative `left` on a positioned sidebar: how far it slides out, at
/// most the window's width.
fn side_shift(style: &Style, vars: &BTreeMap<String, String>, viewport: [f32; 2]) -> f32 {
    if style.position.as_deref().is_some_and(|p| {
        p.trim()
            .trim_end_matches("!important")
            .trim()
            .eq_ignore_ascii_case("static")
    }) {
        return 0.0;
    }
    len(style.left.as_deref(), vars, viewport[0], viewport)
        .filter(|l| *l < 0.0)
        .map_or(0.0, |l| (-l).min(viewport[0]).round())
}

/// `#main` pulled left by a negative `left` and kept as wide as the window
/// (`min-width` or `width` of the window's width): the page then reaches
/// that far under the sidebar. A page that is not kept that wide would leave
/// a gap on its right, which is not a layout Tidalfast draws.
fn main_reach(style: &Style, vars: &BTreeMap<String, String>, viewport: [f32; 2]) -> f32 {
    let [vw, _] = viewport;
    let fixed = style.position.as_deref().is_some_and(|p| {
        p.trim()
            .trim_end_matches("!important")
            .trim()
            .eq_ignore_ascii_case("static")
    });
    if vw <= 0.0 || fixed {
        return 0.0;
    }
    let Some(left) = len(style.left.as_deref(), vars, vw, viewport).filter(|l| *l < -0.5) else {
        return 0.0;
    };
    let spans = [style.min_width.as_deref(), style.width.as_deref()]
        .into_iter()
        .any(|w| len(w, vars, vw, viewport).is_some_and(|w| w >= vw - 1.0));
    if spans { (-left).min(vw).round() } else { 0.0 }
}

/// A `length` or `percentage` as (fraction, points).
fn split_len(
    value: Option<&str>,
    vars: &BTreeMap<String, String>,
    viewport: [f32; 2],
) -> Option<(f32, f32)> {
    let v = css::resolve_vars(value?, vars);
    let v = v.trim().trim_end_matches("!important").trim();
    if let Some(pct) = v.strip_suffix('%') {
        return pct
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|n| n.is_finite())
            .map(|n| (n / 100.0, 0.0));
    }
    css::length(v, 16.0, 0.0, viewport)
        .filter(|n| n.is_finite())
        .map(|n| (0.0, n))
}

/// The tab a negative `margin-top` and a `width` under the window's make of
/// `#mainHeader`. A header pulled up without a width of its own is not a
/// tab Tidalfast draws (it would hide the controls for good).
fn header_tab(
    style: &Style,
    vars: &BTreeMap<String, String>,
    viewport: [f32; 2],
) -> Option<HeaderTab> {
    let [vw, _] = viewport;
    let margin_top = len(style.margin_top.as_deref(), vars, vw, viewport)?;
    if margin_top > -0.5 {
        return None;
    }
    let width = len(style.width.as_deref(), vars, vw, viewport)?;
    if !(1.0..vw).contains(&width) {
        return None;
    }
    let (left_fraction, left) = if offsets_apply(style) || style.position.is_some() {
        split_len(style.left.as_deref(), vars, viewport).unwrap_or_default()
    } else {
        (0.0, 0.0)
    };
    let (shift_fraction, shift) = style
        .transform
        .as_deref()
        .map(|t| css::resolve_vars(t, vars))
        .and_then(|t| {
            let at = t.to_ascii_lowercase().find("translatex(")? + "translatex(".len();
            let end = t[at..].find(')')? + at;
            split_len(Some(&t[at..end]), vars, viewport)
        })
        .unwrap_or_default();
    let side = |v: Option<&String>| {
        v.and_then(|v| css::length(&css::resolve_vars(v, vars), 16.0, 0.0, viewport))
            .filter(|n| n.is_finite())
    };
    let base = style
        .padding
        .as_deref()
        .map_or([0.0; 4], |v| css::sides(&css::resolve_vars(v, vars), 16.0));
    let padding = [
        side(style.padding_top.as_ref()).unwrap_or(base[0]),
        side(style.padding_right.as_ref()).unwrap_or(base[1]),
        base[2],
        side(style.padding_left.as_ref()).unwrap_or(base[3]),
    ]
    .map(|n| n.clamp(0.0, 200.0));
    Some(HeaderTab {
        width,
        left_fraction: left_fraction.clamp(-1.0, 2.0),
        left: left.clamp(-vw, vw),
        shift_fraction: shift_fraction.clamp(-1.0, 1.0),
        shift: shift.clamp(-vw, vw),
        margin_top: margin_top.max(-400.0),
        padding,
        radius: radii(style.border_radius.as_deref(), vars, viewport),
    })
}

fn len(
    value: Option<&str>,
    vars: &BTreeMap<String, String>,
    reference: f32,
    viewport: [f32; 2],
) -> Option<f32> {
    let v = css::resolve_vars(value?, vars);
    css::length(&v, 16.0, reference, viewport).filter(|n| n.is_finite())
}

/// `position` lets `top`/`bottom` move the box (TIDAL's bar and sidebar
/// are positioned; `static` and `relative` keep them in place).
fn offsets_apply(style: &Style) -> bool {
    !style.position.as_deref().is_some_and(|p| {
        let p = p.trim().trim_end_matches("!important").trim();
        p.eq_ignore_ascii_case("static") || p.eq_ignore_ascii_case("relative")
    })
}

fn gap(n: f32) -> i8 {
    n.clamp(0.0, MAX_GAP).round() as i8
}

fn player_bar(
    style: &Style,
    vars: &BTreeMap<String, String>,
    viewport: [f32; 2],
) -> Option<Floating> {
    let [vw, _] = viewport;
    if vw <= 0.0 {
        return None;
    }
    let mut width = len(style.width.as_deref(), vars, vw, viewport).unwrap_or(vw);
    if let Some(max) = len(style.max_width.as_deref(), vars, vw, viewport) {
        width = width.min(max);
    }
    if let Some(min) = len(style.min_width.as_deref(), vars, vw, viewport) {
        width = width.max(min);
    }
    let side = ((vw - width) / 2.0).max(0.0);
    let bottom = if offsets_apply(style) {
        len(style.bottom.as_deref(), vars, viewport[1], viewport).unwrap_or(0.0)
    } else {
        0.0
    };
    let radius = radii(style.border_radius.as_deref(), vars, viewport);
    if side < 0.5 && bottom < 0.5 && radius == CornerRadius::ZERO {
        return None;
    }
    let side = gap(side);
    Some(Floating {
        gaps: Margin {
            left: side,
            right: side,
            top: 0,
            bottom: gap(bottom),
        },
        radius,
        shadow: shadow(style.box_shadow.as_deref(), vars, viewport).unwrap_or_default(),
    })
}

fn side_card(
    style: &Style,
    vars: &BTreeMap<String, String>,
    viewport: [f32; 2],
) -> Option<Floating> {
    let top = if offsets_apply(style) {
        len(style.top.as_deref(), vars, viewport[1], viewport).unwrap_or(0.0)
    } else {
        0.0
    };
    let radius = radii(style.border_radius.as_deref(), vars, viewport);
    if top < 0.5 && radius == CornerRadius::ZERO {
        return None;
    }
    // Themes shorten the box (`height: 76vh`, `calc(100% - ...)`) to end
    // above TIDAL's floating bar; a card as far off the bottom as the top
    // is what that looks like over Tidalfast's own bar.
    let top = gap(top);
    Some(Floating {
        gaps: Margin {
            left: 0,
            right: 0,
            top,
            bottom: top,
        },
        radius,
        shadow: shadow(style.box_shadow.as_deref(), vars, viewport).unwrap_or_default(),
    })
}

/// `border-radius` with one to four corners (`0 20px 20px 0`); the
/// elliptical part after `/` is left out.
pub fn radii(
    value: Option<&str>,
    vars: &BTreeMap<String, String>,
    viewport: [f32; 2],
) -> CornerRadius {
    let Some(value) = value else {
        return CornerRadius::ZERO;
    };
    let v = css::resolve_vars(value, vars);
    let v = v.trim().trim_end_matches("!important").trim();
    let v = v.split('/').next().unwrap_or_default();
    let corners: Vec<u8> = css::words(v)
        .into_iter()
        .map(|w| {
            css::length(w, 16.0, 0.0, viewport)
                .filter(|n| n.is_finite())
                .map_or(0, |n| n.clamp(0.0, 255.0).round() as u8)
        })
        .collect();
    let (nw, ne, se, sw) = match corners.as_slice() {
        [a] => (*a, *a, *a, *a),
        [a, b] => (*a, *b, *a, *b),
        [a, b, c] => (*a, *b, *c, *b),
        [a, b, c, d, ..] => (*a, *b, *c, *d),
        [] => return CornerRadius::ZERO,
    };
    CornerRadius { nw, ne, se, sw }
}

/// The first outer layer of a `box-shadow` (inset layers glow inside the
/// box and are not drawn).
pub fn shadow(
    value: Option<&str>,
    vars: &BTreeMap<String, String>,
    viewport: [f32; 2],
) -> Option<Shadow> {
    let v = css::resolve_vars(value?, vars);
    let v = v.trim().trim_end_matches("!important").trim();
    for layer in css::split_top(v, ',') {
        let words = css::words(layer.trim());
        if words.iter().any(|w| w.eq_ignore_ascii_case("inset"))
            || words
                .first()
                .is_some_and(|w| w.eq_ignore_ascii_case("none"))
        {
            continue;
        }
        let mut lengths = Vec::new();
        let mut color = None;
        for w in &words {
            if let Some(n) = css::length(w, 16.0, 0.0, viewport) {
                lengths.push(n);
            } else if let Some(c) = css::color(w) {
                color = Some(c);
            }
        }
        if lengths.len() < 2 {
            continue;
        }
        let px = |i: usize, lo: f32, hi: f32| lengths.get(i).copied().unwrap_or(0.0).clamp(lo, hi);
        let color = color.unwrap_or(Color32::from_black_alpha(128));
        if color.a() == 0 {
            continue;
        }
        return Some(Shadow {
            offset: [
                px(0, -64.0, 64.0).round() as i8,
                px(1, -64.0, 64.0).round() as i8,
            ],
            blur: px(2, 0.0, 96.0).round() as u8,
            spread: px(3, 0.0, 64.0).round() as u8,
            color,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(css: &[(&str, &str)]) -> Style {
        let mut s = Style::default();
        for (k, v) in css {
            let v = Some((*v).to_string());
            match *k {
                "position" => s.position = v,
                "bottom" => s.bottom = v,
                "top" => s.top = v,
                "width" => s.width = v,
                "max-width" => s.max_width = v,
                "min-width" => s.min_width = v,
                "border-radius" => s.border_radius = v,
                "box-shadow" => s.box_shadow = v,
                "height" => s.height = v,
                "display" => s.display = v,
                "left" => s.left = v,
                "transform" => s.transform = v,
                "margin-top" => s.margin_top = v,
                "padding-top" => s.padding_top = v,
                "padding-left" => s.padding_left = v,
                "padding-right" => s.padding_right = v,
                _ => panic!("{k}"),
            }
        }
        s
    }

    fn host(css: &[(&str, &str)]) -> HostOverride {
        HostOverride {
            style: style(css),
            ..HostOverride::default()
        }
    }

    const VIEW: [f32; 2] = [1400.0, 900.0];

    #[test]
    fn no_theme_and_colour_only_themes_leave_the_shell_alone() {
        let vars = BTreeMap::new();
        assert!(resolve(None, None, None, None, &vars, VIEW).is_empty());
        let colours = HostOverride {
            style: Style {
                background_color: Some("#101018".into()),
                border: Some("solid 1px red".into()),
                ..Style::default()
            },
            ..HostOverride::default()
        };
        assert!(resolve(Some(&colours), Some(&colours), None, None, &vars, VIEW).is_empty());
        // Synthwave's `* { border-radius: 0; box-shadow: none }`.
        let square = host(&[("border-radius", "0"), ("box-shadow", "none")]);
        assert!(resolve(Some(&square), Some(&square), None, None, &vars, VIEW).is_empty());
        // A full-width bar stays put.
        let full = host(&[("width", "100vw"), ("bottom", "0")]);
        assert!(resolve(Some(&full), None, None, None, &vars, VIEW).is_empty());
    }

    /// Lunallena (and Snusehunden's header): `#mainHeader` 30vw wide,
    /// centred by `left: 50%; translateX(-50%)`, pulled up by `margin-top`
    /// under its padding, a tab whose `:hover` rule pulls it less far; the
    /// sidebar slid out by `left: -47px`.
    #[test]
    fn lunallena_lifts_the_header_into_a_tab_and_slides_the_sidebar_out() {
        let vars = BTreeMap::new();
        let header = |margin: &str| {
            host(&[
                ("margin-top", margin),
                ("width", "30vw"),
                ("left", "50%"),
                ("transform", "translateX(-50%)"),
                ("border-radius", "0 0 20px 20px"),
                ("padding-top", "45px"),
                ("padding-left", "20px"),
                ("padding-right", "20px"),
            ])
        };
        let side = host(&[("left", "-47px"), ("top", "2vh")]);
        let l = resolve(None, Some(&side), Some(&header("-80px")), None, &vars, VIEW);
        assert_eq!(l.sidebar_shift, 47.0);
        let tab = l.header.expect("the header is a tab");
        assert!((tab.width - 0.3 * VIEW[0]).abs() < 0.01);
        assert_eq!((tab.left_fraction, tab.shift_fraction), (0.5, -0.5));
        assert_eq!(tab.margin_top, -80.0);
        assert_eq!(tab.padding, [45.0, 20.0, 0.0, 20.0]);
        let page = egui::Rect::from_min_size(egui::pos2(240.0, 40.0), egui::vec2(1000.0, 700.0));
        let rect = tab.rect(page, 56.0);
        assert_eq!(rect.center().x, page.center().x, "centred on the page");
        assert_eq!(rect.top(), page.top() - 80.0);
        // 45 + 56 - 80: only the tab's foot shows until the pointer is on it.
        assert_eq!(rect.bottom() - page.top(), 21.0);
        assert_eq!(tab.content(rect).width(), rect.width() - 40.0);
        // Its `:hover` rule pulls it less far up: the controls show.
        let hovered = resolve(None, None, Some(&header("-30px")), None, &vars, VIEW);
        assert_eq!(
            hovered.header.unwrap().rect(page, 56.0).bottom() - page.top(),
            71.0
        );
        // Pulled up without a width of its own, or not pulled up: no tab.
        let plain = host(&[("margin-top", "-60px")]);
        assert!(
            resolve(None, None, Some(&plain), None, &vars, VIEW)
                .header
                .is_none()
        );
        let down = host(&[("margin-top", "10px"), ("width", "30vw")]);
        assert!(resolve(None, None, Some(&down), None, &vars, VIEW).is_empty());
        let back = host(&[("left", "0px")]);
        assert!(resolve(None, Some(&back), None, None, &vars, VIEW).is_empty());
    }

    /// Snusehunden's Tidaluna-Theme: `position: fixed; bottom: 2vh;
    /// width: 98vw; border-radius: 15px`, and a sidebar card `top: 2vh`
    /// rounded on its right.
    #[test]
    fn snusehunden_floats_a_rounded_bar_and_a_sidebar_card() {
        let vars = BTreeMap::new();
        let bar = host(&[
            ("position", "fixed"),
            ("bottom", "2vh"),
            ("width", "98vw"),
            ("border-radius", "15px"),
        ]);
        let side = host(&[
            ("top", "2vh"),
            ("height", "76vh"),
            ("border-radius", "0 20px 20px 0"),
        ]);
        let l = resolve(Some(&bar), Some(&side), None, None, &vars, VIEW);
        let bar = l.player_bar.unwrap();
        assert_eq!(
            bar.gaps,
            Margin {
                left: 14,
                right: 14,
                top: 0,
                bottom: 18
            }
        );
        assert_eq!(bar.radius, CornerRadius::same(15));
        assert_eq!(bar.shadow, Shadow::NONE);
        let side = l.sidebar.unwrap();
        assert_eq!(
            side.gaps,
            Margin {
                left: 0,
                right: 0,
                top: 18,
                bottom: 18
            }
        );
        assert_eq!(
            side.radius,
            CornerRadius {
                nw: 0,
                ne: 20,
                se: 20,
                sw: 0
            }
        );
    }

    /// Cotton candy's base: `width: 200%` held to `98.3vw` by `max-width`
    /// and `min-width`, `bottom: 7.25px`, `border-radius: 1em`, and a glow
    /// after an inset shadow.
    #[test]
    fn cotton_candy_bar_takes_max_width_bottom_radius_and_outer_glow() {
        let vars = BTreeMap::from([("--glow".to_string(), "rgba(255, 255, 255, 0.2)".to_string())]);
        let bar = host(&[
            ("width", "200%"),
            ("max-width", "98.3vw!important"),
            ("min-width", "98.3vw!important"),
            ("bottom", "7.25px!important"),
            ("border-radius", "1em!important"),
            (
                "box-shadow",
                "inset 0px 0px 6.3em #00000010, 0px 0px 22px 5px var(--glow)",
            ),
        ]);
        let b = resolve(Some(&bar), None, None, None, &vars, VIEW)
            .player_bar
            .unwrap();
        assert_eq!(
            b.gaps,
            Margin {
                left: 12,
                right: 12,
                top: 0,
                bottom: 7
            }
        );
        assert_eq!(b.radius, CornerRadius::same(16));
        assert_eq!(b.shadow.blur, 22);
        assert_eq!(b.shadow.spread, 5);
        assert_eq!(
            b.shadow.color,
            Color32::from_rgba_unmultiplied(255, 255, 255, 51)
        );
    }

    #[test]
    fn hidden_static_and_huge_values_are_bounded() {
        let vars = BTreeMap::new();
        let hidden = host(&[("display", "none"), ("bottom", "20px")]);
        assert!(resolve(Some(&hidden), None, None, None, &vars, VIEW).is_empty());
        let relative = host(&[("position", "relative"), ("bottom", "20px")]);
        assert!(resolve(Some(&relative), None, None, None, &vars, VIEW).is_empty());
        let far = host(&[("bottom", "50vh"), ("width", "10vw")]);
        let b = resolve(Some(&far), None, None, None, &vars, VIEW)
            .player_bar
            .unwrap();
        assert_eq!(b.gaps.bottom, MAX_GAP as i8);
        assert_eq!(b.gaps.left, MAX_GAP as i8);
    }

    /// Lunallena's `#main { left: calc(var(--sidebarWidth) * -1);
    /// min-width: 100vw }` (the variable already worked out): the page
    /// reaches under the sidebar by that much. Pulled left but not kept as
    /// wide as the window, pushed right, or `static`: it stays beside it.
    #[test]
    fn a_page_pulled_left_and_kept_window_wide_reaches_under_the_sidebar() {
        let vars = BTreeMap::new();
        let main = |css: &[(&str, &str)]| {
            resolve(None, None, None, Some(&host(css)), &vars, VIEW).main_reach
        };
        assert_eq!(main(&[("left", "-240px"), ("min-width", "100vw")]), 240.0);
        assert_eq!(
            main(&[("left", "calc(240px * -1)"), ("width", "100vw")]),
            240.0
        );
        assert_eq!(main(&[("left", "-240px")]), 0.0);
        assert_eq!(main(&[("left", "-240px"), ("min-width", "50vw")]), 0.0);
        assert_eq!(main(&[("left", "40px"), ("min-width", "100vw")]), 0.0);
        let still = [
            ("left", "-240px"),
            ("min-width", "100vw"),
            ("position", "static"),
        ];
        assert_eq!(main(&still), 0.0);
        // At most the window's width.
        assert_eq!(
            main(&[("left", "-9000px"), ("min-width", "100vw")]),
            VIEW[0]
        );
    }
}
