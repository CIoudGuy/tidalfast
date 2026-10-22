//! TIDAL's audio quality pill at the player bar's far right.
//!
//! Measured from listen.tidal.com (`._badge`): 40 high, at least 121 wide,
//! fully rounded, 16 either side of the text, a 1.5 px black border. Inside:
//! `#0d1010` with a grey radial glow from the top-left corner, an ellipse of
//! the tier's colour at 20 % rising from the bottom centre to 60 %, a 4 px
//! bar of that colour at 28 % blurred 5.5 px along the top edge, and the text
//! (12/18, teal `#16d8a7` for lossless, gold for Max, white for lossy) with a
//! 10 px glow of its colour at 70 %. Outside: a white 7 % line 1 px under the
//! bottom edge and a white 8 % shadow (0 1.29 5.16). Hovering lays the
//! tier's colour at 10 % over it in 0.2 s.
//!
//! The layered gradients are baked once per size and tier into one small
//! texture at the display's pixel density; the hover fill and the outer
//! shadows are ordinary shapes.

use egui::{Color32, CornerRadius, Rect, Sense, Vec2, vec2};

use crate::theme;

/// The pill's colour family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PillTone {
    /// Hi-res lossless: TIDAL's gold.
    Max,
    /// Lossless: TIDAL's teal.
    Lossless,
    /// Lossy (320 or 96 kbps): white text on the grey glow.
    Lossy,
}

/// TIDAL's `--market-core-teal-text-color`.
pub(crate) const TEAL: Color32 = theme::TEAL;
/// TIDAL's `--market-core-yellow-text-color`.
pub(crate) const GOLD: Color32 = Color32::from_rgb(0xff, 0xd4, 0x32);
/// The grey TIDAL uses for the lossy pill's glow and hover.
const GREY: Color32 = Color32::from_rgb(205, 205, 205);

pub(crate) const HEIGHT: f32 = 40.0;
pub(crate) const MIN_WIDTH: f32 = 121.0;
/// The widest the pill is drawn, whatever a plugin asks for: it is baked
/// into a texture, and a bad size must not be able to exceed the GPU's.
const MAX_WIDTH: f32 = 400.0;
const PADDING: f32 = 16.0;
const BORDER: f32 = 1.5;

impl PillTone {
    /// The text's colour.
    pub(crate) fn text(self) -> Color32 {
        match self {
            Self::Max => GOLD,
            Self::Lossless => TEAL,
            Self::Lossy => Color32::WHITE,
        }
    }

    /// The colour of the glow, the bottom ellipse and the hover fill.
    fn glow(self) -> Color32 {
        match self {
            Self::Max => GOLD,
            Self::Lossless => TEAL,
            Self::Lossy => GREY,
        }
    }
}

/// What Luna plugins did to the pill (AltPlay marks it with Jellyfin's
/// icon and writes its own text in place of the label).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Parts {
    /// Elements plugins put inside the badge or its text: they are drawn
    /// around the label.
    pub inside: bool,
    /// The label is hidden (`display: none` on its span).
    pub hide_label: bool,
    /// The width a plugin set on the badge, in points.
    pub width: Option<f32>,
}

impl Parts {
    pub(crate) fn of_plugins() -> Self {
        use crate::luna::dom_anchors::keys;
        if !super::plugin_dom::on() {
            return Self::default();
        }
        Self {
            inside: super::plugin_slots::any(keys::QUALITY_BADGE)
                || super::plugin_slots::any(keys::QUALITY_TEXT),
            hide_label: super::plugin_dom::hidden(keys::QUALITY_LABEL),
            width: super::plugin_dom::width(keys::QUALITY_BADGE),
        }
    }
}

/// The pill reading `label`, a clickable control named `label` for screen
/// readers; `detail` is its tooltip. `native` is what TIDAL itself writes in
/// it: the pill is as wide as that needs (121 at least), and a longer
/// `label` a Luna plugin wrote in its place (Tidal Tags' "16-bit 44.1kHz
/// 951kb/s") wraps inside it onto centred 18-point lines, as in TIDAL.
/// `color` replaces the tier's text colour (a Luna plugin's `color` on the
/// badge's text); `themed` replaces TIDAL's layers with a theme's fills and
/// corners.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &mut egui::Ui,
    label: &str,
    native: &str,
    detail: &str,
    tone: PillTone,
    color: Option<Color32>,
    themed: Option<super::plugin_dom::ThemedBadge>,
    parts: Parts,
) -> egui::Response {
    let text_color = color.unwrap_or(tone.text());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::regular(12.0), text_color);
    // TIDAL's badge keeps the size its own text gives it: a plugin's longer
    // text wraps (`a._badge` is 121 wide, `padding: 0 16px`). Elements a
    // plugin put inside, or a width it set, size the pill as before.
    let galley = if parts.inside || parts.width.is_some() || parts.hide_label {
        galley
    } else {
        let native_width = if native == label {
            galley.size().x
        } else {
            ui.painter()
                .layout_no_wrap(native.to_owned(), theme::regular(12.0), text_color)
                .size()
                .x
        };
        let room = native_width.max(MIN_WIDTH - 2.0 * PADDING);
        if galley.size().x > room + 0.5 {
            wrapped(ui, label, room, text_color)
        } else {
            galley
        }
    };
    // What plugins put inside took this wide last frame (it is laid out
    // after the pill is sized).
    let inside_id = ui.id().with("pill-inside");
    let inside: f32 = if parts.inside {
        ui.data(|d| d.get_temp(inside_id)).unwrap_or(0.0)
    } else {
        0.0
    };
    let label_width = if parts.hide_label {
        0.0
    } else {
        galley.size().x
    };
    let width = match parts.width {
        // A plugin sized the badge (AltPlay fits its mark and text).
        Some(width) => width.clamp(1.0, MAX_WIDTH),
        None => (label_width + inside + 2.0 * PADDING).clamp(MIN_WIDTH, MAX_WIDTH),
    };
    let size = vec2(width.round(), HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if let Some(themed) = themed.filter(|_| ui.is_rect_visible(rect)) {
        // A theme's badge: its own fill and the tier layer over it, with
        // its corners, in place of TIDAL's gradients and shadows.
        let r = themed
            .radius
            .unwrap_or(HEIGHT / 2.0)
            .clamp(0.0, HEIGHT / 2.0);
        let radius = CornerRadius::same(r.round() as u8);
        let painter = ui.painter();
        for fill in [themed.fill, themed.layer].into_iter().flatten() {
            painter.rect_filled(rect, radius, fill);
        }
        label_and_parts(ui, rect, galley, text_color, parts, inside_id);
    } else if ui.is_rect_visible(rect) {
        let radius = CornerRadius::same((HEIGHT / 2.0) as u8);
        let painter = ui.painter();
        // `box-shadow: 0 1.29px 5.16px rgba(255,255,255,.08)`.
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 1],
                blur: 5,
                spread: 0,
                color: Color32::from_white_alpha(20),
            }
            .as_shape(rect, radius),
        );
        // `box-shadow: 0 1px 0 rgba(255,255,255,.07)`: a sliver under the
        // bottom edge.
        painter.rect_filled(
            rect.translate(vec2(0.0, 1.0)),
            radius,
            Color32::from_white_alpha(18),
        );
        let ppp = ui.ctx().pixels_per_point();
        let texture = baked(ui.ctx(), rect.size(), ppp, tone, galley.size());
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        // `::before`: the tier's colour at 10 % under the pointer, `.2s`.
        let hover = crate::motion::progress(
            ui.ctx(),
            response.id.with("hover"),
            response.hovered() || response.has_focus(),
            crate::motion::tidal::HOVER,
        );
        if hover > 0.0 {
            painter.rect_filled(
                rect.shrink(BORDER),
                radius,
                tone.glow().gamma_multiply(0.1 * hover),
            );
        }
        label_and_parts(ui, rect, galley, text_color, parts, inside_id);
    }
    response.on_hover_text(detail)
}

/// `label` wrapped at `width` points into centred lines 18 points apart
/// (TIDAL's `12px/18px`). The rows sit either side of x = 0: draw it at
/// [`centred`].
fn wrapped(ui: &egui::Ui, label: &str, width: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        label.to_owned(),
        egui::TextFormat {
            font_id: theme::regular(12.0),
            color,
            line_height: Some(18.0),
            ..Default::default()
        },
    );
    job.wrap.max_width = width;
    job.halign = egui::Align::Center;
    ui.painter().layout_job(job)
}

/// Where to draw `galley` for it to sit in the middle of `rect`.
fn centred(rect: Rect, galley: &egui::Galley) -> egui::Pos2 {
    rect.center() - galley.rect.center().to_vec2()
}

fn label_id() -> egui::Id {
    egui::Id::new("quality-pill-label")
}

fn report_label(ui: &egui::Ui, rect: Rect) {
    ui.data_mut(|d| d.insert_temp(label_id(), rect));
}

/// Where the pill's label was drawn last (its own rect, as the label span's
/// `offsetWidth` to plugins; a plugin that sizes the badge from it must not
/// read the pill's width back).
pub(crate) fn label_rect(ui: &egui::Ui) -> Option<Rect> {
    ui.data(|d| d.get_temp(label_id()))
}

/// The label centred in the pill; with plugin elements inside, the run of
/// them, the label and the ones after it, centred together.
fn label_and_parts(
    ui: &mut egui::Ui,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    text_color: Color32,
    parts: Parts,
    inside_id: egui::Id,
) {
    use crate::luna::dom::Position;
    use crate::luna::dom_anchors::keys;
    if !parts.inside {
        let size = if parts.hide_label {
            Vec2::ZERO
        } else {
            galley.size()
        };
        report_label(ui, Rect::from_center_size(rect.center(), size));
        if !parts.hide_label {
            ui.painter()
                .galley(centred(rect, &galley), galley, text_color);
        }
        return;
    }
    let last: f32 = ui.data(|d| d.get_temp(inside_id)).unwrap_or(0.0);
    let label_width = if parts.hide_label {
        0.0
    } else {
        galley.size().x
    };
    let whole = label_width + last;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 0.0;
    child.add_space(((rect.width() - whole) / 2.0).max(0.0));
    let start = child.cursor().left();
    super::plugin_slots::inline_measured(&mut child, keys::QUALITY_BADGE, Position::Prepend);
    super::plugin_slots::inline_measured(&mut child, keys::QUALITY_TEXT, Position::Prepend);
    if !parts.hide_label {
        let (label_rect, _) = child.allocate_exact_size(galley.size(), Sense::hover());
        report_label(ui, label_rect);
        child.painter().galley(label_rect.min, galley, text_color);
    } else {
        report_label(
            ui,
            Rect::from_center_size(child.cursor().left_center(), Vec2::ZERO),
        );
    }
    super::plugin_slots::inline_measured(&mut child, keys::QUALITY_TEXT, Position::Append);
    super::plugin_slots::inline_measured(&mut child, keys::QUALITY_BADGE, Position::Append);
    let used = child.cursor().left() - start - label_width;
    if (used - last).abs() > 0.5 {
        ui.data_mut(|d| d.insert_temp(inside_id, used.max(0.0)));
        ui.ctx().request_repaint();
    }
}

/// The pill's layers at `size` points for `tone`, cached by size, density,
/// tone and the text's size.
fn baked(
    ctx: &egui::Context,
    size: Vec2,
    ppp: f32,
    tone: PillTone,
    text: Vec2,
) -> egui::TextureHandle {
    let w = (size.x * ppp).round().max(1.0) as usize;
    let h = (size.y * ppp).round().max(1.0) as usize;
    let text_px = (text.x * ppp).round() as usize;
    // One line of 12 points, or a wrapped run of 18-point lines.
    let rows = ((text.y / 18.0).round() as usize).max(1);
    let id = egui::Id::new(("quality-pill-texture", w, h, tone, text_px, rows));
    if let Some(texture) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    let image = render(w, h, ppp, tone, text.x * ppp, rows);
    let texture = ctx.load_texture("quality-pill", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    texture
}

/// Paints the pill's inside, `w` x `h` pixels at `ppp` pixels a point, the
/// text `text_px` wide in `rows` lines: what the stylesheet layers make,
/// outside the rounded edge transparent.
pub(crate) fn render(
    w: usize,
    h: usize,
    ppp: f32,
    tone: PillTone,
    text_px: f32,
    rows: usize,
) -> egui::ColorImage {
    let (wf, hf) = (w as f32, h as f32);
    let radius = hf / 2.0;
    let border = BORDER * ppp;
    let glow = tone.glow();
    let text = tone.text();
    // The 4 px bar along the top, 13.5 in from each side, blurred 5.5 px.
    let bar = (13.5 * ppp, wf - 13.5 * ppp, 0.5 * ppp, 4.5 * ppp);
    let bar_sigma = 5.5 * ppp;
    // The text's glow: its box blurred by 10 px (sigma 5); thin 12-point
    // glyphs cover about a fifth of their box.
    let half = (6.0 + 9.0 * rows.saturating_sub(1) as f32) * ppp;
    let text_box = (
        (wf - text_px) / 2.0,
        (wf + text_px) / 2.0,
        hf / 2.0 - half,
        hf / 2.0 + half,
    );
    let text_sigma = 5.0 * ppp;
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let coverage = rounded_coverage(px, py, wf, hf, radius);
            if coverage <= 0.0 {
                pixels.push(Color32::TRANSPARENT);
                continue;
            }
            // `linear-gradient(90deg, #0d1010, #0d1010)`.
            let mut c = [13.0_f32, 16.0, 16.0];
            // `radial-gradient(at 0 0, ...)`: farthest-corner ellipse.
            let t = ((px / (wf * std::f32::consts::SQRT_2)).powi(2)
                + (py / (hf * std::f32::consts::SQRT_2)).powi(2))
            .sqrt();
            let (grey, alpha) = corner_glow(t);
            over(&mut c, [grey; 3], alpha);
            // `radial-gradient(ellipse at center bottom, colour .2, transparent 60%)`.
            let t = (((px - wf / 2.0) / (wf / 2.0 * std::f32::consts::SQRT_2)).powi(2)
                + ((hf - py) / (hf * std::f32::consts::SQRT_2)).powi(2))
            .sqrt();
            if t < 0.6 {
                over(&mut c, rgb(glow), 0.2 * (1.0 - t / 0.6));
            }
            // The blurred bar along the top edge.
            let a = 0.28 * blurred_box(px, py, bar, bar_sigma);
            over(&mut c, rgb(glow), a);
            // The text's glow.
            let a = 0.7 * 0.2 * blurred_box(px, py, text_box, text_sigma);
            over(&mut c, rgb(text), a);
            // The black border, inside the edge.
            let inner = rounded_coverage(
                px - border,
                py - border,
                wf - 2.0 * border,
                hf - 2.0 * border,
                radius - border,
            );
            over(&mut c, [0.0; 3], 1.0 - inner);
            let [r, g, b] = c.map(|v| v.round().clamp(0.0, 255.0) as u8);
            pixels.push(Color32::from_rgba_unmultiplied(
                r,
                g,
                b,
                (coverage * 255.0).round() as u8,
            ));
        }
    }
    egui::ColorImage::new([w, h], pixels)
}

/// The top-left radial's stops: grey 120 at 20 %, 90 at 15 % (12 %), 60 at
/// 10 % (24 %), 30 at 5 % (36 %), nothing from 48 %.
fn corner_glow(t: f32) -> (f32, f32) {
    const STOPS: [(f32, f32, f32); 5] = [
        (0.0, 120.0, 0.2),
        (0.12, 90.0, 0.15),
        (0.24, 60.0, 0.1),
        (0.36, 30.0, 0.05),
        (0.48, 0.0, 0.0),
    ];
    if t >= 0.48 {
        return (0.0, 0.0);
    }
    for pair in STOPS.windows(2) {
        let ((t0, g0, a0), (t1, g1, a1)) = (pair[0], pair[1]);
        if t <= t1 {
            let f = (t - t0) / (t1 - t0);
            // Browsers interpolate premultiplied colours.
            let alpha = a0 + (a1 - a0) * f;
            let premultiplied = g0 * a0 + (g1 * a1 - g0 * a0) * f;
            return (premultiplied / alpha.max(1e-6), alpha);
        }
    }
    (0.0, 0.0)
}

fn rgb(color: Color32) -> [f32; 3] {
    [color.r(), color.g(), color.b()].map(f32::from)
}

/// `top` at `alpha` over `base`.
fn over(base: &mut [f32; 3], top: [f32; 3], alpha: f32) {
    let alpha = alpha.clamp(0.0, 1.0);
    for (b, t) in base.iter_mut().zip(top) {
        *b += (t - *b) * alpha;
    }
}

/// How much of the pixel at (`x`, `y`) lies inside a `w` x `h` rectangle at
/// the origin with corners of `radius`, antialiased over one pixel.
fn rounded_coverage(x: f32, y: f32, w: f32, h: f32, radius: f32) -> f32 {
    if w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    let radius = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let qx = (x - w / 2.0).abs() - (w / 2.0 - radius);
    let qy = (y - h / 2.0).abs() - (h / 2.0 - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let distance = outside + qx.max(qy).min(0.0) - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

/// A box (`left`, `right`, `top`, `bottom`) blurred by a Gaussian of
/// `sigma`, at (`x`, `y`): 1 deep inside, fading out past its edges.
fn blurred_box(
    x: f32,
    y: f32,
    (left, right, top, bottom): (f32, f32, f32, f32),
    sigma: f32,
) -> f32 {
    let along = |v: f32, a: f32, b: f32| {
        let k = 1.0 / (sigma * std::f32::consts::SQRT_2);
        0.5 * (erf((b - v) * k) - erf((a - v) * k))
    };
    along(x, left, right) * along(y, top, bottom)
}

/// Abramowitz and Stegun 7.1.26, good to 1.5e-7.
fn erf(x: f32) -> f32 {
    let sign = x.signum();
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_4 * t - 1.453_152_1) * t) + 1.421_413_8) * t - 0.284_496_74) * t
            + 0.254_829_6)
            * t
            * (-x * x).exp();
    sign * y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(image: &egui::ColorImage, x: usize, y: usize) -> Color32 {
        image.pixels[y * image.size[0] + x]
    }

    #[test]
    fn the_inside_is_tidals_near_black_with_a_black_rim() {
        let image = render(121, 40, 1.0, PillTone::Lossless, 80.0, 1);
        // Outside the rounded corner: nothing.
        assert_eq!(at(&image, 0, 0).a(), 0);
        // The middle of the left edge is the black border.
        let rim = at(&image, 0, 20);
        assert!(rim.r() < 8 && rim.g() < 8, "{rim:?}");
        // Halfway up the middle, beside the text: dark, a little teal.
        let middle = at(&image, 8 + 2, 24);
        assert!(middle.r() < 40 && middle.g() > middle.r(), "{middle:?}");
    }

    #[test]
    fn the_bottom_centre_takes_the_tiers_colour() {
        let teal = render(121, 40, 1.0, PillTone::Lossless, 0.0, 1);
        let gold = render(121, 40, 1.0, PillTone::Max, 0.0, 1);
        let grey = render(121, 40, 1.0, PillTone::Lossy, 0.0, 1);
        let (t, g, l) = (at(&teal, 60, 36), at(&gold, 60, 36), at(&grey, 60, 36));
        assert!(t.g() > t.r() + 15, "teal {t:?}");
        assert!(g.r() > g.b() + 15, "gold {g:?}");
        assert!(
            (i32::from(l.r()) - i32::from(l.b())).abs() < 6,
            "grey {l:?}"
        );
        // The glow is faint: TIDAL's 20 % at the very edge.
        assert!(t.g() < 70);
    }

    #[test]
    fn corner_glow_follows_the_stops() {
        assert_eq!(corner_glow(0.0), (120.0, 0.2));
        assert_eq!(corner_glow(0.6), (0.0, 0.0));
        let (_, alpha) = corner_glow(0.18);
        assert!((alpha - 0.125).abs() < 1e-4);
    }

    #[test]
    fn erf_is_close() {
        assert!((erf(0.5) - 0.520_499_9).abs() < 1e-5);
        assert!((erf(-1.0) + 0.842_700_8).abs() < 1e-5);
    }
}
