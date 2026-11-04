//! One look per anchor: what the theme cascade makes of a native part's
//! TIDAL element, read through one API.
//!
//! A native part that has a TIDAL anchor (a sidebar row, the play queue, a
//! menu, a pill) asks [`anchor_look`] for its anchor's whole box (fill,
//! gradients and pictures, borders, corners, shadows, opacity, text colour,
//! size and weight, icon fill, padding, scale and translation) instead of
//! reading one property at a time, and [`paint_anchor_box`] paints the box
//! under the part's own content and hands the look back for its words and
//! icons. The anchor carries the part's state as TIDAL's element does
//! (`_selected_`, `_activeButton_`, `aria-pressed`, `[disabled]`), and the
//! pointer's `:hover` reaches it through the bridge's hover events, so the
//! look is always the one for the state the part is in. Read once per
//! overlay version and anchor (an overlay changes with every state the
//! runtime reports), never per frame. `None` while Luna is off or no theme
//! styles the anchor: the part keeps the app's own look, so with no theme
//! nothing changes.

use std::sync::Arc;

use egui::{Color32, CornerRadius, Rect};

use super::{css, host, host_moved, moving, on, own_fill, style_color, theme_boxes, viewport};
use crate::luna::dom::{HostOverride, Style};

/// Everything a theme's rules give one anchor, resolved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnchorLook {
    /// The box's `background-color` (with its backdrop filter's veil folded
    /// in where the filter is not drawn, see [`super::super::backdrop`]).
    /// `Some(TRANSPARENT)` when the theme clears the part's own fill
    /// (`background: none`, `background-color: transparent`).
    pub fill: Option<Color32>,
    /// The box's `backdrop-filter`, drawn under its fill (only where
    /// backdrop filters are drawn).
    pub backdrop: Option<Arc<super::super::backdrop::Backdrop>>,
    /// The box has layers beyond its fill: `background-image` gradients and
    /// pictures, `::before` / `::after` boxes, frosted glass or a uniform
    /// border (painted by [`super::paint_host_layers_rounded`]).
    pub layers: bool,
    /// The theme's corners, one to four.
    pub corners: Option<CornerRadius>,
    /// A border on some sides only (`border-width: 0 0 1px 0`), top right
    /// bottom left, and its colour (painted with the layers, as a border on
    /// all four sides is).
    pub sides: Option<([f32; 4], Color32)>,
    /// The first outer `box-shadow` layer.
    pub shadow: Option<egui::epaint::Shadow>,
    /// The first inset `box-shadow` layer: colour, blur, spread (the cotton
    /// candy family's glow inside the selected navigation pill).
    pub glow: Option<(Color32, f32, f32)>,
    /// The box's `opacity`.
    pub opacity: Option<f32>,
    /// The colour of the box's words.
    pub text: Option<Color32>,
    /// A rule leaves the words the colour around them (`color: inherit`,
    /// or a variable the theme never defines).
    pub text_inherits: bool,
    /// The colour of the box's icons: its `fill`, else its words' colour.
    pub icon: Option<Color32>,
    /// The words' size in points and weight.
    pub font_size: Option<f32>,
    pub font_weight: Option<u16>,
    /// `text-transform: uppercase` (`Some(false)` for `none`).
    pub uppercase: Option<bool>,
    /// The box's `padding`, top right bottom left, in points.
    pub padding: Option<[f32; 4]>,
    /// The box's `transform: scale()` (and its `scale`, which the runtime
    /// folds into the `transform`): every scale in it, multiplied.
    pub scale: Option<f32>,
    /// The box's `transform: translate()`.
    pub translate: egui::Vec2,
}

impl AnchorLook {
    /// The look paints a box: a fill, layers, a border or a shadow.
    pub fn paints_box(&self) -> bool {
        self.fill.is_some()
            || self.backdrop.is_some()
            || self.layers
            || self.sides.is_some()
            || self.shadow.is_some()
            || self.glow.is_some()
    }

    /// The part's corners: the theme's, else `native`.
    pub fn corners_or(&self, native: CornerRadius) -> CornerRadius {
        self.corners.unwrap_or(native)
    }

    /// The fill to draw instead of the part's own `native` one: the theme's
    /// (see-through when it clears it), else the native fill.
    pub fn fill_or(&self, native: Color32) -> Color32 {
        self.fill.unwrap_or(native)
    }

    /// The words' colour: the theme's, else `native`.
    pub fn text_or(&self, native: Color32) -> Color32 {
        self.text.unwrap_or(native)
    }

    /// The icons' colour: the theme's, else `native`.
    pub fn icon_or(&self, native: Color32) -> Color32 {
        self.icon.unwrap_or(native)
    }
}

/// The look a theme gives anchor `key` in the state it has now, read once
/// per overlay version. `None` while Luna is off, the anchor is hidden, or
/// no theme rule styles it.
pub fn anchor_look(key: &str) -> Option<Arc<AnchorLook>> {
    if !on() {
        return None;
    }
    if let Some(hit) = theme_boxes(|b| b.looks.get(key).cloned()) {
        return hit;
    }
    let viewport = viewport();
    let made = host(key, |h, vars| look_of(h, vars, viewport)).map(Arc::new);
    theme_boxes(|b| b.looks.insert(key.to_string(), made.clone()));
    made
}

/// [`anchor_look`] for one host's override.
/// Runs inside the bridge's state: nothing here may reach it again.
pub(super) fn look_of(
    h: &HostOverride,
    vars: &std::collections::BTreeMap<String, String>,
    viewport: [f32; 2],
) -> Option<AnchorLook> {
    if h.hidden() {
        return None;
    }
    let s = &h.style;
    let cleared = || {
        // `background: none` beside no fill of its own clears the part's.
        s.background_image
            .as_deref()
            .is_some_and(|v| {
                v.trim()
                    .trim_end_matches("!important")
                    .trim()
                    .eq_ignore_ascii_case("none")
            })
            .then_some(Color32::TRANSPARENT)
    };
    let backdrop = super::backdrop_of(h, vars).map(Arc::new);
    // Over a backdrop filter it draws, the box's fill is the theme's.
    let fill = if backdrop.is_some() {
        super::unveiled_fill(h, vars)
    } else {
        own_fill(h, vars)
    }
    .or_else(cleared);
    let layers = !super::box_layers(h, vars, viewport).is_empty();
    let font_size = s.font_size.as_deref().and_then(|v| {
        let v = css::resolve_vars(v, vars);
        css::length(&v, 14.0, 14.0, viewport).filter(|n| *n > 0.0)
    });
    // `em` in the box's own lengths is its own text size (TIDAL's medium
    // market buttons are 16 points: the cotton candy family's `.9em`
    // corners on the Play pill are 14.4), else TIDAL's 14.
    let em = font_size.unwrap_or(14.0);
    let corners = s
        .border_radius
        .as_deref()
        .and_then(|v| corners(v, vars, viewport, em, own_size(s, vars, viewport, em)));
    let sides = some_sides(s, vars, viewport);
    let shadow = super::super::theme_layout::shadow(s.box_shadow.as_deref(), vars, viewport);
    let glow = super::super::theme_layout::inset_shadow(s.box_shadow.as_deref(), vars, viewport);
    let opacity = s.opacity.as_deref().and_then(css::opacity);
    let text = style_color(s, |s| s.color.as_ref(), vars).filter(|c| c.a() > 0);
    let text_inherits = s
        .color
        .as_deref()
        .is_some_and(|c| c.trim().eq_ignore_ascii_case("inherit"));
    let icon = style_color(s, |s| s.fill.as_ref(), vars)
        .filter(|c| c.a() > 0)
        .or(text);
    let font_weight = s.font_weight.as_deref().and_then(|v| {
        let v = css::resolve_vars(v, vars);
        (!matches!(v.trim(), "inherit" | "")).then(|| css::weight(v.trim()))
    });
    let uppercase = s.text_transform.as_deref().map(|v| {
        v.trim()
            .trim_end_matches("!important")
            .trim()
            .eq_ignore_ascii_case("uppercase")
    });
    let padding = padding(s, vars, viewport, em);
    let transform = s.transform.as_deref().map(|t| css::resolve_vars(t, vars));
    let scale = transform
        .as_deref()
        .and_then(super::super::theme_motion::transform_scale);
    let translate = transform
        .as_deref()
        .map_or(egui::Vec2::ZERO, |t| super::translation_em(t, em));
    let look = AnchorLook {
        fill,
        backdrop,
        layers,
        corners,
        sides,
        shadow,
        glow,
        opacity,
        text,
        text_inherits,
        icon,
        font_size,
        font_weight,
        uppercase,
        padding,
        scale,
        translate,
    };
    (look != AnchorLook::default()).then_some(look)
}

/// The box's own `width` and `height` where the theme gives them as
/// lengths (not `%` of a parent, `auto` or the like).
fn own_size(
    s: &Style,
    vars: &std::collections::BTreeMap<String, String>,
    viewport: [f32; 2],
    em: f32,
) -> [Option<f32>; 2] {
    [s.width.as_deref(), s.height.as_deref()].map(|v| {
        let v = css::resolve_vars(v?, vars);
        let v = v.trim().trim_end_matches("!important").trim();
        if v.ends_with('%') {
            return None;
        }
        css::length(v, em, 0.0, viewport).filter(|n| n.is_finite() && *n > 0.0)
    })
}

/// One corner's radius: a length, or a share of the box (`50%`). CSS
/// takes the share of the box's width across and its height down; one
/// radius a corner takes the smaller of the two, of the box's own `size`
/// where the theme sets it. Half the box or more without a size is round
/// whatever the box (egui keeps a radius to half the box's least side).
fn corner(w: &str, em: f32, viewport: [f32; 2], size: [Option<f32>; 2]) -> Option<f32> {
    let Some(share) = w.trim().strip_suffix('%') else {
        return css::length(w, em, 0.0, viewport);
    };
    let share = share.trim().parse::<f32>().ok()? / 100.0;
    Some(match size {
        [Some(a), Some(b)] => share * a.min(b),
        [Some(a), None] | [None, Some(a)] => share * a,
        [None, None] if share >= 0.5 => 255.0,
        [None, None] => 0.0,
    })
}

/// `border-radius`, one to four corners (`0 20px 20px 0`), `em` of the
/// box's text size `em` (TIDAL's 14-point text unless the box has its
/// own), a share in `%` of the box's own `size` (see [`corner`]); the
/// elliptical part after `/` is left out. `None` for a value that is no
/// length.
fn corners(
    value: &str,
    vars: &std::collections::BTreeMap<String, String>,
    viewport: [f32; 2],
    em: f32,
    size: [Option<f32>; 2],
) -> Option<CornerRadius> {
    let v = css::resolve_vars(value, vars);
    let v = v.trim().trim_end_matches("!important").trim();
    let v = v.split('/').next().unwrap_or_default();
    let n: Vec<u8> = css::words(v)
        .iter()
        .map(|w| {
            corner(w, em, viewport, size)
                .filter(|n| n.is_finite())
                .map(|n| n.clamp(0.0, 255.0).round() as u8)
        })
        .collect::<Option<_>>()?;
    let (nw, ne, se, sw) = match n.as_slice() {
        [a] => (*a, *a, *a, *a),
        [a, b] => (*a, *b, *a, *b),
        [a, b, c] => (*a, *b, *c, *b),
        [a, b, c, d, ..] => (*a, *b, *c, *d),
        [] => return None,
    };
    Some(CornerRadius { nw, ne, se, sw })
}

/// A border on some sides only (`border-width`, which the runtime writes
/// for `border-style: solid none none none` too), as its widths top right
/// bottom left and colour. A border on all four is one of the box's layers.
pub(super) fn some_sides(
    s: &Style,
    vars: &std::collections::BTreeMap<String, String>,
    viewport: [f32; 2],
) -> Option<([f32; 4], Color32)> {
    let (width, color) = css::border(&css::resolve_vars(s.border.as_deref()?, vars))?;
    let four = |v: &str, f: &dyn Fn(&str) -> Option<f32>| -> Option<[f32; 4]> {
        let n: Vec<f32> = css::words(v).iter().map(|w| f(w)).collect::<Option<_>>()?;
        match n.as_slice() {
            [a] => Some([*a; 4]),
            [v, h] => Some([*v, *h, *v, *h]),
            [t, h, b] => Some([*t, *h, *b, *h]),
            [t, r, b, l, ..] => Some([*t, *r, *b, *l]),
            [] => None,
        }
    };
    let widths = s
        .border_width
        .as_deref()
        .map(|v| css::resolve_vars(v, vars))
        .and_then(|v| four(&v, &|w| css::length(w, 14.0, 0.0, viewport)));
    let sides = widths.unwrap_or([width; 4]);
    let all = sides.iter().all(|w| (*w - sides[0]).abs() < 0.01);
    (!all && sides.iter().any(|w| *w > 0.0) && color.a() > 0).then_some((sides, color))
}

/// The box's `padding` and its `padding-*` sides, in points (`em` of the
/// box's text size `em`).
fn padding(
    s: &Style,
    vars: &std::collections::BTreeMap<String, String>,
    viewport: [f32; 2],
    em: f32,
) -> Option<[f32; 4]> {
    let len = |v: &str| css::length(v, em, 0.0, viewport);
    let mut out = s.padding.as_deref().and_then(|v| {
        let v = css::resolve_vars(v, vars);
        let n: Vec<f32> = css::words(&v)
            .iter()
            .map(|w| len(w))
            .collect::<Option<_>>()?;
        match n.as_slice() {
            [a] => Some([*a; 4]),
            [v, h] => Some([*v, *h, *v, *h]),
            [t, h, b] => Some([*t, *h, *b, *h]),
            [t, r, b, l, ..] => Some([*t, *r, *b, *l]),
            [] => None,
        }
    });
    for (i, side) in [
        &s.padding_top,
        &s.padding_right,
        &s.padding_bottom,
        &s.padding_left,
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(v) = side
            .as_deref()
            .and_then(|v| len(&css::resolve_vars(v, vars)))
        {
            out.get_or_insert([0.0; 4])[i] = v;
        }
    }
    out
}

/// Paints the box a theme gives anchor `key` in `r`, under the content the
/// part draws after: its outer shadow, fill (fading by the anchor's
/// `transition`), gradients, pictures and `::before` / `::after` boxes,
/// borders, and inset glow, with the theme's corners or `native`. Returns
/// the look for the part's words and icons; `None` (and nothing drawn)
/// while no theme styles the anchor.
pub fn paint_anchor_box(
    ui: &egui::Ui,
    key: &str,
    r: Rect,
    native: CornerRadius,
) -> Option<Arc<AnchorLook>> {
    let look = anchor_look(key)?;
    paint_look(ui, key, &look, r, native);
    Some(look)
}

/// Draws a native part with `add` as the theme's `transform` moves and
/// scales its anchor `key`: everything `add` paints (the box, its words
/// and icons) scaled about the box's `transform-origin` in `r` (its centre
/// unless set) and moved by its translations, as Chromium draws a box with
/// a `transform` (its `scale` and `translate` properties included, which
/// the runtime folds into it): its room in the layout and where it takes
/// the pointer stay as they were. The scale the box's animations give it
/// now (a `scale` keyframe as the pointer comes onto it) and its
/// `transition` on the way between states are followed. With no transform
/// `add` draws as it would. Turns are left out.
pub fn scaled<R>(ui: &mut egui::Ui, key: &str, r: Rect, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let start = shapes_mark(ui);
    let out = add(ui);
    scale_shapes(ui, key, start..shapes_mark(ui), r);
    out
}

/// Where the next shape painted in `ui`'s layer goes, for [`scale_shapes`].
pub fn shapes_mark(ui: &egui::Ui) -> egui::layers::ShapeIdx {
    let layer = ui.layer_id();
    ui.ctx().graphics_mut(|g| g.entry(layer).next_idx())
}

/// [`scaled`] for a part whose box is known only once it is drawn (the
/// player bar's `._utilityButtons_`, as wide as its buttons): transforms
/// the shapes `ui`'s layer got in `range` as the theme transforms anchor
/// `key`, its box `r`.
pub fn scale_shapes(
    ui: &egui::Ui,
    key: &str,
    range: std::ops::Range<egui::layers::ShapeIdx>,
    r: Rect,
) {
    let Some(t) = box_transform(ui.ctx(), key, r) else {
        return;
    };
    // The shapes move, their clip stays where it is on the screen, as an
    // ancestor's clip does for a transformed box.
    let layer = ui.layer_id();
    ui.ctx().graphics_mut(|g| {
        let list = g.entry(layer);
        for i in range.start.0..range.end.0.min(list.next_idx().0) {
            list.mutate_shape(egui::layers::ShapeIdx(i), |c| c.shape.transform(t));
        }
    });
}

/// The transform the theme gives anchor `key` now, its box `r`, on the
/// screen: `None` while it leaves the box where it is.
pub fn box_transform(ctx: &egui::Context, key: &str, r: Rect) -> Option<egui::emath::TSTransform> {
    use egui::emath::TSTransform;
    let found = super::with(|s| {
        let h = s.overlay.hosts.get(key)?;
        if !super::on() || h.hidden() {
            return None;
        }
        let st = &h.style;
        if st.transform.is_none() && st.animation.is_none() {
            return None;
        }
        let written = st
            .transform
            .as_deref()
            .map(|t| css::resolve_vars(t, &s.vars))
            .unwrap_or_default();
        let own = super::super::theme_motion::transform_scale(&written).unwrap_or(1.0);
        let animated = super::super::theme_motion::scale(ctx, key, st, &s.overlay.keyframes, own);
        Some((
            written,
            own,
            animated.unwrap_or(own),
            super::moving(st, "transform"),
            st.transform_origin.clone(),
        ))
    })?;
    let (written, own, k, spec, origin) = found;
    let k = super::host_moved(key, "scale", Some(k), spec, |m| &mut m.numbers)?;
    // The written transform, function by function, about the origin; a
    // scale its animation or transition has elsewhere now scales on top.
    let mut m = TSTransform::IDENTITY;
    let lower = written.to_ascii_lowercase();
    let mut rest = lower.as_str();
    let len = |v: &str, reference: f32| {
        css::length(v.trim(), 14.0, reference, super::viewport()).filter(|l| l.is_finite())
    };
    while let Some(open) = rest.find('(') {
        let name = rest[..open]
            .trim()
            .rsplit([' ', ')'])
            .next()
            .unwrap_or("")
            .trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args: Vec<&str> = rest[open + 1..open + close]
            .split([',', ' '])
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .collect();
        rest = &rest[open + close + 1..];
        let arg =
            |i: usize, reference: f32| args.get(i).and_then(|a| len(a, reference)).unwrap_or(0.0);
        let step = match name {
            "translate" | "translate3d" => {
                TSTransform::from_translation(egui::vec2(arg(0, r.width()), arg(1, r.height())))
            }
            "translatex" => TSTransform::from_translation(egui::vec2(arg(0, r.width()), 0.0)),
            "translatey" => TSTransform::from_translation(egui::vec2(0.0, arg(0, r.height()))),
            "scale" | "scale3d" => {
                match super::super::theme_motion::scale_property(
                    &args[..args.len().min(2)].join(" "),
                ) {
                    Some(k) => TSTransform::from_scaling(k),
                    None => continue,
                }
            }
            _ => continue,
        };
        m = m * step;
    }
    if own > 0.0 && (k - own).abs() > 1e-4 {
        m = TSTransform::from_scaling((k / own).clamp(0.0, 8.0)) * m;
    }
    if !m.scaling.is_finite() || !m.translation.is_finite() {
        return None;
    }
    let moved = (m.scaling - 1.0).abs() > 1e-4 || m.translation.length() > 1e-3;
    if !moved {
        return None;
    }
    let at = origin_in(origin.as_deref(), r).to_vec2();
    Some(TSTransform::from_translation(at) * m * TSTransform::from_translation(-at))
}

/// The point `transform-origin` names in `r` (keywords, percentages and
/// lengths, `x y`; one value sets x, or y for `top` / `bottom`); the
/// centre while unset.
fn origin_in(value: Option<&str>, r: Rect) -> egui::Pos2 {
    let mut x = 0.5_f32;
    let mut y = 0.5_f32;
    let mut px: Option<f32> = None;
    let mut py: Option<f32> = None;
    let Some(value) = value else {
        return r.center();
    };
    let words: Vec<String> = value
        .trim()
        .trim_end_matches("!important")
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    // Keywords name their own axis; the rest are x then y.
    let mut axis = 0;
    for w in &words {
        match w.as_str() {
            "left" => x = 0.0,
            "right" => x = 1.0,
            "top" => y = 0.0,
            "bottom" => y = 1.0,
            "center" => {}
            other => {
                let v = if let Some(p) = other.strip_suffix('%') {
                    p.parse::<f32>().ok().map(|n| (n / 100.0, None))
                } else {
                    css::length(other, 14.0, f32::NAN, [0.0, 0.0])
                        .filter(|l| l.is_finite())
                        .map(|l| (0.0, Some(l)))
                };
                if let Some((f, l)) = v {
                    if axis == 0 {
                        x = f;
                        px = l;
                    } else {
                        y = f;
                        py = l;
                    }
                }
            }
        }
        if !matches!(w.as_str(), "top" | "bottom") {
            axis += 1;
        }
        if axis > 1 {
            break;
        }
    }
    egui::pos2(
        r.left() + px.unwrap_or(r.width() * x),
        r.top() + py.unwrap_or(r.height() * y),
    )
}

/// The box a theme gives the words at `ui`'s cursor when they are a flex
/// item of their own (a list row's title, `span._titleText_`): as wide as
/// the words and their padding and border, the 20 points of their line
/// high, and centred across the row. Moves the cursor past the box's left
/// edge and returns what the words leave of it on their right.
pub fn paint_words_box(
    ui: &mut egui::Ui,
    key: &str,
    words: &str,
    font: egui::FontId,
    color: Color32,
) -> f32 {
    const LINE: f32 = 20.0;
    let Some(look) = anchor_look(key).filter(|l| l.paints_box()) else {
        return 0.0;
    };
    let [top, right, bottom, left] = card_inset(key, &look);
    let at = ui.available_rect_before_wrap();
    let width = ui
        .painter()
        .layout_no_wrap(words.to_owned(), font, color)
        .size()
        .x;
    let height = LINE + top + bottom;
    let r = Rect::from_min_size(
        egui::pos2(at.left(), at.center().y - height / 2.0),
        egui::vec2((width + left + right).min(at.width()), height),
    );
    paint_look(ui, key, &look, r, CornerRadius::ZERO);
    ui.add_space(left);
    right
}

/// The room TIDAL's `span.context-menu` takes above the main container while
/// a menu is open. It is an empty inline box, which takes no line until
/// something gives it a side: padding or a border on its left or right
/// (measured in Edge on the capture, top and bottom edges alone give none). The line is the 20 points of TIDAL's `line-height`, the bar stays
/// where it is, and so a theme's `[class*="menu"] { border-right }` lowers
/// the sidebar and the page for as long as the menu is open.
pub fn context_menu_line() -> f32 {
    const LINE: f32 = 20.0;
    let key = crate::luna::dom_anchors::keys::CONTEXT_MENU_LINE;
    let Some(look) = anchor_look(key) else {
        return 0.0;
    };
    let inset = card_inset(key, &look);
    let inline = host(key, |h, _| {
        Some(
            h.style
                .display
                .as_deref()
                .is_none_or(|d| d.trim() == "inline"),
        )
    });
    if inline == Some(true) && (inset[1] > 0.0 || inset[3] > 0.0) {
        LINE
    } else {
        0.0
    }
}

/// [`paint_anchor_box`] with a look already read.
pub fn paint_look(ui: &egui::Ui, key: &str, look: &AnchorLook, r: Rect, native: CornerRadius) {
    paint_with(ui, key, look, r, native, Color32::TRANSPARENT);
}

/// A native part with its own fill: paints the part in `r` as the theme
/// has its anchor `key` (see [`paint_anchor_box`]), the theme's fill over
/// `native_fill` (which shows where the theme gives none), or, with no
/// theme rule for the anchor, the part's own `native_fill` in `native`
/// corners as before. Returns the look for the part's words and icons.
pub fn paint_part(
    ui: &egui::Ui,
    key: &str,
    r: Rect,
    native: CornerRadius,
    native_fill: Color32,
) -> Option<Arc<AnchorLook>> {
    match anchor_look(key) {
        Some(look) => {
            paint_with(ui, key, &look, r, native, native_fill);
            Some(look)
        }
        None => {
            if native_fill.a() > 0 {
                ui.painter().rect_filled(r, native, native_fill);
            }
            None
        }
    }
}

/// The look draws more than one colour: a shadow, gradients or pictures,
/// `::before` / `::after` boxes, borders or an inset glow.
pub fn paints_beyond_fill(look: &AnchorLook) -> bool {
    look.layers || look.sides.is_some() || look.shadow.is_some() || look.glow.is_some()
}

/// [`paint_look`] for a part that works its fill out itself from the
/// theme's (a hover shade, a disabled fade): the theme's shadow, then
/// `fill`, then its gradients, pictures, borders and glow, in `corner`.
pub fn paint_look_with_fill(
    ui: &egui::Ui,
    key: &str,
    look: &AnchorLook,
    r: Rect,
    corner: CornerRadius,
    fill: Color32,
) {
    let own = AnchorLook {
        fill: None,
        corners: None,
        ..look.clone()
    };
    paint_with(ui, key, &own, r, corner, fill);
}

/// The look of a box a theme puts round a native part's contents, laid out
/// after it is read (the player bar's `._trackContent_` round the title
/// and artists, `._utilityButtons_` round the right-hand buttons: the
/// cotton candy family's light rounded cards). `None` while no theme
/// gives the anchor a box.
pub fn part_card(key: &str) -> Option<Arc<AnchorLook>> {
    anchor_look(key).filter(|l| l.paints_box())
}

/// What lies between a [`part_card`]'s edge and its contents on each side
/// (top, right, bottom, left): its padding, and its border outside that, as
/// CSS boxes have them (Sugarcoat's 3-point border round its bar cards).
pub fn card_inset(key: &str, look: &AnchorLook) -> [f32; 4] {
    let pad = look.padding.unwrap_or([0.0; 4]).map(|p| p.clamp(0.0, 64.0));
    let border = super::outline_sides(key, Color32::TRANSPARENT)
        .map_or([0.0; 4], |(sides, _)| sides.map(|w| w.clamp(0.0, 16.0)));
    std::array::from_fn(|i| pad[i] + border[i])
}

/// Lays the card [`part_card`] read for anchor `key` round `content`
/// (grown by the box's padding and border, [`card_inset`]) into the paint
/// slot `slot`, kept under the contents before they were drawn: its whole
/// look (shadow, fill, gradients and pictures, borders, inset glow) in its
/// corners.
pub fn paint_card(
    ui: &egui::Ui,
    slot: egui::layers::ShapeIdx,
    key: &str,
    look: &AnchorLook,
    content: Rect,
) {
    let [t, right, b, l] = card_inset(key, look);
    let r = Rect::from_min_max(
        content.min - egui::vec2(l, t),
        content.max + egui::vec2(right, b),
    );
    let ctx = ui.ctx();
    let layer = ui.layer_id();
    let scratch = egui::LayerId::new(
        layer.order,
        egui::Id::new("anchor-look-card").with(layer.id).with(key),
    );
    let scratch_ui = egui::Ui::new(
        ctx.clone(),
        scratch.id,
        egui::UiBuilder::new().layer_id(scratch).max_rect(r),
    );
    paint_with(
        &scratch_ui,
        key,
        look,
        r,
        CornerRadius::ZERO,
        Color32::TRANSPARENT,
    );
    let shapes = ctx.graphics_mut(|g| {
        let list = g.entry(scratch);
        let shapes: Vec<egui::Shape> = list.all_entries().map(|c| c.shape.clone()).collect();
        for i in 0..shapes.len() {
            list.reset_shape(egui::layers::ShapeIdx(i));
        }
        shapes
    });
    ui.painter().set(slot, egui::Shape::Vec(shapes));
}

/// The colour of anchor `key`'s words: the look's, on its way there by the
/// anchor's `transition` for `color`, else `native`.
pub fn look_text(key: &str, look: Option<&AnchorLook>, native: Color32) -> Color32 {
    let Some(look) = look else {
        return native;
    };
    let spec = host(key, |h, _| Some(moving(&h.style, "color"))).flatten();
    host_moved(key, "look-color", look.text, spec, |m| &mut m.colors).unwrap_or(native)
}

fn paint_with(
    ui: &egui::Ui,
    key: &str,
    look: &AnchorLook,
    r: Rect,
    native: CornerRadius,
    native_fill: Color32,
) {
    // A box with a `transform` is drawn where the transform puts it, a
    // fraction of a pixel included, its edges shaded where they cross a
    // pixel: Chromium lays it out and moves it as a whole, where it puts
    // an unmoved box's edges on whole pixels (the cotton candy family's
    // `translateY(-1.1em)` song list box: its border from 490.6, not 491).
    if look.translate != egui::Vec2::ZERO {
        let layer = ui.layer_id();
        let start = ui.ctx().graphics_mut(|g| g.entry(layer).next_idx());
        paint_box(ui, key, look, r, native, native_fill);
        ui.ctx().graphics_mut(|g| {
            let list = g.entry(layer);
            for i in start.0..list.next_idx().0 {
                list.mutate_shape(egui::layers::ShapeIdx(i), |c| unsnap(&mut c.shape));
            }
        });
    } else {
        paint_box(ui, key, look, r, native, native_fill);
    }
}

/// Every rectangle in `shape` drawn where it is, not on whole pixels.
fn unsnap(shape: &mut egui::Shape) {
    match shape {
        egui::Shape::Rect(rect) => rect.round_to_pixels = Some(false),
        egui::Shape::Vec(shapes) => shapes.iter_mut().for_each(unsnap),
        _ => {}
    }
}

fn paint_box(
    ui: &egui::Ui,
    key: &str,
    look: &AnchorLook,
    r: Rect,
    native: CornerRadius,
    native_fill: Color32,
) {
    let corner = look.corners_or(native);
    let painter = ui.painter();
    // What lies behind the box, filtered, before anything of the box.
    if let Some(backdrop) = &look.backdrop {
        super::super::backdrop::paint(painter, r, corner, backdrop);
    }
    if let Some(shadow) = look.shadow {
        super::paint_outer_shadow(painter, r, corner, shadow);
    }
    let spec = host(key, |h, _| Some(moving(&h.style, "background-color"))).flatten();
    let fill =
        host_moved(key, "look-fill", look.fill, spec, |m| &mut m.colors).unwrap_or(native_fill);
    if fill.a() > 0 {
        painter.rect_filled(r, corner, fill);
    }
    // Gradients, pictures, `::before` / `::after` boxes and borders (on
    // every side or some).
    if look.layers {
        if look.backdrop.is_some() {
            super::paint_layers_over_backdrop(ui, key, r, Some(corner));
        } else {
            super::paint_host_layers_rounded(ui, key, r, Some(corner));
        }
    }
    if let Some((color, blur, spread)) = look.glow {
        super::paint_inset_glow(painter, r, corner, color, blur, spread);
    }
}

/// A border on the sides that have one, inside `r`.
pub fn paint_sides(painter: &egui::Painter, r: Rect, sides: [f32; 4], color: Color32) {
    let [t, right, b, l] = sides;
    let edges = [
        (
            t,
            Rect::from_min_max(r.min, egui::pos2(r.max.x, r.min.y + t)),
        ),
        (
            right,
            Rect::from_min_max(egui::pos2(r.max.x - right, r.min.y), r.max),
        ),
        (
            b,
            Rect::from_min_max(egui::pos2(r.min.x, r.max.y - b), r.max),
        ),
        (
            l,
            Rect::from_min_max(r.min, egui::pos2(r.min.x + l, r.max.y)),
        ),
    ];
    for (w, edge) in edges {
        if w > 0.0 {
            painter.rect_filled(edge, 0.0, color);
        }
    }
}

/// The fill [`crate::ui::widgets::menu_frame`] gives a popup menu whose
/// box a theme paints beyond one colour (a gradient, a picture, a border,
/// a `::before` box): a colour nothing else uses, which `paint_menu_boxes`
/// replaces at the frame's end with the theme's box, in the menu's own
/// rect. egui's frames take one colour, and every menu shares that frame,
/// so the box is laid in where the frame put it.
pub const MENU_BOX_MARK: Color32 = Color32::from_rgba_premultiplied(60, 20, 80, 100);

/// The fill egui's submenu frames get (`window_fill`) while a theme gives
/// TIDAL's submenu box (`div._subMenu_706d970`) a look: replaced at the
/// frame's end like [`MENU_BOX_MARK`], with the submenu's box.
pub const SUBMENU_BOX_MARK: Color32 = Color32::from_rgba_premultiplied(20, 60, 80, 100);

/// How opaque a fill marked with [`MENU_BOX_MARK`] is (a menu fading in
/// multiplies the mark like every colour in it), `None` for other fills.
pub fn mark_opacity(fill: Color32) -> Option<f32> {
    mark_of(fill, [0.6, 0.2, 0.8])
}

fn mark_of(fill: Color32, shares: [f32; 3]) -> Option<f32> {
    let a = f32::from(fill.a());
    if a < 4.0 {
        return None;
    }
    let near = |c: u8, share: f32| (f32::from(c) - a * share).abs() <= 1.0;
    (near(fill.r(), shares[0]) && near(fill.g(), shares[1]) && near(fill.b(), shares[2]))
        .then_some(a / 100.0)
}

/// Which box a marked fill stands for: `Some((false, opacity))` for a
/// menu's ([`MENU_BOX_MARK`]), `Some((true, opacity))` for a submenu's
/// ([`SUBMENU_BOX_MARK`]).
fn mark_kind(fill: Color32) -> Option<(bool, f32)> {
    mark_opacity(fill)
        .map(|o| (false, o))
        .or_else(|| mark_of(fill, [0.2, 0.6, 0.8]).map(|o| (true, o)))
}

/// The look a theme gives TIDAL's submenu box (`div._subMenu_706d970`,
/// inside a menu row: Cotton Candy Nights' gradient), when it paints one.
pub fn submenu_box_look() -> Option<Arc<AnchorLook>> {
    if !on() {
        return None;
    }
    anchor_look(crate::luna::dom_anchors::keys::THEME_SUBMENU).filter(|l| l.paints_box())
}

/// The looks of TIDAL's menu box under a theme, when they are more than a
/// colour: the popover round the menu and the menu itself
/// (`div._contextMenu_[role=menu]`, whose fill the palette takes already).
/// `None` while the menus keep their one-colour frame.
pub fn menu_box_looks() -> Option<[Option<Arc<AnchorLook>>; 2]> {
    use crate::luna::dom_anchors::keys::{THEME_MENU, THEME_MENU_BOX};
    if !on() {
        return None;
    }
    let outer = anchor_look(THEME_MENU_BOX);
    let menu = anchor_look(THEME_MENU);
    let more = |l: &Option<Arc<AnchorLook>>| {
        l.as_ref()
            .is_some_and(|l| l.layers || l.sides.is_some() || l.glow.is_some())
    };
    (more(&outer) || more(&menu)).then_some([outer, menu])
}

/// Whether the theme gives TIDAL's context menu (`div._contextMenu_
/// [role=menu]`) a fully transparent `background-color` of its own.
pub fn menu_see_through() -> bool {
    host(crate::luna::dom_anchors::keys::THEME_MENU, |h, vars| {
        style_color(&h.style, |s| s.background_color.as_ref(), vars)
    })
    .is_some_and(|c| c.a() == 0)
}

/// Lays the theme's menu box ([`menu_box_looks`]) into every menu frame
/// painted with [`MENU_BOX_MARK`] this frame: the menu's colour
/// (`native_fill`, the palette's), then the popover's and the menu's boxes.
pub fn paint_menu_boxes(ctx: &egui::Context, native_fill: Color32) {
    use crate::luna::dom_anchors::keys::{THEME_MENU, THEME_MENU_BOX, THEME_SUBMENU};
    let menus = menu_box_looks();
    let submenu = submenu_box_look();
    if menus.is_none() && submenu.is_none() {
        return;
    }
    // The marked frames: layer, shape index, rect, clip, corners, opacity
    // and whether it is a submenu's.
    let layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().collect());
    let mut marked = Vec::new();
    ctx.graphics(|g| {
        for layer in &layers {
            let Some(list) = g.get(*layer) else {
                continue;
            };
            for (i, c) in list.all_entries().enumerate() {
                if let Some((rect, corner, (sub, opacity))) = marked_rect(&c.shape) {
                    marked.push((*layer, i, rect, c.clip_rect, corner, opacity, sub));
                }
            }
        }
    });
    for (layer, index, rect, clip, corner, opacity, sub) in marked {
        let scratch = egui::LayerId::new(
            layer.order,
            egui::Id::new("anchor-look-menu").with(layer.id).with(index),
        );
        let mut ui = egui::Ui::new(
            ctx.clone(),
            scratch.id,
            egui::UiBuilder::new().layer_id(scratch).max_rect(rect),
        );
        ui.multiply_opacity(opacity);
        if sub {
            // The submenu's box: its fill (or the menu's colour where it
            // gives none), gradients, borders and glow.
            if let Some(look) = &submenu {
                paint_with(&ui, THEME_SUBMENU, look, rect, corner, native_fill);
            } else {
                ui.painter().rect_filled(rect, corner, native_fill);
            }
        } else {
            // A menu the theme makes see-through (`background-color:
            // transparent`) has nothing of TIDAL's under the theme's box:
            // its `::before` glass lies over the page (Lunallena's half
            // black over the blurred page), not over the menu's colour.
            if !menu_see_through() {
                ui.painter().rect_filled(rect, corner, native_fill);
            }
            let [outer, menu] = menus.clone().unwrap_or_default();
            for (key, look) in [(THEME_MENU_BOX, &outer), (THEME_MENU, &menu)] {
                if let Some(look) = look {
                    // The menu's own fill is the palette's, laid already.
                    let boxed = AnchorLook {
                        fill: if key == THEME_MENU_BOX {
                            look.fill
                        } else {
                            None
                        },
                        ..AnchorLook::clone(look)
                    };
                    paint_with(&ui, key, &boxed, rect, corner, Color32::TRANSPARENT);
                }
            }
        }
        let shapes = ctx.graphics_mut(|g| {
            let list = g.entry(scratch);
            let shapes: Vec<egui::Shape> = list.all_entries().map(|c| c.shape.clone()).collect();
            for i in 0..shapes.len() {
                list.reset_shape(egui::layers::ShapeIdx(i));
            }
            shapes
        });
        ctx.graphics_mut(|g| {
            g.entry(layer)
                .mutate_shape(egui::layers::ShapeIdx(index), |c| {
                    c.shape =
                        replace_mark(std::mem::replace(&mut c.shape, egui::Shape::Noop), &shapes);
                    c.clip_rect = clip;
                });
        });
    }
}

/// The rect and corners of a frame painted with [`MENU_BOX_MARK`] or
/// [`SUBMENU_BOX_MARK`] (alone, or beside its shadow), and which.
fn marked_rect(shape: &egui::Shape) -> Option<(Rect, CornerRadius, (bool, f32))> {
    match shape {
        egui::Shape::Rect(r) => mark_kind(r.fill).map(|kind| (r.rect, r.corner_radius, kind)),
        egui::Shape::Vec(v) => v.iter().find_map(marked_rect),
        _ => None,
    }
}

/// `shape` with its marked rect replaced by `boxed`.
fn replace_mark(shape: egui::Shape, boxed: &[egui::Shape]) -> egui::Shape {
    match shape {
        egui::Shape::Rect(r) if mark_kind(r.fill).is_some() => egui::Shape::Vec(boxed.to_vec()),
        egui::Shape::Vec(v) => {
            egui::Shape::Vec(v.into_iter().map(|s| replace_mark(s, boxed)).collect())
        }
        other => other,
    }
}
