//! CSS filter functions that recolour a picture (`sepia`, `hue-rotate`,
//! `brightness`, `contrast`, `saturate`, `grayscale`, `invert`), as a theme
//! writes them on `img` (the cotton candy family tints a profile picture
//! with `sepia(.95) hue-rotate(256deg)`). A browser runs each function as
//! one pass over the pixels, clamping in between; so does [`ColorFilter::apply`].
//! `blur()` is applied too, to the scaled copy, as a Gaussian of the given
//! radius (transparent outside the picture, as a browser blurs it).
//! `opacity()` is the box's opacity and `drop-shadow()` is a soft shadow
//! behind the box ([`drop_shadows`]), so they are left to their own handling.

use std::fmt::Write as _;

/// One colour function with its amount: percentages are already fractions,
/// angles are in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Brightness(f32),
    Contrast(f32),
    Grayscale(f32),
    Sepia(f32),
    Saturate(f32),
    HueRotate(f32),
    Invert(f32),
    /// A blur with this standard deviation, in the pixels of the picture
    /// it is applied to (CSS points until [`ColorFilter::at_scale`]).
    Blur(f32),
}

/// The colour functions of a `filter` value, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorFilter(Vec<Step>);

impl ColorFilter {
    /// The colour functions and `blur()` in `value`, or `None` when it has
    /// none that change a pixel (`none`, `opacity(.5)`, `blur(0)`,
    /// `brightness(1)`).
    pub fn parse(value: &str) -> Option<Self> {
        let lower = value.to_ascii_lowercase();
        let mut steps = Vec::new();
        let mut rest = lower.trim();
        while let Some(open) = rest.find('(') {
            let name = rest[..open].trim();
            let Some(close) = rest[open..].find(')') else {
                break;
            };
            let arg = rest[open + 1..open + close].trim();
            rest = rest[open + close + 1..].trim_start();
            let step = match name {
                "brightness" => amount(arg).map(|a| Step::Brightness(a.max(0.0))),
                "contrast" => amount(arg).map(|a| Step::Contrast(a.max(0.0))),
                "grayscale" => amount(arg).map(|a| Step::Grayscale(a.clamp(0.0, 1.0))),
                "sepia" => amount(arg).map(|a| Step::Sepia(a.clamp(0.0, 1.0))),
                "saturate" => amount(arg).map(|a| Step::Saturate(a.max(0.0))),
                "invert" => amount(arg).map(|a| Step::Invert(a.clamp(0.0, 1.0))),
                "hue-rotate" => angle(arg).map(Step::HueRotate),
                "blur" => blur_length(arg).map(Step::Blur),
                _ => None,
            };
            // A function that changes nothing is dropped, so the picture is
            // left as it is (and not copied).
            if let Some(step) = step.filter(|s| !s.is_identity()) {
                steps.push(step);
            }
        }
        (!steps.is_empty()).then_some(Self(steps))
    }

    /// The filter as CSS again: it parses back to the same filter, and
    /// names a picture's filtered copy.
    pub fn css(&self) -> String {
        let mut out = String::new();
        for step in &self.0 {
            if !out.is_empty() {
                out.push(' ');
            }
            let _ = match *step {
                Step::Brightness(a) => write!(out, "brightness({a})"),
                Step::Contrast(a) => write!(out, "contrast({a})"),
                Step::Grayscale(a) => write!(out, "grayscale({a})"),
                Step::Sepia(a) => write!(out, "sepia({a})"),
                Step::Saturate(a) => write!(out, "saturate({a})"),
                Step::HueRotate(r) => write!(out, "hue-rotate({r}rad)"),
                Step::Invert(a) => write!(out, "invert({a})"),
                Step::Blur(a) => write!(out, "blur({a}px)"),
            };
        }
        out
    }

    /// The same filter for a picture drawn at `k` pixels per point: a blur
    /// is in the picture's own pixels from here on (to a tenth of one).
    pub fn at_scale(&self, k: f32) -> Option<Self> {
        let steps: Vec<Step> = self
            .0
            .iter()
            .filter_map(|step| match *step {
                Step::Blur(r) => {
                    let r = (r * k * 10.0).round() / 10.0;
                    (r >= 0.3).then_some(Step::Blur(r))
                }
                other => Some(other),
            })
            .collect();
        (!steps.is_empty()).then_some(Self(steps))
    }

    /// What the colour functions make of one opaque colour (a blur leaves a
    /// flat colour as it is).
    pub fn apply_color(&self, rgb: [u8; 3]) -> [u8; 3] {
        let run: Vec<[f32; 12]> = self
            .0
            .iter()
            .filter(|s| !matches!(s, Step::Blur(_)))
            .map(Step::matrix)
            .collect();
        let mut px = [rgb[0], rgb[1], rgb[2], 255];
        recolour(&mut px, &run);
        [px[0], px[1], px[2]]
    }

    /// Whether the filter blurs.
    pub fn blurs(&self) -> bool {
        self.0.iter().any(|s| matches!(s, Step::Blur(_)))
    }

    /// Filters straight (not premultiplied) RGBA pixels of a `width` by
    /// `height` picture, function by function.
    pub fn apply(&self, rgba: &mut [u8], width: usize, height: usize) {
        let mut run: Vec<[f32; 12]> = Vec::new();
        for step in &self.0 {
            match *step {
                Step::Blur(sigma) => {
                    recolour(rgba, &run);
                    run.clear();
                    blur(rgba, width, height, sigma);
                }
                other => run.push(other.matrix()),
            }
        }
        recolour(rgba, &run);
    }
}

/// Runs colour matrices over straight RGBA pixels, clamping in between.
fn recolour(rgba: &mut [u8], matrices: &[[f32; 12]]) {
    if matrices.is_empty() {
        return;
    }
    for px in rgba.as_chunks_mut::<4>().0 {
        let mut c = [
            f32::from(px[0]) / 255.0,
            f32::from(px[1]) / 255.0,
            f32::from(px[2]) / 255.0,
        ];
        for m in matrices {
            c = [
                (m[0] * c[0] + m[1] * c[1] + m[2] * c[2] + m[3]).clamp(0.0, 1.0),
                (m[4] * c[0] + m[5] * c[1] + m[6] * c[2] + m[7]).clamp(0.0, 1.0),
                (m[8] * c[0] + m[9] * c[1] + m[10] * c[2] + m[11]).clamp(0.0, 1.0),
            ];
        }
        for (out, v) in px.iter_mut().zip(c) {
            *out = (v * 255.0).round() as u8;
        }
    }
}

/// A Gaussian blur of standard deviation `sigma` pixels (three box blurs,
/// as browsers do), on premultiplied colour with nothing beyond the edges:
/// a picture's border fades out as a blurred one does.
fn blur(rgba: &mut [u8], width: usize, height: usize, sigma: f32) {
    if sigma < 0.3 || width == 0 || height == 0 || rgba.len() < width * height * 4 {
        return;
    }
    let mut buf: Vec<[f32; 4]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .take(width * height)
        .map(|px| {
            let a = f32::from(px[3]) / 255.0;
            [
                f32::from(px[0]) / 255.0 * a,
                f32::from(px[1]) / 255.0 * a,
                f32::from(px[2]) / 255.0 * a,
                a,
            ]
        })
        .collect();
    // Box sizes whose three passes make the Gaussian.
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as usize;
    if lower.is_multiple_of(2) {
        lower = lower.saturating_sub(1);
    }
    let lower = lower.max(1);
    let upper = lower + 2;
    let (l, ll) = (lower as f32, (lower * lower) as f32);
    let wide = ((12.0 * sigma * sigma - 3.0 * ll - 12.0 * l - 9.0) / (-4.0 * l - 4.0))
        .round()
        .clamp(0.0, 3.0) as usize;
    for pass in 0..3 {
        let size = if pass < wide { lower } else { upper };
        let radius = (size - 1) / 2;
        box_pass(&mut buf, width, height, radius, true);
        box_pass(&mut buf, width, height, radius, false);
    }
    for (px, c) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(buf) {
        let a = c[3].clamp(0.0, 1.0);
        for i in 0..3 {
            let v = if a > 1e-4 { c[i] / a } else { 0.0 };
            px[i] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        px[3] = (a * 255.0).round() as u8;
    }
}

/// One box blur of `radius` along rows or columns, zero beyond the edges.
fn box_pass(buf: &mut [[f32; 4]], width: usize, height: usize, radius: usize, rows: bool) {
    let (len, lines, step, line_step) = if rows {
        (width, height, 1, width)
    } else {
        (height, width, width, 1)
    };
    let size = (2 * radius + 1) as f32;
    let mut line = vec![[0.0f32; 4]; len];
    for l in 0..lines {
        let base = l * line_step;
        for (i, slot) in line.iter_mut().enumerate() {
            *slot = buf[base + i * step];
        }
        let mut sum = [0.0f32; 4];
        for v in line.iter().take(radius + 1) {
            for (s, c) in sum.iter_mut().zip(v) {
                *s += c;
            }
        }
        for i in 0..len {
            buf[base + i * step] = sum.map(|s| s / size);
            if let Some(add) = line.get(i + radius + 1) {
                for (s, c) in sum.iter_mut().zip(add) {
                    *s += c;
                }
            }
            if i >= radius {
                for (s, c) in sum.iter_mut().zip(&line[i - radius]) {
                    *s -= c;
                }
            }
        }
    }
}

/// The arguments of each `drop-shadow(...)` in a `filter` value, as a
/// `box-shadow` would write them (`0 0 .3em rgba(...)`): a soft shadow of
/// the picture's shape behind it.
pub fn drop_shadows(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(at) = rest.to_ascii_lowercase().find("drop-shadow(") {
        let open = at + "drop-shadow(".len();
        let mut depth = 1;
        let mut end = None;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            break;
        };
        out.push(rest[open..end].trim());
        rest = &rest[end + 1..];
    }
    out
}

impl Step {
    fn is_identity(&self) -> bool {
        match *self {
            Step::Brightness(a) | Step::Contrast(a) | Step::Saturate(a) => (a - 1.0).abs() < 1e-4,
            Step::Grayscale(a) | Step::Sepia(a) | Step::Invert(a) => a < 1e-4,
            Step::HueRotate(r) => r.rem_euclid(std::f32::consts::TAU) < 1e-4,
            Step::Blur(r) => r < 0.05,
        }
    }

    /// The step as three rows of `[r, g, b, offset]`, the matrices of the
    /// Filter Effects specification.
    fn matrix(&self) -> [f32; 12] {
        match *self {
            Step::Blur(_) => [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            Step::Brightness(b) => [b, 0.0, 0.0, 0.0, 0.0, b, 0.0, 0.0, 0.0, 0.0, b, 0.0],
            Step::Contrast(c) => {
                let o = -0.5 * c + 0.5;
                [c, 0.0, 0.0, o, 0.0, c, 0.0, o, 0.0, 0.0, c, o]
            }
            Step::Invert(a) => {
                let (s, o) = (1.0 - 2.0 * a, a);
                [s, 0.0, 0.0, o, 0.0, s, 0.0, o, 0.0, 0.0, s, o]
            }
            Step::Grayscale(a) => {
                let k = 1.0 - a;
                [
                    0.2126 + 0.7874 * k,
                    0.7152 - 0.7152 * k,
                    0.0722 - 0.0722 * k,
                    0.0,
                    0.2126 - 0.2126 * k,
                    0.7152 + 0.2848 * k,
                    0.0722 - 0.0722 * k,
                    0.0,
                    0.2126 - 0.2126 * k,
                    0.7152 - 0.7152 * k,
                    0.0722 + 0.9278 * k,
                    0.0,
                ]
            }
            Step::Sepia(a) => {
                let k = 1.0 - a;
                [
                    0.393 + 0.607 * k,
                    0.769 - 0.769 * k,
                    0.189 - 0.189 * k,
                    0.0,
                    0.349 - 0.349 * k,
                    0.686 + 0.314 * k,
                    0.168 - 0.168 * k,
                    0.0,
                    0.272 - 0.272 * k,
                    0.534 - 0.534 * k,
                    0.131 + 0.869 * k,
                    0.0,
                ]
            }
            Step::Saturate(s) => [
                0.213 + 0.787 * s,
                0.715 - 0.715 * s,
                0.072 - 0.072 * s,
                0.0,
                0.213 - 0.213 * s,
                0.715 + 0.285 * s,
                0.072 - 0.072 * s,
                0.0,
                0.213 - 0.213 * s,
                0.715 - 0.715 * s,
                0.072 + 0.928 * s,
                0.0,
            ],
            Step::HueRotate(r) => {
                let (s, c) = r.sin_cos();
                [
                    0.213 + c * 0.787 - s * 0.213,
                    0.715 - c * 0.715 - s * 0.715,
                    0.072 - c * 0.072 + s * 0.928,
                    0.0,
                    0.213 - c * 0.213 + s * 0.143,
                    0.715 + c * 0.285 + s * 0.140,
                    0.072 - c * 0.072 - s * 0.283,
                    0.0,
                    0.213 - c * 0.213 - s * 0.787,
                    0.715 - c * 0.715 + s * 0.715,
                    0.072 + c * 0.928 + s * 0.072,
                    0.0,
                ]
            }
        }
    }
}

/// A number or a percentage as a fraction (`.5`, `50%`).
fn amount(arg: &str) -> Option<f32> {
    let n = match arg.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok()? / 100.0,
        None => arg.parse::<f32>().ok()?,
    };
    n.is_finite().then_some(n)
}

/// A blur radius in points: `4px`, `.2em` and `1rem` (TIDAL's text is
/// 14 points, the root 16), or a bare `0`.
fn blur_length(arg: &str) -> Option<f32> {
    let number = |suffix: &str| arg.strip_suffix(suffix)?.trim().parse::<f32>().ok();
    let r = number("px")
        .or_else(|| number("rem").map(|v| v * 16.0))
        .or_else(|| number("em").map(|v| v * 14.0))
        .or_else(|| arg.parse::<f32>().ok().filter(|v| *v == 0.0))?;
    (r.is_finite() && r >= 0.0).then_some(r)
}

/// An angle in radians (`256deg`, `.5turn`, `1rad`, `100grad`; a bare `0`).
fn angle(arg: &str) -> Option<f32> {
    let number = |suffix: &str| arg.strip_suffix(suffix)?.trim().parse::<f32>().ok();
    let r = if let Some(v) = number("deg") {
        v.to_radians()
    } else if let Some(v) = number("grad") {
        v * std::f32::consts::PI / 200.0
    } else if let Some(v) = number("rad") {
        v
    } else if let Some(v) = number("turn") {
        v * std::f32::consts::TAU
    } else {
        arg.parse::<f32>().ok().filter(|v| *v == 0.0)?
    };
    r.is_finite().then_some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(css: &str, px: [u8; 3]) -> [u8; 3] {
        let mut data = [px[0], px[1], px[2], 255];
        ColorFilter::parse(css)
            .expect("a colour filter")
            .apply(&mut data, 1, 1);
        [data[0], data[1], data[2]]
    }

    #[test]
    fn filters_that_change_no_pixel_are_none() {
        for css in [
            "none",
            "opacity(50%)",
            "blur(0)",
            "blur(0px)",
            "brightness(1)",
            "grayscale(0%) opacity(50%)",
            "hue-rotate(0deg)",
            "hue-rotate(360deg)",
            "",
        ] {
            assert_eq!(ColorFilter::parse(css), None, "{css}");
        }
    }

    #[test]
    fn grayscale_and_invert_follow_the_specification() {
        // Full grayscale of pure red is its luma, 0.2126.
        assert_eq!(run("grayscale(1)", [255, 0, 0]), [54, 54, 54]);
        assert_eq!(run("invert(100%)", [10, 200, 255]), [245, 55, 0]);
        // Half way keeps half of the colour.
        let half = run("grayscale(.5)", [255, 0, 0]);
        assert!(half[0] > half[1] && half[1] > 0, "{half:?}");
    }

    #[test]
    fn brightness_contrast_and_saturate_clamp_between_functions() {
        assert_eq!(run("brightness(.5)", [200, 100, 50]), [100, 50, 25]);
        // brightness(2) clips white at 1 before contrast(.5) reads it.
        assert_eq!(
            run("brightness(2) contrast(.5)", [200, 0, 0]),
            [191, 64, 64]
        );
        // saturate(0) is the grayscale of the luma weights.
        let gray = run("saturate(0)", [255, 0, 0]);
        assert_eq!(gray[0], gray[1]);
        assert_eq!(gray[1], gray[2]);
    }

    #[test]
    fn sepia_and_hue_rotate_recolour() {
        let sepia = run("sepia(1)", [128, 128, 128]);
        assert!(sepia[0] > sepia[1] && sepia[1] > sepia[2], "{sepia:?}");
        // A half turn moves red to a cyan-ish colour.
        let turned = run("hue-rotate(180deg)", [255, 0, 0]);
        assert!(turned[0] < turned[2], "{turned:?}");
        // The turn and degree units agree.
        assert_eq!(run("hue-rotate(.5turn)", [255, 0, 0]), turned);
    }

    #[test]
    fn the_css_form_parses_back_to_the_same_filter() {
        let filter = ColorFilter::parse(
            "sepia(.95) hue-rotate(256deg) contrast(.9) brightness(1) saturate(2.4) opacity(.8)",
        )
        .expect("a filter");
        // Functions that change nothing or are not recolouring are not kept.
        assert_eq!(filter.css().matches('(').count(), 4);
        assert_eq!(ColorFilter::parse(&filter.css()), Some(filter));
    }

    #[test]
    fn a_blur_softens_an_edge_and_fades_the_border() {
        // Left half black, right half white, 16 by 4, fully opaque.
        let (w, h) = (16usize, 4usize);
        let mut data = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let v = if x < w / 2 { 0 } else { 255 };
                data.extend([v, v, v, 255]);
            }
        }
        let filter = ColorFilter::parse("blur(2px)").expect("a blur is a filter");
        filter.apply(&mut data, w, h);
        let px = |x: usize, y: usize| data[(y * w + x) * 4];
        assert!(
            px(7, 2) > 20 && px(7, 2) < 140,
            "the edge is soft: {}",
            px(7, 2)
        );
        assert!(px(8, 2) > 110 && px(8, 2) < 235, "{}", px(8, 2));
        assert!(
            px(0, 2) < 20 && px(15, 2) > 200,
            "far from the edge it holds"
        );
        let alpha = |x: usize, y: usize| data[(y * w + x) * 4 + 3];
        assert!(
            alpha(0, 0) < 255,
            "the corner fades into nothing: {}",
            alpha(0, 0)
        );
        assert_eq!(
            ColorFilter::parse("blur(4px)").expect("filter").css(),
            "blur(4px)"
        );
    }

    #[test]
    fn a_blur_is_in_points_until_the_picture_is_drawn_at_a_scale() {
        let filter = ColorFilter::parse("sepia(1) blur(.5em)").expect("filter");
        assert!(filter.blurs());
        // .5em of 14 points = 7 points; at 2 pixels per point 14 pixels.
        assert_eq!(
            filter.at_scale(2.0).expect("filter").css(),
            "sepia(1) blur(14px)"
        );
        // Too small to see at this scale: dropped, the colour stays.
        assert_eq!(filter.at_scale(0.01).expect("filter").css(), "sepia(1)");
        // Nothing left of a lone small blur.
        let lone = ColorFilter::parse("blur(1px)").expect("filter");
        assert_eq!(lone.at_scale(0.01), None);
        assert!(!ColorFilter::parse("sepia(1)").expect("filter").blurs());
    }

    #[test]
    fn drop_shadows_are_found_with_their_arguments() {
        assert_eq!(
            drop_shadows(
                "sepia(1) drop-shadow(0px 0px .3em rgba(183, 178, 255, .4)) blur(2px) DROP-SHADOW(0 2px 4px #000)"
            ),
            vec!["0px 0px .3em rgba(183, 178, 255, .4)", "0 2px 4px #000"]
        );
        assert!(drop_shadows("blur(2px)").is_empty());
        assert!(drop_shadows("drop-shadow(0 0 4px red").is_empty());
    }
}
