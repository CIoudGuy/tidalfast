//! Plugin slots: the elements Luna plugins insert into the DOM bridge
//! ([`crate::luna::dom::Injected`]), drawn natively next to the widget
//! they were inserted at.
//!
//! The subset drawn: text, `span`/`div`/`p`/`a`/`button`/`label`/headings/
//! lists, `img` (data:, http(s) and SVG markup), `svg`, `input`
//! (text, range, checkbox), `select`, with a simple flex row/column layout
//! and the resolved styles in [`crate::luna::dom::Style`]. Input on them goes
//! back to the plugins' listeners with [`super::plugin_dom::send`].

use std::collections::BTreeMap;

use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Layout, Margin, Pos2, Rect, RichText, Sense,
    Stroke, UiBuilder, Vec2, vec2,
};
use serde_json::json;

use crate::luna::dom::{DomTarget, Position, Style, VNode};

use super::css_motion;
use super::plugin_dom;

/// CSS value parsing for the native renderer.
pub(crate) mod css {
    use std::collections::BTreeMap;

    use egui::Color32;

    /// Substitute `var(--x, fallback)` from `vars` (nested, a few levels).
    pub fn resolve_vars(value: &str, vars: &BTreeMap<String, String>) -> String {
        let mut out = value.to_string();
        for _ in 0..8 {
            let Some(start) = out.find("var(") else {
                break;
            };
            let Some(end) = matching_paren(&out, start + 3) else {
                break;
            };
            let inner = &out[start + 4..end];
            let (name, fallback) = match split_top(inner, ',').as_slice() {
                [name] => (name.trim().to_string(), None),
                [name, rest @ ..] => (name.trim().to_string(), Some(rest.join(","))),
                [] => (String::new(), None),
            };
            let replacement = vars
                .get(&name)
                .cloned()
                .or(fallback.map(|f| f.trim().to_string()))
                .unwrap_or_default();
            out.replace_range(start..=end, &replacement);
        }
        out
    }

    /// Index of the `)` closing the `(` at `open`.
    fn matching_paren(s: &str, open: usize) -> Option<usize> {
        let mut depth = 0;
        for (i, c) in s.char_indices().skip_while(|(i, _)| *i < open) {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Split on `sep` outside parentheses.
    pub fn split_top(s: &str, sep: char) -> Vec<&str> {
        let mut parts = Vec::new();
        let (mut depth, mut start) = (0i32, 0);
        for (i, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                c if c == sep && depth == 0 => {
                    parts.push(&s[start..i]);
                    start = i + c.len_utf8();
                }
                _ => {}
            }
        }
        parts.push(&s[start..]);
        parts
    }

    /// Whether a picture's address from plugin or theme CSS may be loaded:
    /// `https://`, `http://` and `data:` only. Never a local or network
    /// path (`file:`, `//host/share`, `\\host\share`, `C:\...`): opening a
    /// UNC path makes Windows sign in to that server with the user's
    /// credentials.
    pub fn safe_uri(uri: &str) -> bool {
        let u = uri.trim();
        let lower = u.to_ascii_lowercase();
        if lower.starts_with("data:") {
            return true;
        }
        let rest = lower
            .strip_prefix("https://")
            .or_else(|| lower.strip_prefix("http://"));
        rest.is_some_and(|r| !r.is_empty() && !r.starts_with(['/', '\\']) && !r.contains('\\'))
    }

    /// Split on whitespace outside parentheses.
    pub fn words(s: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let (mut depth, mut start) = (0i32, None);
        for (i, c) in s.char_indices() {
            match c {
                '(' => {
                    depth += 1;
                    start.get_or_insert(i);
                }
                ')' => {
                    depth -= 1;
                    // A function's word ends at its `)`, as CSS tokenises
                    // it (`rgb(0, 0, 0)87%` is a colour and a percentage).
                    if depth == 0
                        && let Some(s0) = start.take()
                    {
                        parts.push(&s[s0..=i]);
                    }
                }
                c if c.is_whitespace() && depth == 0 => {
                    if let Some(s0) = start.take() {
                        parts.push(&s[s0..i]);
                    }
                }
                _ => {
                    start.get_or_insert(i);
                }
            }
        }
        if let Some(s0) = start {
            parts.push(&s[s0..]);
        }
        parts
    }

    fn strip_important(value: &str) -> &str {
        value.trim().trim_end_matches("!important").trim()
    }

    /// A CSS colour: hex, `rgb[a]()`, `hsl[a]()`, `color-mix(in srgb, ...)`,
    /// named colours and `transparent`. `var()` must be resolved first.
    pub fn color(value: &str) -> Option<Color32> {
        let v = strip_important(value);
        let lower = v.to_ascii_lowercase();
        let v = lower.as_str();
        if let Some(hex) = v.strip_prefix('#') {
            return hex_color(hex);
        }
        let relative = ["rgba", "rgb", "hsla", "hsl"]
            .iter()
            .any(|f| func(v, f).is_some_and(|args| args.trim_start().starts_with("from ")));
        if relative {
            return modern(v);
        }
        if let Some(args) = func(v, "rgba").or_else(|| func(v, "rgb")) {
            let n = numbers(args);
            let ch = |i: usize| -> Option<f32> {
                let (x, pct) = n.get(i)?;
                Some(if *pct { x * 2.55 } else { *x })
            };
            let a = alpha(n.get(3));
            return Some(Color32::from_rgba_unmultiplied(
                ch(0)?.round().clamp(0.0, 255.0) as u8,
                ch(1)?.round().clamp(0.0, 255.0) as u8,
                ch(2)?.round().clamp(0.0, 255.0) as u8,
                a,
            ));
        }
        if let Some(args) = func(v, "hsla").or_else(|| func(v, "hsl")) {
            let n = numbers(args);
            let h = n.first()?.0;
            let s = n.get(1)?.0 / 100.0;
            let l = n.get(2)?.0 / 100.0;
            let [r, g, b] = hsl_to_rgb(h, s, l);
            return Some(Color32::from_rgba_unmultiplied(r, g, b, alpha(n.get(3))));
        }
        if let Some(args) = func(v, "color-mix") {
            return color_mix(args);
        }
        named(v).or_else(|| modern(v))
    }

    /// The rest of CSS Color 4 and 5 that Chromium draws, read by
    /// `csscolorparser`: all 148 named colours, `hwb()`, `lab()`, `lch()`,
    /// `oklab()`, `oklch()`, `color(srgb ...)` and the other predefined
    /// spaces, and relative colours whose channels are names or plain
    /// numbers (`rgb(from red r g b / 0.5)`; the crate does not read a
    /// percentage there yet). A name is looked up in the crate's table and
    /// only those functions are passed on: the crate also reads a bare hex
    /// word (`fade`) and `hsv()`, which CSS does not.
    fn modern(v: &str) -> Option<Color32> {
        if v.bytes().all(|b| b.is_ascii_alphabetic()) {
            let [r, g, b] = *csscolorparser::NAMED_COLORS.get(v.into())?;
            return Some(Color32::from_rgb(r, g, b));
        }
        let function = ["hwb", "hwba", "lab", "lch", "oklab", "oklch", "color"]
            .iter()
            .chain(["rgb", "rgba", "hsl", "hsla"].iter())
            .any(|f| func(v, f).is_some());
        if !function {
            return None;
        }
        let [r, g, b, a] = csscolorparser::parse(v).ok()?.clamp().to_rgba8();
        Some(Color32::from_rgba_unmultiplied(r, g, b, a))
    }

    pub fn func<'a>(v: &'a str, name: &str) -> Option<&'a str> {
        let rest = v.strip_prefix(name)?.trim_start();
        if !rest.starts_with('(') {
            return None;
        }
        match matching_paren(rest, 0) {
            Some(end) => Some(&rest[1..end]),
            None => Some(&rest[1..]),
        }
    }

    /// Numbers of a colour function, with whether each was a percentage.
    /// A `calc()` argument is its value (the cotton candy family's
    /// `rgb(47, 10, 61, calc(.1 * .7))`: an alpha of .07, not opaque).
    fn numbers(args: &str) -> Vec<(f32, bool)> {
        let mut args = args.to_string();
        while let Some(at) = args.find("calc(") {
            let Some(end) = matching_paren(&args, at + 4) else {
                break;
            };
            let value = length(&args[at..=end], 14.0, 100.0, [0.0, 0.0])
                .map_or_else(String::new, |n| n.to_string());
            args.replace_range(at..=end, &value);
        }
        args.split(|c: char| c == ',' || c == '/' || c.is_whitespace())
            .filter(|p| !p.is_empty())
            .filter_map(|p| {
                let pct = p.ends_with('%');
                let p = p.trim_end_matches('%').trim_end_matches("deg");
                p.parse::<f32>().ok().map(|x| (x, pct))
            })
            .collect()
    }

    fn alpha(a: Option<&(f32, bool)>) -> u8 {
        match a {
            Some((x, true)) => (x / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8,
            Some((x, false)) => (x * 255.0).round().clamp(0.0, 255.0) as u8,
            None => 255,
        }
    }

    fn hex_color(hex: &str) -> Option<Color32> {
        let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
        let pair = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        match hex.len() {
            3 | 4 => {
                let d = |i| digit(i).map(|x| x * 17);
                let a = if hex.len() == 4 { d(3)? } else { 255 };
                Some(Color32::from_rgba_unmultiplied(d(0)?, d(1)?, d(2)?, a))
            }
            6 | 8 => {
                let a = if hex.len() == 8 { pair(6)? } else { 255 };
                Some(Color32::from_rgba_unmultiplied(
                    pair(0)?,
                    pair(2)?,
                    pair(4)?,
                    a,
                ))
            }
            _ => None,
        }
    }

    fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
        let h = h.rem_euclid(360.0) / 360.0;
        let s = s.clamp(0.0, 1.0);
        let l = l.clamp(0.0, 1.0);
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        let channel = |t: f32| {
            let t = t.rem_euclid(1.0);
            let v = if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            };
            (v * 255.0).round().clamp(0.0, 255.0) as u8
        };
        [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
    }

    /// `color-mix(in <space> [<method> hue], A [p], B [p])`, premultiplied
    /// as CSS mixes, in the space it names (see [`mix_in`]). Percentages
    /// summing to under 100% scale the result's alpha by their sum, as CSS
    /// Color 5 says (`color-mix(in srgb, red 20%, blue 20%)` is 40% opaque).
    fn color_mix(args: &str) -> Option<Color32> {
        let parts = split_top(args, ',');
        if parts.len() != 3 {
            return None;
        }
        let method = words(parts[0].trim());
        let space = match method.as_slice() {
            ["in", space, ..] => *space,
            _ => "srgb",
        };
        let hue = match method.as_slice() {
            ["in", _, h, "hue"] => *h,
            _ => "shorter",
        };
        let side = |p: &str| -> Option<(Color32, Option<f32>)> {
            let w = words(p.trim());
            let (pct, colour): (Vec<&str>, Vec<&str>) = w.iter().partition(|x| x.ends_with('%'));
            let pct = pct
                .first()
                .and_then(|x| x.trim_end_matches('%').parse::<f32>().ok())
                .map(|x| x / 100.0);
            Some((color(&colour.join(" "))?, pct))
        };
        let (a, pa) = side(parts[1])?;
        let (b, pb) = side(parts[2])?;
        let (pa, pb, scale) = match (pa, pb) {
            (Some(pa), Some(pb)) => (
                pa / (pa + pb).max(1e-6),
                pb / (pa + pb).max(1e-6),
                (pa + pb).min(1.0),
            ),
            (Some(pa), None) => (pa, 1.0 - pa, 1.0),
            (None, Some(pb)) => (1.0 - pb, pb, 1.0),
            (None, None) => (0.5, 0.5, 1.0),
        };
        if space != "srgb" {
            let mixed = mix_in(space, hue, a, b, pa, pb)?;
            let [r, g, b, alpha] = mixed.to_srgba_unmultiplied();
            let alpha = (f32::from(alpha) * scale).round() as u8;
            return Some(Color32::from_rgba_unmultiplied(r, g, b, alpha));
        }
        let a = a.to_srgba_unmultiplied().map(f32::from);
        let b = b.to_srgba_unmultiplied().map(f32::from);
        let alpha = a[3] * pa + b[3] * pb;
        if alpha <= 0.0 {
            return Some(Color32::TRANSPARENT);
        }
        let ch = |i: usize| ((a[i] * a[3] * pa + b[i] * b[3] * pb) / alpha).round() as u8;
        Some(Color32::from_rgba_unmultiplied(
            ch(0),
            ch(1),
            ch(2),
            (alpha * scale).round() as u8,
        ))
    }

    /// Two colours mixed in a CSS interpolation space other than `srgb`:
    /// `srgb-linear`, `lab`, `oklab`, `xyz`/`xyz-d65`, `xyz-d50`, or the
    /// polar `hsl`, `hwb`, `lch`, `oklch`, whose hue goes the `shorter`
    /// (default), `longer`, `increasing` or `decreasing` way round. Every
    /// other component is premultiplied by alpha; a hue that is powerless
    /// (a grey) takes the other colour's.
    fn mix_in(space: &str, hue: &str, a: Color32, b: Color32, pa: f32, pb: f32) -> Option<Color32> {
        use csscolorparser::Color;
        type To = fn(&Color) -> [f32; 4];
        type From = fn([f32; 4]) -> Color;
        let (to, from, hue_at): (To, From, Option<usize>) = match space {
            "srgb-linear" => (
                |c| c.to_linear_rgba(),
                |[r, g, b, a]| Color::from_linear_rgba(r, g, b, a),
                None,
            ),
            "lab" => (
                |c| c.to_laba(),
                |[l, x, y, a]| Color::from_laba(l, x, y, a),
                None,
            ),
            "oklab" => (
                |c| c.to_oklaba(),
                |[l, x, y, a]| Color::from_oklaba(l, x, y, a),
                None,
            ),
            "xyz" | "xyz-d65" => (
                |c| c.to_xyz_d65(),
                |[x, y, z, a]| Color::from_xyz_d65(x, y, z, a),
                None,
            ),
            "xyz-d50" => (
                |c| c.to_xyz_d50(),
                |[x, y, z, a]| Color::from_xyz_d50(x, y, z, a),
                None,
            ),
            "hsl" => (
                |c| c.to_hsla(),
                |[h, s, l, a]| Color::from_hsla(h, s, l, a),
                Some(0),
            ),
            "hwb" => (
                |c| c.to_hwba(),
                |[h, w, b, a]| Color::from_hwba(h, w, b, a),
                Some(0),
            ),
            "lch" => (
                |c| {
                    let [l, c, h, a] = c.to_lcha();
                    [l, c, h.to_degrees(), a]
                },
                |[l, c, h, a]| Color::from_lcha(l, c, h.to_radians(), a),
                Some(2),
            ),
            "oklch" => (
                |c| {
                    let [l, c, h, a] = c.to_oklcha();
                    [l, c, h.to_degrees(), a]
                },
                |[l, c, h, a]| Color::from_oklcha(l, c, h.to_radians(), a),
                Some(2),
            ),
            _ => return None,
        };
        let parsed = |c: Color32| {
            let [r, g, b, a] = c.to_srgba_unmultiplied();
            to(&Color::from_rgba8(r, g, b, a))
        };
        let (mut x, mut y) = (parsed(a), parsed(b));
        let alpha = x[3] * pa + y[3] * pb;
        if alpha <= 0.0 {
            return Some(Color32::TRANSPARENT);
        }
        let mut out = [0.0, 0.0, 0.0, alpha];
        if let Some(h) = hue_at {
            // A grey's hue is powerless: hsl with no saturation, hwb as
            // white plus black reaches 1, lch and oklch with no chroma.
            let powerless = |c: &[f32; 4]| match space {
                "hsl" => c[1] < 1e-4,
                "hwb" => c[1] + c[2] >= 1.0 - 1e-4,
                "lch" => c[1] < 1e-2,
                _ => c[1] < 1e-4,
            };
            match (powerless(&x), powerless(&y)) {
                (true, false) => x[h] = y[h],
                (false, true) => y[h] = x[h],
                (true, true) => (x[h], y[h]) = (0.0, 0.0),
                (false, false) => {}
            }
            let (mut h1, mut h2) = (x[h].rem_euclid(360.0), y[h].rem_euclid(360.0));
            let d = h2 - h1;
            match hue {
                "longer" if d > 0.0 && d < 180.0 => h1 += 360.0,
                "longer" if d > -180.0 && d <= 0.0 => h2 += 360.0,
                "increasing" if h2 < h1 => h2 += 360.0,
                "decreasing" if h1 < h2 => h1 += 360.0,
                "shorter" if d > 180.0 => h1 += 360.0,
                "shorter" if d < -180.0 => h2 += 360.0,
                _ => {}
            }
            out[h] = h1 * pa + h2 * pb;
        }
        for i in (0..3).filter(|i| Some(*i) != hue_at) {
            out[i] = (x[i] * x[3] * pa + y[i] * y[3] * pb) / alpha;
        }
        let [r, g, b, a] = from(out).clamp().to_rgba8();
        Some(Color32::from_rgba_unmultiplied(r, g, b, a))
    }

    fn named(v: &str) -> Option<Color32> {
        let rgb = |r, g, b| Some(Color32::from_rgb(r, g, b));
        match v {
            "transparent" | "none" | "unset" | "initial" => Some(Color32::TRANSPARENT),
            "black" => rgb(0, 0, 0),
            "white" => rgb(255, 255, 255),
            "red" => rgb(255, 0, 0),
            "green" => rgb(0, 128, 0),
            "lime" => rgb(0, 255, 0),
            "blue" => rgb(0, 0, 255),
            "yellow" => rgb(255, 255, 0),
            "cyan" | "aqua" => rgb(0, 255, 255),
            "magenta" | "fuchsia" => rgb(255, 0, 255),
            "gray" | "grey" => rgb(128, 128, 128),
            "darkgray" | "darkgrey" => rgb(169, 169, 169),
            "lightgray" | "lightgrey" => rgb(211, 211, 211),
            "silver" => rgb(192, 192, 192),
            "maroon" => rgb(128, 0, 0),
            "olive" => rgb(128, 128, 0),
            "purple" => rgb(128, 0, 128),
            "teal" => rgb(0, 128, 128),
            "navy" => rgb(0, 0, 128),
            "orange" => rgb(255, 165, 0),
            "gold" => rgb(255, 215, 0),
            "pink" => rgb(255, 192, 203),
            "hotpink" => rgb(255, 105, 180),
            "violet" => rgb(238, 130, 238),
            "indigo" => rgb(75, 0, 130),
            "crimson" => rgb(220, 20, 60),
            "tomato" => rgb(255, 99, 71),
            "coral" => rgb(255, 127, 80),
            "salmon" => rgb(250, 128, 114),
            "brown" => rgb(165, 42, 42),
            "skyblue" => rgb(135, 206, 235),
            "deepskyblue" => rgb(0, 191, 255),
            "dodgerblue" => rgb(30, 144, 255),
            "royalblue" => rgb(65, 105, 225),
            "steelblue" => rgb(70, 130, 180),
            "turquoise" => rgb(64, 224, 208),
            "limegreen" => rgb(50, 205, 50),
            "seagreen" => rgb(46, 139, 87),
            "whitesmoke" => rgb(245, 245, 245),
            "gainsboro" => rgb(220, 220, 220),
            "dimgray" | "dimgrey" => rgb(105, 105, 105),
            "slategray" | "slategrey" => rgb(112, 128, 144),
            "rebeccapurple" => rgb(102, 51, 153),
            _ => None,
        }
    }

    /// The colours of a `background`/`background-image` value, in order
    /// (gradient stops or a plain colour).
    pub fn colors_in(value: &str) -> Vec<Color32> {
        let mut out = Vec::new();
        let v = value.to_ascii_lowercase();
        for layer in split_top(&v, ',') {
            let layer = layer.trim();
            let inner = ["linear-gradient", "radial-gradient", "conic-gradient"]
                .iter()
                .find_map(|f| func(layer, f));
            match inner {
                Some(inner) => {
                    for stop in split_top(inner, ',') {
                        let w = words(stop.trim());
                        if let Some(c) = w.first().and_then(|w| color(w)) {
                            out.push(c);
                        }
                    }
                }
                None => {
                    for w in words(layer) {
                        if let Some(c) = color(w) {
                            out.push(c);
                        }
                    }
                }
            }
        }
        out
    }

    /// A length in points: `px`, `em` (of `font`), `rem` (TIDAL's root
    /// font, 14: `html { font-size: calc(var(--baseFontSize) * 1px) }` with
    /// `:root { --baseFontSize: 14 }`, measured in TIDAL 2.46.0), `%` (of
    /// `reference`), `vw`/`vh` (of `viewport`), plain numbers and simple
    /// `calc(a ± b)`. `auto` and unknown units give `None`.
    pub fn length(value: &str, font: f32, reference: f32, viewport: [f32; 2]) -> Option<f32> {
        /// TIDAL's root font size in points, what `1rem` is.
        const ROOT_FONT: f32 = 14.0;
        let v = strip_important(value).to_ascii_lowercase();
        let v = v.trim();
        if ["calc(", "min(", "max(", "clamp("]
            .iter()
            .any(|f| v.starts_with(f))
        {
            let mut m = Math {
                s: v.as_bytes(),
                i: 0,
                font,
                reference,
                viewport,
            };
            let n = m.expr()?;
            m.ws();
            return (m.i == m.s.len()).then_some(n);
        }
        let units: [(&str, f32); 9] = [
            ("vmax", viewport[0].max(viewport[1]) / 100.0),
            ("vmin", viewport[0].min(viewport[1]) / 100.0),
            ("px", 1.0),
            ("rem", ROOT_FONT),
            ("em", font),
            ("%", reference / 100.0),
            ("vw", viewport[0] / 100.0),
            ("vh", viewport[1] / 100.0),
            ("pt", 4.0 / 3.0),
        ];
        for (unit, scale) in units {
            if let Some(n) = v.strip_suffix(unit) {
                return n.trim().parse::<f32>().ok().map(|n| n * scale);
            }
        }
        v.parse::<f32>().ok()
    }

    /// CSS math (`calc()`, `min()`, `max()`, `clamp()`, nested, with `+ -
    /// * /` and parentheses) over lengths in points.
    struct Math<'a> {
        s: &'a [u8],
        i: usize,
        font: f32,
        reference: f32,
        viewport: [f32; 2],
    }

    impl Math<'_> {
        fn ws(&mut self) {
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
        }
        fn eat(&mut self, c: u8) -> bool {
            self.ws();
            if self.s.get(self.i) == Some(&c) {
                self.i += 1;
                true
            } else {
                false
            }
        }
        fn expr(&mut self) -> Option<f32> {
            let mut v = self.term()?;
            loop {
                // `+`/`-` between terms need whitespace in CSS; a sign
                // glued to a number belongs to it.
                let at = self.i;
                self.ws();
                let spaced = self.i > at;
                match self.s.get(self.i) {
                    Some(b'+') if spaced => {
                        self.i += 1;
                        v += self.term()?;
                    }
                    Some(b'-') if spaced => {
                        self.i += 1;
                        v -= self.term()?;
                    }
                    _ => return Some(v),
                }
            }
        }
        fn term(&mut self) -> Option<f32> {
            let mut v = self.factor()?;
            loop {
                let at = self.i;
                if self.eat(b'*') {
                    v *= self.factor()?;
                } else if self.eat(b'/') {
                    let d = self.factor()?;
                    if d == 0.0 {
                        return None;
                    }
                    v /= d;
                } else {
                    // Leave the whitespace for `expr` to see.
                    self.i = at;
                    return Some(v);
                }
            }
        }
        fn args(&mut self) -> Option<Vec<f32>> {
            let mut out = vec![self.expr()?];
            while self.eat(b',') {
                out.push(self.expr()?);
            }
            self.eat(b')').then_some(out)
        }
        fn factor(&mut self) -> Option<f32> {
            self.ws();
            if self.eat(b'(') {
                let v = self.expr()?;
                return self.eat(b')').then_some(v);
            }
            let start = self.i;
            while self.s.get(self.i).is_some_and(|c| {
                c.is_ascii_alphanumeric() || matches!(c, b'.' | b'%' | b'-' | b'+')
            }) {
                // A sign only starts a token.
                if matches!(self.s[self.i], b'-' | b'+') && self.i > start {
                    break;
                }
                self.i += 1;
            }
            let word = std::str::from_utf8(&self.s[start..self.i]).ok()?;
            if self.s.get(self.i) == Some(&b'(') {
                self.i += 1;
                let a = self.args()?;
                return match word {
                    "calc" if a.len() == 1 => Some(a[0]),
                    "min" => a.into_iter().reduce(f32::min),
                    "max" => a.into_iter().reduce(f32::max),
                    "clamp" if a.len() == 3 => Some(a[1].min(a[2]).max(a[0])),
                    _ => None,
                };
            }
            if word.is_empty() {
                return None;
            }
            length(word, self.font, self.reference, self.viewport)
        }
    }

    /// `padding`/`margin` shorthand: [top, right, bottom, left].
    pub fn sides(value: &str, font: f32) -> [f32; 4] {
        let l: Vec<f32> = words(strip_important(value))
            .iter()
            .map(|w| length(w, font, 0.0, [0.0, 0.0]).unwrap_or(0.0))
            .collect();
        match l.as_slice() {
            [a] => [*a; 4],
            [v, h] => [*v, *h, *v, *h],
            [t, h, b] => [*t, *h, *b, *h],
            [t, r, b, l, ..] => [*t, *r, *b, *l],
            [] => [0.0; 4],
        }
    }

    /// `border` (or `outline`) shorthand: width and colour. As in a
    /// browser, a border without a style keyword (`border: 1px #fff`, or a
    /// value left empty by variables nothing defines) draws nothing.
    pub fn border(value: &str) -> Option<(f32, Color32)> {
        let v = strip_important(value);
        if matches!(v, "none" | "0" | "unset") {
            return None;
        }
        let mut width = 3.0;
        let mut colour = None;
        let mut styled = false;
        for w in words(v) {
            // `em` of TIDAL's 14-point text, as on its boxes.
            if let Some(px) = length(w, 14.0, 0.0, [0.0, 0.0]) {
                width = px;
            } else if let Some(c) = color(w) {
                colour = Some(c);
            } else {
                match w.to_ascii_lowercase().as_str() {
                    "none" | "hidden" => return None,
                    "solid" | "dashed" | "dotted" | "double" | "groove" | "ridge" | "inset"
                    | "outset" => styled = true,
                    "thin" => width = 1.0,
                    "medium" => width = 3.0,
                    "thick" => width = 5.0,
                    _ => {}
                }
            }
        }
        styled.then(|| (width, colour.unwrap_or(Color32::GRAY)))
    }

    /// `font-weight` as a number.
    pub fn weight(value: &str) -> u16 {
        match strip_important(value) {
            "bold" | "bolder" => 700,
            "normal" | "lighter" => 400,
            v => v.parse().unwrap_or(400),
        }
    }

    /// `opacity` 0..=1.
    pub fn opacity(value: &str) -> Option<f32> {
        let v = strip_important(value).trim();
        // `calc(.2 * 0.4)` (betterFullscreen's glows, a setting times a
        // factor): worked out as CSS does, a percentage of 1.
        if ["calc(", "min(", "max(", "clamp("]
            .iter()
            .any(|f| v.to_ascii_lowercase().starts_with(f))
        {
            return length(v, 0.0, 1.0, [0.0, 0.0]).map(|o| o.clamp(0.0, 1.0));
        }
        match v.strip_suffix('%') {
            Some(p) => p.parse::<f32>().ok().map(|p| p / 100.0),
            None => v.parse::<f32>().ok(),
        }
        .map(|o| o.clamp(0.0, 1.0))
    }

    /// The first family of a `font-family` list, unquoted.
    /// Every family named in a `font-family` value, generic ones left out.
    pub fn families(value: &str) -> Vec<String> {
        split_top(strip_important(value), ',')
            .into_iter()
            .map(|f| {
                f.trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .trim()
                    .to_string()
            })
            .filter(|f| {
                !f.is_empty()
                    && !matches!(
                        f.to_ascii_lowercase().as_str(),
                        "serif"
                            | "sans-serif"
                            | "monospace"
                            | "cursive"
                            | "fantasy"
                            | "system-ui"
                            | "ui-sans-serif"
                            | "ui-serif"
                            | "ui-monospace"
                            | "ui-rounded"
                            | "emoji"
                            | "math"
                            | "inherit"
                            | "initial"
                            | "unset"
                            | "-apple-system"
                            | "blinkmacsystemfont"
                    )
            })
            .collect()
    }

    /// One entry of a `font-family` list, in order: a family by name, or one
    /// of the generic families a browser always has a face for (`monospace`,
    /// `serif`, `cursive`, `fantasy`, `system-ui`). Other generic words
    /// (`sans-serif`, ...) are left out: the app's own face stands for them.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Family {
        Named(String),
        Generic(Generic),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Generic {
        Monospace,
        Serif,
        Cursive,
        Fantasy,
        /// The system's interface face (Segoe UI on Windows), as Chromium
        /// draws `system-ui` (lyrics-everywhere's line).
        SystemUi,
    }

    impl Generic {
        /// The families a browser maps the generic to on the three desktop
        /// systems, in the order it would find them (Chromium's Windows
        /// defaults first).
        pub fn candidates(self) -> &'static [&'static str] {
            match self {
                Self::Monospace => &[
                    "Consolas",
                    "Cascadia Mono",
                    "Menlo",
                    "DejaVu Sans Mono",
                    "Liberation Mono",
                    "Courier New",
                ],
                Self::Serif => &[
                    "Times New Roman",
                    "Times",
                    "Liberation Serif",
                    "DejaVu Serif",
                    "Georgia",
                ],
                Self::Cursive => &["Comic Sans MS", "Apple Chancery", "Z003"],
                Self::Fantasy => &["Impact", "Papyrus"],
                Self::SystemUi => &[
                    "Segoe UI",
                    "SF Pro Text",
                    "Helvetica Neue",
                    "Cantarell",
                    "Ubuntu",
                    "Noto Sans",
                    "DejaVu Sans",
                ],
            }
        }
    }

    /// The families of a `font-family` value in order, named and generic.
    pub fn family_list(value: &str) -> Vec<Family> {
        split_top(strip_important(value), ',')
            .into_iter()
            .filter_map(|entry| {
                let entry = entry.trim();
                let quoted = entry.starts_with(['"', '\'']);
                let name = entry.trim_matches(|c| c == '"' || c == '\'').trim();
                if name.is_empty() {
                    return None;
                }
                let generic = match name.to_ascii_lowercase().as_str() {
                    "monospace" | "ui-monospace" if !quoted => Some(Generic::Monospace),
                    "serif" | "ui-serif" if !quoted => Some(Generic::Serif),
                    "cursive" if !quoted => Some(Generic::Cursive),
                    "fantasy" if !quoted => Some(Generic::Fantasy),
                    "system-ui" if !quoted => Some(Generic::SystemUi),
                    _ => None,
                };
                match generic {
                    Some(g) => Some(Family::Generic(g)),
                    None if families(name).is_empty() => None,
                    None => Some(Family::Named(name.to_string())),
                }
            })
            .collect()
    }

    pub fn first_family(value: &str) -> Option<String> {
        let first = split_top(strip_important(value), ',').into_iter().next()?;
        let name = first.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        (!name.is_empty()).then(|| name.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn colours_parse() {
            assert_eq!(color("#f00"), Some(Color32::from_rgb(255, 0, 0)));
            assert_eq!(
                color("#00ff0080"),
                Some(Color32::from_rgba_unmultiplied(0, 255, 0, 128))
            );
            assert_eq!(color("rgb(1, 2, 3)"), Some(Color32::from_rgb(1, 2, 3)));
            assert_eq!(
                color("rgba(255,255,255,.5) !important"),
                Some(Color32::from_rgba_unmultiplied(255, 255, 255, 128))
            );
            assert_eq!(
                color("rgb(10 20 30 / 50%)"),
                Some(Color32::from_rgba_unmultiplied(10, 20, 30, 128))
            );
            assert_eq!(
                color("hsl(0, 100%, 50%)"),
                Some(Color32::from_rgb(255, 0, 0))
            );
            assert_eq!(
                color("hsl(120deg 100% 25%)"),
                Some(Color32::from_rgb(0, 128, 0))
            );
            assert_eq!(color("Transparent"), Some(Color32::TRANSPARENT));
            assert_eq!(color("hotpink"), Some(Color32::from_rgb(255, 105, 180)));
            assert_eq!(color("nonsense"), None);
            assert_eq!(color("currentColor"), None);
        }

        #[test]
        fn vars_and_color_mix_resolve() {
            let mut vars = BTreeMap::new();
            vars.insert("--wave-color-solid-base-brighter".into(), "#202020".into());
            vars.insert("--cover-DarkVibrant".into(), "10, 20, 30".into());
            let mixed = resolve_vars(
                "color-mix(in srgb, var(--wave-color-solid-base-brighter), transparent 70%)",
                &vars,
            );
            assert_eq!(
                color(&mixed),
                Some(Color32::from_rgba_unmultiplied(32, 32, 32, 77))
            );
            let rgb = resolve_vars(
                "rgb(var(--cover-DarkVibrant), var(--cover-opacity, 0.5))",
                &vars,
            );
            assert_eq!(rgb, "rgb(10, 20, 30, 0.5)");
            assert_eq!(
                color(&rgb),
                Some(Color32::from_rgba_unmultiplied(10, 20, 30, 128))
            );
            assert_eq!(resolve_vars("var(--missing, red)", &vars), "red");
            assert_eq!(resolve_vars("var(--missing)", &vars), "");
        }

        /// Within one step per channel of `want` (float rounding).
        #[track_caller]
        fn near(got: Option<Color32>, want: [u8; 4]) {
            let got = got.expect("a colour").to_srgba_unmultiplied();
            assert!(
                got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1),
                "{got:?} is not {want:?}"
            );
        }

        /// Every colour CSS names draws as Chromium draws it, not only the
        /// few the parser listed (`deeppink` was dropped).
        #[test]
        fn all_named_colours_parse() {
            near(color("deeppink"), [255, 20, 147, 255]);
            near(color("DarkSlateGray"), [47, 79, 79, 255]);
            near(color("lavender"), [230, 230, 250, 255]);
            near(color("mediumseagreen"), [60, 179, 113, 255]);
            // Words that only look like hex or other syntaxes stay unknown.
            assert_eq!(color("fade"), None);
            assert_eq!(color("solid"), None);
            assert_eq!(color("hsv(0, 100%, 100%)"), None);
        }

        /// CSS Color 4's functions beyond rgb() and hsl(), with values
        /// worked from the specification's conversions.
        #[test]
        fn css_colour_4_functions_parse() {
            near(color("hwb(120 0% 50%)"), [0, 128, 0, 255]);
            near(color("lab(54.29% 80.8 69.89)"), [255, 0, 0, 255]);
            near(color("lab(50 40 -30)"), [165, 91, 171, 255]);
            near(color("lch(60 50 300)"), [157, 131, 222, 255]);
            near(color("oklab(0.6 0.1 -0.1)"), [159, 99, 186, 255]);
            near(color("oklch(70% 0.15 200 / 50%)"), [0, 185, 195, 128]);
            near(color("color(srgb 1 0.5 0)"), [255, 128, 0, 255]);
            // Relative colours as far as the crate reads them: channel
            // names and plain numbers (not yet a percentage literal).
            near(color("rgb(from red r g b / 0.5)"), [255, 0, 0, 128]);
            near(color("hsl(from #00f h s 25)"), [0, 0, 128, 255]);
        }

        /// `color-mix()` mixes in the space it names, and percentages that
        /// add up to less than 100% make it see-through.
        #[test]
        fn color_mix_mixes_in_its_space() {
            near(color("color-mix(in oklab, red, blue)"), [140, 83, 162, 255]);
            near(
                color("color-mix(in srgb-linear, red, blue)"),
                [188, 0, 188, 255],
            );
            near(
                color("color-mix(in oklch, oklch(0.7 0.1 350), oklch(0.7 0.1 30))"),
                [212, 132, 144, 255],
            );
            near(
                color("color-mix(in oklch longer hue, oklch(0.7 0.1 350), oklch(0.7 0.1 30))"),
                [67, 178, 172, 255],
            );
            near(
                color("color-mix(in hsl, hsl(350 80% 50%), hsl(30 80% 50%))"),
                [230, 60, 26, 255],
            );
            near(
                color("color-mix(in srgb, red 20%, blue 20%)"),
                [128, 0, 128, 102],
            );
            near(
                color("color-mix(in oklab, red 30%, blue 30%)"),
                [140, 83, 162, 153],
            );
        }

        /// A function ends its word at its `)`, as CSS tokenises it:
        /// Sugarcoat's label colour writes the second percentage with no
        /// space before it (`rgb(0, 0, 0)87%`).
        #[test]
        fn a_percentage_right_after_a_colour_function_still_mixes() {
            assert_eq!(
                words("rgb(0, 0, 0)87% red"),
                vec!["rgb(0, 0, 0)", "87%", "red"]
            );
            assert_eq!(
                color("color-mix(in srgb, rgb(7, 0, 39) 13% , rgb(0, 0, 0)87%)"),
                Some(Color32::from_rgb(1, 0, 5))
            );
        }

        #[test]
        fn lengths_parse() {
            let vp = [1000.0, 800.0];
            assert_eq!(length("12px", 14.0, 0.0, vp), Some(12.0));
            assert_eq!(length("2em", 14.0, 0.0, vp), Some(28.0));
            assert_eq!(length("1.5rem", 14.0, 0.0, vp), Some(21.0));
            assert_eq!(length("50%", 14.0, 200.0, vp), Some(100.0));
            assert_eq!(length("10vh", 14.0, 0.0, vp), Some(80.0));
            assert_eq!(length("calc(100vh - 126px)", 14.0, 0.0, vp), Some(674.0));
            // betterFullscreen's sizes.
            assert_eq!(
                length(
                    "calc(\n    clamp(1.5rem, 5vw, 3rem) * 1\n  )",
                    14.0,
                    0.0,
                    vp
                ),
                Some(42.0)
            );
            assert_eq!(length("clamp(1rem, 1vw, 3rem)", 14.0, 0.0, vp), Some(14.0));
            assert_eq!(length("min(90%, 80px)", 14.0, 200.0, vp), Some(80.0));
            assert_eq!(length("max(10px, 2em)", 14.0, 0.0, vp), Some(28.0));
            assert_eq!(
                length("calc((100% - 20px) / 2)", 14.0, 200.0, vp),
                Some(90.0)
            );
            assert_eq!(length("calc(-1 * 4px)", 14.0, 0.0, vp), Some(-4.0));
            assert_eq!(length("calc(10px + auto)", 14.0, 0.0, vp), None);
            assert_eq!(length("auto", 14.0, 0.0, vp), None);
            assert_eq!(length("0", 14.0, 0.0, vp), Some(0.0));
            assert_eq!(sides("4px 8px", 14.0), [4.0, 8.0, 4.0, 8.0]);
            assert_eq!(sides("1px 2px 3px", 14.0), [1.0, 2.0, 3.0, 2.0]);
            assert_eq!(border("2px solid #fff"), Some((2.0, Color32::WHITE)));
            assert_eq!(border("none"), None);
            assert_eq!(weight("bold"), 700);
            assert_eq!(opacity("0.4"), Some(0.4));
            assert_eq!(
                first_family("\"Comic Sans MS\", sans-serif"),
                Some("Comic Sans MS".into())
            );
        }

        /// An opacity worked out by `calc()` and its kin, as CSS has it:
        /// it read as none, so betterFullscreen's faint glows (`calc(.2 *
        /// var(--vibrant-color-opacity))`) covered the player in white.
        #[test]
        fn an_opacity_can_be_worked_out() {
            assert_eq!(
                opacity("calc(.2 * 0.4)").map(|o| (o * 1000.0).round()),
                Some(80.0)
            );
            assert_eq!(opacity("min(0.5, 0.3)"), Some(0.3));
            assert_eq!(opacity("clamp(0, 2, 1)"), Some(1.0));
            assert_eq!(opacity("calc(50% / 2)"), Some(0.25));
            assert_eq!(opacity("0.4"), Some(0.4));
            assert_eq!(opacity("40%"), Some(0.4));
        }

        #[test]
        fn gradient_colours_are_found() {
            let c = colors_in("linear-gradient(180deg, #000000, #142b44) fixed");
            assert_eq!(c, vec![Color32::BLACK, Color32::from_rgb(0x14, 0x2b, 0x44)]);
        }
    }
}

/// What children inherit while drawing.
#[derive(Clone, Copy)]
struct Inherit {
    color: Color32,
    /// What the text is read against: the panel, then each plugin box's own
    /// fill over it.
    back: Color32,
    size: f32,
    weight: u16,
    italic: bool,
    /// The family an element's `font-family` named, numbered by
    /// [`plugin_dom::element_font_id`] and inherited by the text inside;
    /// `None` is the app's face.
    family: Option<u32>,
    /// `text-transform: uppercase`.
    upper: bool,
    /// SVG `fill` set by CSS on the element or an ancestor.
    fill: Option<Color32>,
    /// `line-height`, inherited as CSS does: a number scales each
    /// element's own font size, a length is kept. `None` is `normal` (the
    /// face's own row height).
    line_height: Option<LineHeight>,
    /// `text-shadow` layers (inherited, as in CSS), offset and colour; the
    /// first is painted on top.
    shadow: [Option<(Vec2, Color32)>; 2],
    /// `filter: blur()` on the element or one round it, in points (the
    /// standard deviation): its words are drawn soft. 0 is sharp.
    blur: f32,
    /// The block round this text cuts it with "…" (`text-overflow:
    /// ellipsis`): its inline content, words in spans included.
    ellipsis: bool,
    /// `list-style-type`, inherited from the list: what a list item's
    /// marker is.
    marker: Marker,
}

/// A list item's marker (`list-style-type`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Marker {
    #[default]
    Disc,
    Circle,
    Square,
    Decimal,
    None,
}

impl Marker {
    fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "disc" => Self::Disc,
            "circle" => Self::Circle,
            "square" => Self::Square,
            "decimal" | "decimal-leading-zero" => Self::Decimal,
            "none" => Self::None,
            _ => return None,
        })
    }

    /// The marker's text for the `n`-th item (from 1), as Chromium writes
    /// it before the item: a bullet, or the number and a full stop.
    fn text(self, n: usize) -> Option<String> {
        Some(match self {
            Self::Disc => "\u{2022}".into(),
            Self::Circle => "\u{25E6}".into(),
            Self::Square => "\u{25AA}".into(),
            Self::Decimal => format!("{n}."),
            Self::None => return None,
        })
    }
}

thread_local! {
    /// The items counted so far in each list being drawn, innermost last
    /// (an `<ol>`'s numbers).
    static LIST_ITEMS: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LineHeight {
    Factor(f32),
    Points(f32),
}

impl LineHeight {
    /// A `line-height` value for an element whose font is `font` points:
    /// `Some(None)` for `normal`, `None` when it cannot be read.
    fn parse(value: &str, font: f32, viewport: [f32; 2]) -> Option<Option<Self>> {
        let v = value.trim().trim_end_matches("!important").trim();
        if v.eq_ignore_ascii_case("normal") {
            return Some(None);
        }
        if let Ok(f) = v.parse::<f32>() {
            return (f >= 0.0).then_some(Some(Self::Factor(f)));
        }
        let px = css::length(v, font, font, viewport)?;
        (px >= 0.0).then_some(Some(Self::Points(px)))
    }

    /// The row height for text `size` points tall.
    fn points(self, size: f32) -> f32 {
        match self {
            Self::Factor(f) => f * size,
            Self::Points(p) => p,
        }
        .max(1.0)
    }
}

impl Inherit {
    /// What a slot starts from: the app's text on its panel.
    fn root(ui: &egui::Ui) -> Self {
        Self {
            color: ui.visuals().text_color(),
            back: ui.visuals().panel_fill,
            size: 13.0,
            weight: 400,
            italic: false,
            family: None,
            upper: false,
            fill: None,
            line_height: None,
            shadow: [None; 2],
            blur: 0.0,
            ellipsis: false,
            marker: Marker::Disc,
        }
    }

    /// What a plugin window's page starts from: a browser's defaults for a
    /// document (black 16-pixel text on a white page), which the page's own
    /// stylesheet then sets.
    fn page(ui: &egui::Ui) -> Self {
        Self {
            color: Color32::BLACK,
            back: Color32::WHITE,
            size: 16.0,
            weight: 400,
            ..Self::root(ui)
        }
    }

    /// What an element appended to `body` starts from: TIDAL's body text is
    /// weight 500 (measured in TIDAL 2.43.2), and plugin text that sets no
    /// weight keeps it. In Chromium on Windows a family without a 500 cut
    /// draws it with its semibold one (DownloadManager's window title and
    /// its "Output:" row, in Segoe UI), as [`crate::fonts::pick_weight`]
    /// picks it here.
    fn body(ui: &egui::Ui) -> Self {
        Self {
            weight: 500,
            ..Self::root(ui)
        }
    }
}

/// The text a host element's own class gives what plugins put in it (a
/// clone of TIDAL's time heading reads as a heading: 12-point semibold
/// capitals; a cell cloned from a row's time reads as the row's text).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub color: Color32,
    pub size: f32,
    pub weight: u16,
    pub upper: bool,
}

/// Elements injected at `key`/`position` laid out as table columns: the
/// n-th in a cell `widths[n]` wide, one after another from `point` (the
/// left edge, at the row's middle), each clipped to its cell, its text in
/// `look`. Nodes past `widths` are left out.
pub fn columns(
    ui: &mut egui::Ui,
    key: &str,
    position: Position,
    point: Pos2,
    widths: &[f32],
    look: Look,
) {
    let Some(overlay) = plugin_dom::overlay() else {
        return;
    };
    let vars = plugin_dom::vars();
    let cx = Cx {
        vars: &vars,
        viewport: plugin_dom::viewport(),
        keyframes: &overlay.keyframes,
    };
    let inherit = Inherit {
        color: look.color,
        size: look.size,
        weight: look.weight,
        upper: look.upper,
        ..Inherit::root(ui)
    };
    let mut x = point.x;
    for (n, (node, width)) in overlay.at(key, position).zip(widths).enumerate() {
        // One line of the text, centred on the row.
        let line = (look.size * 1.25).round();
        let cell = Rect::from_min_size(egui::pos2(x, point.y - line / 2.0), vec2(*width, line));
        x += width;
        if !ui.is_rect_visible(cell) {
            continue;
        }
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(("luna-column", key, position as u8, n))
                .max_rect(cell)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(cell.intersect(ui.clip_rect()));
        draw_at(&mut child, node, key, inherit, &cx);
    }
}

/// `text-shadow` as up to two sharp layers (offset, colour), first on top.
/// A blurred layer (a glow) is left out: drawn sharp it would be a second
/// copy of the words, not a glow.
fn text_shadows(value: &str, font: f32, current: Color32) -> [Option<(Vec2, Color32)>; 2] {
    let mut out = [None; 2];
    if value.trim().eq_ignore_ascii_case("none") {
        return out;
    }
    let mut n = 0;
    for layer in css::split_top(value, ',') {
        let mut lengths = Vec::new();
        let mut colour = None;
        for w in css::words(layer) {
            if let Some(c) = css::color(w) {
                colour = Some(c);
            } else if let Some(v) = css::length(w, font, 0.0, plugin_dom::viewport()) {
                lengths.push(v);
            }
        }
        let (Some(x), Some(y)) = (lengths.first(), lengths.get(1)) else {
            continue;
        };
        if lengths.get(2).is_some_and(|b| *b > 1.0) {
            continue;
        }
        if n < out.len() {
            out[n] = Some((vec2(*x, *y), colour.unwrap_or(current)));
            n += 1;
        }
    }
    out
}

/// Add a text label, with the `text-shadow` it inherits painted under it:
/// copies of its glyphs at each layer's offset in the layer's colour, the
/// last layer lowest (custom-fonts' gold and pink Comic Sans tag).
fn add_text(
    ui: &mut egui::Ui,
    text: RichText,
    wrap: Option<egui::TextWrapMode>,
    inherit: Inherit,
) -> egui::Response {
    let layers: Vec<(Vec2, Color32)> = inherit.shadow.iter().flatten().copied().collect();
    if inherit.blur >= 0.5 {
        return add_blurred(ui, text, wrap, inherit);
    }
    if layers.is_empty() {
        return add_line(ui, text, wrap, inherit);
    }
    let slots: Vec<egui::layers::ShapeIdx> = layers
        .iter()
        .map(|_| ui.painter().add(egui::Shape::Noop))
        .collect();
    let layer = ui.layer_id();
    let from = ui.ctx().graphics_mut(|g| g.entry(layer).next_idx().0);
    let response = add_line(ui, text, wrap, inherit);
    ui.ctx().graphics_mut(|g| {
        let list = g.entry(layer);
        let texts: Vec<(Rect, egui::epaint::TextShape)> = list
            .all_entries()
            .skip(from)
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((c.clip_rect, t.clone())),
                _ => None,
            })
            .collect();
        let clip = texts.first().map(|(c, _)| *c);
        for (slot, (offset, colour)) in slots.iter().zip(layers.iter().rev()) {
            let copies: Vec<egui::Shape> = texts
                .iter()
                .map(|(_, t)| {
                    let mut t = t.clone();
                    t.pos += *offset;
                    t.override_text_color = Some(*colour);
                    egui::Shape::Text(t)
                })
                .collect();
            if let Some(clip) = clip {
                list.set(*slot, clip, egui::Shape::Vec(copies));
            }
        }
    });
    response
}

/// Where the copies of blurred words sit, in standard deviations, and how
/// much of the Gaussian each carries: the centre, then six points at 0.7
/// and six at 1.4 between them.
fn blur_taps() -> impl Iterator<Item = (Vec2, f32)> {
    let ring = |r: f32, turn: f32| {
        (0..6).map(move |i| {
            let a = std::f32::consts::TAU * (i as f32 + turn) / 6.0;
            (vec2(a.cos(), a.sin()) * r, (-r * r / 2.0).exp())
        })
    };
    std::iter::once((Vec2::ZERO, 1.0))
        .chain(ring(0.7, 0.0))
        .chain(ring(1.4, 0.5))
}

/// Add a text label whose words are blurred (`filter: blur()`): the glyphs
/// are drawn as see-through copies spread over the blur, which together
/// cover as much as the sharp words did where they all overlap and fade
/// at the edges. Its `text-shadow` is left out, as the blur would soften
/// it into the words.
fn add_blurred(
    ui: &mut egui::Ui,
    text: RichText,
    wrap: Option<egui::TextWrapMode>,
    inherit: Inherit,
) -> egui::Response {
    let layer = ui.layer_id();
    let from = ui.ctx().graphics_mut(|g| g.entry(layer).next_idx().0);
    let response = add_line(ui, text, wrap, inherit);
    let total: f32 = blur_taps().map(|(_, w)| w).sum();
    // Opaque words would stay opaque in every copy; a tenth short of it
    // keeps the overlap solid and lets the edges fade.
    let clear = 0.1_f32;
    ui.ctx().graphics_mut(|g| {
        let list = g.entry(layer);
        let texts: Vec<(usize, Rect, egui::epaint::TextShape)> = list
            .all_entries()
            .enumerate()
            .skip(from)
            .filter_map(|(i, c)| match &c.shape {
                egui::Shape::Text(t) => Some((i, c.clip_rect, t.clone())),
                _ => None,
            })
            .collect();
        for (i, clip, t) in texts {
            let copies = blur_taps()
                .map(|(at, w)| {
                    let mut c = t.clone();
                    c.pos += at * inherit.blur;
                    c.opacity_factor *= 1.0 - clear.powf(w / total);
                    egui::Shape::Text(c)
                })
                .collect();
            list.set(egui::layers::ShapeIdx(i), clip, egui::Shape::Vec(copies));
        }
    });
    response
}

/// `top` laid over `under`.
fn over(top: Color32, under: Color32) -> Color32 {
    // Colour32 holds premultiplied channels.
    let [r, g, b, a] = top.to_srgba_unmultiplied();
    let a = f32::from(a) / 255.0;
    let mix = |t: u8, u: u8| (f32::from(t) * a + f32::from(u) * (1.0 - a)).round() as u8;
    Color32::from_rgb(mix(r, under.r()), mix(g, under.g()), mix(b, under.b()))
}

fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// Text a plugin wrote for TIDAL's dark window (white on a pill that is
/// near black there) stays readable where the backdrop is not that: the
/// colour is kept while it reads (contrast 1.6 or more over `back`), else the
/// ink that contrasts, dark on a light backdrop and white on a dark one,
/// with the colour's own opacity. TIDAL has no light window, so this is
/// what its plugins' colours would have been had it one.
fn readable(text: Color32, back: Color32) -> Color32 {
    // A plugin window's page has the colours it chose, as in a browser.
    if plugin_dom::drawing_page() || plugin_dom::contrast(over(text, back), back) >= 1.6 {
        return text;
    }
    let ink = if luminance(back) > 0.4 {
        Color32::from_rgb(0x1a, 0x1a, 0x1a)
    } else {
        Color32::WHITE
    };
    Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), text.a())
}

/// One drawing pass over a slot.
struct Cx<'a> {
    vars: &'a BTreeMap<String, String>,
    viewport: [f32; 2],
    keyframes: &'a BTreeMap<String, Vec<(f32, Style)>>,
}

impl Cx<'_> {
    fn color(&self, value: &Option<String>) -> Option<Color32> {
        let v = value.as_deref()?;
        if v.contains("var(") {
            css::color(&css::resolve_vars(v, self.vars))
        } else {
            css::color(v)
        }
    }

    fn len(&self, value: &Option<String>, font: f32, reference: f32) -> Option<f32> {
        let v = value.as_deref()?;
        let v = if v.contains("var(") {
            css::resolve_vars(v, self.vars)
        } else {
            v.to_string()
        };
        css::length(&v, font, reference, self.viewport)
    }
}

/// Elements injected at `key`/`position`, drawn into `ui`'s current layout
/// (a horizontal run). Returns whether anything was drawn. Costs one bool
/// check while Luna is off.
pub fn inline(ui: &mut egui::Ui, key: &str, position: Position) -> bool {
    inline_with(ui, key, position, false, None, None)
}

/// [`inline`] for elements inside a host element whose text is `size`
/// points, which they inherit (custom-fonts' tag in TIDAL's 12-pixel title).
pub fn inline_sized(ui: &mut egui::Ui, key: &str, position: Position, size: f32) -> bool {
    inline_with(ui, key, position, false, None, Some(size))
}

/// Which side of a flex row's free space the elements at `key`/`position`
/// sit on, when the host element before them has `margin-right: auto`
/// (TIDAL's navigation arrows push the search field to the bar's right
/// end): the first `n` stay at the start, the rest go to the end beside
/// the next host element. As in CSS flex, a shown element with
/// `margin-right: auto` keeps itself and those before it at the start,
/// one with `margin-left: auto` takes itself and those after it to the
/// end; with neither, all go to the end.
pub fn auto_margin_split(key: &str, position: Position) -> usize {
    let Some(overlay) = plugin_dom::overlay() else {
        return 0;
    };
    let auto = |v: &Option<String>| v.as_deref().is_some_and(|v| v.trim() == "auto");
    let side_auto = |s: &Style, side: usize| {
        let longhand = if side == 1 {
            &s.margin_right
        } else {
            &s.margin_left
        };
        if longhand.is_some() {
            return auto(longhand);
        }
        s.margin.as_deref().is_some_and(|m| {
            let words: Vec<&str> = m.split_whitespace().collect();
            let at = match words.len() {
                1 => 0,
                2 | 3 => 1,
                _ => side,
            };
            words.get(at).is_some_and(|w| *w == "auto")
        })
    };
    for (i, node) in overlay.at(key, position).enumerate() {
        if node.style.hidden() {
            continue;
        }
        if side_auto(&node.style, 1) {
            return i + 1;
        }
        if side_auto(&node.style, 3) {
            return i;
        }
    }
    0
}

/// [`inline`] for the elements `range` of those at `key`/`position`, in
/// document order (see [`auto_margin_split`]).
pub fn inline_range(
    ui: &mut egui::Ui,
    key: &str,
    position: Position,
    range: std::ops::Range<usize>,
) -> bool {
    if range.is_empty() {
        return false;
    }
    inline_with(ui, key, position, false, Some(range), None)
}

/// Elements injected at `key`/`position` of a block container (TIDAL's
/// context menu), laid out as a browser lays a block's children: each
/// block-level element on a line of its own, as wide as the container, and
/// runs of inline-level ones side by side. element-hider's divider and its
/// two `display: flex` buttons stack under the menu's rows, as in TIDAL.
pub fn column(ui: &mut egui::Ui, key: &str, position: Position) -> bool {
    let Some(overlay) = plugin_dom::overlay() else {
        return false;
    };
    if !overlay
        .injected
        .iter()
        .any(|i| i.anchor == key && i.position == position)
    {
        return false;
    }
    let vars = plugin_dom::vars();
    let cx = Cx {
        vars: &vars,
        viewport: plugin_dom::viewport(),
        keyframes: &overlay.keyframes,
    };
    let inherit = Inherit::root(ui);
    let nodes: Vec<&VNode> = overlay.at(key, position).collect();
    let mut drawn = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut i = 0;
        while i < nodes.len() {
            if !flows_in_line(nodes[i]) {
                // A block is as wide as the menu (element-hider's divider).
                IN_COLUMN.with(|s| s.set(true));
                drawn |= draw_at(ui, nodes[i], key, inherit, &cx);
                IN_COLUMN.with(|s| s.set(false));
                i += 1;
                continue;
            }
            let start = i;
            while i < nodes.len() && flows_in_line(nodes[i]) {
                i += 1;
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for node in &nodes[start..i] {
                    drawn |= draw_at(ui, node, key, inherit, &cx);
                }
            });
        }
    });
    drawn
}

/// Whether a top-level element sits in a line with its inline-level
/// neighbours: an inline element, or a replaced or form element (`button`,
/// `img`, `input`, ...) with no `display` of its own, which a browser lays
/// out inline.
fn flows_in_line(n: &VNode) -> bool {
    inline_level(n)
        || (n
            .style
            .display
            .as_deref()
            .is_none_or(|d| d.trim().is_empty())
            && matches!(
                n.tag.as_str(),
                "button" | "img" | "input" | "select" | "textarea" | "svg" | "video" | "canvas"
            ))
}

/// [`inline`] that also tells each top-level element's own width to its
/// plugin (`offsetWidth`, which is the anchor's until an element has a box
/// of its own): a plugin sizing a pill from its mark's width must not read
/// the pill's.
pub fn inline_measured(ui: &mut egui::Ui, key: &str, position: Position) -> bool {
    inline_with(ui, key, position, true, None, None)
}

/// [`inline`] for a run with a limit (the player bar's right side): when
/// `show` is false the elements are laid out beside the run, not in it, so
/// their width is known without their taking room or being drawn. Returns
/// the width they take in the run, spacing included, 0 when there are none.
pub fn inline_fit(ui: &mut egui::Ui, key: &str, position: Position, show: bool) -> f32 {
    if plugin_dom::overlay().is_none() {
        return 0.0;
    }
    let right_to_left = ui.layout().prefer_right_to_left();
    let edge = |ui: &egui::Ui| {
        if right_to_left {
            ui.cursor().right()
        } else {
            -ui.cursor().left()
        }
    };
    if show {
        let from = edge(ui);
        inline(ui, key, position);
        return (from - edge(ui)).max(0.0);
    }
    let mut aside = ui.new_child(
        UiBuilder::new()
            .max_rect(ui.available_rect_before_wrap())
            .layout(*ui.layout())
            .invisible(),
    );
    aside.spacing_mut().item_spacing = ui.spacing().item_spacing;
    let from = edge(&aside);
    inline(&mut aside, key, position);
    (from - edge(&aside)).max(0.0)
}

fn inline_with(
    ui: &mut egui::Ui,
    key: &str,
    position: Position,
    measure: bool,
    range: Option<std::ops::Range<usize>>,
    size: Option<f32>,
) -> bool {
    let Some(overlay) = plugin_dom::overlay() else {
        return false;
    };
    if !overlay
        .injected
        .iter()
        .any(|i| i.anchor == key && i.position == position)
    {
        return false;
    }
    let vars = plugin_dom::vars();
    let cx = Cx {
        vars: &vars,
        viewport: plugin_dom::viewport(),
        keyframes: &overlay.keyframes,
    };
    let mut inherit = Inherit::root(ui);
    if let Some(size) = size {
        inherit.size = size;
    }
    let mut drawn = false;
    // A run as tall as what it held last frame, so a centred row centres
    // it: an element taller than egui's 18-point row used to hang down
    // from that row's top (OneClickLyrics' 32-point button sat 3 points
    // below the player bar's icons).
    let run_id = ui.id().with(("luna-run", key, format!("{position:?}")));
    let last_height: f32 = ui.data(|d| d.get_temp(run_id)).unwrap_or(0.0);
    let layout = if ui.layout().prefer_right_to_left() {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    };
    let size = vec2(
        ui.available_size_before_wrap().x,
        last_height.max(ui.spacing().interact_size.y),
    );
    // The tallest element of a right-to-left run, from the boxes it laid
    // out: the run's own rect can be taller than what it holds once it
    // outgrew its row, and would keep growing.
    let mut tallest: f32 = 0.0;
    let rtl_run = ui.layout().prefer_right_to_left();
    let centre = ui.available_rect_before_wrap().center().y;
    let run = ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        // In a right-to-left run the first element goes rightmost.
        let rtl = ui.layout().prefer_right_to_left();
        let mut nodes: Vec<&VNode> = overlay.at(key, position).collect();
        if let Some(range) = &range {
            let end = range.end.min(nodes.len());
            nodes = nodes[range.start.min(end)..end].to_vec();
        }
        if rtl {
            nodes.reverse();
        }
        for node in nodes {
            if !rtl {
                let from = ui.cursor().left();
                let spacing = ui.spacing().item_spacing.x;
                let did = draw_at(ui, node, key, inherit, &cx);
                drawn |= did;
                if measure && did {
                    let width = (ui.cursor().left() - from - spacing).max(0.0);
                    let min = ui.min_rect();
                    let rect =
                        Rect::from_min_size(egui::pos2(from, min.top()), vec2(width, min.height()));
                    report_measure(ui, node.id, rect);
                }
                continue;
            }
            // Each element is laid out left to right in a box the size it
            // took last frame, taken from the right-to-left run: drawn
            // straight into that run, a box would claim all the room left.
            // The box is centred on the row the run sits in: egui would
            // hang one about as tall as the row from the row's top
            // (the audio-visualizer's 48-point panel in the top bar).
            let id = egui::Id::new(("luna-inline", node.id));
            let last: Vec2 = ui.data(|d| d.get_temp(id)).unwrap_or(Vec2::ZERO);
            let size = vec2(last.x.max(1.0), last.y.max(1.0));
            let right = ui.available_rect_before_wrap().right();
            let rect = Rect::from_min_max(
                egui::pos2(right - size.x, centre - size.y / 2.0),
                egui::pos2(right, centre + size.y / 2.0),
            );
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            drawn |= draw_at(&mut child, node, key, inherit, &cx);
            let used = child.min_rect().size();
            ui.advance_cursor_after_rect(rect);
            tallest = tallest.max(used.y);
            if (used - last).length() > 0.5 {
                ui.data_mut(|d| d.insert_temp(id, used));
                plugin_dom::request_frame(ui.ctx());
            }
        }
    });
    let height = if rtl_run {
        tallest
    } else {
        run.response.rect.height()
    };
    if (height - last_height).abs() > 0.5 {
        ui.data_mut(|d| d.insert_temp(run_id, height));
        plugin_dom::request_frame(ui.ctx());
    }
    drawn
}

/// A gradient layer of a plugin box's background.
enum BoxGradient {
    Linear(super::theme_gradient::Angled),
    /// A `radial-gradient()`, as written.
    Radial(String),
}

/// A plugin box's `background-image` when every layer is a gradient: its
/// linear (see [`box_gradient`]) and radial layers, the first on top as CSS
/// paints them. A radial glow (betterFullscreen's round light behind the
/// cover, fading out at 70%) was a box filled with its first colour, and
/// layers after the first went unpainted.
fn box_gradients(value: &str) -> Option<Vec<BoxGradient>> {
    let mut out = Vec::new();
    for layer in css::split_top(value, ',') {
        let layer = layer.trim();
        let lower = layer.to_ascii_lowercase();
        if lower.starts_with("linear-gradient(") {
            out.push(BoxGradient::Linear(box_gradient(layer)?));
        } else if lower.starts_with("radial-gradient(")
            || lower.starts_with("-webkit-radial-gradient(")
        {
            out.push(BoxGradient::Radial(layer.to_string()));
        } else {
            return None;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// A `linear-gradient()` a plugin box is filled with: an angled one as a
/// theme writes it, or a plain ramp (`to top`, `to right`, none) with its
/// colours spread evenly.
fn box_gradient(value: &str) -> Option<super::theme_gradient::Angled> {
    let lower = value.trim().to_ascii_lowercase();
    if !lower.starts_with("linear-gradient(") {
        return None;
    }
    if let Some(g) = super::theme_gradient::parse(value) {
        return Some(g);
    }
    let colours = css::colors_in(value);
    if colours.len() < 2 {
        return None;
    }
    let angle = if lower.contains("to top") {
        0.0
    } else if lower.contains("to right") {
        90.0
    } else if lower.contains("to left") {
        270.0
    } else {
        180.0
    };
    let last = (colours.len() - 1) as f32;
    Some(super::theme_gradient::Angled {
        stops: colours
            .into_iter()
            .enumerate()
            .map(|(i, c)| (i as f32 / last, c))
            .collect(),
        angle,
        spin: None,
        size: [1.0, 1.0],
        position: [0.0, 0.0],
        lengths: None,
    })
}

/// Tell a plugin element its own drawn box, when it changed.
fn report_measure(ui: &egui::Ui, id: u64, rect: Rect) {
    if id & MEASURE_ID != 0 {
        return;
    }
    if id & MEASURE_ID != 0 {
        return;
    }
    let key = egui::Id::new(("luna-measure", id));
    let last: Option<Rect> = ui.data(|d| d.get_temp(key));
    if last.is_some_and(|l| {
        (l.width() - rect.width()).abs() < 0.5 && (l.min - rect.min).length() < 0.5
    }) {
        return;
    }
    ui.data_mut(|d| d.insert_temp(key, rect));
    plugin_dom::send(
        DomTarget::Node(id),
        "measure",
        json!({ "rect": [rect.min.x, rect.min.y, rect.width(), rect.height()] }),
    );
}

/// A top-level slot element: positioned ones go against their anchor's rect
/// (star-ratings' row stars, discord-utils' title-bar label).
fn draw_at(ui: &mut egui::Ui, node: &VNode, key: &str, inherit: Inherit, cx: &Cx) -> bool {
    if fixed_behind(node) {
        return false;
    }
    if positioned_kind(&node.style).is_some()
        && !node.style.hidden()
        && let Some(container) = plugin_dom::host_rect(key)
    {
        if z_index(&node.style) > 0 {
            above(ui, node, container, inherit, cx);
        } else {
            positioned(ui, node, container, inherit, cx);
        }
        return true;
    }
    draw(ui, node, inherit, cx)
}

/// A positioned element with a positive `z-index` stacks over its anchor's
/// native content, which the view may paint after its slots
/// (betterFullscreen's player over the now-playing view): it gets a layer
/// of its own above the panels, clipped as its slot is.
fn above(ui: &egui::Ui, node: &VNode, container: Rect, inherit: Inherit, cx: &Cx) {
    let clip = ui.clip_rect();
    let inert = node
        .style
        .pointer_events
        .as_deref()
        .is_some_and(|p| p.trim() == "none");
    let (pos, placed) = layer_origin(ui.ctx(), ("luna-above", node.id), container.min);
    egui::Area::new(egui::Id::new(("luna-above", node.id)))
        .order(egui::Order::Middle)
        .fixed_pos(pos)
        .constrain(false)
        .interactable(!inert && placed)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(clip);
            let used = positioned(ui, node, container, inherit, cx);
            ui.expand_to_include_rect(used);
            place_layer(ui.ctx(), ("luna-above", node.id), used);
        });
}

/// Where the layer of a positioned plugin element starts: the top-left
/// corner of the box it drew last frame. An egui area takes input over its
/// whole rect, from its origin to the far edge of what it drew, so a layer
/// placed at the window's corner with a bar at the bottom (DownloadManager's
/// taskbar) took every click in the window. Until the box is known (its
/// first frame) the layer is drawn but lets input through (`false`).
fn layer_origin(ctx: &egui::Context, key: (&'static str, u64), fallback: Pos2) -> (Pos2, bool) {
    let at: Option<Pos2> = ctx.data(|d| d.get_temp(egui::Id::new(("luna-layer-at", key))));
    (at.unwrap_or(fallback), at.is_some())
}

/// Remember where a positioned element's box starts, for [`layer_origin`];
/// a move is drawn again at once so the layer follows.
fn place_layer(ctx: &egui::Context, key: (&'static str, u64), used: Rect) {
    let id = egui::Id::new(("luna-layer-at", key));
    let at = used.min;
    if !at.is_finite() || ctx.data(|d| d.get_temp::<Pos2>(id)) == Some(at) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, at));
    plugin_dom::request_frame(ctx);
}

/// Elements injected at `key`/`position`, drawn as a row placed by `align`
/// at `point` (e.g. right of a title: `LEFT_CENTER` at its right edge). The
/// row's size comes from the last frame, so it lines up after one frame.
/// Returns the rect used.
pub fn at(
    ui: &mut egui::Ui,
    key: &str,
    position: Position,
    point: Pos2,
    align: Align2,
) -> Option<Rect> {
    slot(ui, key, position, point, align, false, None)
}

/// [`at`] for a place with a limit (the player bar's track info, which ends
/// where the transport begins): the row is laid out where it would go, so
/// its size is always known, but drawn only when all of it fits within
/// `bounds`; one that does not is left out whole, not clipped through its
/// text. The decision depends on the row's own size and place, so a window
/// resized across the limit switches it once, never back and forth. The
/// rect returned is where the row goes, shown or not.
pub fn at_within(
    ui: &mut egui::Ui,
    key: &str,
    position: Position,
    point: Pos2,
    align: Align2,
    bounds: Rect,
) -> Option<Rect> {
    slot(ui, key, position, point, align, false, Some(bounds))
}

/// How wide what plugins put at `key`/`position` was when last drawn
/// there; `None` while nothing is there.
pub fn slot_width(ctx: &egui::Context, key: &str, position: Position) -> Option<f32> {
    let overlay = plugin_dom::overlay()?;
    overlay
        .injected
        .iter()
        .any(|i| i.anchor == key && i.position == position && !i.nodes.is_empty())
        .then(|| {
            ctx.data(|d| d.get_temp::<Vec2>(egui::Id::new(("luna-slot", key, position as u8))))
                .map(|v| v.x)
        })
        .flatten()
}

/// [`at`] for an anchor whose box does not clip (TIDAL's player bar cover):
/// what plugins put there may stand out of the bar (a `position: relative`
/// offset can lift an element past the bar's top), so it is drawn above
/// the page and clipped only by the window.
pub fn at_unclipped(
    ui: &mut egui::Ui,
    key: &str,
    position: Position,
    point: Pos2,
    align: Align2,
) -> Option<Rect> {
    slot(ui, key, position, point, align, true, None)
}

fn slot(
    ui: &mut egui::Ui,
    key: &str,
    position: Position,
    point: Pos2,
    align: Align2,
    unclipped: bool,
    within: Option<Rect>,
) -> Option<Rect> {
    let overlay = plugin_dom::overlay()?;
    if !overlay
        .injected
        .iter()
        .any(|i| i.anchor == key && i.position == position)
    {
        return None;
    }
    let id = egui::Id::new(("luna-slot", key, position as u8));
    let measured = ui.ctx().data(|d| d.get_temp::<Vec2>(id));
    let last = measured.unwrap_or(vec2(0.0, 20.0));
    let rect = align.anchor_size(point, last);
    // Not known to fit until it has been measured once, so that it never
    // shows for a frame where it does not belong.
    let shown = within.is_none_or(|b| {
        measured.is_some() && rect.min.x >= b.min.x - 0.5 && rect.max.x <= b.max.x + 0.5
    });
    let mut builder = UiBuilder::new()
        .id_salt(id)
        .max_rect(Rect::from_min_size(
            rect.min,
            vec2(last.x.max(400.0), last.y.max(20.0)),
        ))
        .layout(Layout::left_to_right(Align::Center));
    if unclipped {
        builder = builder.layer_id(egui::LayerId::new(egui::Order::Middle, id));
    }
    if !shown {
        builder = builder.invisible();
    }
    let mut child = ui.new_child(builder);
    if unclipped {
        child.set_clip_rect(ui.ctx().content_rect());
    }
    let vars = plugin_dom::vars();
    let cx = Cx {
        vars: &vars,
        viewport: plugin_dom::viewport(),
        keyframes: &overlay.keyframes,
    };
    let inherit = Inherit::root(ui);
    child.spacing_mut().item_spacing.x = 6.0;
    for node in overlay.at(key, position) {
        draw_at(&mut child, node, key, inherit, &cx);
    }
    let used = child.min_rect().size();
    if (used - last).length() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, used));
        plugin_dom::request_frame(ui.ctx());
    }
    Some(Rect::from_min_size(rect.min, used))
}

/// A plugin window's page (`plugin_windows.rs`): `root`, its `<html>`
/// element, drawn into `rect` (the window's inside) as a browser draws a
/// document, from the browser's defaults. `canvas` is the background that
/// covers the whole window: the root's, or the body's when the root has
/// none, as CSS gives the canvas (the caller took it off that element).
pub fn page(ui: &mut egui::Ui, root: &VNode, canvas: &Style, rect: Rect) {
    let vars = plugin_dom::vars();
    let overlay = plugin_dom::overlay();
    let none = BTreeMap::new();
    let cx = Cx {
        vars: &vars,
        viewport: [rect.width(), rect.height()],
        keyframes: overlay.as_ref().map_or(&none, |o| &o.keyframes),
    };
    if let Some(fill) = cx.color(&canvas.background_color) {
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    let idx = ui.painter().add(egui::Shape::Noop);
    paint_background(ui, idx, rect, canvas, 0.0, &cx);
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt(("luna-page", root.id))
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(rect);
    child.spacing_mut().item_spacing = Vec2::ZERO;
    // The root fills the window's width, as a document's root does.
    IN_COLUMN.with(|s| s.set(true));
    draw(&mut child, root, Inherit::page(ui), &cx);
}

/// Positioned (`absolute`/`fixed`) plugin elements appended to anchor `key`,
/// placed in the anchor's box (the audiovisualiser's bars fill the player
/// bar). Elements in the anchor's flow are left out: where they would go
/// among the host's own widgets is not known.
pub fn positioned_in(ui: &mut egui::Ui, key: &str) {
    let Some(overlay) = plugin_dom::overlay() else {
        return;
    };
    if !overlay
        .injected
        .iter()
        .any(|i| i.anchor == key && i.position == Position::Append)
    {
        return;
    }
    let vars = plugin_dom::vars();
    let cx = Cx {
        vars: &vars,
        viewport: plugin_dom::viewport(),
        keyframes: &overlay.keyframes,
    };
    let inherit = Inherit::root(ui);
    for node in overlay.at(key, Position::Append) {
        if positioned_kind(&node.style).is_some() {
            draw_at(ui, node, key, inherit, &cx);
        }
    }
}

/// Positioned plugin elements at `key`/`position` drawn in place, in the
/// layer being painted, whatever their `z-index`: inside the now-playing
/// cover's tilt they lean with the cover, as children of TIDAL's `_tilt_`
/// box do (3d-cover's canvas and spinner after the cover's `img`).
pub fn positioned_here(ui: &mut egui::Ui, key: &str, position: Position) {
    if let Some(container) = plugin_dom::host_rect(key) {
        positioned_over(ui, key, position, container);
    }
}

/// [`positioned_here`] in the box `container` the anchor is drawn in this
/// frame (a now-playing tab's pill, which a plugin's padding widens).
pub fn positioned_over(ui: &mut egui::Ui, key: &str, position: Position, container: Rect) {
    positioned_over_on(ui, key, position, container, None);
}

/// [`positioned_over`] on a box filled with `back` (the open now-playing
/// tab's white pill): plugin colours are kept readable against that fill,
/// not the window's. radiant-lyrics' `#1e1e1e` sparkle in the open Lyrics
/// tab was read against the dark window, turned white, and vanished into
/// the white pill.
pub fn positioned_over_on(
    ui: &mut egui::Ui,
    key: &str,
    position: Position,
    container: Rect,
    back: Option<Color32>,
) {
    let Some(overlay) = plugin_dom::overlay() else {
        return;
    };
    if !overlay
        .injected
        .iter()
        .any(|i| i.anchor == key && i.position == position)
    {
        return;
    }
    let vars = plugin_dom::vars();
    let cx = Cx {
        vars: &vars,
        viewport: plugin_dom::viewport(),
        keyframes: &overlay.keyframes,
    };
    let mut inherit = Inherit::root(ui);
    if let Some(back) = back {
        inherit.back = back;
    }
    for node in overlay.at(key, position) {
        if positioned_kind(&node.style).is_some() && !node.style.hidden() {
            positioned(ui, node, container, inherit, &cx);
        }
    }
}

/// Whether plugin elements at `anchor`/`position` sit in `body`, or
/// directly in TIDAL's root beside its own children
/// ([`crate::luna::dom_anchors::in_root`]): their box is the window's.
fn window_level(anchor: &str, position: Position) -> bool {
    anchor.is_empty() || crate::luna::dom_anchors::in_root(anchor, position)
}

/// Plugin elements appended to `body` (anchor `""`) or placed directly in
/// TIDAL's root: positioned ones (`fixed`/`absolute`, scroll-to-playing's
/// jump button, modals, betterFullscreen's player in the place of
/// `section._fullscreen_`) float over the window at their `top`/`right`/
/// `bottom`/`left`, above it for `z-index` > 0. Elements behind the page
/// (negative `z-index`) are drawn by [`backdrop`]; unpositioned ones are
/// not drawn.
pub fn floating(ui: &egui::Ui) {
    let Some(overlay) = plugin_dom::overlay() else {
        return;
    };
    if !overlay
        .injected
        .iter()
        .any(|i| window_level(&i.anchor, i.position))
    {
        return;
    }
    let vars = plugin_dom::vars();
    let cx = Cx {
        vars: &vars,
        viewport: plugin_dom::viewport(),
        keyframes: &overlay.keyframes,
    };
    let screen = ui.ctx().content_rect();
    let inherit = Inherit::body(ui);
    let nodes = overlay
        .injected
        .iter()
        .filter(|i| window_level(&i.anchor, i.position))
        .flat_map(|i| i.nodes.iter());
    for node in nodes {
        let style = &node.style;
        let positioned_now = style
            .position
            .as_deref()
            .is_some_and(|p| matches!(p.trim(), "fixed" | "absolute" | "sticky"));
        let z = z_index(style);
        if style.hidden() || !positioned_now || z < 0 {
            continue;
        }
        let inert = style
            .pointer_events
            .as_deref()
            .is_some_and(|p| p.trim() == "none");
        let (pos, placed) = layer_origin(ui.ctx(), ("luna-float", node.id), screen.min);
        egui::Area::new(egui::Id::new(("luna-float", node.id)))
            .order(if z > 0 {
                egui::Order::Foreground
            } else {
                egui::Order::Middle
            })
            .fixed_pos(pos)
            // Kept inside the window, the area would be moved back to the
            // corner while it still has the size it had there.
            .constrain(false)
            .interactable(!inert && placed)
            .show(ui.ctx(), |ui| {
                // A box over most of the window (a modal's backdrop) keeps
                // clicks from reaching the window under it (its size from
                // the last frame, so it sits under its own content).
                let big_id = egui::Id::new(("luna-float-big", node.id));
                let big: Option<Rect> = ui.data(|d| d.get_temp(big_id));
                if let Some(r) = big {
                    ui.interact(
                        r,
                        egui::Id::new(("luna-float-block", node.id)),
                        Sense::click(),
                    );
                }
                let used = positioned(ui, node, screen, inherit, &cx);
                let now = (!inert && used.area() > screen.area() * 0.5).then_some(used);
                if now != big {
                    ui.data_mut(|d| d.insert_temp(big_id, now));
                    plugin_dom::request_frame(ui.ctx());
                }
                ui.expand_to_include_rect(used);
                place_layer(ui.ctx(), ("luna-float", node.id), used);
            });
    }
}

// ---------------------------------------------------------------- effects

/// A CSS `transform` reduced to what the renderer applies: a translation
/// (points plus a fraction of the element's own size), a uniform scale and
/// a rotation in radians (drawn for images and background images).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Xf {
    tx: f32,
    ty: f32,
    fx: f32,
    fy: f32,
    scale: f32,
    rot: f32,
}

impl Default for Xf {
    fn default() -> Self {
        Xf {
            tx: 0.0,
            ty: 0.0,
            fx: 0.0,
            fy: 0.0,
            scale: 1.0,
            rot: 0.0,
        }
    }
}

fn angle(a: &str) -> f32 {
    let a = a.trim();
    let n = |s: &str| s.trim().parse::<f32>().unwrap_or(0.0);
    if let Some(v) = a.strip_suffix("deg") {
        n(v).to_radians()
    } else if let Some(v) = a.strip_suffix("grad") {
        n(v) * std::f32::consts::PI / 200.0
    } else if let Some(v) = a.strip_suffix("rad") {
        n(v)
    } else if let Some(v) = a.strip_suffix("turn") {
        n(v) * std::f32::consts::TAU
    } else {
        n(a).to_radians()
    }
}

impl Xf {
    fn parse(value: &str, font: f32) -> Xf {
        let mut x = Xf::default();
        let v = value.to_ascii_lowercase();
        let mut rest = v.as_str();
        while let Some(open) = rest.find('(') {
            let name = rest[..open].trim();
            let Some(close) = rest[open..].find(')') else {
                break;
            };
            let args: Vec<&str> = rest[open + 1..open + close]
                .split([',', ' '])
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .collect();
            rest = &rest[open + close + 1..];
            let len = |a: &str| -> (f32, f32) {
                match a.strip_suffix('%') {
                    Some(p) => (0.0, p.trim().parse::<f32>().unwrap_or(0.0) / 100.0),
                    None => (
                        css::length(a, font, 0.0, plugin_dom::viewport()).unwrap_or(0.0),
                        0.0,
                    ),
                }
            };
            let num = |a: &str| match a.strip_suffix('%') {
                Some(p) => p.trim().parse::<f32>().ok().map(|n| n / 100.0),
                None => a.parse::<f32>().ok(),
            };
            match name {
                "translate" | "translate3d" => {
                    if let Some(a) = args.first() {
                        let (p, f) = len(a);
                        x.tx += p;
                        x.fx += f;
                    }
                    if let Some(a) = args.get(1) {
                        let (p, f) = len(a);
                        x.ty += p;
                        x.fy += f;
                    }
                }
                "translatex" => {
                    let (p, f) = len(args.first().copied().unwrap_or("0"));
                    x.tx += p;
                    x.fx += f;
                }
                "translatey" => {
                    let (p, f) = len(args.first().copied().unwrap_or("0"));
                    x.ty += p;
                    x.fy += f;
                }
                "scale" | "scale3d" => {
                    let sx = args.first().and_then(|a| num(a)).unwrap_or(1.0);
                    let sy = args.get(1).and_then(|a| num(a)).unwrap_or(sx);
                    x.scale *= (sx + sy) / 2.0;
                }
                // One-axis scales stretch the box itself (see `stretch`).
                "scalex" | "scaley" => {}
                "rotate" | "rotatez" => x.rot += angle(args.first().copied().unwrap_or("0")),
                _ => {}
            }
        }
        x
    }

    fn lerp(a: Xf, b: Xf, t: f32) -> Xf {
        let l = |x: f32, y: f32| x + (y - x) * t;
        Xf {
            tx: l(a.tx, b.tx),
            ty: l(a.ty, b.ty),
            fx: l(a.fx, b.fx),
            fy: l(a.fy, b.fy),
            scale: l(a.scale, b.scale),
            rot: l(a.rot, b.rot),
        }
    }

    fn css(&self) -> String {
        format!(
            "translate({}px,{}px) translate({}%,{}%) scale({}) rotate({}rad)",
            self.tx,
            self.ty,
            self.fx * 100.0,
            self.fy * 100.0,
            self.scale,
            self.rot
        )
    }

    fn offset(&self, size: Vec2) -> Vec2 {
        vec2(self.tx + self.fx * size.x, self.ty + self.fy * size.y)
    }
}

fn rgba(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    format!("rgba({r},{g},{b},{})", f32::from(a) / 255.0)
}

/// A `background-position` as two lengths, keywords as percentages (a share
/// of the room around the image, as [`paint_background`] places it): what an
/// animation moves on a sprite sheet or a gradient.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BgPos {
    /// (value, whether it is a percentage) per axis.
    x: (f32, bool),
    y: (f32, bool),
}

impl BgPos {
    fn parse(value: &str, font: f32, viewport: [f32; 2]) -> Option<BgPos> {
        let lower = value.to_ascii_lowercase();
        let words = css::words(&lower);
        let axis = |w: &str| -> Option<(f32, bool)> {
            match w {
                "left" | "top" => Some((0.0, true)),
                "center" => Some((50.0, true)),
                "right" | "bottom" => Some((100.0, true)),
                w if w.ends_with('%') => Some((w.trim_end_matches('%').trim().parse().ok()?, true)),
                w => Some((css::length(w, font, 0.0, viewport)?, false)),
            }
        };
        let centre = (50.0, true);
        match words.as_slice() {
            [w] if matches!(*w, "top" | "bottom") => Some(BgPos {
                x: centre,
                y: axis(w)?,
            }),
            [w] => Some(BgPos {
                x: axis(w)?,
                y: centre,
            }),
            [a, b] if matches!(*a, "top" | "bottom") || matches!(*b, "left" | "right") => {
                Some(BgPos {
                    x: axis(b)?,
                    y: axis(a)?,
                })
            }
            [a, b] => Some(BgPos {
                x: axis(a)?,
                y: axis(b)?,
            }),
            _ => None,
        }
    }

    fn css(&self) -> String {
        let f = |(v, pct): (f32, bool)| {
            if pct {
                format!("{v}%")
            } else {
                format!("{v}px")
            }
        };
        format!("{} {}", f(self.x), f(self.y))
    }
}

impl css_motion::Lerp for BgPos {
    /// Lengths of the same kind blend; a length and a percentage swap half
    /// way, as CSS does for values it cannot blend.
    fn lerp(&self, to: &Self, t: f32) -> Self {
        let l = |a: (f32, bool), b: (f32, bool)| {
            if a.1 == b.1 {
                (a.0 + (b.0 - a.0) * t, a.1)
            } else if t < 0.5 {
                a
            } else {
                b
            }
        };
        BgPos {
            x: l(self.x, to.x),
            y: l(self.y, to.y),
        }
    }
}

impl css_motion::Lerp for Xf {
    fn lerp(&self, to: &Self, t: f32) -> Self {
        Xf::lerp(*self, *to, t)
    }
}

/// A `background-size` or `background-position` list (one entry per
/// background layer) an animation moves: each length between the two ends
/// as a `calc()` of both, worked out against the box when it is painted, so
/// `0 100%` to `100% 100%` and `calc(.75em / -2) 0` to `calc(100% + .75em /
/// 2) 0` blend as in a browser.
#[derive(Clone, PartialEq)]
struct BgList(String);

impl css_motion::Lerp for BgList {
    fn lerp(&self, to: &Self, t: f32) -> Self {
        if t <= 0.0 || self == to {
            return self.clone();
        }
        if t >= 1.0 {
            return to.clone();
        }
        let near = |a: &str, b: &str| if t < 0.5 { a } else { b }.to_string();
        // Keywords that are lengths along their axis.
        fn length(w: &str) -> &str {
            match w {
                "left" | "top" => "0%",
                "center" => "50%",
                "right" | "bottom" => "100%",
                w => w,
            }
        }
        let from = css::split_top(&self.0, ',');
        let to_list = css::split_top(&to.0, ',');
        if from.len() != to_list.len() {
            return BgList(near(&self.0, &to.0));
        }
        let layers: Vec<String> = from
            .iter()
            .zip(&to_list)
            .map(|(a, b)| {
                let (wa, wb) = (css::words(a), css::words(b));
                if wa.len() != wb.len() {
                    return near(a, b);
                }
                wa.iter()
                    .zip(&wb)
                    .map(|(x, y)| {
                        let (x, y) = (length(x), length(y));
                        if x == y {
                            x.to_string()
                        } else if [x, y]
                            .iter()
                            .any(|v| matches!(*v, "auto" | "cover" | "contain"))
                        {
                            near(x, y)
                        } else {
                            format!("calc(({x}) * {} + ({y}) * {t})", 1.0 - t)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        BgList(layers.join(","))
    }
}

/// What moves one plugin element: its transitions and its animations'
/// clock, kept while it is drawn.
#[derive(Default)]
struct NodeMotion {
    /// The pass it was last drawn in.
    pass: Option<u64>,
    opacity: Option<css_motion::Eased<f32>>,
    transform: Option<css_motion::Eased<Xf>>,
    color: Option<css_motion::Eased<Color32>>,
    background: Option<css_motion::Eased<Color32>>,
    width: Option<css_motion::Eased<f32>>,
    height: Option<css_motion::Eased<f32>>,
    clock: css_motion::Clock,
}

thread_local! {
    static MOTION: std::cell::RefCell<(u64, std::collections::HashMap<u64, NodeMotion>)> =
        std::cell::RefCell::new((0, std::collections::HashMap::new()));
}

/// The motion of plugin element `id`. One not drawn for more than a frame
/// starts over, as an element shown again does in CSS (its transitions do
/// not run from what it was before it went, its animations start again);
/// those no longer drawn are dropped every couple of seconds.
fn with_motion<R>(ctx: &egui::Context, id: u64, f: impl FnOnce(&mut NodeMotion) -> R) -> R {
    let pass = ctx.cumulative_pass_nr();
    MOTION.with(|m| {
        let (pruned, map) = &mut *m.borrow_mut();
        if pass >= *pruned + 120 {
            *pruned = pass;
            map.retain(|_, s| s.pass.is_some_and(|p| p + 2 >= pass));
        }
        let s = map.entry(id).or_default();
        if s.pass.is_some_and(|p| pass > p + 1) {
            *s = NodeMotion::default();
        }
        s.pass = Some(pass);
        f(s)
    })
}

/// One property's step: towards `target` by `spec`, from `from` on first
/// sight; the soonest wake of all goes in `wake`.
fn step<V: css_motion::Lerp>(
    slot: &mut Option<css_motion::Eased<V>>,
    target: V,
    from: Option<V>,
    spec: Option<&css_motion::Transition>,
    now: f64,
    wake: &mut Option<f64>,
) -> V {
    let eased =
        slot.get_or_insert_with(|| css_motion::Eased::new(from.unwrap_or_else(|| target.clone())));
    let (value, w) = eased.step(target, spec, now);
    if let Some(w) = w {
        *wake = Some(wake.map_or(w, |o| o.min(w)));
    }
    value
}

/// The keyframes of `frames` that set what `get` reads.
fn track<V: css_motion::Lerp>(
    frames: &[(f32, Style)],
    get: impl Fn(&Style) -> Option<V>,
) -> Option<css_motion::Track<V>> {
    let frames: Vec<_> = frames
        .iter()
        .filter_map(|(o, s)| Some((*o, get(s)?, css_motion::keyframe_timing(s))))
        .collect();
    (!frames.is_empty()).then_some(css_motion::Track { frames })
}

/// `style` as it shows now: its transitions under way (each property from
/// where it was when it changed) and its running animations over them.
/// `None` when nothing moves it. Asks for frames only while something
/// moves. `inherited` is the colour it inherits.
fn moved(
    ctx: &egui::Context,
    node: &VNode,
    style: &Style,
    inherited: Color32,
    cx: &Cx,
) -> Option<Style> {
    if node.id & MEASURE_ID != 0 {
        return None;
    }
    let now = ctx.input(|i| i.time);
    let reduced = crate::motion::reduced(ctx);
    let font = 14.0;
    let spec = |p: &str| {
        (!reduced && style.transition.is_some())
            .then(|| css_motion::transition(style, p))
            .flatten()
    };
    let opacity_of = |s: &Style| s.opacity.as_deref().and_then(css::opacity).unwrap_or(1.0);
    let xf_of = |s: &Style| {
        s.transform
            .as_deref()
            .map(|t| Xf::parse(t, font))
            .unwrap_or_default()
    };
    let color_of = |s: &Style| cx.color(&s.color).unwrap_or(inherited);
    let back_of = |s: &Style| {
        cx.color(&s.background_color)
            .unwrap_or(Color32::TRANSPARENT)
    };
    let from = node.from.as_ref();
    let anims = if style.animation.is_some() && !cx.keyframes.is_empty() {
        css_motion::animations(style)
    } else {
        Vec::new()
    };
    let mut wake = None;
    let out = with_motion(ctx, node.id, |m| {
        let mut out: Option<Style> = None;
        let target = opacity_of(style);
        let o = step(
            &mut m.opacity,
            target,
            from.map(opacity_of),
            spec("opacity").as_ref(),
            now,
            &mut wake,
        );
        if o != target {
            out.get_or_insert_with(|| style.clone()).opacity = Some(o.to_string());
        }
        let target = xf_of(style);
        let x = step(
            &mut m.transform,
            target,
            from.map(xf_of),
            spec("transform").as_ref(),
            now,
            &mut wake,
        );
        if x != target {
            out.get_or_insert_with(|| style.clone()).transform = Some(x.css());
        }
        let target = color_of(style);
        let c = step(
            &mut m.color,
            target,
            from.map(color_of),
            spec("color").as_ref(),
            now,
            &mut wake,
        );
        if c != target {
            out.get_or_insert_with(|| style.clone()).color = Some(rgba(c));
        }
        let target = back_of(style);
        let b = step(
            &mut m.background,
            target,
            from.map(back_of),
            spec("background-color").as_ref(),
            now,
            &mut wake,
        );
        if b != target {
            out.get_or_insert_with(|| style.clone()).background_color = Some(rgba(b));
        }
        if !anims.is_empty() {
            let moments = css_motion::moments_with(ctx, &mut m.clock, &anims);
            let base = out.clone().unwrap_or_else(|| style.clone());
            let own_opacity = opacity_of(&base);
            let own_xf = xf_of(&base);
            let own_color = color_of(&base);
            let own_back = back_of(&base);
            let own_pos = base
                .background_position
                .as_deref()
                .and_then(|v| BgPos::parse(v, font, cx.viewport))
                .unwrap_or(BgPos {
                    x: (0.0, false),
                    y: (0.0, false),
                });
            for (a, moment) in anims.iter().zip(moments) {
                let Some(moment) = moment else {
                    continue;
                };
                let Some(frames) = cx.keyframes.get(&a.name).filter(|f| !f.is_empty()) else {
                    continue;
                };
                let p = moment.progress;
                let s = out.get_or_insert_with(|| base.clone());
                if let Some(t) = track(frames, |f| f.opacity.as_deref().and_then(css::opacity)) {
                    s.opacity = Some(t.at(p, &own_opacity, &a.timing).to_string());
                }
                if let Some(t) = track(frames, |f| {
                    f.transform.as_deref().map(|v| Xf::parse(v, font))
                }) {
                    s.transform = Some(t.at(p, &own_xf, &a.timing).css());
                }
                if let Some(t) = track(frames, |f| cx.color(&f.color)) {
                    s.color = Some(rgba(t.at(p, &own_color, &a.timing)));
                }
                if let Some(t) = track(frames, |f| cx.color(&f.background_color)) {
                    s.background_color = Some(rgba(t.at(p, &own_back, &a.timing)));
                }
                if let Some(t) = track(frames, |f| {
                    f.background_position
                        .as_deref()
                        .and_then(|v| BgPos::parse(v, font, cx.viewport))
                }) {
                    s.background_position = Some(t.at(p, &own_pos, &a.timing).css());
                } else if let Some(t) = track(frames, |f| f.background_position.clone().map(BgList))
                {
                    // One position per layer (a list): blended entry by entry.
                    let own = BgList(base.background_position.clone().unwrap_or_default());
                    s.background_position = Some(t.at(p, &own, &a.timing).0);
                }
                if let Some(t) = track(frames, |f| f.background_size.clone().map(BgList)) {
                    let own = BgList(base.background_size.clone().unwrap_or_default());
                    s.background_size = Some(t.at(p, &own, &a.timing).0);
                }
            }
        }
        out
    });
    css_motion::wake(ctx, wake);
    out
}

/// A width or height of plugin element `node` that its style transitions
/// (`width .3s`): where it is on its way to `target` points.
fn moved_length(
    ctx: &egui::Context,
    node: &VNode,
    style: &Style,
    property: &str,
    target: Option<f32>,
) -> Option<f32> {
    let target = target?;
    if node.id & MEASURE_ID != 0 || style.transition.is_none() {
        return Some(target);
    }
    let spec = (!crate::motion::reduced(ctx))
        .then(|| css_motion::transition(style, property))
        .flatten();
    let now = ctx.input(|i| i.time);
    let mut wake = None;
    let value = with_motion(ctx, node.id, |m| {
        let slot = if property == "width" {
            &mut m.width
        } else {
            &mut m.height
        };
        step(slot, target, None, spec.as_ref(), now, &mut wake)
    });
    css_motion::wake(ctx, wake);
    Some(value)
}

/// A box's fill and border drawn by hand, turned `rot` radians about its
/// centre, each side (top, right, bottom, left) in its own colour: the ring
/// of a CSS spinner (`border-radius: 50%`, a bright `border-top-color`,
/// `animation: spin 1s linear infinite`) turns as in a browser. A round box
/// is a circle whose sides are its quarters; any other is its rectangle
/// (corners of a turned box are not rounded).
/// A hand-drawn box's border: each side's width and colour, and the dash
/// and gap lengths of a `dashed` or `dotted` one.
#[derive(Clone, Copy)]
struct Edges {
    widths: [f32; 4],
    colours: [Color32; 4],
    dash: Option<(f32, f32)>,
}

/// The outline of `q` with corners rounded by `radius`, clockwise from the
/// top-left corner's arc.
fn rounded_points(q: Rect, radius: f32) -> Vec<Pos2> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let rad = radius.min(q.width() / 2.0).min(q.height() / 2.0).max(0.0);
    if rad <= 0.0 {
        return vec![
            q.left_top(),
            q.right_top(),
            q.right_bottom(),
            q.left_bottom(),
        ];
    }
    let centres = [
        (q.left_top() + vec2(rad, rad), PI),
        (q.right_top() + vec2(-rad, rad), PI + FRAC_PI_2),
        (q.right_bottom() + vec2(-rad, -rad), 0.0),
        (q.left_bottom() + vec2(rad, -rad), FRAC_PI_2),
    ];
    let steps = 8;
    let mut points = Vec::with_capacity(4 * (steps + 1));
    for (c, begin) in centres {
        for i in 0..=steps {
            let a = begin + FRAC_PI_2 * i as f32 / steps as f32;
            points.push(c + vec2(a.cos(), a.sin()) * rad);
        }
    }
    points
}

fn box_shape(
    r: Rect,
    radius: f32,
    fill: Option<Color32>,
    edges: Option<Edges>,
    rot: f32,
) -> egui::Shape {
    let border = edges.map(|e| (e.widths[0], e.colours));
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
    let mut shapes = Vec::new();
    let c = r.center();
    let round =
        (r.width() - r.height()).abs() < 1.0 && radius >= r.width().min(r.height()) / 2.0 - 0.5;
    if round {
        let outer = r.width().min(r.height()) / 2.0;
        if let Some(f) = fill {
            shapes.push(egui::Shape::circle_filled(c, outer, f));
        }
        if let Some((w, colours)) = border {
            let mid = (outer - w / 2.0).max(0.0);
            if colours.iter().all(|k| *k == colours[0]) {
                shapes.push(egui::Shape::circle_stroke(
                    c,
                    mid,
                    Stroke::new(w, colours[0]),
                ));
            } else {
                // Screen angles grow clockwise (y points down): the top side
                // is centred at -90 degrees. Neighbouring sides of one colour
                // are one arc, so the ring only breaks where colours change.
                let first = (0..4)
                    .find(|&s| colours[s] != colours[(s + 3) % 4])
                    .unwrap_or(0);
                let mut side = first;
                loop {
                    let colour = colours[side];
                    let mut quarters = 1;
                    while quarters < 4 && colours[(side + quarters) % 4] == colour {
                        quarters += 1;
                    }
                    let begin = -FRAC_PI_2 - FRAC_PI_4 + side as f32 * FRAC_PI_2 + rot;
                    let steps = 12 * quarters;
                    let points: Vec<Pos2> = (0..=steps)
                        .map(|i| {
                            let a = begin + FRAC_PI_2 * i as f32 / 12.0;
                            c + vec2(a.cos(), a.sin()) * mid
                        })
                        .collect();
                    shapes.push(egui::Shape::line(points, Stroke::new(w, colour)));
                    side = (side + quarters) % 4;
                    if side == first {
                        break;
                    }
                }
            }
        }
        return egui::Shape::Vec(shapes);
    }
    let turn = egui::emath::Rot2::from_angle(rot);
    let at = |p: Pos2| c + turn * (p - c);
    let corners = |q: Rect| {
        [
            q.left_top(),
            q.right_top(),
            q.right_bottom(),
            q.left_bottom(),
        ]
        .map(at)
    };
    if let Some(f) = fill {
        let outline: Vec<Pos2> = rounded_points(r, radius).into_iter().map(at).collect();
        shapes.push(egui::Shape::convex_polygon(outline, f, Stroke::NONE));
    }
    let Some(e) = edges else {
        return egui::Shape::Vec(shapes);
    };
    let [t, rt, b, l] = e.widths;
    if e.widths.iter().any(|w| *w != t) {
        // Sides of their own widths (a CSS triangle: a box with no size,
        // `border: 10px solid transparent; border-top-color: #111;
        // border-bottom: 0`): each side is the trapezoid between the outer
        // and the inner corners.
        let outer = corners(r);
        let inner = corners(Rect::from_min_max(
            r.min + vec2(l, t),
            egui::pos2(
                (r.max.x - rt).max(r.min.x + l),
                (r.max.y - b).max(r.min.y + t),
            ),
        ));
        for side in 0..4 {
            let colour = e.colours[side];
            if e.widths[side] <= 0.0 || colour.a() == 0 {
                continue;
            }
            let next = (side + 1) % 4;
            shapes.push(egui::Shape::convex_polygon(
                vec![outer[side], outer[next], inner[next], inner[side]],
                colour,
                Stroke::NONE,
            ));
        }
        return egui::Shape::Vec(shapes);
    }
    let w = t;
    if (radius > 0.0 || e.dash.is_some()) && e.colours.iter().all(|k| *k == e.colours[0]) {
        let mut path: Vec<Pos2> = rounded_points(r.shrink(w / 2.0), radius - w / 2.0)
            .into_iter()
            .map(at)
            .collect();
        let stroke = Stroke::new(w, e.colours[0]);
        match e.dash {
            Some((dash, gap)) => {
                path.push(path[0]);
                shapes.extend(egui::Shape::dashed_line(&path, stroke, dash, gap));
            }
            None => shapes.push(egui::Shape::closed_line(path, stroke)),
        }
        return egui::Shape::Vec(shapes);
    }
    let k = corners(r.shrink(w / 2.0));
    for side in 0..4 {
        shapes.push(egui::Shape::line_segment(
            [k[side], k[(side + 1) % 4]],
            Stroke::new(w, e.colours[side]),
        ));
    }
    egui::Shape::Vec(shapes)
}

/// The first `url(...)` in a background value.
fn background_url(value: &str) -> Option<&str> {
    let at = value.find("url(")?;
    let rest = &value[at + 4..];
    let end = rest.find(')')?;
    Some(rest[..end].trim().trim_matches(['"', '\'']))
}

/// Paint a `url()` background image into `rect` at the painter slot `idx`
/// (so it sits under the element's content): `background-size`
/// (`cover`/`contain`/lengths), `background-position` (keywords, `%`, px:
/// sprite sheets), nearest filtering for `image-rendering: pixelated`, and
/// the element's rotation.
fn paint_background(
    ui: &egui::Ui,
    idx: egui::layers::ShapeIdx,
    rect: Rect,
    style: &Style,
    rot: f32,
    cx: &Cx,
) {
    let Some(value) = style.background_image.as_deref() else {
        return;
    };
    let value = css::resolve_vars(value, cx.vars);
    let Some(url) = background_url(&value) else {
        return;
    };
    let Some(source) = source_of(url, ui.visuals().text_color()) else {
        return;
    };
    let nearest = style
        .image_rendering
        .as_deref()
        .is_some_and(|r| r.contains("pixel") || r.contains("crisp"));
    let image = match source {
        ImageSource::Uri(uri) => egui::Image::new(uri),
        ImageSource::Bytes(uri, bytes) => egui::Image::from_bytes(uri, bytes),
    }
    .texture_options(if nearest {
        egui::TextureOptions::NEAREST
    } else {
        egui::TextureOptions::LINEAR
    });
    let Ok(egui::load::TexturePoll::Ready { texture }) = image.load_for_size(ui.ctx(), rect.size())
    else {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
        return;
    };
    let natural = texture.size;
    if natural.x <= 0.0 || natural.y <= 0.0 {
        return;
    }
    let font = 14.0;
    let size_v = style
        .background_size
        .as_deref()
        .unwrap_or("auto")
        .trim()
        .to_ascii_lowercase();
    let size = match size_v.as_str() {
        "cover" => natural * (rect.width() / natural.x).max(rect.height() / natural.y),
        "contain" => natural * (rect.width() / natural.x).min(rect.height() / natural.y),
        v => {
            let parts = css::words(v);
            let w = parts
                .first()
                .and_then(|w| css::length(w, font, rect.width(), cx.viewport));
            let h = parts
                .get(1)
                .and_then(|h| css::length(h, font, rect.height(), cx.viewport));
            match (w, h) {
                (Some(w), Some(h)) => vec2(w, h),
                (Some(w), None) => vec2(w, w * natural.y / natural.x),
                (None, Some(h)) => vec2(h * natural.x / natural.y, h),
                (None, None) => natural,
            }
        }
    };
    let pos_v = style
        .background_position
        .as_deref()
        .unwrap_or("0 0")
        .to_ascii_lowercase();
    let mut parts: Vec<&str> = css::words(&pos_v);
    if parts.len() == 1 {
        parts.push(if matches!(parts[0], "top" | "bottom") {
            parts[0]
        } else {
            "center"
        });
        if matches!(parts[0], "top" | "bottom") {
            parts[0] = "center";
        }
    }
    let axis = |v: &str, free: f32, reference: f32| match v {
        "left" | "top" => 0.0,
        "center" => free / 2.0,
        "right" | "bottom" => free,
        v if v.ends_with('%') => {
            free * v.trim_end_matches('%').parse::<f32>().unwrap_or(0.0) / 100.0
        }
        v => css::length(v, font, reference, cx.viewport).unwrap_or(0.0),
    };
    let (px, py) = if parts.len() >= 2 && matches!(parts[0], "top" | "bottom") {
        (parts[1], parts[0])
    } else {
        (parts[0], *parts.get(1).unwrap_or(&"0"))
    };
    let x = axis(px, rect.width() - size.x, rect.width());
    let y = axis(py, rect.height() - size.y, rect.height());
    let img = Rect::from_min_size(rect.min + vec2(x, y), size);
    let clip = img.intersect(rect);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let uv = Rect::from_min_max(
        Pos2::new(
            (clip.min.x - img.min.x) / size.x,
            (clip.min.y - img.min.y) / size.y,
        ),
        Pos2::new(
            (clip.max.x - img.min.x) / size.x,
            (clip.max.y - img.min.y) / size.y,
        ),
    );
    let tint = Color32::WHITE.gamma_multiply(ui.opacity());
    // `filter: blur()`: a copy scaled down to about one texel per blur
    // radius, drawn with linear filtering (spinny-cover's backdrop).
    let blur = style
        .filter
        .as_deref()
        .and_then(blur_radius)
        .map(|r| css::length(&r, font, 0.0, cx.viewport).unwrap_or(0.0))
        .unwrap_or(0.0);
    let texture_id = if blur >= 1.0 {
        blurred(
            ui.ctx(),
            image.source(ui.ctx()).uri().unwrap_or_default(),
            size,
            blur,
        )
        .unwrap_or(texture.id)
    } else {
        texture.id
    };
    let mut mesh = egui::Mesh::with_texture(texture_id);
    mesh.add_rect_with_uv(clip, uv, tint);
    if rot != 0.0 {
        let c = rect.center();
        let rotation = egui::emath::Rot2::from_angle(rot);
        for v in &mut mesh.vertices {
            v.pos = c + rotation * (v.pos - c);
        }
    }
    ui.painter().set(idx, egui::Shape::mesh(mesh));
}

/// Paint the `url()` image of `style`'s background into `rect` (an anchor's
/// box or `::before` layer), sized and placed as the style says.
pub(crate) fn paint_style_image(ui: &egui::Ui, rect: Rect, style: &Style) {
    let vars = plugin_dom::vars();
    let none = BTreeMap::new();
    let cx = Cx {
        vars: &vars,
        viewport: plugin_dom::viewport(),
        keyframes: &none,
    };
    let idx = ui.painter().add(egui::Shape::Noop);
    paint_background(ui, idx, rect, style, 0.0, &cx);
}

/// `filter`/`backdrop-filter` `blur(...)` radius in points.
pub(crate) fn blur_points(filter: &str, viewport: [f32; 2]) -> Option<f32> {
    let r = blur_radius(filter)?;
    css::length(&r, 14.0, 0.0, viewport).filter(|r| *r >= 1.0)
}

/// A blurred copy of an image egui has loaded (see [`blurred`]).
pub(crate) fn blurred_texture(
    ctx: &egui::Context,
    uri: &str,
    size: Vec2,
    radius: f32,
) -> Option<egui::TextureId> {
    blurred(ctx, uri, size, radius)
}

/// The length inside a `filter`'s `blur(...)`, if any.
/// `border-radius: 50%` of a `w` by `h` box: a share of its shorter side.
fn radius_share(value: &str, w: f32, h: f32) -> Option<f32> {
    let pct = value.trim().strip_suffix('%')?.trim().parse::<f32>().ok()?;
    Some(pct / 100.0 * w.min(h))
}

fn blur_radius(filter: &str) -> Option<String> {
    let at = filter.find("blur(")?;
    let rest = &filter[at + 5..];
    Some(rest[..rest.find(')')?].trim().to_string())
}

/// The factor of a `filter`'s `brightness(...)` (number or percentage).
fn brightness(filter: &str) -> Option<f32> {
    let at = filter.find("brightness(")?;
    let rest = &filter[at + 11..];
    let v = rest[..rest.find(')')?].trim();
    let n = match v.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok()? / 100.0,
        None => v.parse::<f32>().ok()?,
    };
    Some(n.max(0.0))
}

type BlurKey = (String, u32);

thread_local! {
    /// Blurred copies of background images, by URI and texel width.
    static BLURRED: std::cell::RefCell<std::collections::HashMap<BlurKey, Option<egui::TextureHandle>>> =
        std::cell::RefCell::default();
}

/// Blurred images made off the UI thread, waiting to become textures.
static BLUR_DONE: std::sync::Mutex<Vec<(BlurKey, Option<egui::ColorImage>)>> =
    std::sync::Mutex::new(Vec::new());

/// A copy of image `uri` blurred by `radius` points when drawn at `size`:
/// scaled to about one texel per radius and smoothed, made on a worker
/// thread from the bytes egui already loaded. `None` until it is ready.
fn blurred(ctx: &egui::Context, uri: &str, size: Vec2, radius: f32) -> Option<egui::TextureId> {
    if uri.is_empty() {
        return None;
    }
    let texels = (size.x.max(size.y) / radius).clamp(8.0, 256.0);
    let key: BlurKey = (uri.to_string(), texels.log2().round().exp2() as u32);
    let done: Vec<_> = std::mem::take(&mut *BLUR_DONE.lock().unwrap_or_else(|p| p.into_inner()));
    BLURRED.with(|cache| {
        let mut cache = cache.borrow_mut();
        for (k, img) in done {
            let tex = img.map(|img| {
                ctx.load_texture(
                    format!("luna-blur-{}-{}", k.0, k.1),
                    img,
                    egui::TextureOptions::LINEAR,
                )
            });
            cache.insert(k, tex);
        }
        if let Some(entry) = cache.get(&key) {
            return entry.as_ref().map(egui::TextureHandle::id);
        }
        let Ok(egui::load::BytesPoll::Ready { bytes, .. }) = ctx.try_load_bytes(uri) else {
            return None;
        };
        // Pending: a `None` entry until the worker hands its result over.
        cache.insert(key.clone(), None);
        let bytes = bytes.to_vec();
        let ctx = ctx.clone();
        let side = key.1;
        let spawned = std::thread::Builder::new()
            .name("luna-blur".into())
            .spawn(move || {
                let img = image::load_from_memory(&bytes).ok().map(|img| {
                    let (w, h) = (img.width().max(1), img.height().max(1));
                    let scale = side as f32 / w.max(h) as f32;
                    let small = img.resize_exact(
                        ((w as f32 * scale).round() as u32).max(2),
                        ((h as f32 * scale).round() as u32).max(2),
                        image::imageops::FilterType::Triangle,
                    );
                    let soft = image::imageops::blur(&small.to_rgba8(), 1.0);
                    egui::ColorImage::from_rgba_unmultiplied(
                        [soft.width() as usize, soft.height() as usize],
                        soft.as_raw(),
                    )
                });
                BLUR_DONE
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push((key, img));
                ctx.request_repaint();
            });
        if spawned.is_err() {
            return None;
        }
        // Until then the sharp image stands in.
        None
    })
}

fn positioned_kind(style: &Style) -> Option<&str> {
    let p = style.position.as_deref()?.trim();
    matches!(p, "absolute" | "fixed").then_some(p)
}

/// Set on the id of the copy of an element drawn only to measure it: events,
/// hover and measure reports for such an id are dropped.
pub const MEASURE_ID: u64 = 1 << 62;

/// Some element below has a `width` that is a share of its parent.
fn has_percent_width<'a>(mut nodes: impl Iterator<Item = &'a VNode>) -> bool {
    nodes.any(|n| {
        n.style
            .width
            .as_deref()
            .is_some_and(|w| w.trim().ends_with('%'))
            || has_percent_width(n.children.iter())
    })
}

/// `node` with the shares of the parent's width taken out and ids marked as
/// a measuring copy.
fn without_percent_widths(node: &VNode) -> VNode {
    let mut copy = node.clone();
    copy.id |= MEASURE_ID;
    if copy
        .style
        .width
        .as_deref()
        .is_some_and(|w| w.trim().ends_with('%'))
    {
        copy.style.width = None;
    }
    copy.children = node.children.iter().map(without_percent_widths).collect();
    copy
}

/// The width a box with no width of its own takes: its content's, a child's
/// share of the box's width counting as `auto` (a cycle otherwise), at most
/// the room there is. Drawn once, unseen, in `room`.
fn preferred_width(ui: &mut egui::Ui, node: &VNode, room: Rect, inherit: Inherit, cx: &Cx) -> f32 {
    let copy = without_percent_widths(node);
    let mut probe = ui.new_child(
        UiBuilder::new()
            .id_salt(("luna-probe", node.id))
            .max_rect(room)
            .layout(Layout::top_down(Align::Min))
            .invisible(),
    );
    draw(&mut probe, &copy, inherit, cx);
    probe.min_rect().width().min(room.width())
}

/// Draw an `absolute`/`fixed` element inside `container` at its
/// `top/right/bottom/left` (both of a pair give the size), offset by its
/// transform's translation. Sizes from the last frame place right/bottom
/// anchored and percentage-translated elements.
fn positioned(ui: &mut egui::Ui, node: &VNode, container: Rect, inherit: Inherit, cx: &Cx) -> Rect {
    // Animate here so an animated translation moves the box once.
    let animated = moved(ui.ctx(), node, &node.style, inherit.color, cx);
    let style = animated.as_ref().unwrap_or(&node.style);
    let id = egui::Id::new(("luna-pos", node.id));
    let last: Vec2 = ui.data(|d| d.get_temp(id)).unwrap_or(Vec2::ZERO);
    let (w, h) = (container.width(), container.height());
    let font = inherit.size;
    let left = cx.len(&style.left, font, w);
    let right = cx.len(&style.right, font, w);
    let top = cx.len(&style.top, font, h);
    let bottom = cx.len(&style.bottom, font, h);
    let width = cx.len(&style.width, font, w).or(match (left, right) {
        (Some(l), Some(r)) => Some((w - l - r).max(0.0)),
        _ => None,
    });
    let height = cx.len(&style.height, font, h).or(match (top, bottom) {
        (Some(t), Some(b)) => Some((h - t - b).max(0.0)),
        _ => None,
    });
    // A `border-box` is never smaller than its padding and border (a CSS
    // triangle: `width: 0; height: 0` with a 10 px border is 20 by 10).
    let least = frame_extent(style, font, cx);
    let width = width.map(|w| w.max(least.x));
    let height = height.map(|h| h.max(least.y));
    let size = vec2(width.unwrap_or(last.x), height.unwrap_or(last.y));
    let x = match (left, right) {
        (Some(l), _) => container.left() + l,
        (None, Some(r)) => container.right() - r - size.x,
        _ => container.left(),
    };
    let y = match (top, bottom) {
        (Some(t), _) => container.top() + t,
        (None, Some(b)) => container.bottom() - b - size.y,
        _ => container.top(),
    };
    let xf = style
        .transform
        .as_deref()
        .map(|t| Xf::parse(t, font))
        .unwrap_or_default();
    let min = Pos2::new(x, y) + xf.offset(size);
    let mut styled = node.clone();
    styled.style = style.clone();
    // Moved once, here.
    styled.style.animation = None;
    styled.style.transition = None;
    styled.from = None;
    // No width of its own: the box is as wide as its content wants (CSS 2.1
    // 10.3.7, shrink-to-fit), and a child's `width: 100%` is a share of that,
    // not of the window (element-hider's menu items made the menu as wide
    // as the window).
    let width = width.or_else(|| {
        has_percent_width(node.children.iter()).then(|| {
            let room = vec2(w.max(last.x).max(1.0), h.max(last.y).max(1.0));
            preferred_width(ui, &styled, Rect::from_min_size(min, room), inherit, cx)
        })
    });
    if let Some(w) = width {
        styled.style.width = Some(format!("{w}px"));
    }
    if let Some(h) = height {
        styled.style.height = Some(format!("{h}px"));
    }
    // The translation is applied here; draw() only rotates/scales.
    styled.style.transform = (xf.rot != 0.0 || xf.scale != 1.0).then(|| {
        Xf {
            scale: xf.scale,
            rot: xf.rot,
            ..Xf::default()
        }
        .css()
    });
    // Its position and translation are worked out here and taken off the
    // style below; whether it forms a stacking context goes with it.
    let context = stacking_context(style);
    styled.style.position = None;
    let max = vec2(
        width.unwrap_or(w.max(last.x).max(1.0)),
        height.unwrap_or(h.max(last.y).max(1.0)),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(Rect::from_min_size(min, max))
            .layout(Layout::top_down(Align::Min)),
    );
    OWN_CONTEXT.with(|c| c.set(context));
    draw_styled(&mut child, &styled, &styled.style, inherit, cx);
    OWN_CONTEXT.with(|c| c.set(false));
    let used = child.min_rect();
    if (used.size() - last).length() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, used.size()));
        plugin_dom::request_frame(ui.ctx());
    }
    used
}

/// The room a box's padding and border take, across and down.
fn frame_extent(style: &Style, font: f32, cx: &Cx) -> Vec2 {
    let mut pad = style.padding.as_deref().map_or([0.0; 4], |p| {
        css::sides(&css::resolve_vars(p, cx.vars), font)
    });
    for (i, side) in [
        &style.padding_top,
        &style.padding_right,
        &style.padding_bottom,
        &style.padding_left,
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(v) = cx.len(side, font, 0.0) {
            pad[i] = v;
        }
    }
    let border = style
        .border
        .as_deref()
        .and_then(|b| css::border(&css::resolve_vars(b, cx.vars)))
        .map_or(0.0, |(w, _)| w);
    let mut bw = [border; 4];
    if border > 0.0
        && let Some(v) = style.border_width.as_deref()
    {
        let w: Vec<f32> = css::words(&css::resolve_vars(v, cx.vars))
            .iter()
            .map(|w| css::length(w, font, 0.0, cx.viewport).unwrap_or(0.0))
            .collect();
        if let [t, r, b, l] = w.as_slice() {
            bw = [*t, *r, *b, *l];
        }
    }
    vec2(
        pad[1] + pad[3] + bw[1] + bw[3],
        pad[0] + pad[2] + bw[0] + bw[2],
    )
}

/// Fixed/absolute elements plugins put in `body` with a negative `z-index`
/// (spinny-cover's spinning cover, StarryNight's sky): drawn under the
/// panels, which turn see-through while one shows.
pub fn backdrop(ui: &egui::Ui) {
    let Some(overlay) = plugin_dom::overlay() else {
        return;
    };
    if behind(&overlay).next().is_none() {
        return;
    }
    let vars = plugin_dom::vars();
    let cx = Cx {
        vars: &vars,
        viewport: plugin_dom::viewport(),
        keyframes: &overlay.keyframes,
    };
    let screen = ui.ctx().content_rect();
    let inherit = Inherit::root(ui);
    let mut layer = egui::Ui::new(
        ui.ctx().clone(),
        egui::Id::new("luna-backdrop"),
        UiBuilder::new().layer_id(ui.layer_id()).max_rect(screen),
    );
    for node in behind(&overlay) {
        positioned(&mut layer, node, screen, inherit, &cx);
    }
}

fn z_index(style: &Style) -> i64 {
    style
        .z_index
        .as_deref()
        .and_then(|z| z.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

/// Elements drawn by [`backdrop`]: positioned body elements behind the page
/// (negative `z-index`), and `position: fixed` ones behind it wherever they
/// were inserted, as fixed boxes leave their parent for the viewport
/// (radiant-lyrics' WebGL background inside `main`).
fn behind(overlay: &crate::luna::dom::DomOverlay) -> impl Iterator<Item = &VNode> {
    overlay
        .injected
        .iter()
        .flat_map(|i| {
            let body = window_level(&i.anchor, i.position);
            i.nodes.iter().map(move |n| (body, n))
        })
        .filter(|(body, n)| {
            let placed = if *body {
                positioned_kind(&n.style).is_some()
            } else {
                fixed_behind(n)
            };
            placed && z_index(&n.style) < 0 && !n.style.hidden()
        })
        .map(|(_, n)| n)
}

/// Whether an element forms a stacking context (CSS 2.1 9.9.1, CSS
/// Positioned Layout 3): positioned with an integer `z-index`, or with an
/// `opacity` under 1, a `transform`, a `filter` or a `backdrop-filter`. Its
/// children with a negative `z-index` paint over its own background.
fn stacking_context(style: &Style) -> bool {
    let set = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .is_some_and(|v| !v.is_empty() && !v.eq_ignore_ascii_case("none"))
    };
    let z = style
        .z_index
        .as_deref()
        .is_some_and(|z| z.trim().parse::<i64>().is_ok());
    (positioned_kind(style).is_some() && z)
        || style
            .opacity
            .as_deref()
            .and_then(|o| o.trim().parse::<f32>().ok())
            .is_some_and(|o| o < 1.0)
        || set(&style.transform)
        || set(&style.filter)
        || set(&style.backdrop_filter)
}

/// A `position: fixed` element behind the page: drawn by [`backdrop`], not
/// in its slot.
fn fixed_behind(node: &VNode) -> bool {
    positioned_kind(&node.style) == Some("fixed") && z_index(&node.style) < 0
}

/// Some body element is drawn under the panels.
pub fn has_backdrop() -> bool {
    plugin_dom::overlay().is_some_and(|o| behind(&o).next().is_some())
}

/// Whether anything is injected at `key` in `position`.
pub fn any_at(key: &str, position: Position) -> bool {
    plugin_dom::overlay().is_some_and(|o| {
        o.injected
            .iter()
            .any(|i| i.anchor == key && i.position == position && !i.nodes.is_empty())
    })
}

/// Whether anything is injected at `key` in any position.
pub fn any(key: &str) -> bool {
    plugin_dom::overlay().is_some_and(|o| o.injected.iter().any(|i| i.anchor == key))
}

fn block(tag: &str) -> bool {
    matches!(
        tag,
        "html"
            | "body"
            | "div"
            | "p"
            | "ul"
            | "ol"
            | "li"
            | "section"
            | "header"
            | "footer"
            | "nav"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "form"
            | "table"
            | "tr"
            | "hr"
            | "menu"
    )
}

/// CSS `font-weight: bolder` from the parent's weight.
fn bolder(weight: u16) -> u16 {
    match weight {
        0..350 => 400,
        350..550 => 700,
        _ => 900,
    }
}

/// Draw `add` in a vertical scroll area when `scroll` gives its height
/// (`true`: the box is that tall; `false`: at most that tall), else as it is.
fn scroll_box<R>(
    ui: &mut egui::Ui,
    id: u64,
    scroll: Option<(f32, bool)>,
    style: &Style,
    offset: Option<f32>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    match scroll {
        Some((height, fixed)) => {
            let mut area = egui::ScrollArea::vertical()
                .id_salt(("luna-scroll", id))
                .max_height(height.max(1.0))
                // egui's 64-point least height for a scroll area: a box of a
                // definite height is that tall (lyrics-everywhere's lyrics
                // got 64 points of a 14-point share and ran past the window),
                // and one held to a smaller `max-height` is at most that.
                .min_scrolled_height(if fixed {
                    height.max(0.0)
                } else {
                    height.clamp(0.0, 64.0)
                })
                .auto_shrink([false, !fixed]);
            // `scrollbar-width: none` scrolls with no bar
            // (lyrics-everywhere's lyrics).
            if style
                .scrollbar_width
                .as_deref()
                .is_some_and(|w| w.trim() == "none")
            {
                area = area
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            }
            // `scrollTop =`, `scrollTo()`, `scrollBy()` from the page.
            if let Some(top) = offset {
                area = area.vertical_scroll_offset(top.max(0.0));
            }
            let out = area.show(ui, add);
            // The page reads where the box is (`scrollTop`, `scrollHeight`)
            // and hears `scroll` when the user or a script moves it; the
            // first report and a change of what it holds only update what
            // it reads, as no scroll happened.
            let top = out.state.offset.y;
            let held = out.content_size.y;
            let sent_id = egui::Id::new(("luna-scroll-sent", id));
            let last: Option<[f32; 2]> = ui.data(|d| d.get_temp(sent_id));
            let moved = last.is_some_and(|l| (l[0] - top).abs() > 0.5);
            if moved || last.is_none_or(|l| (l[1] - held).abs() > 0.5) {
                ui.data_mut(|d| d.insert_temp(sent_id, [top, held]));
                plugin_dom::send(
                    DomTarget::Node(id),
                    "scroll",
                    json!({
                        "scrollTop": top,
                        "scrollHeight": held.max(out.inner_rect.height()),
                        "quiet": !moved,
                    }),
                );
            }
            out.inner
        }
        None => add(ui),
    }
}

/// Whether a child sits on a line of text: a text node, an element with
/// `display: inline*`, or a phrasing tag with no `display` of its own.
fn inline_level(n: &VNode) -> bool {
    let d = n.style.display.as_deref().map_or("", str::trim);
    if d.starts_with("inline") {
        return true;
    }
    d.is_empty()
        && matches!(
            n.tag.as_str(),
            "#text"
                | "span"
                | "a"
                | "b"
                | "i"
                | "em"
                | "strong"
                | "small"
                | "label"
                | "code"
                | "mark"
                | "u"
                | "s"
                | "sub"
                | "sup"
                | "abbr"
        )
}

/// A block box whose children are all inline-level and either at least one
/// asks for `display: inline*` explicitly (radiant-lyrics' words are
/// `inline-block` spans in a `<p>`) or words sit beside a phrasing element
/// (DownloadManager's About text: `as <strong>FLAC</strong>, with`): its
/// children flow in lines and wrap, as a browser sets them. A block of
/// phrasing tags alone keeps its column.
fn inline_flow(tag: &str, display: &str, flow: &[&VNode]) -> bool {
    let shown = |c: &&&VNode| !c.style.hidden();
    let words = flow
        .iter()
        .filter(shown)
        .any(|c| c.tag == "#text" && c.text.as_deref().is_some_and(|t| !t.trim().is_empty()));
    let elements = flow.iter().filter(shown).any(|c| c.tag != "#text");
    block(tag)
        && !display.contains("flex")
        && !display.contains("grid")
        && flow.iter().all(|c| c.style.hidden() || inline_level(c))
        && ((words && elements)
            || flow.iter().filter(shown).any(|c| {
                c.style
                    .display
                    .as_deref()
                    .is_some_and(|d| d.trim().starts_with("inline"))
            }))
}

/// Break inline children into lines that wrap at `avail`: `widths` are the
/// children's widths (`None` before they were drawn once), `space` the width
/// of a gap where the source has white space, `spaced[i]` whether one comes
/// before child `i`. Returns the indexes on each line.
fn inline_lines(
    widths: &[Option<f32>],
    spaced: &[bool],
    space: f32,
    avail: f32,
) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut x = 0.0;
    let known = widths.iter().all(Option::is_some);
    for (i, w) in widths.iter().enumerate() {
        let w = w.unwrap_or(0.0);
        let first = lines.last().is_some_and(Vec::is_empty);
        let gap = if spaced[i] && !first { space } else { 0.0 };
        if known && !first && x + gap + w > avail + 0.5 {
            lines.push(Vec::new());
            x = w;
        } else {
            x += gap + w;
        }
        if let Some(line) = lines.last_mut() {
            line.push(i);
        }
    }
    lines
}

/// Draw `flow` as lines of inline boxes inside the block `owner` (see
/// [`inline_flow`]). A child is measured as it is drawn and placed from the
/// width it had last frame; until every width is known the first pass is
/// drawn invisible, on one line.
fn draw_inline_flow(
    ui: &mut egui::Ui,
    owner: u64,
    flow: &[&VNode],
    align: &str,
    inherit: Inherit,
    cx: &Cx,
) -> bool {
    use std::borrow::Cow;
    let mut units: Vec<Cow<VNode>> = Vec::new();
    let mut spaced: Vec<bool> = Vec::new();
    let mut gap = false;
    for &child in flow {
        if child.tag == "#text" {
            let text = child.text.as_deref().unwrap_or("");
            gap |= text.starts_with(char::is_whitespace);
            for (n, word) in text.split_whitespace().enumerate() {
                gap |= n > 0;
                let mut piece = child.clone();
                piece.text = Some(word.to_owned());
                spaced.push(std::mem::take(&mut gap) && !units.is_empty());
                units.push(Cow::Owned(piece));
            }
            gap |= text.ends_with(char::is_whitespace);
        } else {
            spaced.push(std::mem::take(&mut gap) && !units.is_empty());
            units.push(Cow::Borrowed(child));
        }
    }
    let space = inherit.size * 0.28;
    let key = |i: usize| egui::Id::new(("luna-inline-w", owner, i));
    let widths: Vec<Option<f32>> = (0..units.len())
        .map(|i| ui.data(|d| d.get_temp::<f32>(key(i))))
        .collect();
    let known = widths.iter().all(Option::is_some);
    let avail = ui.available_width();
    let lines = inline_lines(&widths, &spaced, space, avail);
    // Each line is at least the block's own line box tall (its strut:
    // `line-height`, else the face's row), and lines follow one another with
    // no room between, as in CSS. egui's own rows are 28 points tall
    // (DownloadManager's 18.6-point About lines were 30 apart).
    let strut = inherit.line_height.map_or_else(
        || {
            let font = font_of(ui.ctx(), inherit, &Style::default());
            ui.ctx().fonts_mut(|f| f.row_height(&font))
        },
        |l| l.points(inherit.size),
    );
    let mut drawn = false;
    ui.scope(|ui| {
        if !known {
            ui.set_invisible();
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        for line in &lines {
            let used: f32 = line
                .iter()
                .enumerate()
                .map(|(n, &i)| {
                    widths[i].unwrap_or(0.0) + if n > 0 && spaced[i] { space } else { 0.0 }
                })
                .sum();
            ui.allocate_ui_with_layout(
                vec2(avail, strut),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_height(strut);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if known {
                        match align {
                            "center" => ui.add_space(((avail - used) / 2.0).max(0.0)),
                            "right" | "end" => ui.add_space((avail - used).max(0.0)),
                            _ => {}
                        }
                    }
                    for (n, &i) in line.iter().enumerate() {
                        if n > 0 && spaced[i] {
                            ui.add_space(space);
                        }
                        // A box wider than the line is alone on it and wraps
                        // its own text at the line's width (as a block of text
                        // does); its natural width stays as measured.
                        let too_wide = widths[i].is_some_and(|w| w > avail + 0.5);
                        let r = ui.scope(|ui| {
                            if too_wide {
                                ui.set_max_width(avail);
                            } else {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            }
                            draw(ui, &units[i], inherit, cx)
                        });
                        drawn |= r.inner;
                        let w = r.response.rect.width().max(0.0);
                        if !too_wide && widths[i].is_none_or(|l| (l - w).abs() > 0.5) {
                            ui.data_mut(|d| d.insert_temp(key(i), w));
                            plugin_dom::request_frame(ui.ctx());
                        }
                    }
                },
            );
        }
    });
    drawn
}

fn heading_size(tag: &str) -> Option<f32> {
    Some(match tag {
        "h1" => 28.0,
        "h2" => 22.0,
        "h3" => 18.0,
        "h4" => 16.0,
        "h5" | "h6" => 14.0,
        _ => return None,
    })
}

fn attr<'a>(node: &'a VNode, name: &str) -> Option<&'a str> {
    node.attrs
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// `scaleX()`/`scaleY()` in a transform, as factors on the box's own width
/// and height (the renderer's visual transform scales both axes alike).
/// The audiovisualiser's bars are `scaleY()` of full-height boxes.
fn axis_scale(transform: &str) -> Vec2 {
    let mut s = Vec2::ONE;
    let lower = transform.to_ascii_lowercase();
    for (name, axis) in [("scalex(", 0), ("scaley(", 1)] {
        let mut rest = lower.as_str();
        while let Some(at) = rest.find(name) {
            rest = &rest[at + name.len()..];
            let arg = rest.split(')').next().unwrap_or("").trim();
            let v = match arg.strip_suffix('%') {
                Some(p) => p.trim().parse::<f32>().ok().map(|n| n / 100.0),
                None => arg.parse::<f32>().ok(),
            };
            if let Some(v) = v.filter(|v| v.is_finite()) {
                s[axis] *= v.abs().min(16.0);
            }
        }
    }
    s
}

/// A track of `grid-template-columns`: a length, a share of the free room
/// (`fr`), or as wide as what its items hold (`auto`, `max-content`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Track {
    Fixed(f32),
    Share(f32),
    Auto,
}

/// The widths of `grid-template-columns` in `room` (lengths, `fr`
/// shares of what the other tracks and the gaps leave, `minmax(min, max)`
/// as its max, `repeat(n, ...)`, `auto` as wide as its items' content,
/// `content[i]` for track `i`, with the free room shared out among the
/// `auto` tracks when none takes `fr`, as CSS Grid stretches them);
/// `None` for what it cannot read.
#[cfg(test)]
fn grid_columns(
    template: &str,
    room: f32,
    gap: f32,
    font: f32,
    viewport: [f32; 2],
    content: &[f32],
) -> Option<Vec<f32>> {
    Some(track_widths(
        &grid_tracks(template, room, font, viewport)?,
        room,
        gap,
        content,
    ))
}

/// The widths of `tracks` in `room`, `gap` apart; see `grid_columns`.
fn track_widths(tracks: &[Track], room: f32, gap: f32, content: &[f32]) -> Vec<f32> {
    let content_of = |i: usize| content.get(i).copied().unwrap_or(0.0).max(0.0);
    let mut fixed = 0.0;
    let mut shares = 0.0;
    let mut autos = 0;
    for (i, t) in tracks.iter().enumerate() {
        match t {
            Track::Fixed(w) => fixed += w,
            Track::Share(f) => shares += f,
            Track::Auto => {
                fixed += content_of(i);
                autos += 1;
            }
        }
    }
    let free = (room - fixed - gap * tracks.len().saturating_sub(1) as f32).max(0.0);
    tracks
        .iter()
        .enumerate()
        .map(|(i, t)| match t {
            Track::Fixed(w) => *w,
            Track::Share(f) if shares > 0.0 => free * f / shares,
            Track::Share(_) => 0.0,
            Track::Auto if shares > 0.0 => content_of(i),
            Track::Auto => content_of(i) + free / autos as f32,
        })
        .collect()
}

/// The tracks of `grid-template-columns`; `None` for what it cannot read.
fn grid_tracks(template: &str, room: f32, font: f32, viewport: [f32; 2]) -> Option<Vec<Track>> {
    fn track(t: &str, room: f32, font: f32, viewport: [f32; 2]) -> Option<Track> {
        let t = t.trim();
        if let Some(inner) = t.strip_prefix("minmax(").and_then(|r| r.strip_suffix(')')) {
            let parts = css::split_top(inner, ',');
            return track(parts.get(1)?, room, font, viewport);
        }
        if matches!(t, "auto" | "max-content" | "min-content" | "fit-content") {
            return Some(Track::Auto);
        }
        if let Some(n) = t.strip_suffix("fr") {
            return n
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(Track::Share);
        }
        css::length(t, font, room, viewport).map(|w| Track::Fixed(w.max(0.0)))
    }
    let mut tracks = Vec::new();
    for tok in css::split_top(template.trim(), ' ')
        .into_iter()
        .filter(|t| !t.trim().is_empty())
    {
        if let Some(inner) = tok
            .trim()
            .strip_prefix("repeat(")
            .and_then(|r| r.strip_suffix(')'))
        {
            let (n, what) = inner.split_once(',')?;
            let n: usize = n.trim().parse().ok()?;
            for _ in 0..n.min(64) {
                for t in css::split_top(what.trim(), ' ')
                    .into_iter()
                    .filter(|t| !t.trim().is_empty())
                {
                    tracks.push(track(t, room, font, viewport)?);
                }
            }
        } else {
            tracks.push(track(tok, room, font, viewport)?);
        }
    }
    if tracks.is_empty() || tracks.len() > 64 {
        return None;
    }
    Some(tracks)
}

/// `display: grid` items in rows of `tracks`, `gaps` (`[row, column]`)
/// apart. An item in an `auto` track is as wide as what it holds, on one
/// line (its max-content width, measured: the track is the widest of its
/// items last frame); any other is stretched over its cell's width, as
/// `justify-items: stretch` places it (lyrics-everywhere's colour
/// presets, miniplayer's progress bar between its two times). A row is
/// as tall as its tallest item; `align` places the others in it by their
/// heights last frame (`align-items`: the start unless it says `center`
/// or `end`).
fn draw_grid(
    ui: &mut egui::Ui,
    items: &[&VNode],
    tracks: &[Track],
    gaps: [f32; 2],
    align: Align,
    inherit: Inherit,
    cx: &Cx,
) -> bool {
    let width_id = |c: &VNode| egui::Id::new(("luna-grid-w", c.id));
    let height_id = |c: &VNode| egui::Id::new(("luna-grid-h", c.id));
    let n = tracks.len().max(1);
    let mut content = vec![0.0f32; n];
    for (i, c) in items.iter().enumerate() {
        if let Some(w) = ui.data(|d| d.get_temp::<f32>(width_id(c))) {
            content[i % n] = content[i % n].max(w);
        }
    }
    let cols = track_widths(tracks, ui.available_width(), gaps[1], &content);
    let mut drawn = false;
    let room = vec2(ui.available_width(), ui.available_height());
    ui.allocate_ui_with_layout(room, Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = gaps[0];
        for row in items.chunks(n) {
            let heights: Vec<Option<f32>> = row
                .iter()
                .map(|c| ui.data(|d| d.get_temp::<f32>(height_id(c))))
                .collect();
            let row_h = heights.iter().flatten().fold(0.0f32, |a, h| a.max(*h));
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = gaps[1];
                for (((child, w), track), own) in row.iter().zip(&cols).zip(tracks).zip(&heights) {
                    let skip = own.map_or(0.0, |own| match align {
                        Align::Center => ((row_h - own) / 2.0).max(0.0),
                        Align::Max => (row_h - own).max(0.0),
                        Align::Min => 0.0,
                    });
                    ui.allocate_ui_with_layout(
                        vec2(*w, ui.available_height()),
                        Layout::top_down(Align::Min),
                        |ui| {
                            if skip > 0.0 {
                                ui.add_space(skip);
                            }
                            let auto = *track == Track::Auto;
                            if auto {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            } else {
                                ui.set_width(*w);
                                IN_COLUMN.with(|s| s.set(true));
                            }
                            let r = ui.scope(|ui| draw(ui, child, inherit, cx));
                            IN_COLUMN.with(|s| s.set(false));
                            drawn |= r.inner;
                            let used = r.response.rect.size();
                            if auto {
                                let last: Option<f32> = ui.data(|d| d.get_temp(width_id(child)));
                                if last.is_none_or(|l| (l - used.x).abs() > 0.5) {
                                    ui.data_mut(|d| d.insert_temp(width_id(child), used.x));
                                    plugin_dom::request_frame(ui.ctx());
                                }
                            }
                            if own.is_none_or(|l| (l - used.y).abs() > 0.5) {
                                ui.data_mut(|d| d.insert_temp(height_id(child), used.y));
                                plugin_dom::request_frame(ui.ctx());
                            }
                        },
                    );
                }
            });
        }
    });
    drawn
}

/// A flex row with `flex-wrap: wrap`: its items in lines as wide as the
/// row, `gap` apart both ways. Each item is as wide as it was last frame
/// (its own `width` before it was drawn), so a line ends where the next
/// item would not fit (DownloadManager's 104-point tiles, two to a line in
/// the 188 points beside its app list).
fn draw_wrapped(
    ui: &mut egui::Ui,
    items: &[&VNode],
    gap: f32,
    font: f32,
    inherit: Inherit,
    cx: &Cx,
) -> bool {
    let width_id = |c: &VNode| egui::Id::new(("luna-flex-w", c.id));
    let room = ui.available_width();
    let widths: Vec<f32> = items
        .iter()
        .map(|c| {
            ui.data(|d| d.get_temp::<f32>(width_id(c)))
                .or_else(|| {
                    let own = cx.len(&c.style.width, font, room)?;
                    let m = c.style.margin.as_deref().map_or([0.0; 4], |m| {
                        css::sides(&css::resolve_vars(m, cx.vars), font)
                    });
                    Some(own + m[1] + m[3])
                })
                .unwrap_or(0.0)
        })
        .collect();
    let mut drawn = false;
    ui.allocate_ui_with_layout(
        vec2(room, ui.available_height()),
        Layout::top_down(Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.y = gap;
            for line in wrap_lines(&widths, room, gap) {
                // Items hang from the line's top: a line that centred them
                // in all the height left made a box's own height (set
                // from the middle) run from the top to past the middle.
                ui.allocate_ui_with_layout(
                    vec2(room, ui.available_height()),
                    Layout::left_to_right(Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for child in &items[line] {
                            let r = ui.scope(|ui| draw(ui, child, inherit, cx));
                            drawn |= r.inner;
                            let used = r.response.rect.width();
                            let last: Option<f32> = ui.data(|d| d.get_temp(width_id(child)));
                            if last.is_none_or(|l| (l - used).abs() > 0.5) {
                                ui.data_mut(|d| d.insert_temp(width_id(child), used));
                                plugin_dom::request_frame(ui.ctx());
                            }
                        }
                    },
                );
            }
        },
    );
    drawn
}

/// An element's `flex-grow` (the first number of `flex`).
fn flex_grow(style: &Style) -> f32 {
    let Some(flex) = style.flex.as_deref() else {
        return 0.0;
    };
    match flex.trim() {
        "auto" => 1.0,
        "none" | "initial" => 0.0,
        f => f
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<f32>().ok())
            .filter(|n| n.is_finite() && *n > 0.0)
            .unwrap_or(0.0),
    }
}

/// Draw one element and its children; `true` when something showed.
fn draw(ui: &mut egui::Ui, node: &VNode, parent: Inherit, cx: &Cx) -> bool {
    let animated = moved(ui.ctx(), node, &node.style, parent.color, cx);
    let style = animated.as_ref().unwrap_or(&node.style);
    draw_styled(ui, node, style, parent, cx)
}

/// [`draw`] with transitions and animations worked out (`style`).
fn draw_styled(ui: &mut egui::Ui, node: &VNode, style: &Style, parent: Inherit, cx: &Cx) -> bool {
    let xf = style
        .transform
        .as_deref()
        .map(|t| Xf::parse(t, parent.size))
        .unwrap_or_default();
    let stretch = style.transform.as_deref().map_or(Vec2::ONE, axis_scale);
    // `position: relative` offsets and the transform's translation/scale
    // move the drawing, not the layout.
    let mut shift = Vec2::ZERO;
    if style
        .position
        .as_deref()
        .is_some_and(|p| p.trim() == "relative")
    {
        let len = |v: &Option<String>| cx.len(v, parent.size, 0.0);
        shift.x = len(&style.left)
            .or(len(&style.right).map(|r| -r))
            .unwrap_or(0.0);
        shift.y = len(&style.top)
            .or(len(&style.bottom).map(|b| -b))
            .unwrap_or(0.0);
    }
    let size_id = egui::Id::new(("luna-size", node.id));
    let moved = shift != Vec2::ZERO || xf.offset(Vec2::ONE) != Vec2::ZERO || xf.scale != 1.0;
    if moved {
        let last: Rect = ui
            .data(|d| d.get_temp(size_id))
            .unwrap_or(Rect::from_min_size(ui.cursor().min, Vec2::ZERO));
        let offset = shift + xf.offset(last.size());
        let c = last.center();
        let t = egui::emath::TSTransform::from_translation(offset + c.to_vec2())
            * egui::emath::TSTransform::from_scaling(xf.scale)
            * egui::emath::TSTransform::from_translation(-c.to_vec2());
        let mut shown = false;
        // The clip stays where it is on the screen, as an ancestor's clip
        // does for a moved box: egui moves a clip with the shapes, which
        // cut CatJam's cat, lifted 52 points from under the cover, to the
        // strip that had been inside the window before the move.
        let clip = t.inverse().mul_rect(ui.clip_rect());
        let r = ui
            .with_visual_transform(t, |ui| {
                ui.set_clip_rect(clip);
                shown = draw_turned(ui, node, style, xf.rot, stretch, parent, cx);
            })
            .response
            .rect;
        if last != r {
            ui.data_mut(|d| d.insert_temp(size_id, r));
        }
        return shown;
    }
    draw_turned(ui, node, style, xf.rot, stretch, parent, cx)
}

/// [`draw_box`] for a box its transform turns. A box holding words or other
/// elements (custom-fonts' tilted Comic Sans tag) is drawn upright, then
/// everything it painted is turned about its centre, words included, as a
/// browser turns the whole box; its place in the layout stays upright. A
/// picture or an empty box (a spinner's ring) turns itself.
fn draw_turned(
    ui: &mut egui::Ui,
    node: &VNode,
    style: &Style,
    rot: f32,
    stretch: Vec2,
    parent: Inherit,
    cx: &Cx,
) -> bool {
    let holds = node.children.iter().any(|c| !c.tag.starts_with("::"))
        || node.text.as_deref().is_some_and(|t| !t.trim().is_empty());
    if holds
        && style
            .background_clip
            .as_deref()
            .is_some_and(|c| c.trim() == "text")
    {
        return draw_clipped_to_text(ui, node, style, stretch, parent, cx);
    }
    if rot == 0.0
        || !holds
        || matches!(
            node.tag.as_str(),
            "img" | "svg" | "canvas" | "video" | "input" | "textarea" | "select"
        )
    {
        return draw_box(ui, node, style, rot, stretch, parent, cx);
    }
    let layer = ui.layer_id();
    let from = ui.ctx().graphics_mut(|g| g.entry(layer).next_idx().0);
    let r = ui.scope(|ui| draw_box(ui, node, style, 0.0, stretch, parent, cx));
    let to = ui.ctx().graphics_mut(|g| g.entry(layer).next_idx().0);
    let centre = r.response.rect.center();
    ui.ctx().graphics_mut(|g| {
        let list = g.entry(layer);
        for i in from..to {
            list.mutate_shape(egui::layers::ShapeIdx(i), |c| {
                turn_shape(&mut c.shape, centre, rot);
            });
        }
    });
    r.inner
}

/// [`draw_box`] for a box whose background shows only through its words
/// (`background-clip: text`, usually with `color: transparent`): the box is
/// drawn without its background, then each of its words is painted in
/// narrow upright strips, each strip in the colour its background layers
/// (`linear-gradient()`s, each with its own `background-size` and
/// `background-position`, no repeat) have there. A wipe across a sung word
/// (a gradient whose width an animation grows) moves through the letters.
fn draw_clipped_to_text(
    ui: &mut egui::Ui,
    node: &VNode,
    style: &Style,
    stretch: Vec2,
    parent: Inherit,
    cx: &Cx,
) -> bool {
    let mut plain = style.clone();
    plain.background_image = None;
    plain.background_color = None;
    plain.background_clip = None;
    // The words are painted, whatever colour hides them otherwise.
    plain.color = Some("#ffffff".into());
    let layer = ui.layer_id();
    let from = ui.ctx().graphics_mut(|g| g.entry(layer).next_idx().0);
    let r = ui.scope(|ui| draw_box(ui, node, &plain, 0.0, stretch, parent, cx));
    let to = ui.ctx().graphics_mut(|g| g.entry(layer).next_idx().0);
    let rect = r.response.rect;
    let layers = clip_layers(style, rect, parent.size, cx);
    // The words' own colour lies over the background they show: only where
    // it lets it through (`transparent`) is the background seen.
    let own = cx.color(&style.color).unwrap_or(parent.color);
    let mut words = Vec::new();
    ui.ctx().graphics_mut(|g| {
        let list = g.entry(layer);
        for i in from..to {
            list.mutate_shape(egui::layers::ShapeIdx(i), |c| {
                if let egui::Shape::Text(t) = &c.shape {
                    words.push((c.clip_rect, t.clone()));
                    c.shape = egui::Shape::Noop;
                }
            });
        }
    });
    if words.is_empty() || rect.width() <= 0.0 {
        return r.inner;
    }
    // Strips of about two points, at most 128 across the box.
    let strips = (rect.width() / 2.0).ceil().clamp(1.0, 128.0) as usize;
    let step = rect.width() / strips as f32;
    let paint = ui.painter().clone();
    for (clip, text) in words {
        for k in 0..strips {
            let x0 = rect.left() + step * k as f32;
            let colour =
                over_premultiplied(own, clip_colour(&layers, x0 + step / 2.0 - rect.left()));
            if colour.a() == 0 {
                continue;
            }
            // A hair wider than the strip, so no seam shows between two.
            let band = Rect::from_x_y_ranges(x0 - 0.25..=x0 + step + 0.25, clip.y_range());
            let mut t = text.clone();
            t.override_text_color = Some(colour);
            paint
                .with_clip_rect(band.intersect(clip))
                .add(egui::Shape::Text(t));
        }
    }
    r.inner
}

/// One background layer of a box painted through its words: where it lies
/// across the box (`x`, `width`) and its colours from left to right.
struct ClipLayer {
    x: f32,
    width: f32,
    stops: Vec<(f32, Color32)>,
}

/// The background layers of `style` over `rect`, top layer first. Only
/// `linear-gradient()`s are read; their colours run along the box's width.
fn clip_layers(style: &Style, rect: Rect, font: f32, cx: &Cx) -> Vec<ClipLayer> {
    let Some(images) = style.background_image.as_deref() else {
        return Vec::new();
    };
    let images = css::resolve_vars(images, cx.vars);
    let sizes = style.background_size.clone().unwrap_or_default();
    let sizes = css::split_top(&sizes, ',');
    let positions = style.background_position.clone().unwrap_or_default();
    let positions = css::split_top(&positions, ',');
    let pick = |list: &[&str], i: usize| -> String {
        list.get(i)
            .or(list.last())
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let w = rect.width();
    css::split_top(&images, ',')
        .iter()
        .enumerate()
        .filter_map(|(i, image)| {
            let stops = gradient_stops(image)?;
            let size = pick(&sizes, i);
            let width = css::words(&size)
                .first()
                .filter(|v| !matches!(**v, "auto" | "cover" | "contain"))
                .and_then(|v| css::length(v, font, w, cx.viewport))
                .unwrap_or(w)
                .max(0.0);
            let position = pick(&positions, i);
            let free = w - width;
            let x = match css::words(&position).first().copied().unwrap_or("0") {
                "left" => 0.0,
                "center" => free / 2.0,
                "right" => free,
                v => css::length(v, font, free, cx.viewport).unwrap_or(0.0),
            };
            Some(ClipLayer { x, width, stops })
        })
        .collect()
}

/// The colour stops of a `linear-gradient()`, at fractions of its length.
/// Stops without a place share the room between their neighbours.
fn gradient_stops(image: &str) -> Option<Vec<(f32, Color32)>> {
    let lower = image.trim().to_ascii_lowercase();
    let inner = css::func(&lower, "linear-gradient")?;
    let mut parts = css::split_top(inner, ',');
    if parts.first().is_some_and(|p| {
        let p = p.trim();
        p.starts_with("to ") || p.ends_with("deg") || p.ends_with("turn") || p.ends_with("rad")
    }) {
        parts.remove(0);
    }
    let mut stops: Vec<(Option<f32>, Color32)> = Vec::new();
    for part in parts {
        let words = css::words(part.trim());
        let colour = css::color(words.first()?)?;
        let at = words.get(1).and_then(|p| {
            p.strip_suffix('%')
                .and_then(|n| n.trim().parse::<f32>().ok())
                .map(|n| n / 100.0)
                .or_else(|| (p.trim() == "0").then_some(0.0))
        });
        stops.push((at, colour));
    }
    if stops.is_empty() {
        return None;
    }
    let last = stops.len() - 1;
    if stops[0].0.is_none() {
        stops[0].0 = Some(0.0);
    }
    if stops[last].0.is_none() {
        stops[last].0 = Some(1.0);
    }
    // Places left out are spread evenly between the stops that have one.
    let mut i = 0;
    while i < stops.len() {
        if stops[i].0.is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let end = (i..stops.len())
            .find(|&j| stops[j].0.is_some())
            .unwrap_or(last);
        let (a, b) = (stops[start].0.unwrap_or(0.0), stops[end].0.unwrap_or(1.0));
        for (j, stop) in stops.iter_mut().enumerate().take(end).skip(i) {
            stop.0 = Some(a + (b - a) * (j - start) as f32 / (end - start) as f32);
        }
        i = end;
    }
    Some(
        stops
            .into_iter()
            .map(|(at, c)| (at.unwrap_or(0.0), c))
            .collect(),
    )
}

/// The colour the layers paint at `x` points from the box's left edge, top
/// layer over the ones under it.
fn clip_colour(layers: &[ClipLayer], x: f32) -> Color32 {
    let mut out = Color32::TRANSPARENT;
    for layer in layers.iter().rev() {
        if layer.width <= 0.0 || x < layer.x || x > layer.x + layer.width {
            continue;
        }
        let t = (x - layer.x) / layer.width;
        out = over_premultiplied(stop_colour(&layer.stops, t), out);
    }
    out
}

/// `top` over `under`, keeping what shows through (premultiplied colours).
fn over_premultiplied(top: Color32, under: Color32) -> Color32 {
    let keep = 1.0 - f32::from(top.a()) / 255.0;
    let mix = |t: u8, u: u8| (f32::from(t) + f32::from(u) * keep).round().min(255.0) as u8;
    Color32::from_rgba_premultiplied(
        mix(top.r(), under.r()),
        mix(top.g(), under.g()),
        mix(top.b(), under.b()),
        mix(top.a(), under.a()),
    )
}

fn stop_colour(stops: &[(f32, Color32)], t: f32) -> Color32 {
    let Some(first) = stops.first() else {
        return Color32::TRANSPARENT;
    };
    if t <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        let ((a, ca), (b, cb)) = (pair[0], pair[1]);
        if t <= b {
            let f = if b > a { (t - a) / (b - a) } else { 1.0 };
            return css_motion::Lerp::lerp(&ca, &cb, f);
        }
    }
    stops[stops.len() - 1].1
}

/// Turn a painted shape by `rot` radians clockwise about `centre`.
fn turn_shape(shape: &mut egui::Shape, centre: Pos2, rot: f32) {
    let turn = egui::emath::Rot2::from_angle(rot);
    let at = |p: Pos2| centre + turn * (p - centre);
    match shape {
        egui::Shape::Vec(shapes) => {
            for s in shapes {
                turn_shape(s, centre, rot);
            }
        }
        egui::Shape::Rect(r) => {
            r.rect = Rect::from_center_size(at(r.rect.center()), r.rect.size());
            r.angle += rot;
        }
        egui::Shape::Text(t) => {
            t.pos = at(t.pos);
            t.angle += rot;
        }
        egui::Shape::Circle(c) => c.center = at(c.center),
        egui::Shape::Ellipse(e) => {
            e.center = at(e.center);
            e.angle += rot;
        }
        egui::Shape::LineSegment { points, .. } => {
            for p in points {
                *p = at(*p);
            }
        }
        egui::Shape::Path(p) => {
            for q in &mut p.points {
                *q = at(*q);
            }
        }
        egui::Shape::Mesh(m) => {
            let m = std::sync::Arc::make_mut(m);
            for v in &mut m.vertices {
                v.pos = at(v.pos);
            }
        }
        egui::Shape::QuadraticBezier(b) => {
            for p in &mut b.points {
                *p = at(*p);
            }
        }
        egui::Shape::CubicBezier(b) => {
            for p in &mut b.points {
                *p = at(*p);
            }
        }
        egui::Shape::Noop | egui::Shape::Callback(_) => {}
    }
}

thread_local! {
    /// The element about to be drawn is an item of a column that centres
    /// its items (`align-items: center`): like a flex item there, a row
    /// with no width of its own takes its content's width, not the column's.
    static SHRINK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The element about to be drawn is an item of a plugin column that
    /// stretches its items (a block's children): a block with a background
    /// or border is as wide as the column, as in CSS (multipleplaylists'
    /// song box was as wide as its words).
    static IN_COLUMN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// That column is a flex column: its items are stretched to its width
    /// whatever their tag (DownloadManager's nav buttons, each a full-width
    /// row with its highlight across the nav).
    static FLEX_COLUMN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The element about to be drawn is an item of a flex column that
    /// centres or ends its items (`align-items: center`, `flex-end`): as in
    /// CSS it is as wide as what it holds (up to the column's width), so it
    /// can stand in the middle or at the end (betterFullscreen's "No lyrics
    /// available" sat at the column's left edge).
    static FIT_ITEM: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The next box is a positioned element [`positioned`] placed, whose
    /// position and transform it took off the style: whether it forms a
    /// stacking context, read before they went.
    static OWN_CONTEXT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The outer height the flex container gives the next element, margins
    /// included: a row that stretches its items (`align-items: stretch`,
    /// a flex row's default) or a column sharing its free height among
    /// items that grow (`flex: 1`). DownloadManager's window nav runs the
    /// window's height, its start menu's footer sits at the foot.
    static GIVEN_HEIGHT: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
    /// Set while a plugin box draws what it holds.
    static NESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The definite content height of the box whose children are being
    /// drawn: what their `%` heights are a share of, as in CSS (a fullscreen
    /// view's `height: 100%` column in its `100vh` player). Without one a
    /// `%` height is a share of the room left, at most 400 points.
    static CONTAINER_HEIGHT: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// Where a flex row (`row`, `display`, `style`) puts its items across:
/// the part of the room an item leaves under it that goes above it (0.5
/// for `align-items: center`, 1 for `flex-end`), or `None` where egui's row
/// places them (a stretching or wrapping row, or `flex-start`). In a
/// plugin window's page a row that says nothing centres its items too.
fn across_part(row: bool, display: &str, style: &Style, stretch_items: bool) -> Option<f32> {
    if !row || stretch_items || wraps(style) || !display.contains("flex") {
        return None;
    }
    match style.align_items.as_deref().map(str::trim) {
        Some(a) if a.contains("center") => Some(0.5),
        Some("flex-end" | "end" | "self-end") => Some(1.0),
        None => plugin_dom::drawing_page().then_some(0.5),
        _ => None,
    }
}

/// Where a flex item's own `align-self` puts it across its line, over its
/// container's `align-items`: the part of the room it leaves that goes
/// before it (0 the start, 0.5 the middle, 1 the end), `Some(None)` for
/// `stretch`, and `None` where it says nothing (`auto`).
fn self_part(child: &VNode) -> Option<Option<f32>> {
    match child.style.align_self.as_deref().map(str::trim)? {
        "center" => Some(Some(0.5)),
        "flex-end" | "end" | "self-end" => Some(Some(1.0)),
        "flex-start" | "start" | "self-start" | "baseline" | "first baseline" => Some(Some(0.0)),
        "stretch" | "normal" => Some(None),
        _ => None,
    }
}

/// Draws `child`, an item of a row whose content is `row_h` points tall,
/// `part` of the way down the room it leaves under it ([`across_part`]),
/// by its height last frame, and notes its height for the next (an item
/// beside a taller one: betterFullscreen's song words beside the cover,
/// miniplayer's cover beside its song details). With `width` the item is
/// that wide (a growing item's share). Such a row stretches no item. Returns
/// whether anything was drawn and the item's height.
fn draw_across(
    ui: &mut egui::Ui,
    child: &VNode,
    row_h: f32,
    part: f32,
    width: Option<f32>,
    inherit: Inherit,
    cx: &Cx,
) -> (bool, f32) {
    let id = egui::Id::new(("luna-cross", child.id));
    let last: Option<f32> = ui.data(|d| d.get_temp(id));
    let skip = last.map_or(0.0, |h| ((row_h - h) * part).max(0.0));
    let at = ui.cursor().min;
    let room = vec2(width.unwrap_or(ui.available_width()), ui.available_height());
    let mut cell = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(egui::pos2(at.x, at.y + skip), room))
            .layout(Layout::top_down(Align::Min)),
    );
    if let Some(w) = width {
        cell.set_width(w);
    }
    let shown = draw(&mut cell, child, inherit, cx);
    let r = cell.min_rect();
    let h = r.height();
    ui.advance_cursor_after_rect(Rect::from_min_max(
        at,
        egui::pos2(r.max.x.max(at.x), at.y + skip + h),
    ));
    if last.is_none_or(|l| (l - h).abs() > 0.5) {
        ui.data_mut(|d| d.insert_temp(id, h));
        plugin_dom::request_frame(ui.ctx());
    }
    (shown, h)
}

/// Draw `child` as a flex item that is `given` points tall (margins
/// included) when its container sets that, else at its own height.
fn draw_given(
    ui: &mut egui::Ui,
    child: &VNode,
    given: Option<f32>,
    inherit: Inherit,
    cx: &Cx,
) -> bool {
    GIVEN_HEIGHT.with(|s| s.set(given));
    let drawn = draw(ui, child, inherit, cx);
    GIVEN_HEIGHT.with(|s| s.set(None));
    drawn
}

/// The height element `id` would take, margins included, if nothing held
/// it (a flex share, a scroll box): its own `height`, else what it holds,
/// within its `min-height` and `max-height`. Only an element drawn in this
/// pass has one.
fn natural_of(ui: &egui::Ui, id: u64) -> Option<f32> {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data(|d| d.get_temp::<(u64, f32)>(egui::Id::new(("luna-natural", id))))
        .filter(|(p, _)| *p == pass)
        .map(|(_, h)| h)
}

fn note_natural(ui: &egui::Ui, id: u64, h: f32) {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data_mut(|d| d.insert_temp(egui::Id::new(("luna-natural", id)), (pass, h)));
}

/// Whether a flex container `style` lets its items run onto more lines.
fn wraps(style: &Style) -> bool {
    style
        .flex_wrap
        .as_deref()
        .is_some_and(|w| w.trim().starts_with("wrap"))
}

/// Split `widths` (each item's outer width) into the lines of a wrapping
/// flex row `room` wide with `gap` between items: the index ranges of
/// each line. An item wider than the room has a line of its own.
fn wrap_lines(widths: &[f32], room: f32, gap: f32) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (i, w) in widths.iter().enumerate() {
        if i > start && used + gap + w > room + 0.5 {
            lines.push(start..i);
            start = i;
            used = *w;
        } else {
            used += if i > start { gap + w } else { *w };
        }
    }
    if start < widths.len() {
        lines.push(start..widths.len());
    }
    lines
}

/// [`draw`] after animation and transforms are worked out. `stretch`
/// scales the box's own width and height (`scaleX`/`scaleY`).
fn draw_box(
    ui: &mut egui::Ui,
    node: &VNode,
    style: &Style,
    rot: f32,
    stretch: Vec2,
    parent: Inherit,
    cx: &Cx,
) -> bool {
    let shrink = SHRINK.with(|s| s.replace(false));
    let fit_item = FIT_ITEM.with(|s| s.replace(false));
    // What a `%` height or max-height is a share of.
    let basis = CONTAINER_HEIGHT
        .with(std::cell::Cell::get)
        .unwrap_or_else(|| ui.available_height().min(400.0));
    let in_column = IN_COLUMN.with(|s| s.replace(false));
    let flex_item = FLEX_COLUMN.with(|s| s.replace(false));
    let placed_context = OWN_CONTEXT.with(|s| s.replace(false));
    // Pictures keep their own height (they are not stretched here).
    let given = GIVEN_HEIGHT
        .with(|s| s.replace(None))
        .filter(|_| !matches!(node.tag.as_str(), "img" | "svg" | "canvas" | "video"));
    if style.hidden()
        || matches!(node.tag.as_str(), "style" | "script" | "template" | "audio")
        || (node.tag == "canvas" && !super::luna_canvas::has_frame(node))
        || (node.tag == "video" && !super::luna_video::has_clip(node))
    {
        return false;
    }
    if attr(node, "hidden").is_some() {
        return false;
    }
    // A picture with no width of its own, in a column that centres its
    // items: its box is the picture's, in the middle of the column, as a
    // browser sizes it. qr-share's QR code sat at the left of a box as wide
    // as its `max-width: 90%`.
    if shrink
        && style.width.is_none()
        && matches!(node.tag.as_str(), "img" | "svg" | "canvas" | "video")
        && let Some([nw, nh]) = node.natural
        && nw > 0.0
        && nh > 0.0
    {
        let room = cx
            .len(&style.max_width, parent.size, ui.available_width())
            .unwrap_or(f32::INFINITY)
            .min(ui.available_width());
        let size = vec2(nw.min(room).max(1.0), ui.available_height());
        // Its `max-width` is spent: a share of the column, not of its box.
        let own = Style {
            max_width: None,
            ..style.clone()
        };
        return ui
            .allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
                draw_box(ui, node, &own, rot, stretch, parent, cx)
            })
            .inner;
    }
    let mut inherit = parent;
    if let Some(c) = cx.color(&style.color) {
        inherit.color = c;
    }
    if let Some(size) = heading_size(&node.tag) {
        inherit.size = size;
        inherit.weight = 700;
    }
    // A browser's own style for a button, `font: -webkit-small-control`,
    // is not inherited from the page: 13.33 px, normal weight, Arial on
    // Windows (qr-share's Save and Close), unless the plugin sets its own.
    // A button wearing TIDAL's own module classes (LangRomanizer's copy of
    // the Lyrics pill, multipleplaylists' menu row) takes TIDAL's
    // stylesheet's font, which is the page's.
    // `inherit` (a reset's `font: inherit`: koreansearch's result rows)
    // keeps what the parent has.
    let inherits = |v: &Option<String>| v.as_deref().is_some_and(|v| v.trim() == "inherit");
    if node.tag == "button" && !tidal_styled(node) {
        if !inherits(&style.font_size) {
            inherit.size = 40.0 / 3.0;
        }
        if !inherits(&style.font_weight) {
            inherit.weight = 400;
        }
        if !inherits(&style.font_style) {
            inherit.italic = false;
        }
        if style.font_family.is_none() {
            inherit.family = plugin_dom::element_font_id(BUTTON_FONT);
        }
    }
    // The browser's own sheet: `strong` and `b` are `bolder` than their
    // parent, `em` and `i` italic (DownloadManager's About text).
    match node.tag.as_str() {
        "strong" | "b" => inherit.weight = bolder(parent.weight),
        "em" | "i" | "cite" | "dfn" | "var" => inherit.italic = true,
        _ => {}
    }
    // `text-overflow` cuts a block's own line of words, spans in it
    // included, not what a block inside it holds.
    if !inline_level(node) {
        inherit.ellipsis = false;
    }
    // Lists: `ul` draws discs and `ol` numbers unless the list says
    // otherwise; the items inherit it.
    match node.tag.as_str() {
        "ul" => inherit.marker = Marker::Disc,
        "ol" => inherit.marker = Marker::Decimal,
        _ => {}
    }
    if let Some(m) = style.list_style_type.as_deref().and_then(Marker::parse) {
        inherit.marker = m;
    }
    if let Some(size) = cx.len(&style.font_size, parent.size, parent.size) {
        inherit.size = size.clamp(6.0, 96.0);
    }
    if let Some(w) = style
        .font_weight
        .as_ref()
        .filter(|_| !inherits(&style.font_weight))
    {
        inherit.weight = css::weight(w);
    }
    if let Some(s) = style
        .font_style
        .as_ref()
        .filter(|_| !inherits(&style.font_style))
    {
        inherit.italic = s.contains("italic") || s.contains("oblique");
    }
    // Text inside takes the element's family too (lyrics-everywhere sets
    // `font-family: system-ui` on its box, not on the words).
    if let Some(f) = style
        .font_family
        .as_ref()
        .filter(|_| !inherits(&style.font_family))
    {
        inherit.family = plugin_dom::element_font_id(f);
    }
    if let Some(t) = &style.text_transform {
        inherit.upper = t.trim() == "uppercase";
    }
    if let Some(fill) = cx.color(&style.fill) {
        inherit.fill = Some(fill);
    }
    if let Some(lh) = style
        .line_height
        .as_deref()
        .and_then(|v| LineHeight::parse(v, inherit.size, cx.viewport))
    {
        inherit.line_height = lh;
    }
    if let Some(t) = &style.text_shadow {
        inherit.shadow = text_shadows(&css::resolve_vars(t, cx.vars), inherit.size, inherit.color);
    }
    // A blur applies to everything inside the element, whatever their own
    // `filter` says (radiant-lyrics blurs lines away from the sung one by
    // `.035em` to `.07em`).
    if let Some(r) = style
        .filter
        .as_deref()
        .and_then(|f| blur_radius(&css::resolve_vars(f, cx.vars)))
        .and_then(|r| css::length(&r, inherit.size, 0.0, cx.viewport))
    {
        inherit.blur = inherit.blur.max(r);
    }
    if node.tag == "#text" {
        let Some(text) = node.text.as_deref().filter(|t| !t.trim().is_empty()) else {
            return false;
        };
        let wrap = if inherit.ellipsis {
            Some(egui::TextWrapMode::Truncate)
        } else {
            (!breakable(text)).then_some(egui::TextWrapMode::Extend)
        };
        let text = rich(ui.ctx(), text, inherit, style);
        add_text(ui, text, wrap, inherit);
        return true;
    }

    let opacity = style.opacity.as_deref().and_then(css::opacity);
    let font = inherit.size;
    let gradient = style
        .background_image
        .as_deref()
        .and_then(|b| box_gradients(&css::resolve_vars(b, cx.vars)));
    let fill = cx.color(&style.background_color).or_else(|| {
        style.background_image.as_deref().and_then(|b| {
            css::colors_in(&css::resolve_vars(b, cx.vars))
                .first()
                .copied()
        })
    });
    // The box's fill lies over what is behind it; the text over it is the
    // plugin's colour when that reads there.
    if let Some(fill) = fill {
        inherit.back = over(fill, inherit.back);
    }
    if fill.is_some() || style.color.is_some() {
        inherit.color = readable(inherit.color, inherit.back);
    }
    // The shorthand, then the sides a longhand sets after it (miniplayer's
    // button: `margin-left: 8px; margin-top: 4px`).
    let sides = |short: Option<&String>, long: [Option<&String>; 4]| {
        let mut out = short.map(|p| css::sides(&css::resolve_vars(p, cx.vars), font));
        for (i, side) in long.into_iter().enumerate() {
            if let Some(v) = side
                && let Some(n) = cx.len(&Some(v.clone()), font, 0.0)
            {
                out.get_or_insert([0.0; 4])[i] = n;
            }
        }
        out
    };
    let padding = sides(
        style.padding.as_ref(),
        [
            style.padding_top.as_ref(),
            style.padding_right.as_ref(),
            style.padding_bottom.as_ref(),
            style.padding_left.as_ref(),
        ],
    );
    let margin = sides(
        style.margin.as_ref(),
        [
            style.margin_top.as_ref(),
            style.margin_right.as_ref(),
            style.margin_bottom.as_ref(),
            style.margin_left.as_ref(),
        ],
    );
    let border = style
        .border
        .as_deref()
        .and_then(|b| css::border(&css::resolve_vars(b, cx.vars)))
        .or_else(|| {
            // `border: none` and then one side's own (`border-left: 3px
            // solid`: DownloadManager's open nav item has its blue bar):
            // that side is drawn, the others stay at 0.
            let v = css::resolve_vars(style.border_width.as_deref()?, cx.vars);
            let widest = css::words(&v)
                .iter()
                .filter_map(|w| css::length(w, font, 0.0, cx.viewport))
                .fold(0.0_f32, f32::max);
            (widest > 0.0).then_some((widest, inherit.color))
        });
    // `border-color`: one colour for all sides, or each side's own (a
    // spinner's bright top arc). `currentcolor` is the text's colour.
    let sides: Option<[Color32; 4]> = border.and_then(|_| {
        let v = css::resolve_vars(style.border_color.as_deref()?, cx.vars);
        let c: Vec<Color32> = css::words(&v)
            .iter()
            .filter_map(|w| {
                if w.eq_ignore_ascii_case("currentcolor") {
                    Some(inherit.color)
                } else {
                    css::color(w)
                }
            })
            .collect();
        match c.as_slice() {
            [a] => Some([*a; 4]),
            [t, r] => Some([*t, *r, *t, *r]),
            [t, r, b] => Some([*t, *r, *b, *r]),
            [t, r, b, l] => Some([*t, *r, *b, *l]),
            _ => None,
        }
    });
    let border = match (border, sides) {
        (Some((w, _)), Some(s)) => Some((w, s[0])),
        (b, _) => b,
    };
    let sides = sides.filter(|s| s.iter().any(|c| *c != s[0]));
    // Sides of their own widths (`border-bottom: 0`), when they differ.
    let widths: Option<[f32; 4]> = border.and_then(|_| {
        let v = css::resolve_vars(style.border_width.as_deref()?, cx.vars);
        let w: Vec<f32> = css::words(&v)
            .iter()
            .map(|w| css::length(w, font, 0.0, cx.viewport).unwrap_or(0.0))
            .collect();
        let four = match w.as_slice() {
            [a] => [*a; 4],
            [t, r] => [*t, *r, *t, *r],
            [t, r, b] => [*t, *r, *b, *r],
            [t, r, b, l, ..] => [*t, *r, *b, *l],
            [] => return None,
        };
        four.iter().any(|x| *x != four[0]).then_some(four)
    });
    // `dashed` and `dotted` borders: Chromium's dashes are three widths long
    // with about two widths between (custom-fonts' pink tag: 6 and 4 points
    // at 2), dots one width with one between.
    let dash = border.and_then(|(w, _)| {
        let v = style.border.as_deref()?.to_ascii_lowercase();
        if v.contains("dashed") {
            Some((3.0 * w, 2.0 * w))
        } else if v.contains("dotted") {
            Some((w, w))
        } else {
            None
        }
    });
    // Drawn by hand: a box turned by its transform (a spinner's ring),
    // sides of different colours or widths, a dashed or dotted border;
    // otherwise egui's frame draws it.
    let by_hand = (rot != 0.0 && (border.is_some() || style.background_color.is_some()))
        || sides.is_some()
        || widths.is_some()
        || dash.is_some();
    // Each side's border width (`[top, right, bottom, left]`).
    let bw = widths.unwrap_or([border.map_or(0.0, |(w, _)| w); 4]);
    let radius = cx.len(&style.border_radius, font, 0.0).unwrap_or(0.0);
    // TIDAL's CSS sizes every box `border-box`: a width or height includes
    // the padding and border (AltPlay's `width: 100%` filter field with
    // padding stays inside its page; OneClickLyrics' `32px` button with
    // `padding: 1px 6px` is 32 points wide, not 44).
    // A button is sized `border-box` by the browser itself, its default
    // padding (Chromium's `1px 6px`, drawn below) included: scroll-to-
    // playing's 40-point round button is 40 points.
    let is_button = node.tag == "button" || attr(node, "role") == Some("button");
    let pads = padding.or(is_button.then_some([1.0, 6.0, 1.0, 6.0]));
    let inset = pads.map_or(0.0, |p| p[1] + p[3]) + bw[1] + bw[3];
    let block_inset = pads.map_or(0.0, |p| p[0] + p[2]) + bw[0] + bw[2];
    let outer_w = cx.len(&style.width, font, ui.available_width());
    // A height the flex container gives (stretched or grown) is the margin
    // box's; the element's own `height` wins over it.
    let outer_h = cx
        .len(&style.height, font, basis)
        .or_else(|| given.map(|g| (g - margin.map_or(0.0, |m| m[0] + m[2])).max(0.0)));
    // A growing flex item without a width fills the share its row gave it:
    // its border box, inside its margins (DownloadManager's About body keeps
    // its 24-point padding at the right).
    let margin_x = margin.map_or(0.0, |m| m[1] + m[3]);
    let width = outer_w
        .map(|w| (w - inset).max(0.0))
        .or_else(|| {
            (flex_grow(style) > 0.0).then(|| (ui.available_width() - inset - margin_x).max(0.0))
        })
        .map(|w| w * stretch.x);
    let height = outer_h.map(|h| (h - block_inset).max(0.0) * stretch.y);
    // `transition: width .3s` (a bar growing): the size on its way. Only a
    // length of its own moves: a share of the window or the parent is the
    // same value in CSS when the window resizes, so nothing transitions.
    let fixed = |v: &Option<String>| {
        v.as_deref().is_some_and(|v| {
            ![
                "%",
                "vw",
                "vh",
                "vmin",
                "vmax",
                "auto",
                "fit-content",
                "content",
            ]
            .iter()
            .any(|u| v.contains(u))
        })
    };
    let width = if fixed(&style.width) {
        moved_length(ui.ctx(), node, style, "width", width)
    } else {
        width
    };
    let height = if fixed(&style.height) {
        moved_length(ui.ctx(), node, style, "height", height)
    } else {
        height
    };
    // Its own height (or the share its flex container gave it) is definite:
    // what its children's `%` heights are a share of.
    let definite = height;
    // A radius that is a share of the box (`50%`: AltPlay's round play
    // button) is a share of its shorter side, when the box has a size.
    // The box is its content with padding and border round it, as the
    // frame draws it, so `50%` of a bordered box is a circle.
    let radius = match (style.border_radius.as_deref(), width, height) {
        (Some(r), Some(w), Some(h)) => {
            radius_share(r, w + inset, h + block_inset).unwrap_or(radius)
        }
        _ => radius,
    };
    let is_link = node.tag == "a";
    let listens = |kind: &str| node.listens.iter().any(|l| l == kind);
    let inert = style
        .pointer_events
        .as_deref()
        .is_some_and(|p| p.trim() == "none");
    let clickable = !inert
        && (is_button
            || is_link
            || listens("click")
            || listens("dblclick")
            || listens("contextmenu"));
    let min_width = cx.len(&style.min_width, font, ui.available_width());
    let max_width = cx.len(&style.max_width, font, ui.available_width());
    let min_height = cx
        .len(&style.min_height, font, 0.0)
        .map(|h| (h - block_inset).max(0.0));
    let display = style.display.as_deref().unwrap_or("");
    // A flex row that moves its items along (`justify-content: flex-end`:
    // multipleplaylists' buttons, `space-between`) needs its width: a
    // block's, all the room.
    let spreads = display.trim() == "flex"
        && !style
            .flex_direction
            .as_deref()
            .is_some_and(|d| d.trim().starts_with("column"))
        && matches!(
            style.justify_content.as_deref().map(str::trim),
            Some("center" | "flex-end" | "end" | "space-between" | "space-around" | "space-evenly")
        )
        || (plugin_dom::drawing_page()
            && style.justify_content.as_deref().map(str::trim) == Some("space-between"));
    // A block that places its words by `text-align` (DownloadManager's
    // empty-queue note, centred) needs its width, as every block in a
    // column has in CSS: shrunk to its words it had no room to centre in.
    let aligns = matches!(
        style.text_align.as_deref().map(str::trim),
        Some("center" | "right" | "end")
    );
    // In a plugin window's page every block fills its container, as in
    // CSS; in the app a plugin's block shrinks to what it holds unless it
    // shows a box, so it sits beside the app's own widgets.
    let width = width.or_else(|| {
        (in_column
            && (fill.is_some()
                || border.is_some()
                || spreads
                || aligns
                || plugin_dom::drawing_page())
            && (block(&node.tag) || flex_item)
            && !display.contains("inline"))
        .then(|| {
            let sides = |s: Option<[f32; 4]>| s.map_or(0.0, |s| s[1] + s[3]);
            (ui.available_width() - sides(pads) - sides(margin) - bw[1] - bw[3]).max(0.0)
        })
    });
    // As in CSS, a width gives way to `max-width`, then `min-width`
    // (multipleplaylists' dialog: `width: 90%; max-width: 500px`).
    // Those bound the border box too, when the width is one.
    let width = width.map(|w| {
        let w = max_width.map_or(w, |m| w.min((m - inset).max(0.0)));
        min_width.map_or(w, |m| w.max((m - inset).max(0.0)))
    });

    let margin_of = |s: [f32; 4]| Margin {
        left: s[3].round().clamp(-128.0, 127.0) as i8,
        right: s[1].round().clamp(-128.0, 127.0) as i8,
        top: s[0].round().clamp(-128.0, 127.0) as i8,
        bottom: s[2].round().clamp(-128.0, 127.0) as i8,
    };
    let mut frame = Frame::new();
    // A gradient is painted whole under the content; the frame keeps only
    // a colour set beside it.
    let fill = if gradient.is_some() {
        cx.color(&style.background_color)
    } else {
        fill
    };
    if by_hand {
        // Painted below; the frame keeps the border's room.
    } else if let Some(fill) = fill {
        frame = frame.fill(fill);
    }
    // A bare `<button>` as TIDAL draws one: its stylesheet takes away the
    // background and border, Chromium's `padding: 1px 6px` stays.
    if let Some(w) = widths {
        // The frame's stroke is one width all round: the sides' own widths
        // are room inside it instead, and drawn by hand.
        let p = pads.unwrap_or([0.0; 4]);
        frame = frame.inner_margin(margin_of([
            p[0] + w[0],
            p[1] + w[1],
            p[2] + w[2],
            p[3] + w[3],
        ]));
    } else if let Some(p) = padding {
        frame = frame.inner_margin(margin_of(p));
    } else if is_button {
        frame = frame.inner_margin(Margin::symmetric(6, 1));
    }
    if let Some(m) = margin {
        frame = frame.outer_margin(margin_of(m));
    }
    if let Some((w, c)) = border
        && widths.is_none()
    {
        frame = frame.stroke(Stroke::new(
            w,
            if by_hand { Color32::TRANSPARENT } else { c },
        ));
    }
    // The first outer `box-shadow` layer (lyrics-everywhere's bubble, the
    // audio-visualizer's panels, colorama's colour dialog), painted under
    // the box once it is placed: outside it only, as a browser draws one
    // (egui's frame shadow also lies under the box's inside, which a
    // see-through box showed darkened).
    let shadow = super::theme_layout::shadow(style.box_shadow.as_deref(), cx.vars, cx.viewport);
    frame = frame.corner_radius(CornerRadius::same(radius.clamp(0.0, 255.0) as u8));

    let row = if display.contains("flex") {
        !style
            .flex_direction
            .as_deref()
            .is_some_and(|d| d.starts_with("column"))
    } else {
        !block(&node.tag)
    };
    // A flex or grid container's `gap` is 0 unless set (star-ratings'
    // five stars touch, up-next's title and artist lines sit one on the
    // other); other boxes keep a little room between what they hold, as
    // the spaces between inline elements give.
    // In a plugin window's page the white space between elements is there
    // as text, and blocks touch, as in CSS.
    let gap = cx.len(&style.gap, font, 0.0).unwrap_or(
        if display.contains("flex") || display.contains("grid") || plugin_dom::drawing_page() {
            0.0
        } else if row {
            4.0
        } else {
            2.0
        },
    );
    let center = style
        .align_items
        .as_deref()
        .is_none_or(|a| a.contains("center"));
    // A column centres its children only when asked (flex stretches).
    let cross_center = style
        .align_items
        .as_deref()
        .is_some_and(|a| a.contains("center"));
    let justify = style.justify_content.as_deref().unwrap_or("").trim();
    // A flex row of a definite height stretches its items to it unless it
    // aligns them otherwise (`stretch` is the default): DownloadManager's
    // window nav runs the window's height, its caption buttons the title
    // bar's. A row sized by what it holds keeps centring them.
    let stretch_items = row
        && display.contains("flex")
        && !wraps(style)
        && height.is_some()
        && style
            .align_items
            .as_deref()
            .is_none_or(|a| matches!(a.trim(), "stretch" | "normal"));
    // A block of a known width places its own words by `text-align`.
    let text_place = (!row && aligns && width.is_some()).then(|| {
        if style.text_align.as_deref().map(str::trim) == Some("center") {
            Align::Center
        } else {
            Align::Max
        }
    });
    // The outer height a stretched item takes: the row's.
    let stretched = |child: &VNode| {
        (stretch_items && child.tag != "#text" && child.style.height.is_none())
            .then_some(height)
            .flatten()
    };
    let content_id = egui::Id::new(("luna-content", node.id));
    let has_url = style
        .background_image
        .as_deref()
        .is_some_and(|b| b.contains("url("));
    let ellipsis = style
        .text_overflow
        .as_deref()
        .is_some_and(|t| t.contains("ellipsis"))
        || style
            .white_space
            .as_deref()
            .is_some_and(|w| w.contains("nowrap"))
            && style
                .overflow
                .as_deref()
                .is_some_and(|o| o.contains("hidden"));
    // A block's words, in it or in its spans, end in "…" where they reach
    // its edge (DownloadManager's tile labels); a flex or grid box has no
    // line of its own to cut.
    if ellipsis && !display.contains("flex") && !display.contains("grid") {
        inherit.ellipsis = true;
    }
    let (flow, placed): (Vec<&VNode>, Vec<&VNode>) = node
        .children
        .iter()
        .partition(|c| positioned_kind(&c.style).is_none());
    // Positioned children with a negative `z-index` go under the flow
    // (betterFullscreen's background layer under its art and title), in
    // the box this element took last frame. An element that forms a
    // stacking context paints its own background first and them over it
    // (radiant-lyrics' backdrop canvas over its container's black); one
    // that does not leaves them under its background too.
    let (under, placed): (Vec<&VNode>, Vec<&VNode>) =
        placed.into_iter().partition(|c| z_index(&c.style) < 0);
    // A child in the flow that is positioned (`relative`, `sticky`) with
    // a `z-index` stacks by it (CSS 2.1 Appendix E): the absolutely placed
    // children with a lower one paint under it, over this element's
    // background (betterFullscreen's backdrop at TIDAL's `--z-index-low`,
    // 10, under its words at `--z-index-high`, 1000; it was painted over
    // them, as every placed child went after the flow).
    let flow_z = flow
        .iter()
        .filter(|c| {
            c.style
                .position
                .as_deref()
                .is_some_and(|p| matches!(p.trim(), "relative" | "sticky"))
        })
        .filter_map(|c| c.style.z_index.as_deref()?.trim().parse::<i64>().ok())
        .max();
    let (mut between, mut placed): (Vec<&VNode>, Vec<&VNode>) = placed
        .into_iter()
        .partition(|c| flow_z.is_some_and(|z| z_index(&c.style) < z));
    // Placed children stack by `z-index`, in tree order within one.
    between.sort_by_key(|c| z_index(&c.style));
    placed.sort_by_key(|c| z_index(&c.style));
    let flows = inline_flow(&node.tag, display, &flow);
    // White space between children is a gap only in a line of inline
    // boxes; a column, row or grid has no place for it.
    let mut flow = flow;
    if !flows {
        flow.retain(|c| {
            c.tag != "#text" || c.text.as_deref().is_some_and(|t| !t.trim().is_empty())
        });
    }
    // `overflow-y: auto|scroll` (or `overflow`): a box of a definite height,
    // or one held to a `max-height`, scrolls what it holds instead of
    // growing past it (DownloadManager's About window keeps 440 by 540).
    let scrolls = style
        .overflow_y
        .as_deref()
        .or_else(|| {
            style
                .overflow
                .as_deref()
                .and_then(|o| css::words(o).last().copied())
        })
        .is_some_and(|o| matches!(o.trim(), "auto" | "scroll" | "overlay"));
    let scroll = scrolls
        .then(|| {
            height.map(|h| (h, true)).or_else(|| {
                cx.len(&style.max_height, font, basis)
                    .map(|m| ((m - block_inset).max(0.0), false))
            })
        })
        .flatten();
    // A flex column held to a `max-height`, with no height of its own, is
    // as tall as what it holds up to that: once what it holds would run
    // past it, it is that tall, and its growing items share what its other
    // items leave (lyrics-everywhere's container: the growing middle holds
    // the player and the lyrics, which scroll). What it holds is measured
    // as if nothing held it (see `note_natural`), so a held column stays
    // held until its content fits.
    let natural_id = egui::Id::new(("luna-natural-content", node.id));
    let max_inner = cx
        .len(&style.max_height, font, basis)
        .map(|m| (m - block_inset).max(0.0));
    let own_inner = cx
        .len(&style.height, font, basis)
        .map(|h| (h - block_inset).max(0.0));
    let held_column = display.contains("flex") && !row && scroll.is_none() && max_inner.is_some();
    let height = height.or_else(|| {
        let m = max_inner?;
        held_column
            .then(|| ui.data(|d| d.get_temp::<f32>(natural_id)))
            .flatten()
            .filter(|n| *n > m + 0.5)
            .map(|_| m)
    });
    let mut content_natural: Option<f32> = None;
    // A scroll the plugin asked of this element and not made yet: a new
    // `scrollTop` for its scroll box, or a place for it in the scroll
    // boxes round it (`scrollIntoView`).
    let asked = plugin_dom::node_scroll(node.id).filter(|a| {
        ui.data(|d| d.get_temp::<u64>(egui::Id::new(("luna-scroll-seq", node.id))))
            .is_none_or(|seq| seq < a.seq)
    });
    if let Some(a) = &asked {
        ui.data_mut(|d| d.insert_temp(egui::Id::new(("luna-scroll-seq", node.id)), a.seq));
    }
    // An `<ol>`'s items are numbered from 1 within it.
    let list = matches!(node.tag.as_str(), "ul" | "ol" | "menu");
    if list {
        LIST_ITEMS.with(|l| l.borrow_mut().push(0));
    }
    let box_id = egui::Id::new(("luna-box", node.id));
    let mut drawn = false;
    // Positioned children sit in the padding box, inside the border, and
    // take the element's opacity (lyrics-everywhere's bubble tail).
    let padding_box = |r: Rect| {
        Rect::from_min_max(
            r.min + vec2(bw[3], bw[0]),
            egui::pos2(
                (r.max.x - bw[1]).max(r.min.x + bw[3]),
                (r.max.y - bw[2]).max(r.min.y + bw[0]),
            ),
        )
    };
    // A layer of its own, not a scope: a scope takes room at the cursor
    // (an empty one too), which made a box with placed children a gap
    // taller in a column or grid with a gap.
    let place = |ui: &mut egui::Ui, child: &VNode, r: Rect, inherit: Inherit| {
        let mut layer = ui.new_child(UiBuilder::new().max_rect(ui.max_rect()));
        if let Some(o) = opacity {
            layer.multiply_opacity(o);
        }
        positioned(&mut layer, child, padding_box(r), inherit, cx);
    };
    let under_box: Option<Rect> = (!under.is_empty() || !between.is_empty())
        .then(|| ui.data(|d| d.get_temp(box_id)))
        .flatten();
    // Under the element's own background (inside its opacity already) or,
    // with no stacking context of its own, under it at its opacity.
    let paint_under = |ui: &mut egui::Ui, inside: bool| {
        let mut drawn = false;
        if let Some(r) = under_box {
            for child in under.iter().filter(|c| !c.style.hidden()) {
                if inside {
                    positioned(ui, child, padding_box(r), parent, cx);
                } else {
                    place(ui, child, r, parent);
                }
                drawn = true;
            }
        }
        drawn
    };
    let own_context = placed_context || stacking_context(style);
    let outer_basis = CONTAINER_HEIGHT.with(|c| c.replace(definite));
    if !own_context {
        drawn |= paint_under(ui, false);
    }
    // Whether this box sits inside another plugin box (not a slot's own
    // element): its children are nested.
    let nested = NESTED.with(|n| n.replace(true));
    let inner = ui.scope(|ui| {
        if let Some(o) = opacity {
            ui.multiply_opacity(o);
        }
        let shadow_slot = shadow.map(|_| ui.painter().add(egui::Shape::Noop));
        let hand = by_hand.then(|| ui.painter().add(egui::Shape::Noop));
        let bg = (has_url || gradient.is_some()).then(|| ui.painter().add(egui::Shape::Noop));
        // A border goes over the background picture (custom-fonts' dashed
        // pink border round its gradient).
        let hand_edge = (by_hand && bg.is_some()).then(|| ui.painter().add(egui::Shape::Noop));
        let shown = frame
            .show(ui, |ui| {
                if own_context {
                    drawn |= paint_under(ui, true);
                }
                if let Some(r) = under_box {
                    for child in between.iter().filter(|c| !c.style.hidden()) {
                        positioned(ui, child, padding_box(r), parent, cx);
                        drawn = true;
                    }
                }
                // A width already gave way to `min-width` and `max-width`
                // above; they bound only a box without one. (A larger
                // `max-width` set after the width undid it: DownloadManager's
                // 400-point start menu and 520-point window, both with
                // `max-width: calc(100vw - ...)`, ran to the window's edge.)
                if let Some(w) = width {
                    ui.set_min_width(w);
                    ui.set_max_width(w.max(1.0));
                } else {
                    if let Some(w) = min_width {
                        ui.set_min_width((w - inset).max(0.0));
                    }
                    if let Some(w) = max_width {
                        ui.set_max_width((w - inset).max(1.0));
                    }
                }
                if let Some(h) = height.or(min_height) {
                    ui.set_min_height(h);
                }
                // A definite height is the box's own: what it holds is laid
                // out in it, so a child's `height: 100%` is that height
                // (a 6-pixel progress bar's fill), not the window's.
                if let Some(h) = height {
                    ui.set_max_height(h.max(1.0));
                }
                let end = style
                    .align_items
                    .as_deref()
                    .is_some_and(|a| matches!(a.trim(), "flex-end" | "end"));
                // A flex column that ends its items puts them at its right.
                let column_end = !row && end && display.contains("flex");
                let layout = if row {
                    Layout::left_to_right(if stretch_items {
                        Align::Min
                    } else if center {
                        Align::Center
                    } else if end {
                        Align::Max
                    } else {
                        Align::Min
                    })
                } else {
                    Layout::top_down(if cross_center {
                        Align::Center
                    } else if column_end {
                        Align::Max
                    } else {
                        Align::Min
                    })
                };
                // A row takes the height its content needs, as egui's own
                // horizontal runs do: a centred row given all the room left
                // would fill it (an image in qr-share's dialog pushed its
                // buttons off the window).
                let shrink_id = egui::Id::new(("luna-row-width", node.id));
                // Only boxes sized by what they hold: an image or a form
                // control sizes itself from the room it is given.
                // In a page a column that centres or ends its items and has
                // no width of its own is as wide as its widest item, as a
                // flex item is sized by what it holds (miniplayer's cover
                // column sat in the middle of the row): it is placed in the
                // room it took last frame.
                // An item of a flex column that centres or ends its items
                // is as wide as what it holds too, unless that was wider
                // than the column last frame: it then fills it and its words
                // wrap (CSS's fit-content).
                let last_used: Option<f32> = ui.data(|d| d.get_temp(shrink_id));
                let fits = fit_item && last_used.is_none_or(|w| w <= ui.available_width() + 0.5);
                let shrink = (shrink
                    && (row || (plugin_dom::drawing_page() && (cross_center || end)))
                    || fits)
                    && width.is_none()
                    && !matches!(
                        node.tag.as_str(),
                        "img" | "svg" | "canvas" | "input" | "textarea" | "select"
                    );
                let wrapped: Option<f32> = shrink.then_some(last_used).flatten();
                // A row with no height of its own is as tall as what it held
                // last frame, so it centres that: egui's 18-point row hung a
                // taller child from its top (the audio-visualizer's panel
                // sat 11 points low in its group).
                let row_height_id = egui::Id::new(("luna-row-height", node.id));
                let free_height = row && height.or(min_height).is_none();
                let held: Option<f32> = free_height
                    .then(|| ui.data(|d| d.get_temp(row_height_id)))
                    .flatten();
                let room = if row {
                    vec2(
                        wrapped
                            .unwrap_or(ui.available_width())
                            .min(ui.available_width()),
                        // A line box is `line-height` tall when it says.
                        height
                            .or(min_height)
                            .or(inherit.line_height.map(|l| l.points(inherit.size)))
                            .unwrap_or_else(|| {
                                // A row inside another box is as tall as
                                // what it holds, as in CSS (DownloadManager's
                                // nav items are 34 points, not egui's
                                // 28-point row and their padding); a slot's
                                // own element keeps egui's row.
                                let replaced = matches!(
                                    node.tag.as_str(),
                                    "img"
                                        | "svg"
                                        | "canvas"
                                        | "video"
                                        | "input"
                                        | "textarea"
                                        | "select"
                                );
                                let least = if nested && !replaced {
                                    0.0
                                } else {
                                    ui.spacing().interact_size.y
                                };
                                held.unwrap_or(least).max(least)
                            })
                            .min(ui.available_height()),
                    )
                } else if shrink {
                    vec2(
                        wrapped
                            .unwrap_or(ui.available_width())
                            .min(ui.available_width()),
                        ui.available_height(),
                    )
                } else {
                    ui.available_size()
                };
                let offset = asked.as_ref().and_then(|a| a.top);
                scroll_box(ui, node.id, scroll, style, offset, |ui| {
                    ui.allocate_ui_with_layout(room, layout, |ui| {
                        if shrink {
                            // Its width is what it holds, which must not depend
                            // on the room it got last frame.
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        }
                        if row {
                            ui.spacing_mut().item_spacing.x = gap;
                        } else {
                            ui.spacing_mut().item_spacing.y = gap;
                        }
                        // `justify-content` centre/end: shift by the free room
                        // left last frame.
                        let last: Option<Vec2> = ui.data(|d| d.get_temp(content_id));
                        // In a page `space-between`, `-around` and `-evenly`
                        // share the free room out round the items, as CSS
                        // does (miniplayer's header and time row); `last` is
                        // then what the items took without it.
                        let items = flow.iter().filter(|c| !c.style.hidden()).count();
                        let spaced = items > 0
                            && matches!(justify, "space-between" | "space-around" | "space-evenly");
                        let mut between = 0.0;
                        if spaced && let Some(used) = last {
                            let free = if row {
                                width.or(min_width).map_or(0.0, |w| w - used.x)
                            } else {
                                height.or(min_height).map_or(0.0, |h| h - used.y)
                            }
                            .max(0.0);
                            let n = items as f32;
                            let (lead, more) = match justify {
                                "space-between" if items > 1 => (0.0, free / (n - 1.0)),
                                "space-between" => (0.0, 0.0),
                                "space-around" => (free / (2.0 * n), free / n),
                                _ => (free / (n + 1.0), free / (n + 1.0)),
                            };
                            if lead > 0.5 {
                                ui.add_space(lead);
                            }
                            between = more;
                            if row {
                                ui.spacing_mut().item_spacing.x += between;
                            } else {
                                ui.spacing_mut().item_spacing.y += between;
                            }
                        }
                        if let Some(used) = last
                            && !spaced
                            && matches!(
                                justify,
                                "center" | "flex-end" | "end" | "space-around" | "space-evenly"
                            )
                        {
                            // Only a box with a definite size has room to share.
                            let free = if row {
                                width.or(min_width).map_or(0.0, |w| w - used.x)
                            } else {
                                height.or(min_height).map_or(0.0, |h| h - used.y)
                            };
                            let part = if justify == "flex-end" || justify == "end" {
                                1.0
                            } else {
                                0.5
                            };
                            if free > 0.5 {
                                ui.add_space(free * part);
                            }
                        }
                        let begin = ui.cursor().min;
                        // Where a flex row puts its items across (by the
                        // height of its content: its own, or what it held
                        // last frame).
                        let across = across_part(row, display, style, stretch_items);
                        // An item's own `align-self` over the row's
                        // `align-items`: where it stands across, and the
                        // height a stretched one is given.
                        let flex_line = row && display.contains("flex") && !wraps(style);
                        let item_part = |child: &VNode| match self_part(child) {
                            Some(p) if flex_line && child.tag != "#text" => p,
                            _ => across,
                        };
                        let item_stretched = |child: &VNode| match self_part(child) {
                            Some(Some(_)) if flex_line => None,
                            Some(None)
                                if flex_line
                                    && child.tag != "#text"
                                    && child.style.height.is_none() =>
                            {
                                height
                            }
                            _ => stretched(child),
                        };
                        let row_h = ui.available_height();
                        // What the children would take if nothing held
                        // them: in a column, the sum of the room each
                        // would take over what it took; in a row, the
                        // tallest.
                        let mut natural_more = 0.0f32;
                        let mut natural_row: Option<f32> = None;
                        drawn |= leaf(ui, node, inherit, width, height, rot, cx);
                        if let Some(text) = node.text.as_deref().filter(|t| !t.is_empty())
                            && !matches!(
                                node.tag.as_str(),
                                "input" | "textarea" | "select" | "svg" | "img"
                            )
                        {
                            let wrap = if ellipsis {
                                Some(egui::TextWrapMode::Truncate)
                            } else {
                                (!breakable(text)).then_some(egui::TextWrapMode::Extend)
                            };
                            let text = rich(ui.ctx(), text, inherit, style);
                            match text_place {
                                Some(align) if !ellipsis => {
                                    add_aligned(ui, text, inherit, align);
                                }
                                _ => {
                                    add_text(ui, text, wrap, inherit);
                                }
                            }
                            drawn = true;
                        }
                        // A grid's gaps are its own (none unless set):
                        // `gap: <row> <column>`, `column-gap` over it.
                        let grid_gaps = {
                            let gap = style
                                .gap
                                .as_deref()
                                .map(|g| css::resolve_vars(g, cx.vars))
                                .unwrap_or_default();
                            let words = css::split_top(gap.trim(), ' ');
                            let len = |w: Option<&&str>| {
                                w.and_then(|w| css::length(w, font, 0.0, cx.viewport))
                                    .unwrap_or(0.0)
                                    .max(0.0)
                            };
                            let row_gap = len(words.first());
                            let column_gap = cx
                                .len(&style.column_gap, font, 0.0)
                                .unwrap_or_else(|| len(words.get(1).or(words.first())));
                            [row_gap, column_gap.max(0.0)]
                        };
                        let grid = display
                            .contains("grid")
                            .then_some(style.grid_template_columns.as_deref())
                            .flatten()
                            .and_then(|t| {
                                grid_tracks(
                                    &css::resolve_vars(t, cx.vars),
                                    ui.available_width(),
                                    font,
                                    cx.viewport,
                                )
                            });
                        if let Some(tracks) = &grid {
                            let align = match style.align_items.as_deref().map(str::trim) {
                                Some("center") => Align::Center,
                                Some("end" | "flex-end" | "self-end") => Align::Max,
                                _ => Align::Min,
                            };
                            drawn |= draw_grid(ui, &flow, tracks, grid_gaps, align, inherit, cx);
                        } else if flows {
                            let align = style.text_align.as_deref().map_or("", str::trim);
                            drawn |= draw_inline_flow(ui, node.id, &flow, align, inherit, cx);
                        } else if !matches!(
                            node.tag.as_str(),
                            "select" | "svg" | "canvas" | "video"
                        ) {
                            // A row of a set width whose items all grow shares
                            // it out by their `flex-grow` (the audiovisualiser's
                            // bars); otherwise items take their own sizes.
                            let grows: Vec<f32> =
                                flow.iter().map(|c| flex_grow(&c.style)).collect();
                            let total: f32 = grows.iter().sum();
                            let shared = row
                                && width.is_some()
                                && !flow.is_empty()
                                && grows.iter().all(|g| *g > 0.0);
                            if shared {
                                let gaps = gap * (flow.len() - 1) as f32;
                                let room = (ui.available_width() - gaps).max(0.0);
                                let h = ui.available_height();
                                for (child, g) in flow.iter().zip(&grows) {
                                    let share = room * g / total;
                                    // `align-items` places the item in its cell
                                    // by the height its style gives it, else by
                                    // the height it had last frame.
                                    let cross_id = egui::Id::new(("luna-cross", child.id));
                                    let last: Option<f32> = ui.data(|d| d.get_temp(cross_id));
                                    let own = cx
                                        .len(&child.style.height, font, h)
                                        .map(|v| {
                                            v * child
                                                .style
                                                .transform
                                                .as_deref()
                                                .map_or(1.0, |t| axis_scale(t).y)
                                        })
                                        .or(last)
                                        .unwrap_or(h)
                                        .clamp(0.0, h);
                                    let part = match self_part(child) {
                                        Some(Some(p)) => p,
                                        Some(None) => 0.0,
                                        None if end => 1.0,
                                        None if center => 0.5,
                                        None => 0.0,
                                    };
                                    let skip = (h - own) * part;
                                    // The cell keeps the row's height (a `%`
                                    // height is of it), moved down by `skip`.
                                    let at = ui.cursor().min;
                                    let mut cell = ui.new_child(
                                        UiBuilder::new()
                                            .max_rect(Rect::from_min_size(
                                                egui::pos2(at.x, at.y + skip),
                                                vec2(share, h),
                                            ))
                                            .layout(Layout::top_down(Align::Min)),
                                    );
                                    drawn |= draw_given(
                                        &mut cell,
                                        child,
                                        item_stretched(child),
                                        inherit,
                                        cx,
                                    );
                                    let took = cell.min_rect().height();
                                    if last.is_none_or(|l| (l - took).abs() > 0.5) {
                                        ui.data_mut(|d| d.insert_temp(cross_id, took));
                                        plugin_dom::request_frame(ui.ctx());
                                    }
                                    let n = natural_of(ui, child.id).unwrap_or(took);
                                    natural_row = Some(natural_row.map_or(n, |r| r.max(n)));
                                    ui.allocate_exact_size(vec2(share, h), Sense::hover());
                                }
                            } else if row && total > 0.0 {
                                // Some items grow (AltPlay's `flex: 1` spacer
                                // between its status and its buttons): they
                                // share what the others, as wide as last frame,
                                // leave, so the ones after them stay inside.
                                let width_id = |c: &VNode| egui::Id::new(("luna-flex-w", c.id));
                                let others: f32 = flow
                                    .iter()
                                    .zip(&grows)
                                    .filter(|(_, g)| **g <= 0.0)
                                    .map(|(c, _)| {
                                        ui.data(|d| d.get_temp::<f32>(width_id(c))).unwrap_or(0.0)
                                    })
                                    .sum();
                                let gaps = gap * flow.len().saturating_sub(1) as f32;
                                let free = (ui.available_width() - others - gaps).max(0.0);
                                for (child, g) in flow.iter().zip(&grows) {
                                    if *g > 0.0
                                        && let Some(part) = item_part(child)
                                    {
                                        let w = free * g / total;
                                        let (shown, took) = draw_across(
                                            ui,
                                            child,
                                            row_h,
                                            part,
                                            Some(w),
                                            inherit,
                                            cx,
                                        );
                                        drawn |= shown;
                                        let n = natural_of(ui, child.id).unwrap_or(took);
                                        natural_row = Some(natural_row.map_or(n, |r| r.max(n)));
                                    } else if *g > 0.0 {
                                        let w = free * g / total;
                                        let r = ui.allocate_ui_with_layout(
                                            vec2(w, ui.available_height()),
                                            Layout::left_to_right(Align::Center),
                                            |ui| {
                                                ui.set_width(w);
                                                drawn |= draw_given(
                                                    ui,
                                                    child,
                                                    item_stretched(child),
                                                    inherit,
                                                    cx,
                                                );
                                            },
                                        );
                                        let n = natural_of(ui, child.id)
                                            .unwrap_or_else(|| r.response.rect.height());
                                        natural_row = Some(natural_row.map_or(n, |r| r.max(n)));
                                    } else {
                                        // In a page an item that does not grow
                                        // is as wide as what it holds.
                                        SHRINK.with(|s| s.set(plugin_dom::drawing_page()));
                                        // A row that centres or ends its items
                                        // puts one shorter than the row in its
                                        // middle or at its foot, by the heights
                                        // of last frame (miniplayer's cover
                                        // beside its taller song details).
                                        let r = ui.scope(|ui| match item_part(child) {
                                            Some(part) => draw_across(
                                                ui, child, row_h, part, None, inherit, cx,
                                            ),
                                            None => {
                                                let top = ui.cursor().min.y;
                                                let shown = draw_given(
                                                    ui,
                                                    child,
                                                    item_stretched(child),
                                                    inherit,
                                                    cx,
                                                );
                                                (shown, ui.min_rect().max.y - top)
                                            }
                                        });
                                        SHRINK.with(|s| s.set(false));
                                        let (shown, own_h) = r.inner;
                                        drawn |= shown;
                                        let n = natural_of(ui, child.id).unwrap_or(own_h);
                                        natural_row = Some(natural_row.map_or(n, |r| r.max(n)));
                                        let used = r.response.rect.width();
                                        let last: Option<f32> =
                                            ui.data(|d| d.get_temp(width_id(child)));
                                        if last.is_none_or(|l| (l - used).abs() > 0.5) {
                                            ui.data_mut(|d| d.insert_temp(width_id(child), used));
                                            plugin_dom::request_frame(ui.ctx());
                                        }
                                    }
                                }
                            } else if row && wraps(style) {
                                drawn |= draw_wrapped(ui, &flow, gap, font, inherit, cx);
                            } else {
                                // A row of a set height that centres its items
                                // (a backdrop holding a dialog) puts each box in
                                // the middle by the height it had last frame,
                                // and so does a flex row that centres or ends
                                // them, whatever its height.
                                let cross = !stretch_items
                                    && row
                                    && center
                                    && height.or(min_height).is_some();
                                // A column of a definite height shares what its
                                // other items, as tall as last frame, leave among
                                // those that grow (DownloadManager's app list,
                                // `flex: 1`, puts its footer at the menu's foot).
                                let height_id = |c: &VNode| egui::Id::new(("luna-flex-h", c.id));
                                let column_free =
                                    (!row && total > 0.0).then_some(height).flatten().map(|h| {
                                        let others: f32 = flow
                                            .iter()
                                            .zip(&grows)
                                            .filter(|(_, g)| **g <= 0.0)
                                            .map(|(c, _)| {
                                                ui.data(|d| d.get_temp::<f32>(height_id(c)))
                                                    .unwrap_or(0.0)
                                            })
                                            .sum();
                                        let gaps = gap * flow.len().saturating_sub(1) as f32;
                                        (h - others - gaps).max(0.0)
                                    });
                                for (child, g) in flow.iter().zip(&grows) {
                                    if let Some(align) = text_place
                                        && child.tag == "#text"
                                    {
                                        if let Some(text) =
                                            child.text.as_deref().filter(|t| !t.trim().is_empty())
                                        {
                                            add_aligned(
                                                ui,
                                                rich(ui.ctx(), text, inherit, &child.style),
                                                inherit,
                                                align,
                                            );
                                            drawn = true;
                                        }
                                        continue;
                                    }
                                    if let Some(part) = item_part(child)
                                        && child.tag != "#text"
                                    {
                                        let (shown, _) =
                                            draw_across(ui, child, row_h, part, None, inherit, cx);
                                        drawn |= shown;
                                        continue;
                                    }
                                    if cross && child.tag != "#text" && self_part(child).is_none() {
                                        let full = ui.available_height();
                                        let hid = egui::Id::new(("luna-cross", child.id));
                                        let last: Option<f32> = ui.data(|d| d.get_temp(hid));
                                        let skip = ((full - last.unwrap_or(full)) / 2.0).max(0.0);
                                        let r = ui.allocate_ui_with_layout(
                                            vec2(ui.available_width(), full),
                                            Layout::top_down(Align::Min),
                                            |ui| {
                                                ui.add_space(skip);
                                                let top = ui.cursor().min.y;
                                                let shown = draw(ui, child, inherit, cx);
                                                (shown, ui.min_rect().max.y - top)
                                            },
                                        );
                                        let (shown, used) = r.inner;
                                        drawn |= shown;
                                        if last.is_none_or(|l| (l - used).abs() > 0.5) {
                                            ui.data_mut(|d| d.insert_temp(hid, used));
                                            plugin_dom::request_frame(ui.ctx());
                                        }
                                        continue;
                                    }
                                    // A centring column's items, and in a page
                                    // a flex row's, are as wide as what they
                                    // hold; a flex column's item may say where
                                    // it stands itself (`align-self`).
                                    let own = (!row && display.contains("flex"))
                                        .then(|| self_part(child))
                                        .flatten();
                                    let (fit, stretch) = match own {
                                        Some(Some(_)) => (true, false),
                                        Some(None) => (false, true),
                                        None => (
                                            cross_center || column_end,
                                            !cross_center && !column_end,
                                        ),
                                    };
                                    SHRINK.with(|s| {
                                        s.set(
                                            (!row && fit)
                                                || (row
                                                    && plugin_dom::drawing_page()
                                                    && display.contains("flex")),
                                        )
                                    });
                                    FIT_ITEM
                                        .with(|s| s.set(!row && fit && display.contains("flex")));
                                    IN_COLUMN.with(|s| s.set(!row && stretch));
                                    FLEX_COLUMN.with(|s| {
                                        s.set(!row && stretch && display.contains("flex"))
                                    });
                                    let top = ui.cursor().min.y;
                                    let item = |ui: &mut egui::Ui| -> bool {
                                        match column_free {
                                            Some(free) if *g > 0.0 => {
                                                let share = free * g / total;
                                                draw_given(ui, child, Some(share), inherit, cx)
                                            }
                                            Some(_) => {
                                                let r = ui.scope(|ui| draw(ui, child, inherit, cx));
                                                let used = r.response.rect.height();
                                                let last: Option<f32> =
                                                    ui.data(|d| d.get_temp(height_id(child)));
                                                if last.is_none_or(|l| (l - used).abs() > 0.5) {
                                                    ui.data_mut(|d| {
                                                        d.insert_temp(height_id(child), used)
                                                    });
                                                    plugin_dom::request_frame(ui.ctx());
                                                }
                                                r.inner
                                            }
                                            None => draw_given(
                                                ui,
                                                child,
                                                item_stretched(child),
                                                inherit,
                                                cx,
                                            ),
                                        }
                                    };
                                    // An item that stands elsewhere across the
                                    // column than its siblings is laid out in a
                                    // run of its own, placed so.
                                    drawn |= match own {
                                        Some(part) => {
                                            let align = match part {
                                                Some(p) if p >= 0.75 => Align::Max,
                                                Some(p) if p >= 0.25 => Align::Center,
                                                _ => Align::Min,
                                            };
                                            ui.allocate_ui_with_layout(
                                                vec2(ui.available_width(), 0.0),
                                                Layout::top_down(align),
                                                item,
                                            )
                                            .inner
                                        }
                                        None => item(ui),
                                    };
                                    if let Some(n) = natural_of(ui, child.id) {
                                        if row {
                                            natural_row = Some(natural_row.map_or(n, |r| r.max(n)));
                                        } else {
                                            natural_more +=
                                                n - (ui.min_rect().max.y - top).max(0.0);
                                        }
                                    }
                                    SHRINK.with(|s| s.set(false));
                                    FIT_ITEM.with(|s| s.set(false));
                                    IN_COLUMN.with(|s| s.set(false));
                                    FLEX_COLUMN.with(|s| s.set(false));
                                }
                            }
                        }
                        let took = (ui.min_rect().max.y - begin.y).max(0.0);
                        content_natural = Some(if row {
                            natural_row.unwrap_or(took)
                        } else {
                            (took + natural_more).max(0.0)
                        });
                        if shrink {
                            let used = ui.min_rect().width();
                            if wrapped.is_none_or(|w| (w - used).abs() > 0.5) {
                                ui.data_mut(|d| d.insert_temp(shrink_id, used));
                                plugin_dom::request_frame(ui.ctx());
                            }
                        }
                        if free_height {
                            let h = ui.min_rect().height();
                            if held.is_none_or(|l| (l - h).abs() > 0.5) {
                                ui.data_mut(|d| d.insert_temp(row_height_id, h));
                                plugin_dom::request_frame(ui.ctx());
                            }
                        }
                        if !justify.is_empty() {
                            let used = ui.min_rect().max - begin;
                            // The items' own room, without what was shared
                            // out between them.
                            let shared = between * items.saturating_sub(1) as f32;
                            let used = if row {
                                vec2(used.x - shared, used.y)
                            } else {
                                vec2(used.x, used.y - shared)
                            };
                            let used = vec2(used.x.max(0.0), used.y.max(0.0));
                            if last.is_none_or(|l| (l - used).length() > 0.5) {
                                ui.data_mut(|d| d.insert_temp(content_id, used));
                                plugin_dom::request_frame(ui.ctx());
                            }
                        }
                    })
                });
            })
            .response;
        if let (Some(idx), Some(shadow)) = (shadow_slot, shadow) {
            let r = shown.rect - margin.map_or(Margin::ZERO, margin_of);
            ui.painter().set(
                idx,
                plugin_dom::outer_shadow_shape(
                    r,
                    CornerRadius::same(radius.clamp(0.0, 255.0) as u8),
                    shadow,
                ),
            );
        }
        if let Some(idx) = hand {
            let r = shown.rect - margin.map_or(Margin::ZERO, margin_of);
            let edges = border.map(|(_, c)| Edges {
                widths: bw,
                colours: sides.unwrap_or([c; 4]),
                dash,
            });
            // `border-radius: 50%` rounds the whole box, border included.
            let half = style
                .border_radius
                .as_deref()
                .and_then(|v| v.trim().strip_suffix('%')?.trim().parse::<f32>().ok())
                .is_some_and(|p| p >= 50.0);
            let radius = if half {
                r.width().max(r.height())
            } else {
                radius
            };
            if let Some(edge) = hand_edge {
                ui.painter().set(idx, box_shape(r, radius, fill, None, rot));
                ui.painter()
                    .set(edge, box_shape(r, radius, None, edges, rot));
            } else {
                ui.painter()
                    .set(idx, box_shape(r, radius, fill, edges, rot));
            }
        }
        if let Some(idx) = bg {
            let r = shown.rect - margin.map_or(Margin::ZERO, margin_of);
            if has_url {
                paint_background(ui, idx, r, style, rot, cx);
            } else if let Some(layers) = &gradient {
                // The whole gradients, inside the box's corners
                // (custom-fonts' pink-to-blue tag), the first on top.
                let corners = CornerRadius::same(radius.clamp(0.0, 255.0) as u8);
                let meshes: Vec<egui::Shape> = layers
                    .iter()
                    .rev()
                    .filter_map(|layer| match layer {
                        BoxGradient::Linear(g) => {
                            super::theme_gradient::mesh(r, Some(corners), g, g.angle, g.position)
                        }
                        BoxGradient::Radial(layer) => {
                            plugin_dom::radial_box_mesh(layer, r, Some(corners))
                        }
                    })
                    .map(egui::Shape::mesh)
                    .collect();
                ui.painter().set(idx, egui::Shape::Vec(meshes));
            }
        }
        // `filter: brightness(<1)`: the box and what it holds, dimmed (as
        // black over opaque pixels is).
        if let Some(b) = style.filter.as_deref().and_then(brightness)
            && b < 1.0
        {
            let r = shown.rect - margin.map_or(Margin::ZERO, margin_of);
            ui.painter().rect_filled(
                r,
                CornerRadius::same(radius.clamp(0.0, 255.0) as u8),
                Color32::from_black_alpha(((1.0 - b) * 255.0).round() as u8),
            );
        }
        shown
    });
    let rect = inner.inner.rect;
    if let Some(c) = content_natural {
        if held_column {
            let last: Option<f32> = ui.data(|d| d.get_temp(natural_id));
            if last.is_none_or(|l| (l - c).abs() > 0.5) {
                ui.data_mut(|d| d.insert_temp(natural_id, c));
                plugin_dom::request_frame(ui.ctx());
            }
        }
        // Its own height, else what it holds, within its `min-height` and
        // `max-height`, with its padding, border and margins.
        let mut h = own_inner.unwrap_or(c);
        if let Some(m) = max_inner {
            h = h.min(m);
        }
        if let Some(m) = min_height {
            h = h.max(m);
        }
        note_natural(
            ui,
            node.id,
            h + block_inset + margin.map_or(0.0, |m| m[0] + m[2]),
        );
    }
    NESTED.with(|n| n.set(nested));
    CONTAINER_HEIGHT.with(|c| c.set(outer_basis));
    if list {
        LIST_ITEMS.with(|l| l.borrow_mut().pop());
    }
    // A list item's marker hangs outside it, as Chromium's `outside`
    // marker: its text ends a space before the item's border box, on the
    // middle of its first line (DownloadManager's About bullets).
    if node.tag == "li" && (display.is_empty() || display.contains("list-item")) {
        let n = LIST_ITEMS.with(|l| {
            let mut l = l.borrow_mut();
            l.last_mut().map_or(1, |c| {
                *c += 1;
                *c
            })
        });
        let m = margin.unwrap_or([0.0; 4]);
        let font_id = font_of(ui.ctx(), inherit, style);
        let row = ui.ctx().fonts_mut(|f| f.row_height(&font_id));
        let line = inherit.line_height.map_or(row, |l| l.points(inherit.size));
        let left = rect.left() + m[3];
        let top = rect.top() + m[0] + bw[0] + pads.map_or(0.0, |p| p[0]);
        // The first line's baseline, the face's ascent being about 0.8 of
        // its row.
        let ascent = row * 0.8;
        let baseline = top + (line - row) / 2.0 + ascent;
        match inherit.marker {
            Marker::None => {}
            Marker::Decimal => {
                if let Some(text) = inherit.marker.text(n) {
                    // "1. ": the number, then a space before the item.
                    let galley = ui.painter().layout_no_wrap(text, font_id, inherit.color);
                    let pos = egui::pos2(
                        left - inherit.size * 0.28 - galley.size().x,
                        top + (line - galley.size().y) / 2.0,
                    );
                    ui.painter().galley(pos, galley, inherit.color);
                }
            }
            shape => {
                // Chromium's symbol marker (`LayoutListMarker`): a box
                // `(ascent * 2/3 + 1) / 2` wide, its top half the ascent over
                // the baseline, 7 pixels of padding before the item.
                let d = ((ascent * 2.0 / 3.0 + 1.0) / 2.0).floor().max(2.0);
                let r = Rect::from_min_size(
                    egui::pos2(left - 7.0 - d + 1.0, baseline - ascent / 2.0),
                    vec2(d, d),
                );
                let p = ui.painter();
                match shape {
                    Marker::Square => {
                        p.rect_filled(r, 0.0, inherit.color);
                    }
                    Marker::Circle => {
                        p.circle_stroke(r.center(), d / 2.0 - 0.5, Stroke::new(1.0, inherit.color));
                    }
                    _ => {
                        p.circle_filled(r.center(), d / 2.0, inherit.color);
                    }
                }
            }
        }
    }
    if !under.is_empty() || !between.is_empty() {
        let last: Option<Rect> = ui.data(|d| d.get_temp(box_id));
        if last.is_none_or(|l| {
            (l.min - rect.min).length() > 0.5 || (l.size() - rect.size()).length() > 0.5
        }) {
            ui.data_mut(|d| d.insert_temp(box_id, rect));
            plugin_dom::request_frame(ui.ctx());
        }
    }
    if node.id & MEASURE_ID == 0 {
        // Scripts read the border box (`getBoundingClientRect`), and a drag
        // region is the border box too: the margins are outside both.
        let border_box = rect - margin.map_or(Margin::ZERO, margin_of);
        plugin_dom::node_drawn(node.id, border_box);
        if let Some(block) = asked.as_ref().and_then(|a| a.block.as_deref()) {
            let align = match block {
                "center" => Some(Align::Center),
                "end" => Some(Align::Max),
                "nearest" => None,
                _ => Some(Align::Min),
            };
            let animation = if asked.as_ref().is_some_and(|a| a.smooth) {
                ui.style().scroll_animation
            } else {
                egui::style::ScrollAnimation::none()
            };
            ui.scroll_to_rect_animation(border_box, align, animation);
        }
        if let Some(region) = style.app_region.as_deref() {
            plugin_dom::app_region(border_box, region.trim() == "drag");
        }
    }
    // Absolutely positioned children sit in this element's box.
    for child in placed {
        if !child.style.hidden() {
            place(ui, child, rect, inherit);
            drawn = true;
        }
    }
    // `pointer-events: none` (radiant-lyrics' full-window background canvas)
    // lets the pointer through to what is under it.
    if !inert && ui.rect_contains_pointer(rect) {
        let moves = node
            .listens
            .iter()
            .any(|l| l == "mousemove" || l == "pointermove");
        plugin_dom::pointer_over_node(node.id, rect, moves);
    }
    drawn |= fill.is_some() || width.is_some() || height.is_some() || has_url;
    if !drawn {
        return false;
    }

    if inert {
        return true;
    }
    let sense = if clickable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, egui::Id::new(("luna-node", node.id)), sense);
    let pointer = style.cursor.as_deref() == Some("pointer") || is_button || is_link;
    let response = if pointer {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    if let Some(title) = attr(node, "title").or_else(|| attr(node, "aria-label"))
        && !title.is_empty()
        && (is_button || is_link)
    {
        response.clone().on_hover_text(title);
    }
    events(ui, node, &response);
    true
}

/// Send the listened-for events of an element's response.
fn events(ui: &egui::Ui, node: &VNode, response: &egui::Response) {
    let listens = |kind: &str| node.listens.iter().any(|l| l == kind);
    // egui gives a press to the last widget laid over the pointer, which is
    // the outermost element (its box is known only after its children
    // drew). A browser aims clicks at the deepest element under the pointer
    // and lets them bubble, so they go there (star-ratings tells its five
    // star SVGs apart by `event.target`).
    let deepest = plugin_dom::pointer_node()
        .filter(|(_, r, _)| response.rect.contains_rect(*r) && *r != response.rect);
    let target = DomTarget::Node(deepest.map_or(node.id, |(id, _, _)| id));
    let data = |ui: &egui::Ui| {
        let pos = response
            .interact_pointer_pos()
            .or(response.hover_pos())
            .unwrap_or_default();
        let m = ui.input(|i| i.modifiers);
        let r = deepest.map_or(response.rect, |(_, r, _)| r);
        // The element's drawn box goes along, so `getBoundingClientRect()`
        // in the handler is the element's own, not its anchor's (star-ratings
        // reads which half of a star was clicked).
        json!({
            "clientX": pos.x, "clientY": pos.y, "button": 0,
            "shiftKey": m.shift, "ctrlKey": m.ctrl, "altKey": m.alt, "metaKey": m.mac_cmd,
            "rect": [r.min.x, r.min.y, r.width(), r.height()],
        })
    };
    if response.clicked() && (listens("click") || node.tag == "button" || node.tag == "a") {
        plugin_dom::send(target.clone(), "click", data(ui));
    }
    if response.double_clicked() && listens("dblclick") {
        plugin_dom::send(target.clone(), "dblclick", data(ui));
    }
    // Unless it went with the press (`plugin_dom::filter_raw_input`).
    if response.secondary_clicked() && listens("contextmenu") && !plugin_dom::contextmenu_sent() {
        let mut d = data(ui);
        d["button"] = json!(2);
        plugin_dom::send(target.clone(), "contextmenu", d);
    }
    let hover_kinds = ["mouseenter", "mouseleave", "mouseover", "mouseout"];
    if hover_kinds.iter().any(|k| listens(k)) {
        // The pointer is over an element whatever lies inside or around it
        // on the same layer: egui counts as hovered only the last widget
        // laid there, which is the outermost box when that box listens too
        // (element-hider's menu item under its menu), so `mouseenter` never
        // reached the item.
        let over = response.contains_pointer();
        match plugin_dom::hover_change(plugin_dom::HoverKey::Node(node.id), over) {
            Some(true) => {
                for kind in ["mouseenter", "mouseover"] {
                    if listens(kind) {
                        let mut d = data(ui);
                        let r = response.rect;
                        d["rect"] = json!([r.min.x, r.min.y, r.width(), r.height()]);
                        plugin_dom::send(DomTarget::Node(node.id), kind, d);
                    }
                }
            }
            Some(false) => {
                for kind in ["mouseleave", "mouseout"] {
                    if listens(kind) {
                        let mut d = data(ui);
                        let r = response.rect;
                        d["rect"] = json!([r.min.x, r.min.y, r.width(), r.height()]);
                        plugin_dom::send(DomTarget::Node(node.id), kind, d);
                    }
                }
            }
            None => {}
        }
    }
    if listens("wheel") && response.hovered() {
        let delta = plugin_dom::wheel_delta(ui.ctx());
        if delta != Vec2::ZERO {
            plugin_dom::send(
                target,
                "wheel",
                json!({ "deltaX": -delta.x, "deltaY": -delta.y, "deltaMode": 0,
                        "shiftKey": ui.input(|i| i.modifiers.shift) }),
            );
            plugin_dom::take_wheel(ui.ctx());
        }
    }
}

/// Whether a browser could wrap the text somewhere: at a space, after a
/// hyphen or slash, or between CJK characters. One that cannot ("00:40:30",
/// Total Play Time's pill) stays on one line and overflows, as in CSS.
fn breakable(text: &str) -> bool {
    text.trim().chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '-' | '/' | '\u{200B}' | '\u{2010}' | '\u{2013}' | '\u{2014}'
            )
            || u32::from(c) >= 0x2E80
    })
}

/// A text label, without its shadows. With a `line-height` taller than the face's own row each
/// line is that tall and its glyphs sit in its middle, as CSS splits the
/// leading above and below them (egui puts the room under the glyphs).
/// A line shorter than the face's row (`line-height: 1.3` in a face with a
/// tall row) takes only its own height, the glyphs centred in it.
fn add_line(
    ui: &mut egui::Ui,
    text: RichText,
    wrap: Option<egui::TextWrapMode>,
    inherit: Inherit,
) -> egui::Response {
    let Some(lh) = inherit.line_height else {
        let mut label = egui::Label::new(text);
        if let Some(wrap) = wrap {
            label = label.wrap_mode(wrap);
        }
        return ui.add(label);
    };
    let line = lh.points(inherit.size);
    let font = font_of(ui.ctx(), inherit, &Style::default());
    let face = ui.ctx().fonts_mut(|f| f.row_height(&font));
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        wrap,
        ui.available_width(),
        egui::FontSelection::Default,
    );
    let one_line = galley.rows.len() <= 1;
    let size = if one_line {
        vec2(galley.size().x, line)
    } else {
        galley.size()
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        // egui puts a tall line's room under the glyphs.
        let top = if one_line {
            rect.center().y - face / 2.0
        } else {
            rect.top() + ((line - face) / 2.0).max(0.0)
        };
        ui.painter()
            .galley(egui::pos2(rect.left(), top), galley, inherit.color);
    }
    response
}

/// Words that are a block's own text, wrapped at the block's width with
/// each line placed by its `text-align` (`align`: centre or right), as
/// DownloadManager's two-line empty-queue note stands centred.
fn add_aligned(
    ui: &mut egui::Ui,
    text: RichText,
    inherit: Inherit,
    align: Align,
) -> egui::Response {
    let width = ui.available_width().max(1.0);
    let mut job = egui::WidgetText::from(text).into_layout_job(
        ui.style(),
        egui::FontSelection::Default,
        Align::Min,
    );
    let job_mut = std::sync::Arc::make_mut(&mut job);
    job_mut.wrap.max_width = width;
    job_mut.halign = align;
    let line = inherit.line_height.map(|l| l.points(inherit.size));
    if let Some(line) = line {
        for s in &mut job_mut.sections {
            s.format.line_height = Some(line);
        }
    }
    let galley = ui.ctx().fonts_mut(|f| f.layout_job((*job).clone()));
    let (rect, response) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::hover());
    if ui.is_rect_visible(rect) {
        let x = match align {
            Align::Center => rect.center().x,
            Align::Max => rect.right(),
            Align::Min => rect.left(),
        };
        // egui puts a tall line's room under the glyphs: half of it goes
        // above, as CSS centres the glyphs in their line.
        let font = font_of(ui.ctx(), inherit, &Style::default());
        let face = ui.ctx().fonts_mut(|f| f.row_height(&font));
        let lift = line.map_or(0.0, |l| ((l - face) / 2.0).max(0.0));
        ui.painter()
            .galley(egui::pos2(x, rect.top() + lift), galley, inherit.color);
    }
    response
}

/// The font plugin text is drawn in: its weight and size, or a font the
/// plugin declared that egui's fonts in `ctx` have now.
fn font_of(ctx: &egui::Context, inherit: Inherit, style: &Style) -> egui::FontId {
    let font = if inherit.weight >= 700 {
        crate::theme::bold(inherit.size)
    } else if inherit.weight >= 600 {
        crate::theme::semibold(inherit.size)
    } else if inherit.weight >= 500 {
        crate::theme::medium(inherit.size)
    } else {
        crate::theme::regular(inherit.size)
    };
    let family = match style.font_family.as_deref().map(str::trim) {
        Some(value) if value != "inherit" => plugin_dom::element_font_id(value),
        _ => inherit.family,
    };
    match family.and_then(|f| plugin_dom::font_by_id(ctx, f, inherit.weight)) {
        Some(family) => egui::FontId::new(inherit.size, family),
        None => font,
    }
}

fn rich(ctx: &egui::Context, text: &str, inherit: Inherit, style: &Style) -> RichText {
    let font = font_of(ctx, inherit, style);
    let mut text = if inherit.upper {
        RichText::new(text.to_uppercase())
    } else {
        RichText::new(text)
    }
    .font(font)
    .color(inherit.color);
    if let Some(lh) = inherit.line_height {
        text = text.line_height(Some(lh.points(inherit.size)));
    }
    if inherit.italic {
        text = text.italics();
    }
    if let Some(d) = &style.text_decoration {
        if d.contains("underline") {
            text = text.underline();
        }
        if d.contains("line-through") {
            text = text.strikethrough();
        }
    }
    text
}

/// Replaced elements: images, SVG, form controls. `true` when drawn.
fn leaf(
    ui: &mut egui::Ui,
    node: &VNode,
    inherit: Inherit,
    width: Option<f32>,
    height: Option<f32>,
    rot: f32,
    cx: &Cx,
) -> bool {
    let attr_len =
        |name: &str| attr(node, name).and_then(|v| v.trim_end_matches("px").parse::<f32>().ok());
    match node.tag.as_str() {
        "canvas" => super::luna_canvas::show(ui, node, width, height),
        "video" => super::luna_video::show(ui, node, width, height),
        "svg" | "img" => {
            let (w, h) = (width.or(attr_len("width")), height.or(attr_len("height")));
            // An image CSS gives no size keeps its natural one (and its
            // shape when only one side is set), within the room it has.
            let natural = node.natural.filter(|[nw, nh]| *nw > 0.0 && *nh > 0.0);
            let size = match (w, h, natural) {
                (Some(w), Some(h), _) => vec2(w, h),
                (Some(w), None, Some([nw, nh])) => vec2(w, w * nh / nw),
                (None, Some(h), Some([nw, nh])) => vec2(h * nw / nh, h),
                (None, None, Some([nw, nh])) => {
                    let fit = (ui.available_width().max(1.0) / nw).min(1.0);
                    vec2(nw * fit, nh * fit)
                }
                (w, h, None) => vec2(
                    w.unwrap_or(inherit.size + 4.0),
                    h.unwrap_or(inherit.size + 4.0),
                ),
            };
            let Some(source) = image_source(node, inherit.color, inherit.fill, inherit.back) else {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let _ = rect;
                return true;
            };
            let image = match source {
                ImageSource::Uri(uri) => egui::Image::new(uri),
                ImageSource::Bytes(uri, bytes) => egui::Image::from_bytes(uri, bytes),
            };
            let pixelated = node
                .style
                .image_rendering
                .as_deref()
                .is_some_and(|r| r.contains("pixel") || r.contains("crisp"));
            let mut image = image.fit_to_exact_size(size).maintain_aspect_ratio(true);
            if pixelated {
                image = image.texture_options(egui::TextureOptions::NEAREST);
            }
            if rot != 0.0 {
                image = image.rotate(rot, Vec2::splat(0.5));
            }
            if let Some(r) = image_radius(&node.style, size, inherit.size, cx) {
                image = image.corner_radius(CornerRadius::same(r));
            }
            ui.add(image);
            true
        }
        "input" => input(ui, node, inherit, width, height, cx),
        "textarea" => text_input(ui, node, inherit, width, true),
        "select" => select(ui, node, width),
        _ => false,
    }
}

/// Whether an element wears one of TIDAL's CSS-module classes
/// (`_actionText_1d3b4c2`): TIDAL's own stylesheet styles it.
fn tidal_styled(node: &VNode) -> bool {
    attr(node, "class").is_some_and(|c| {
        c.split_whitespace().any(|w| {
            let Some(rest) = w.strip_prefix('_') else {
                return false;
            };
            let Some((name, hash)) = rest.rsplit_once('_') else {
                return false;
            };
            !name.is_empty()
                && name.chars().all(|ch| ch.is_ascii_alphanumeric())
                && (5..=7).contains(&hash.len())
                && hash.chars().all(|ch| ch.is_ascii_hexdigit())
        })
    })
}

/// The family a browser gives a `<button>` whose style names none
/// (Chromium's control font: Arial on Windows, else the system's face).
pub const BUTTON_FONT: &str = "Arial, system-ui";

/// An image's rounded corners: its `border-radius` (a length, or a share
/// of its shorter side: `50%` is a circle; qr-share's 12 px card).
fn image_radius(style: &Style, size: Vec2, font: f32, cx: &Cx) -> Option<u8> {
    let value = style.border_radius.as_deref()?;
    let r = radius_share(value, size.x, size.y)
        .or_else(|| cx.len(&style.border_radius, font, size.min_elem()))?;
    (r > 0.0).then(|| r.min(size.min_elem() / 2.0).min(255.0).round() as u8)
}

enum ImageSource {
    Uri(String),
    Bytes(String, Vec<u8>),
}

/// Where an `img`/`svg` element's picture comes from. SVG markup is drawn
/// by egui_extras' resvg loader; `currentColor` takes the text colour, and
/// TIDAL's sprite icons (`__luna-paint__`) the CSS `fill`, else the text
/// colour.
fn image_source(
    node: &VNode,
    color: Color32,
    fill: Option<Color32>,
    back: Color32,
) -> Option<ImageSource> {
    let markup = node.markup.as_deref().or_else(|| attr(node, "src"))?;
    let markup: std::borrow::Cow<'_, str> = if markup.contains(PAINT) {
        sprite_paint(markup, fill.unwrap_or(color)).into()
    } else {
        markup.into()
    };
    if luminance(back) > 0.4 && markup.trim_start().starts_with('<') && !plugin_dom::drawing_page()
    {
        let resolved = if markup.contains("var(") {
            css::resolve_vars(&markup, &plugin_dom::vars())
        } else {
            markup.into_owned()
        };
        return source_of(&light_ink(&resolved, back), color);
    }
    source_of(&markup, color)
}

/// SVG paints a plugin chose for TIDAL's dark window (star-ratings' 28 %
/// white empty stars, OneClickLyrics' white icon) on a light backdrop:
/// each `fill`/`stroke` colour that would not read there takes the dark ink
/// [`readable`] gives plugin text, with its own opacity. Only on a light
/// backdrop: in Dark the plugin's colours are drawn as in TIDAL.
fn light_ink(svg: &str, back: Color32) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    loop {
        // The next `fill=`/`stroke=` attribute or `fill:`/`stroke:` style.
        let next = ["fill=", "stroke=", "fill:", "stroke:"]
            .iter()
            .filter_map(|p| rest.find(p).map(|at| (at, p.len())))
            .filter(|(at, _)| {
                // Not the tail of another name (`stop-fill`, `-webkit-...`).
                rest[..*at]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-'))
            })
            .min_by_key(|(at, _)| *at);
        let Some((at, len)) = next else {
            out.push_str(rest);
            return out;
        };
        let (head, tail) = rest.split_at(at + len);
        out.push_str(head);
        let quote = tail.chars().next().filter(|c| *c == '"' || *c == '\'');
        let body = if quote.is_some() { &tail[1..] } else { tail };
        let end = body
            .find(|c: char| match quote {
                Some(q) => c == q,
                None => c == ';' || c == '"' || c == '\'',
            })
            .unwrap_or(body.len());
        let value = &body[..end];
        if let Some(q) = quote {
            out.push(q);
        }
        match css::color(value.trim()) {
            Some(c) if c.a() > 0 && readable(c, back) != c => {
                let [r, g, b, a] = readable(c, back).to_srgba_unmultiplied();
                out.push_str(&format!("rgba({r},{g},{b},{:.3})", f32::from(a) / 255.0));
            }
            _ => out.push_str(value),
        }
        rest = &body[end..];
    }
}

/// What the DOM shim writes for the paint of a TIDAL sprite icon.
const PAINT: &str = "__luna-paint__";

/// `fill="__luna-paint__"` / `stroke="__luna-paint__"` as `paint`, its
/// alpha as the matching opacity (resvg reads no alpha in a colour).
fn sprite_paint(markup: &str, paint: Color32) -> String {
    let [r, g, b, a] = paint.to_srgba_unmultiplied();
    let mut out = markup.to_string();
    for attr in ["fill", "stroke"] {
        out = out.replace(
            &format!("{attr}=\"{PAINT}\""),
            &format!(
                "{attr}=\"#{r:02x}{g:02x}{b:02x}\" {attr}-opacity=\"{:.3}\"",
                f32::from(a) / 255.0
            ),
        );
    }
    out
}

/// An image from SVG markup, a data: URL or an http(s) URL.
fn source_of(markup: &str, color: Color32) -> Option<ImageSource> {
    let markup = markup.trim();
    if markup.starts_with('<') {
        let [r, g, b, a] = color.to_srgba_unmultiplied();
        // `currentColor` keeps the colour's alpha (AltPlay's icon in its
        // 55 % white link).
        let current = if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("rgba({r},{g},{b},{:.3})", f32::from(a) / 255.0)
        };
        let svg = markup.replace("currentColor", &current);
        // `fill="var(--star-off, ...)"` (star-ratings): the SVG renderer
        // knows no CSS variables.
        let svg = if svg.contains("var(") {
            css::resolve_vars(&svg, &plugin_dom::vars())
        } else {
            svg
        };
        let svg = svg_local_only(&svg);
        let svg = if svg.contains("xmlns=") {
            svg
        } else {
            svg.replacen("<svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"", 1)
        };
        let uri = format!("bytes://luna-svg-{:016x}.svg", hash(&svg));
        return Some(ImageSource::Bytes(uri, svg.into_bytes()));
    }
    if let Some(data) = markup.strip_prefix("data:") {
        let (meta, payload) = data.split_once(',')?;
        let bytes = if meta.ends_with(";base64") {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(payload.trim())
                .ok()?
        } else {
            percent_decode(payload).into_bytes()
        };
        let ext = if meta.contains("svg") {
            "svg"
        } else if meta.contains("png") {
            "png"
        } else if meta.contains("webp") {
            "webp"
        } else if meta.contains("gif") {
            "gif"
        } else {
            "jpg"
        };
        let uri = format!("bytes://luna-img-{:016x}.{ext}", hash(payload));
        return Some(ImageSource::Bytes(uri, bytes));
    }
    if css::safe_uri(markup) && !markup.to_ascii_lowercase().starts_with("data:") {
        return Some(ImageSource::Uri(markup.to_string()));
    }
    None
}

/// SVG markup without references the renderer would load from disk or the
/// network (`<image href="file:///...">`, `\\host\share`): only
/// in-document (`#id`) and `data:` references stay.
fn svg_local_only(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find("href=") {
        let (head, tail) = rest.split_at(at + 5);
        out.push_str(head);
        let Some(q) = tail.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            rest = tail;
            continue;
        };
        let body = &tail[1..];
        let end = body.find(q).unwrap_or(body.len());
        let value = &body[..end];
        let keep = value.starts_with('#') || value.trim().to_ascii_lowercase().starts_with("data:");
        out.push(q);
        out.push_str(if keep { value } else { "#" });
        rest = &body[end..];
    }
    out.push_str(rest);
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(h) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(h);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn input(
    ui: &mut egui::Ui,
    node: &VNode,
    inherit: Inherit,
    width: Option<f32>,
    height: Option<f32>,
    cx: &Cx,
) -> bool {
    let kind = attr(node, "type").unwrap_or("text").to_ascii_lowercase();
    match kind.as_str() {
        "range" => {
            let num = |name: &str, d: f64| {
                attr(node, name)
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(d)
            };
            let (min, max, step) = (num("min", 0.0), num("max", 100.0), num("step", 1.0));
            let id = egui::Id::new(("luna-range", node.id));
            let shown = node
                .value
                .as_deref()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(min);
            let mut value = ui.data(|d| d.get_temp::<f64>(id)).unwrap_or(shown);
            let response = if node.track.is_some() || node.thumb.is_some() {
                let size = vec2(width.unwrap_or(129.0), height.unwrap_or(16.0));
                styled_range(
                    ui,
                    node,
                    size,
                    cx,
                    inherit.back,
                    [min, max, step],
                    &mut value,
                )
            } else {
                ui.spacing_mut().slider_width = width.unwrap_or(86.0);
                ui.add(
                    egui::Slider::new(&mut value, min..=max)
                        .step_by(step.max(1e-6))
                        .show_value(false),
                )
            };
            if response.changed() {
                ui.data_mut(|d| d.insert_temp(id, value));
                plugin_dom::send(
                    DomTarget::Node(node.id),
                    "input",
                    json!({ "value": value.to_string() }),
                );
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                plugin_dom::send(
                    DomTarget::Node(node.id),
                    "change",
                    json!({ "value": value.to_string() }),
                );
            }
            if !response.dragged() {
                ui.data_mut(|d| d.remove::<f64>(id));
            }
            events(ui, node, &response);
            true
        }
        "checkbox" | "radio" => {
            let shown = node
                .value
                .as_deref()
                .is_some_and(|v| v == "true" || v == "checked" || v == "on");
            // The box shows the click at once and holds it until the
            // plugins' document reports it (or a second goes by).
            let id = egui::Id::new(("luna-check", node.id));
            let now = ui.input(|i| i.time);
            let pending: Option<(bool, f64)> = ui.data(|d| d.get_temp(id));
            let mut checked = match pending {
                Some((want, at)) if want != shown && now - at < 1.0 => want,
                Some(_) => {
                    ui.data_mut(|d| d.remove::<(bool, f64)>(id));
                    shown
                }
                None => shown,
            };
            let label = attr(node, "aria-label").unwrap_or("");
            // Chromium's form controls on a dark page: a grey box with a
            // light border (egui's own widget colours vanish on a dark card).
            // A checkbox is a square with slightly rounded corners, a radio
            // button a circle.
            let corner = CornerRadius::same(if kind == "radio" { 255 } else { 2 });
            let response = ui
                .scope(|ui| {
                    let w = &mut ui.visuals_mut().widgets;
                    for (s, border) in [
                        (&mut w.inactive, 0x85),
                        (&mut w.hovered, 0xac),
                        (&mut w.active, 0xc4),
                    ] {
                        s.bg_fill = Color32::from_rgb(0x3b, 0x3b, 0x3b);
                        s.bg_stroke = Stroke::new(1.0, Color32::from_gray(border));
                        s.fg_stroke.color = Color32::WHITE;
                        s.corner_radius = corner;
                    }
                    ui.checkbox(&mut checked, label)
                })
                .inner;
            if response.changed() {
                // The document toggles it on the click, as a browser does,
                // and reports `input` and `change` to the plugin.
                ui.data_mut(|d| d.insert_temp(id, (checked, now)));
                plugin_dom::send(DomTarget::Node(node.id), "click", json!({}));
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(1100));
            }
            true
        }
        "hidden" => false,
        _ => text_input(ui, node, inherit, width, false),
    }
}

/// A range input whose track or thumb a plugin styled
/// (`::-webkit-slider-runnable-track`, `::-webkit-slider-thumb`), drawn as
/// Chromium draws it: the track across the input's width, centred in its
/// height, painted with its background (a hard-stop gradient is the filled
/// part); the thumb travels inside the track, its top at the track's top
/// moved by its `margin-top`. A part no rule styles keeps Chromium's look (a
/// 4-point track, a 16-point round thumb).
fn styled_range(
    ui: &mut egui::Ui,
    node: &VNode,
    size: Vec2,
    cx: &Cx,
    back: Color32,
    [min, max, step]: [f64; 3],
    value: &mut f64,
) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let font = 14.0;
    let empty = Style::default();
    let track = node.track.as_ref().unwrap_or(&empty);
    let thumb = node.thumb.as_ref();
    let track_h = cx.len(&track.height, font, rect.height()).unwrap_or(4.0);
    let (thumb_w, thumb_h) = match thumb {
        Some(t) => (
            cx.len(&t.width, font, rect.width()).unwrap_or(16.0),
            cx.len(&t.height, font, rect.height()).unwrap_or(16.0),
        ),
        None => (16.0, 16.0),
    };
    let track_rect = Rect::from_center_size(rect.center(), vec2(rect.width(), track_h));
    let span = (max - min).max(1e-9);
    if let Some(pos) = response.interact_pointer_pos()
        && (response.is_pointer_button_down_on() || response.clicked())
    {
        let room = (rect.width() - thumb_w).max(1.0);
        let t = ((pos.x - rect.left() - thumb_w / 2.0) / room).clamp(0.0, 1.0) as f64;
        let step = step.max(1e-6);
        let next = (min + ((t * span) / step).round() * step).clamp(min, max);
        if (next - *value).abs() > f64::EPSILON {
            *value = next;
            response.mark_changed();
        }
    }
    let corners = |s: &Style, w: f32, h: f32| {
        CornerRadius::same(
            cx.len(&s.border_radius, font, w.min(h))
                .unwrap_or(0.0)
                .clamp(0.0, 255.0) as u8,
        )
    };
    let painter = ui.painter();
    let fill = |s: &Style| -> (Option<super::theme_gradient::Angled>, Option<Color32>) {
        let image = s
            .background_image
            .as_deref()
            .map(|v| css::resolve_vars(v, cx.vars));
        // Colours that would not read over the bar (HorizontalVolumeSlider's
        // white in Light) take dark ink, as plugin text does.
        let gradient = image
            .as_deref()
            .and_then(super::theme_gradient::parse)
            .map(|mut g| {
                for stop in &mut g.stops {
                    stop.1 = readable(stop.1, back);
                }
                g
            });
        let flat = cx
            .color(&s.background_color)
            .or_else(|| {
                image
                    .as_deref()
                    .and_then(|v| css::colors_in(v).first().copied())
            })
            .map(|c| readable(c, back));
        (gradient, flat)
    };
    match fill(track) {
        (Some(g), _) => {
            let c = corners(track, track_rect.width(), track_rect.height());
            super::theme_gradient::paint(painter, track_rect, Some(c), &g, g.angle, [0.0, 0.0]);
        }
        (None, Some(flat)) => {
            painter.rect_filled(
                track_rect,
                corners(track, track_rect.width(), track_rect.height()),
                flat,
            );
        }
        // Chromium's own track: grey, the part up to the thumb in its accent.
        (None, None) if node.track.is_none() => {
            painter.rect_filled(track_rect, CornerRadius::same(2), Color32::from_gray(0x80));
        }
        (None, None) => {}
    }
    let t = ((*value - min) / span).clamp(0.0, 1.0) as f32;
    let left = rect.left() + (rect.width() - thumb_w) * t;
    let top = match thumb {
        // An unstyled thumb is centred on the track; a styled one starts at
        // the track's top, moved by its margin.
        Some(s) => track_rect.top() + cx.len(&s.margin_top, font, rect.width()).unwrap_or(0.0),
        None => rect.center().y - thumb_h / 2.0,
    };
    let thumb_rect = Rect::from_min_size(Pos2::new(left, top), vec2(thumb_w, thumb_h));
    match thumb {
        Some(s) => {
            if let (_, Some(c)) = fill(s) {
                painter.rect_filled(thumb_rect, corners(s, thumb_w, thumb_h), c);
            }
        }
        None => {
            painter.circle_filled(thumb_rect.center(), thumb_w / 2.0, Color32::WHITE);
        }
    }
    response
}

/// A `border` value that draws no border (`none`, `0`, `0px solid`,
/// `hidden`).
fn borderless(value: &str) -> bool {
    let words = css::words(value.trim());
    words.iter().any(|w| matches!(*w, "none" | "hidden"))
        || words
            .first()
            .and_then(|w| w.trim_end_matches("px").parse::<f32>().ok())
            .is_some_and(|w| w <= 0.0)
}

fn text_input(
    ui: &mut egui::Ui,
    node: &VNode,
    inherit: Inherit,
    width: Option<f32>,
    multiline: bool,
) -> bool {
    let id = egui::Id::new(("luna-text", node.id));
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| node.value.clone().unwrap_or_default());
    let mut edit = if multiline {
        egui::TextEdit::multiline(&mut text)
    } else {
        egui::TextEdit::singleline(&mut text)
    };
    if let Some(p) = attr(node, "placeholder") {
        edit = edit.hint_text(p);
    }
    if let Some(max) = attr(node, "maxlength").and_then(|m| m.parse::<usize>().ok()) {
        edit = edit.char_limit(max);
    }
    // The field's text in the font its CSS gives it, as other plugin text.
    edit = edit.font(font_of(ui.ctx(), inherit, &node.style));
    // What the plugin's CSS says about the field itself: a field with
    // `border: none` has no box of its own (HorizontalVolumeSlider's
    // volume number sits bare beside its slider, the element's own
    // background drawn behind it), Chromium's `padding: 1px 2px` inside.
    let st = &node.style;
    if st.border.as_deref().is_some_and(borderless) {
        let fill = st
            .background_color
            .as_deref()
            .and_then(css::color)
            .unwrap_or(Color32::TRANSPARENT);
        edit = edit.frame(
            Frame::new()
                .fill(fill)
                .inner_margin(Margin::symmetric(2, 1)),
        );
    } else if st.background_color.is_some() || st.border.is_some() {
        // The element's own box (drawn round it) is the field's
        // background and border: DownloadManager's white search box with
        // its grey border, which egui's own dark field covered.
        edit = edit.frame(Frame::new().inner_margin(Margin::symmetric(2, 1)));
    }
    // Its colour, made to read over what is behind it (white in Light).
    if st.color.is_some() {
        edit = edit.text_color(inherit.color);
    }
    match st.text_align.as_deref().map(str::trim) {
        Some("center") => edit = edit.horizontal_align(Align::Center),
        Some("right" | "end") => edit = edit.horizontal_align(Align::Max),
        _ => {}
    }
    let response = ui.add(edit.desired_width(width.unwrap_or(120.0)));
    if response.changed() {
        plugin_dom::send(DomTarget::Node(node.id), "input", json!({ "value": text }));
    }
    if response.lost_focus() {
        plugin_dom::send(DomTarget::Node(node.id), "change", json!({ "value": text }));
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            plugin_dom::send(
                DomTarget::Node(node.id),
                "keydown",
                json!({ "key": "Enter", "code": "Enter" }),
            );
        }
    }
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    true
}

fn select(ui: &mut egui::Ui, node: &VNode, width: Option<f32>) -> bool {
    let options: Vec<(String, String)> = node
        .children
        .iter()
        .filter(|c| c.tag == "option")
        .map(|c| {
            let label = c
                .text
                .clone()
                .or_else(|| c.children.iter().find_map(|t| t.text.clone()))
                .unwrap_or_default();
            let value = attr(c, "value")
                .map(str::to_string)
                .unwrap_or_else(|| label.clone());
            (value, label)
        })
        .collect();
    let current = node.value.clone().unwrap_or_default();
    let shown = options
        .iter()
        .find(|(v, _)| *v == current)
        .map(|(_, l)| l.clone())
        .unwrap_or(current.clone());
    egui::ComboBox::from_id_salt(("luna-select", node.id))
        .width(width.unwrap_or(120.0))
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for (value, label) in &options {
                if ui.selectable_label(*value == current, label).clicked() && *value != current {
                    let data = json!({ "value": value });
                    plugin_dom::send(DomTarget::Node(node.id), "input", data.clone());
                    plugin_dom::send(DomTarget::Node(node.id), "change", data);
                }
            }
        });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::luna::dom::{DomOverlay, Injected};

    fn button(id: u64, text: &str) -> VNode {
        VNode {
            id,
            tag: "button".into(),
            children: vec![VNode {
                id: id + 100,
                tag: "#text".into(),
                text: Some(text.into()),
                ..VNode::default()
            }],
            listens: vec!["click".into()],
            ..VNode::default()
        }
    }

    /// Draw a slot in a headless pass and report the used rect.
    fn layout(overlay: DomOverlay, key: &str) -> Vec2 {
        let ctx = egui::Context::default();
        plugin_dom::test_overlay(Some(overlay));
        let mut size = Vec2::ZERO;
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let before = ui.cursor().min;
                inline(ui, key, Position::Append);
                size = ui.min_rect().max - before;
            });
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        size
    }

    /// OneClickLyrics' button among the player bar's icons: TIDAL sizes
    /// boxes `border-box`, so `width: 32px` with `padding: 1px 6px` is 32
    /// wide (it was 44), and a run taller than egui's row is centred on the
    /// row (its 16-point icon hung 8 points below the middle of a 60-point
    /// row, 3 below the player bar's icons).
    #[test]
    fn a_tall_slot_element_is_border_box_and_centred_in_its_row() {
        let svg = VNode {
            id: 3,
            tag: "svg".into(),
            markup: Some(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="32px" height="32px" viewBox="0 0 24 24"><rect width="24" height="24" fill="#fff"/></svg>"##
                    .into(),
            ),
            style: Style {
                width: Some("16px".into()),
                height: Some("16px".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let div = VNode {
            id: 2,
            tag: "div".into(),
            children: vec![svg],
            ..VNode::default()
        };
        let node = VNode {
            id: 1,
            tag: "button".into(),
            children: vec![div],
            style: Style {
                display: Some("flex".into()),
                width: Some("32px".into()),
                height: Some("32px".into()),
                padding: Some("1px 6px".into()),
                background_color: Some("#f00".into()),
                align_items: Some("center".into()),
                justify_content: Some("center".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let overlay = DomOverlay {
            injected: vec![Injected {
                anchor: "k".into(),
                position: Position::Append,
                nodes: vec![node],
            }],
            version: 1,
            ..DomOverlay::default()
        };
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        plugin_dom::test_overlay(Some(overlay));
        let mut rects = Vec::new();
        for _ in 0..4 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.allocate_ui_with_layout(
                    vec2(300.0, 60.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        ui.add_sized(vec2(10.0, 60.0), egui::Label::new("x"));
                        inline(ui, "k", Position::Append)
                    },
                );
            });
            rects = out
                .shapes
                .iter()
                .map(|s| s.shape.visual_bounding_rect())
                .filter(|r| r.is_positive() && r.width() > 12.0)
                .collect();
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        // The button's fill, then its icon.
        let [button, icon] = rects[..] else {
            panic!("a button and its icon: {rects:?}")
        };
        assert!((button.width() - 32.0).abs() < 0.5, "button {button:?}");
        assert!((button.height() - 32.0).abs() < 0.5, "button {button:?}");
        assert!((button.center().y - 30.0).abs() < 0.5, "button {button:?}");
        assert!((icon.center().y - 30.0).abs() < 0.5, "icon {icon:?}");
    }

    /// lyrics-everywhere's bubble has `box-shadow: 0 4px 12px rgba(0, 0, 0,
    /// .3)`: a soft shadow is drawn around and below its box. Plugin boxes
    /// had none.
    #[test]
    fn a_box_shadow_is_drawn_round_the_box() {
        let node = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                width: Some("100px".into()),
                height: Some("40px".into()),
                background_color: Some("#000".into()),
                box_shadow: Some("0 4px 12px rgba(0, 0, 0, 0.3)".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let overlay = DomOverlay {
            injected: vec![Injected {
                anchor: "k".into(),
                position: Position::Append,
                nodes: vec![node],
            }],
            version: 1,
            ..DomOverlay::default()
        };
        let ctx = egui::Context::default();
        plugin_dom::test_overlay(Some(overlay));
        let mut rects = Vec::new();
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.add_space(40.0);
                ui.horizontal(|ui| {
                    ui.add_space(40.0);
                    inline(ui, "k", Position::Append);
                });
            });
            rects = out
                .shapes
                .iter()
                .map(|s| s.shape.visual_bounding_rect())
                .filter(|r| r.is_positive())
                .collect();
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        // The box is 100 by 40 at (40, 40); its shadow reaches past it.
        assert!(
            rects
                .iter()
                .any(|r| r.width() > 110.0 && r.bottom() > 40.0 + 40.0 + 4.0),
            "a shadow beyond the box: {rects:?}"
        );
    }

    /// miniplayer's button sets `margin-left: 8px; margin-top: 4px` as
    /// longhands: the box starts 8 points in and 4 down. Longhands were
    /// left out (the button sat at the sidebar's edge).
    #[test]
    fn margin_longhands_move_the_box() {
        let node = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                width: Some("50px".into()),
                height: Some("20px".into()),
                background_color: Some("#f00".into()),
                margin_left: Some("8px".into()),
                margin_top: Some("4px".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let overlay = DomOverlay {
            injected: vec![Injected {
                anchor: "k".into(),
                position: Position::Append,
                nodes: vec![node],
            }],
            version: 1,
            ..DomOverlay::default()
        };
        let ctx = egui::Context::default();
        plugin_dom::test_overlay(Some(overlay));
        let mut fill = None;
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.vertical(|ui| {
                    inline(ui, "k", Position::Append);
                });
            });
            fill = out
                .shapes
                .iter()
                .map(|s| s.shape.visual_bounding_rect())
                .find(|r| (r.width() - 50.0).abs() < 0.5);
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        let fill = fill.expect("the box");
        assert!((fill.left() - 8.0).abs() < 0.5, "{fill:?}");
    }

    /// radiant-lyrics puts a `position: fixed; z-index: -3` box (its WebGL
    /// background) inside `main`: it is drawn behind the page, not in the slot.
    #[test]
    fn fixed_boxes_behind_the_page_leave_their_slot() {
        let fixed = VNode {
            id: 7,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                z_index: Some("-3".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let mut front = fixed.clone();
        front.id = 8;
        front.style.z_index = Some("2".into());
        let overlay = DomOverlay {
            injected: vec![Injected {
                anchor: "main".into(),
                position: Position::Append,
                nodes: vec![fixed.clone(), front.clone()],
            }],
            ..DomOverlay::default()
        };
        let ids: Vec<u64> = behind(&overlay).map(|n| n.id).collect();
        assert_eq!(ids, vec![7]);
        assert!(fixed_behind(&fixed));
        assert!(!fixed_behind(&front));
    }

    /// star-ratings colours its stars with `fill="var(--star-off, ...)"`.
    /// Total Play Time's pill: white text on a fill that is the app's
    /// surface at 30 %, near white in Light. It stays white where the
    /// window is dark and reads dark where it is light.
    #[test]
    fn plugin_text_stays_readable_on_a_light_backdrop() {
        let white = Color32::WHITE;
        let light = Color32::from_rgb(0xf4, 0xf4, 0xf6);
        let dark = Color32::from_rgb(0x12, 0x12, 0x12);
        assert_eq!(readable(white, dark), white, "TIDAL's own look is kept");
        assert_eq!(
            readable(white, light),
            Color32::from_rgb(0x1a, 0x1a, 0x1a),
            "white on near white"
        );
        assert_eq!(
            readable(Color32::from_rgb(0x20, 0x20, 0x20), dark),
            Color32::WHITE,
            "dark on dark"
        );
        let dim = Color32::from_white_alpha(97);
        assert_eq!(readable(dim, dark), dim, "a dimmed ink that still reads");
        let faint = readable(Color32::from_white_alpha(97), light);
        assert_eq!(faint.a(), 97, "its opacity is kept");
        // A translucent fill over a light panel is a light backdrop.
        let pill = over(Color32::from_rgba_unmultiplied(0xf4, 0xf4, 0xf6, 77), light);
        assert!(luminance(pill) > 0.8);
    }

    #[test]
    fn a_percent_radius_is_a_share_of_the_shorter_side() {
        assert_eq!(radius_share("50%", 44.0, 40.0), Some(20.0));
        assert_eq!(radius_share("8px", 44.0, 40.0), None);
    }

    #[test]
    fn svg_markup_gets_css_variables_resolved() {
        let mut overlay = DomOverlay::default();
        overlay.vars.insert("--star-on".into(), "#ffc531".into());
        plugin_dom::test_overlay(Some(overlay));
        let svg = r#"<svg><path fill="var(--star-on, red)"/><path fill="var(--star-off, rgba(255,255,255,0.28))"/></svg>"#;
        let Some(ImageSource::Bytes(_, bytes)) = source_of(svg, Color32::WHITE) else {
            panic!("no svg source");
        };
        plugin_dom::test_overlay(None);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains(r##"fill="#ffc531""##), "{text}");
        assert!(text.contains(r#"fill="rgba(255,255,255,0.28)""#), "{text}");
    }

    /// SVG markup from plugins never makes the renderer read a file or a
    /// network share (`<image href>`); in-document and data: references stay.
    #[test]
    fn svg_images_keep_only_local_references() {
        let svg = r##"<svg><image href="file:////host/share/x.png"/><image xlink:href='C:\x.png'/><use href="#a"/><image href="data:image/png;base64,AA"/></svg>"##;
        let out = svg_local_only(svg);
        assert!(!out.contains("host") && !out.contains("C:"), "{out}");
        assert!(
            out.contains(r##"href="#a""##) && out.contains("data:image/png"),
            "{out}"
        );
        assert!(source_of("file:///C:/x.png", Color32::WHITE).is_none());
        assert!(source_of("//host/share/x.png", Color32::WHITE).is_none());
    }

    #[test]
    fn blur_filters_give_their_radius() {
        assert_eq!(blur_radius("blur(10px)").as_deref(), Some("10px"));
        assert_eq!(
            blur_radius("brightness(0.5) blur( 2em )").as_deref(),
            Some("2em")
        );
        assert_eq!(blur_radius("saturate(2)"), None);
        assert_eq!(brightness("contrast(125%) brightness(0.652)"), Some(0.652));
        assert_eq!(brightness("brightness(50%)"), Some(0.5));
        assert_eq!(brightness("blur(2px)"), None);
    }

    #[test]
    fn slots_lay_out_injected_buttons_in_a_row() {
        let one = layout(
            DomOverlay {
                version: 1,
                injected: vec![Injected {
                    anchor: "utility-buttons".into(),
                    position: Position::Append,
                    nodes: vec![button(1, "One")],
                }],
                ..DomOverlay::default()
            },
            "utility-buttons",
        );
        let two = layout(
            DomOverlay {
                version: 1,
                injected: vec![Injected {
                    anchor: "utility-buttons".into(),
                    position: Position::Append,
                    nodes: vec![button(1, "One"), button(2, "Two")],
                }],
                ..DomOverlay::default()
            },
            "utility-buttons",
        );
        assert!(one.x > 10.0, "{one:?}");
        assert!(two.x > one.x + 10.0, "a row: {one:?} -> {two:?}");
        assert!((two.y - one.y).abs() < 1.0, "same height in a row");
    }

    /// betterFullscreen's icon button in the player bar's right-to-left
    /// cluster stays icon-sized.
    #[test]
    fn icon_buttons_in_a_right_to_left_row_stay_small() {
        let icon = VNode {
            id: 2,
            tag: "button".into(),
            listens: vec!["click".into()],
            children: vec![VNode {
                id: 3,
                tag: "svg".into(),
                markup: Some(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M7 14H5v5h5v-2H7v-3z"/></svg>"#
                        .into(),
                ),
                ..VNode::default()
            }],
            ..VNode::default()
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "more-container".into(),
                position: Position::Append,
                nodes: vec![icon],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        let mut width = 0.0;
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let before = ui.cursor().max.x;
                    inline(ui, "more-container", Position::Append);
                    width = before - ui.cursor().max.x;
                });
            });
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        assert!(width > 10.0 && width < 60.0, "{width}");
    }

    /// Total Play Time's pill ("00:40:30") in a crowded player bar: a word
    /// with no place to break stays on one line, as in a browser, instead of
    /// breaking after "00:40:".
    #[test]
    fn a_word_with_no_break_stays_on_one_line_in_a_narrow_slot() {
        let pill = VNode {
            id: 2,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                padding: Some("3px 6px".into()),
                font_size: Some("12px".into()),
                background_color: Some("#333333".into()),
                ..Style::default()
            },
            children: vec![VNode {
                id: 3,
                tag: "div".into(),
                style: Style {
                    font_size: Some("12px".into()),
                    ..Style::default()
                },
                children: vec![VNode {
                    id: 4,
                    tag: "#text".into(),
                    text: Some("00:40:30".into()),
                    ..VNode::default()
                }],
                ..VNode::default()
            }],
            ..VNode::default()
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "more-container".into(),
                position: Position::Prepend,
                nodes: vec![pill],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        let mut height = 0.0;
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_max_width(30.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let top = ui.min_rect().top();
                    inline(ui, "more-container", Position::Prepend);
                    height = ui.min_rect().bottom() - top;
                });
            });
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        assert!(height < 26.0, "one line: {height}");
    }

    /// betterFullscreen's player (`position: absolute; inset: 0; z-index:
    /// 10` in the now-playing view) gets a layer over the view's own
    /// content, which is painted after the view's slots; `z-index: 0` stays
    /// in the view's layer.
    #[test]
    fn positioned_elements_with_a_positive_z_index_draw_above_the_view() {
        let player = |id: u64, z: &str| VNode {
            id,
            tag: "div".into(),
            style: Style {
                position: Some("absolute".into()),
                top: Some("0".into()),
                right: Some("0".into()),
                bottom: Some("0".into()),
                left: Some("0".into()),
                z_index: Some(z.into()),
                ..Style::default()
            },
            children: vec![VNode {
                id: id + 100,
                tag: "#text".into(),
                text: Some("Lyrics here".into()),
                ..VNode::default()
            }],
            ..VNode::default()
        };
        let key = crate::luna::dom_anchors::keys::NOW_PLAYING;
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: key.into(),
                position: Position::Append,
                nodes: vec![player(9, "10"), player(11, "0")],
            }],
            ..DomOverlay::default()
        }));
        plugin_dom::test_sent(crate::luna::dom_anchors::sample_tree());
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                at(
                    ui,
                    key,
                    Position::Append,
                    ui.max_rect().center(),
                    Align2::CENTER_TOP,
                );
            });
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        let layer =
            |id: u64| egui::LayerId::new(egui::Order::Middle, egui::Id::new(("luna-above", id)));
        assert!(ctx.memory(|m| m.areas().is_visible(&layer(9))));
        assert!(!ctx.memory(|m| m.areas().is_visible(&layer(11))));
    }

    /// betterFullscreen's background layer (`position: absolute; z-index:
    /// -1`) is painted under its siblings in the flow (the art and title),
    /// not over them.
    #[test]
    fn negative_z_children_paint_under_the_flow() {
        let layer = VNode {
            id: 2,
            tag: "div".into(),
            style: Style {
                position: Some("absolute".into()),
                top: Some("0".into()),
                right: Some("0".into()),
                bottom: Some("0".into()),
                left: Some("0".into()),
                z_index: Some("-1".into()),
                background_color: Some("rgb(1, 2, 3)".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let title = VNode {
            id: 3,
            tag: "#text".into(),
            text: Some("Rosewood".into()),
            ..VNode::default()
        };
        let player = VNode {
            id: 1,
            tag: "div".into(),
            children: vec![layer, title],
            ..VNode::default()
        };
        let overlay = DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "x".into(),
                position: Position::Append,
                nodes: vec![player],
            }],
            ..DomOverlay::default()
        };
        plugin_dom::test_overlay(Some(overlay));
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                inline(ui, "x", Position::Append);
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let fill = shapes.iter().position(
            |s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == Color32::from_rgb(1, 2, 3)),
        );
        let text = shapes
            .iter()
            .position(|s| matches!(&s.shape, egui::Shape::Text(_)));
        assert!(fill.is_some() && text.is_some(), "{fill:?} {text:?}");
        assert!(fill < text, "the layer is under the text");
    }

    /// radiant-lyrics' backdrop: a fixed `#000` box with `z-index: -3` (a
    /// stacking context) holding its canvas at `z-index: -1`. The canvas is
    /// painted over the box's black, as in a browser; under a box that
    /// forms no stacking context the child stays under its background.
    #[test]
    fn negative_z_children_paint_over_their_stacking_contexts_background() {
        let layer = |id| VNode {
            id,
            tag: "div".into(),
            style: Style {
                position: Some("absolute".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                width: Some("100%".into()),
                height: Some("100%".into()),
                z_index: Some("-1".into()),
                background_color: Some("rgb(1, 2, 3)".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let container = |context: bool| VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                width: Some("100px".into()),
                height: Some("100px".into()),
                background_color: Some("#000".into()),
                transform: context.then(|| "translateZ(0)".into()),
                ..Style::default()
            },
            children: vec![layer(2)],
            ..VNode::default()
        };
        let order = |context: bool| {
            let overlay = DomOverlay {
                version: 1,
                injected: vec![Injected {
                    anchor: "x".into(),
                    position: Position::Append,
                    nodes: vec![container(context)],
                }],
                ..DomOverlay::default()
            };
            plugin_dom::test_overlay(Some(overlay));
            let ctx = egui::Context::default();
            let mut shapes = Vec::new();
            for _ in 0..3 {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    inline(ui, "x", Position::Append);
                });
                out.textures_delta.clear();
                shapes = out.shapes;
            }
            plugin_dom::test_overlay(None);
            let find = |c: Color32| {
                shapes
                    .iter()
                    .position(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == c))
            };
            (find(Color32::BLACK), find(Color32::from_rgb(1, 2, 3)))
        };
        let (black, canvas) = order(true);
        assert!(black.is_some() && canvas.is_some(), "{black:?} {canvas:?}");
        assert!(black < canvas, "the canvas is over its container's black");
        let (black, canvas) = order(false);
        assert!(black.is_some() && canvas.is_some(), "{black:?} {canvas:?}");
        assert!(canvas < black, "no stacking context: under the background");
        // As radiant-lyrics places it: a fixed box in `main`, drawn by
        // [`backdrop`] under the panels.
        let fixed = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                left: Some("0".into()),
                top: Some("0".into()),
                width: Some("100px".into()),
                height: Some("100px".into()),
                z_index: Some("-3".into()),
                background_color: Some("#000".into()),
                transform: Some("translateZ(0)".into()),
                ..Style::default()
            },
            children: vec![layer(2)],
            ..VNode::default()
        };
        let overlay = DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "main".into(),
                position: Position::Append,
                nodes: vec![fixed],
            }],
            ..DomOverlay::default()
        };
        plugin_dom::test_overlay(Some(overlay));
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| backdrop(ui));
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let find = |c: Color32| {
            shapes
                .iter()
                .position(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == c))
        };
        let (black, canvas) = (find(Color32::BLACK), find(Color32::from_rgb(1, 2, 3)));
        assert!(black.is_some() && canvas.is_some(), "{black:?} {canvas:?}");
        assert!(black < canvas, "the backdrop's canvas is over its black");
        let mut style = Style {
            position: Some("fixed".into()),
            z_index: Some("-3".into()),
            ..Style::default()
        };
        assert!(stacking_context(&style));
        style.z_index = Some("auto".into());
        assert!(!stacking_context(&style));
        style.opacity = Some("0.75".into());
        assert!(stacking_context(&style));
    }

    /// Draws the slot `x` of `overlay` at each of `times` (seconds) and
    /// returns each pass's shapes.
    fn frames_at(
        ctx: &egui::Context,
        overlay: &DomOverlay,
        times: &[f64],
    ) -> Vec<Vec<egui::epaint::ClippedShape>> {
        plugin_dom::test_overlay(Some(overlay.clone()));
        let mut all = Vec::new();
        for t in times {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*t),
                    ..Default::default()
                },
                |ui| {
                    inline(ui, "x", Position::Append);
                },
            );
            out.textures_delta.clear();
            all.push(out.shapes);
        }
        plugin_dom::test_overlay(None);
        all
    }

    fn one_box(id: u64, style: Style) -> DomOverlay {
        DomOverlay {
            version: id,
            injected: vec![Injected {
                anchor: "x".into(),
                position: Position::Append,
                nodes: vec![VNode {
                    id,
                    tag: "div".into(),
                    style,
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }
    }

    /// A plugin box's `box-shadow` lies outside the box only, as a browser
    /// draws one: a see-through box (a 30 % white card over a white page)
    /// shows what is under it, not its own shadow. The box took the shadow
    /// as egui's frame shadow, which also lies under the box's inside: the
    /// card's middle was grey 94 instead of white (the bug round 40 fixed
    /// for the sidebar's card and the player bar).
    #[test]
    fn a_see_through_boxs_shadow_stays_outside_it() {
        let ctx = egui::Context::default();
        let style = Style {
            width: Some("120px".into()),
            height: Some("80px".into()),
            background_color: Some("rgba(255, 255, 255, 0.3)".into()),
            box_shadow: Some("0 0 20px 6px rgba(0, 0, 0, 0.9)".into()),
            ..Style::default()
        };
        let shapes = frames_at(&ctx, &one_box(42, style), &[1.0, 1.1])
            .pop()
            .expect("drawn");
        let card = shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == Color32::from_white_alpha(77) => Some(r.rect),
                _ => None,
            })
            .expect("the card's fill");
        let at = |p: egui::Pos2| plugin_dom::test_colour_at(&shapes, p, Color32::WHITE);
        assert_eq!(at(card.center()), Color32::WHITE, "inside the card");
        // The shadow is still drawn, round the card.
        let rings = shapes
            .iter()
            .filter(|s| {
                let mut found = false;
                fn walk(shape: &egui::Shape, card: Rect, found: &mut bool) {
                    match shape {
                        egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, card, found)),
                        egui::Shape::Rect(r) => {
                            *found |= r.stroke.width > 0.0
                                && r.stroke.color.a() > 0
                                && r.rect.contains_rect(card.expand(1.0));
                        }
                        _ => {}
                    }
                }
                walk(&s.shape, card, &mut found);
                found
            })
            .count();
        assert!(rings > 0, "no shadow round the card");
    }

    /// The alpha of the red box painted in `shapes`.
    fn red_alpha(shapes: &[egui::epaint::ClippedShape]) -> Option<u8> {
        shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill.r() > 0 && r.fill.g() == 0 && r.fill.b() == 0 => {
                Some(r.fill.a())
            }
            _ => None,
        })
    }

    /// `transition: opacity 1s linear` on a plugin element: when the style
    /// changes, the box fades over the second instead of jumping, and a
    /// change half way turns back from where it is.
    #[test]
    fn a_plugin_box_fades_by_its_transition() {
        let style = |opacity: &str| Style {
            width: Some("40px".into()),
            height: Some("40px".into()),
            background_color: Some("rgb(200, 0, 0)".into()),
            opacity: Some(opacity.into()),
            transition: Some("opacity 1 0 linear".into()),
            ..Style::default()
        };
        let ctx = egui::Context::default();
        let shown = frames_at(&ctx, &one_box(41, style("0")), &[10.0, 10.1]);
        assert_eq!(red_alpha(&shown[1]), None, "at opacity 0 nothing shows");
        let shown = frames_at(&ctx, &one_box(41, style("1")), &[10.2, 10.7, 11.3]);
        let half = red_alpha(&shown[1]).expect("half way") as f32;
        assert!((half - 127.0).abs() < 12.0, "{half}");
        assert_eq!(red_alpha(&shown[2]), Some(255));
        // Back to 0 a quarter of the way in: it turns back from a quarter.
        frames_at(&ctx, &one_box(41, style("0")), &[20.0]);
        let shown = frames_at(&ctx, &one_box(41, style("1")), &[21.0, 21.25]);
        let quarter = red_alpha(&shown[1]).unwrap() as f32;
        let shown = frames_at(&ctx, &one_box(41, style("0")), &[21.25, 21.375, 21.6]);
        let back = red_alpha(&shown[1]).unwrap() as f32;
        assert!(
            (quarter - 64.0).abs() < 10.0 && (back - 32.0).abs() < 10.0,
            "{quarter} {back}"
        );
        assert_eq!(red_alpha(&shown[2]), None, "a quarter second back");
    }

    /// The runtime's `from` (the style an element had before the change that
    /// brought it in, 3d-cover's canvas) starts its transition there.
    #[test]
    fn a_new_element_fades_from_its_first_style() {
        let mut overlay = one_box(
            42,
            Style {
                width: Some("40px".into()),
                height: Some("40px".into()),
                background_color: Some("rgb(200, 0, 0)".into()),
                opacity: Some("1".into()),
                transition: Some("opacity 1 0 linear".into()),
                ..Style::default()
            },
        );
        overlay.injected[0].nodes[0].from = Some(Style {
            opacity: Some("0".into()),
            ..Style::default()
        });
        let ctx = egui::Context::default();
        let shown = frames_at(&ctx, &overlay, &[5.0, 5.5]);
        assert_eq!(red_alpha(&shown[0]), None);
        let half = red_alpha(&shown[1]).unwrap() as f32;
        assert!((half - 127.0).abs() < 12.0, "{half}");
    }

    /// 3d-cover's spinner: a 50% round box whose top border is brighter,
    /// turned by `@keyframes { to { transform: rotate(360deg) } }`. Its
    /// bright arc goes round with the time.
    #[test]
    fn a_css_spinner_turns_its_bright_arc() {
        let bright = Color32::from_rgba_unmultiplied(255, 255, 255, 230);
        let dim = "rgba(255, 255, 255, 0.25)";
        let mut overlay = one_box(
            43,
            Style {
                width: Some("40px".into()),
                height: Some("40px".into()),
                border_radius: Some("50%".into()),
                border: Some(format!("4px solid {dim}")),
                border_color: Some(format!("rgba(255, 255, 255, 0.9) {dim} {dim} {dim}")),
                animation: Some("spin 2 0 infinite normal linear running none".into()),
                ..Style::default()
            },
        );
        overlay.keyframes.insert(
            "spin".into(),
            vec![(
                1.0,
                Style {
                    transform: Some("rotate(360deg)".into()),
                    ..Style::default()
                },
            )],
        );
        // Where the bright arc's middle is, in degrees (screen angles: 0 is
        // to the right, 90 down), about the ring's centre.
        let arc = |shapes: &[egui::epaint::ClippedShape]| -> f32 {
            let mut lines = Vec::new();
            for s in shapes {
                if let egui::Shape::Vec(v) = &s.shape {
                    for x in v {
                        if let egui::Shape::Path(p) = x {
                            lines.push(p.clone());
                        }
                    }
                }
            }
            let all: Vec<Pos2> = lines.iter().flat_map(|p| p.points.clone()).collect();
            assert!(!all.is_empty(), "the ring is drawn as arcs");
            let centre = all.iter().fold(Vec2::ZERO, |a, p| a + p.to_vec2()) / all.len() as f32;
            let lit = lines
                .iter()
                .find(|p| match &p.stroke.color {
                    egui::epaint::ColorMode::Solid(c) => {
                        c.a() > 200 && (c.a() as i32 - bright.a() as i32).abs() < 20
                    }
                    _ => false,
                })
                .expect("the bright arc");
            let mid = lit.points[lit.points.len() / 2];
            (mid.y - centre.y).atan2(mid.x - centre.x).to_degrees()
        };
        let ctx = egui::Context::default();
        let shown = frames_at(&ctx, &overlay, &[3.0, 3.0, 3.5, 4.0]);
        let (a, b, c) = (arc(&shown[1]), arc(&shown[2]), arc(&shown[3]));
        let turned = |x: f32, y: f32| (y - x).rem_euclid(360.0);
        assert!((a + 90.0).abs() < 2.0, "starts at the top: {a}");
        assert!(
            (turned(a, b) - 90.0).abs() < 2.0,
            "a quarter turn in half a second: {a} {b}"
        );
        assert!((turned(a, c) - 180.0).abs() < 2.0, "{a} {c}");
    }

    #[test]
    fn hidden_nodes_take_no_room() {
        let mut hidden = button(1, "Gone");
        hidden.style.display = Some("none".into());
        let size = layout(
            DomOverlay {
                version: 1,
                injected: vec![Injected {
                    anchor: "x".into(),
                    position: Position::Append,
                    nodes: vec![hidden],
                }],
                ..DomOverlay::default()
            },
            "x",
        );
        assert!(size.x < 1.0, "{size:?}");
    }

    /// DownloadManager's taskbar: a bar fixed to the window's bottom edge
    /// with a high `z-index`. Clicks anywhere above it reach the app; before,
    /// its layer stretched from the window's top-left corner to the bar, so
    /// every click in the window went to it and the app looked frozen.
    #[test]
    fn a_fixed_bar_at_the_bottom_leaves_the_window_clickable() {
        let bar = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                left: Some("0".into()),
                right: Some("0".into()),
                bottom: Some("0".into()),
                height: Some("40px".into()),
                display: Some("flex".into()),
                z_index: Some("99995".into()),
                background_color: Some("#101010".into()),
                ..Style::default()
            },
            children: vec![button(2, "Start")],
            ..VNode::default()
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![bar],
            }],
            ..DomOverlay::default()
        }));
        plugin_dom::test_viewport([1240.0, 800.0]);
        let ctx = app_fonts_ctx();
        let at = Pos2::new(60.0, 40.0);
        let input = |events: Vec<egui::Event>| egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1240.0, 800.0))),
            events,
            ..egui::RawInput::default()
        };
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let frames = [
            vec![],
            vec![],
            vec![egui::Event::PointerMoved(at)],
            vec![press(true)],
            vec![press(false)],
        ];
        let mut clicked = false;
        for events in frames {
            let mut out = ctx.run_ui(input(events), |ui| {
                let r = ui.put(
                    Rect::from_center_size(at, vec2(100.0, 30.0)),
                    egui::Button::new("Explore"),
                );
                clicked |= r.clicked();
                floating(ui);
            });
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        assert!(
            clicked,
            "the app's own button under no plugin element was clicked"
        );
    }

    /// The same for an element stacked over its anchor (`z-index` > 0) in
    /// the anchor's bottom-right corner: the rest of the anchor's box stays
    /// clickable.
    #[test]
    fn a_raised_element_in_a_corner_leaves_its_anchor_clickable() {
        let badge = VNode {
            id: 21,
            tag: "div".into(),
            style: Style {
                position: Some("absolute".into()),
                right: Some("0".into()),
                bottom: Some("0".into()),
                width: Some("40px".into()),
                height: Some("40px".into()),
                z_index: Some("10".into()),
                background_color: Some("#101010".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let key = crate::luna::dom_anchors::keys::NOW_PLAYING;
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: key.into(),
                position: Position::Append,
                nodes: vec![badge],
            }],
            ..DomOverlay::default()
        }));
        plugin_dom::test_sent(crate::luna::dom_anchors::sample_tree());
        let container = plugin_dom::host_rect(key).expect("the anchor has a box");
        assert!(container.width() > 120.0 && container.height() > 120.0);
        let at_ = container.min + vec2(40.0, 40.0);
        let ctx = egui::Context::default();
        let press = |pressed| egui::Event::PointerButton {
            pos: at_,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let frames = [
            vec![],
            vec![],
            vec![egui::Event::PointerMoved(at_)],
            vec![press(true)],
            vec![press(false)],
        ];
        let mut clicked = false;
        for events in frames {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
                events,
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let r = ui.put(
                    Rect::from_center_size(at_, vec2(60.0, 30.0)),
                    egui::Button::new("Lyrics"),
                );
                clicked |= r.clicked();
                at(ui, key, Position::Append, container.min, Align2::LEFT_TOP);
            });
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        assert!(
            clicked,
            "the anchor's own button beside the element was clicked"
        );
    }

    /// A button whose style sets no font takes the browser's control font,
    /// not the page's: qr-share's Save and Close inside its 16 px column
    /// are 13.33 px at normal weight before their own `font-size`, and a
    /// `font-size` of their own still wins.
    #[test]
    fn a_button_takes_the_browsers_control_font_not_its_parents() {
        let text = |id, t: &str| VNode {
            id,
            tag: "#text".into(),
            text: Some(t.into()),
            ..VNode::default()
        };
        let modal = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                font_size: Some("20px".into()),
                ..Style::default()
            },
            children: vec![
                VNode {
                    id: 2,
                    tag: "span".into(),
                    children: vec![text(12, "Plain")],
                    ..VNode::default()
                },
                VNode {
                    children: vec![text(13, "Save")],
                    ..button(3, "")
                },
                VNode {
                    style: Style {
                        font_size: Some("16px".into()),
                        ..Style::default()
                    },
                    children: vec![text(14, "Close")],
                    ..button(4, "")
                },
            ],
            ..VNode::default()
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![modal],
            }],
            ..DomOverlay::default()
        }));
        plugin_dom::test_viewport([800.0, 600.0]);
        let ctx = app_fonts_ctx();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                    ..egui::RawInput::default()
                },
                |ui| floating(ui),
            );
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let size = |label: &str| {
            shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == label => {
                        Some(t.galley.job.sections[0].format.font_id.size)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no {label}"))
        };
        assert_eq!(size("Plain"), 20.0);
        assert!((size("Save") - 40.0 / 3.0).abs() < 0.01, "{}", size("Save"));
        assert_eq!(size("Close"), 16.0);
    }

    /// A button wearing TIDAL's module classes (LangRomanizer copies the
    /// Lyrics pill's) is styled by TIDAL's stylesheet, not the browser's
    /// control font: it keeps the size it inherits.
    #[test]
    fn a_button_with_tidals_classes_keeps_the_pages_font() {
        let pill = VNode {
            attrs: vec![(
                "class".into(),
                "romanize-button _button_8b3a1f2 _tertiary_c0ffee1".into(),
            )],
            children: vec![text_node(5, "Romanize Lyrics")],
            ..button(4, "")
        };
        let holder = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                font_size: Some("20px".into()),
                ..Style::default()
            },
            children: vec![pill],
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![holder]);
        let size = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text() == "Romanize Lyrics" => {
                    Some(t.galley.job.sections[0].format.font_id.size)
                }
                _ => None,
            })
            .expect("the pill's words");
        assert_eq!(size, 20.0);
        assert!(!tidal_styled(&button(9, "Save")));
    }

    /// A button reset with `font: inherit` (koreansearch's result rows)
    /// keeps the size and family it inherits, not the control font.
    #[test]
    fn a_button_reset_with_font_inherit_keeps_its_parents_font() {
        let row = VNode {
            style: Style {
                font_family: Some("inherit".into()),
                font_size: Some("inherit".into()),
                font_weight: Some("inherit".into()),
                font_style: Some("inherit".into()),
                ..Style::default()
            },
            children: vec![text_node(5, "Result")],
            ..button(4, "")
        };
        let holder = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                font_size: Some("18px".into()),
                ..Style::default()
            },
            children: vec![row],
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![holder]);
        let font = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text() == "Result" => {
                    Some(t.galley.job.sections[0].format.font_id.clone())
                }
                _ => None,
            })
            .expect("the row's words");
        assert_eq!(font.size, 18.0);
        // The page's weight is TIDAL's body's, 500: the app's medium face.
        assert_eq!(font, crate::theme::medium(18.0));
    }

    /// A box in the flow that is `position: relative` with a `z-index`
    /// stacks over an absolutely placed sibling with a lower one (CSS 2.1
    /// Appendix E): a full-window player's backdrop at `z-index: 10` lies
    /// under its words at `z-index: 1000`, whichever comes first in the
    /// tree, and placed layers stack among themselves by `z-index`.
    #[test]
    fn a_placed_layer_under_a_relative_box_with_a_higher_z_index_paints_under_it() {
        let red = Color32::from_rgb(200, 0, 0);
        let blue = Color32::from_rgb(0, 0, 200);
        let green = Color32::from_rgb(0, 200, 0);
        let layer = |id: u64, z: Option<&str>, colour: &str| VNode {
            id,
            tag: "div".into(),
            style: Style {
                position: Some("absolute".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                right: Some("0".into()),
                bottom: Some("0".into()),
                z_index: z.map(str::to_string),
                background_color: Some(colour.into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let words = VNode {
            id: 3,
            tag: "div".into(),
            style: Style {
                position: Some("relative".into()),
                z_index: Some("1000".into()),
                width: Some("100px".into()),
                height: Some("50px".into()),
                background_color: Some("rgb(0, 0, 200)".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let player = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                width: Some("400px".into()),
                height: Some("300px".into()),
                ..Style::default()
            },
            children: vec![
                layer(2, Some("10"), "rgb(200, 0, 0)"),
                words,
                layer(4, None, "rgb(0, 200, 0)"),
            ],
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![player]);
        let at = |colour: Color32| {
            shapes
                .iter()
                .position(|s| match s {
                    egui::Shape::Rect(r) => same_hue(r.fill, colour),
                    _ => false,
                })
                .unwrap_or_else(|| panic!("{colour:?} not painted: {shapes:?}"))
        };
        assert!(at(red) < at(blue), "the backdrop over the words");
        // A placed layer with no `z-index` stacks at level 0: under the
        // one at 10, though it comes later in the tree.
        assert!(at(green) < at(red));
    }

    /// The shapes a floating overlay of `nodes` paints in an 800 by 600
    /// window, nested lists flattened.
    fn floating_shapes(nodes: Vec<VNode>) -> Vec<egui::Shape> {
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes,
            }],
            ..DomOverlay::default()
        }));
        plugin_dom::test_viewport([800.0, 600.0]);
        let ctx = app_fonts_ctx();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                    ..egui::RawInput::default()
                },
                |ui| floating(ui),
            );
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        fn flat(s: egui::Shape, out: &mut Vec<egui::Shape>) {
            match s {
                egui::Shape::Vec(v) => v.into_iter().for_each(|s| flat(s, out)),
                s => out.push(s),
            }
        }
        let mut out = Vec::new();
        for c in shapes {
            flat(c.shape, &mut out);
        }
        out
    }

    /// The same colour, whatever opacity it was painted at.
    fn same_hue(painted: Color32, colour: Color32) -> bool {
        let [r, g, b, a] = painted.to_srgba_unmultiplied();
        a > 0
            && [r, g, b]
                .iter()
                .zip([colour.r(), colour.g(), colour.b()])
                .all(|(x, y)| x.abs_diff(y) <= 4)
    }

    fn text_node(id: u64, t: &str) -> VNode {
        VNode {
            id,
            tag: "#text".into(),
            text: Some(t.into()),
            ..VNode::default()
        }
    }

    /// lyrics-everywhere's CatJam bubble: its tail is an `::after` box with
    /// no size, `border: 10px solid transparent`, a dark top and no bottom
    /// border, 10 points under the bubble's padding box and 20 in. It is
    /// drawn as a 20 by 10 triangle hanging from inside the bubble's 3-point
    /// border (a box with no size had no height to hang it by, and one
    /// border width all round drew four 10-point lines).
    #[test]
    fn a_css_triangle_is_drawn_from_its_sides_own_widths() {
        let dark = Color32::from_rgb(0x11, 0x11, 0x11);
        let tail = VNode {
            id: 3,
            tag: "::after".into(),
            style: Style {
                position: Some("absolute".into()),
                bottom: Some("-10px".into()),
                left: Some("20px".into()),
                width: Some("0".into()),
                height: Some("0".into()),
                border: Some("10px solid transparent".into()),
                border_color: Some("#111 transparent transparent transparent".into()),
                border_width: Some("10px 10px 0px 10px".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let bubble = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("100px".into()),
                left: Some("20px".into()),
                padding: Some("10px 16px".into()),
                border: Some("3px solid #111".into()),
                background_color: Some("#ffffff".into()),
                ..Style::default()
            },
            children: vec![text_node(2, "line"), tail],
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![bubble]);
        let frame = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Rect(r) if r.stroke.width == 3.0 => Some(r.rect),
                _ => None,
            })
            .unwrap_or_else(|| {
                let rects: Vec<_> = shapes
                    .iter()
                    .filter(|s| matches!(s, egui::Shape::Rect(_)))
                    .collect();
                panic!("no bubble box: {rects:?}")
            });
        let points: Vec<Pos2> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Path(p) if same_hue(p.fill, dark) => Some(p.points.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert!(!points.is_empty(), "no filled triangle: {shapes:?}");
        let tri = Rect::from_points(&points);
        assert!((tri.width() - 20.0).abs() < 0.5, "{tri:?}");
        assert!((tri.height() - 10.0).abs() < 0.5, "{tri:?}");
        // The bubble's border box ends at its frame's outer edge; the
        // padding box 3 points above it.
        let border_box_bottom = frame.bottom();
        assert!(
            (tri.top() - (border_box_bottom - 3.0)).abs() < 1.0,
            "{tri:?} under {frame:?}"
        );
        assert!((tri.left() - (20.0 + 3.0 + 20.0)).abs() < 1.0, "{tri:?}");
        // Its tip is at the middle of its foot.
        let tip = points
            .iter()
            .max_by(|a, b| a.y.total_cmp(&b.y))
            .copied()
            .unwrap();
        assert!((tip.x - tri.center().x).abs() < 0.5, "{points:?}");
    }

    /// `filter: blur()` on a line softens the words inside it, its
    /// children's own `filter: none` included (radiant-lyrics blurs the
    /// lines away from the sung one by `.07em`): see-through copies spread
    /// round the words, none of them as solid as the sharp words. A line
    /// without one stays one sharp copy.
    #[test]
    fn a_blurred_line_draws_its_words_soft() {
        let line = |id: u64, words: &str, filter: Option<&str>| VNode {
            id,
            tag: "div".into(),
            style: Style {
                font_size: Some("40px".into()),
                filter: filter.map(Into::into),
                ..Style::default()
            },
            children: vec![VNode {
                id: id + 1,
                tag: "span".into(),
                style: Style {
                    filter: Some("none".into()),
                    ..Style::default()
                },
                children: vec![text_node(id + 2, words)],
                ..VNode::default()
            }],
            ..VNode::default()
        };
        let holder = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("10px".into()),
                left: Some("10px".into()),
                ..Style::default()
            },
            children: vec![
                line(10, "Sung", None),
                line(20, "Coming", Some("blur(.07em)")),
            ],
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![holder]);
        let copies = |words: &str| -> Vec<egui::epaint::TextShape> {
            shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) if t.galley.text() == words => Some(t.clone()),
                    _ => None,
                })
                .collect()
        };
        let sharp = copies("Sung");
        assert_eq!(sharp.len(), 1, "{sharp:?}");
        assert!((sharp[0].opacity_factor - 1.0).abs() < 1e-6);
        let soft = copies("Coming");
        assert!(soft.len() > 1, "one sharp copy: {soft:?}");
        assert!(soft.iter().all(|t| t.opacity_factor < 0.9), "{soft:?}");
        // Spread over the blur: 0.07em of 40 points is 2.8 points.
        let xs: Vec<f32> = soft.iter().map(|t| t.pos.x).collect();
        let spread = xs.iter().copied().fold(f32::MIN, f32::max)
            - xs.iter().copied().fold(f32::MAX, f32::min);
        assert!(spread > 4.0 && spread < 10.0, "{xs:?}");
    }

    /// A box's radial background is a round glow fading out where its stops
    /// say, at its opacity, and a background of several gradient layers
    /// paints each, the first on top: betterFullscreen's glow behind the
    /// cover (`radial-gradient(circle at center, <colour> 0, transparent
    /// 70%)` at `opacity: calc(.4 * 0.4)`) was a square of its first colour
    /// at full opacity, and its player's dark radial layer over TIDAL's
    /// profile gradient was one flat colour.
    #[test]
    fn a_radial_background_is_a_glow_and_layers_paint_in_order() {
        let fixed = |id: u64, left: &str, background: &str, opacity: Option<&str>| VNode {
            id,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("10px".into()),
                left: Some(left.into()),
                width: Some("200px".into()),
                height: Some("200px".into()),
                background_image: Some(background.into()),
                opacity: opacity.map(str::to_string),
                ..Style::default()
            },
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![
            fixed(
                1,
                "10px",
                "radial-gradient(circle at center,rgba(255, 255, 255, 0.5) 0,transparent 70%)",
                Some("calc(.4 * 0.4)"),
            ),
            fixed(
                2,
                "300px",
                "radial-gradient(60% 60% at 25% 35%,rgb(255, 0, 0) 0,transparent 70%),linear-gradient(90deg,#0000ff,#00ff00)",
                None,
            ),
        ]);
        let meshes: Vec<&egui::Mesh> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Mesh(m) => Some(&**m),
                _ => None,
            })
            .collect();
        // No box is filled with a gradient's first colour.
        assert!(
            !shapes
                .iter()
                .any(|s| matches!(s, egui::Shape::Rect(r) if r.fill.a() > 0)),
            "{shapes:?}"
        );
        let inside = |m: &egui::Mesh, x0: f32| {
            m.vertices
                .iter()
                .all(|v| v.pos.x >= x0 - 1.0 && v.pos.x <= x0 + 201.0)
        };
        let glow = meshes.iter().find(|m| inside(m, 10.0)).expect("the glow");
        // Its centre: half-white at 0.16; its corners: nothing.
        let at = |m: &egui::Mesh, p: Pos2| {
            m.vertices
                .iter()
                .min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p)))
                .map(|v| v.color)
                .unwrap()
        };
        let centre = at(glow, Pos2::new(110.0, 110.0));
        assert_eq!(at(glow, Pos2::new(10.0, 10.0)).a(), 0);
        // The second box: the linear layer under the radial one, which is
        // an ellipse of 120 by 120 at (350, 80).
        let second: Vec<usize> = meshes
            .iter()
            .enumerate()
            .filter(|(_, m)| inside(m, 300.0))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(second.len(), 2, "two layers");
        let (under, over) = (meshes[second[0]], meshes[second[1]]);
        assert!(
            under.vertices.iter().all(|v| v.color.r() == 0)
                && under.vertices.iter().any(|v| v.color.b() > 0),
            "the blue-green ramp first"
        );
        let red = at(over, Pos2::new(350.0, 80.0));
        assert!(red.r() > 3 * red.g().max(1) && red.a() > 0, "{red:?}");
        // The glow's centre is half-white at 0.16 against the red's full
        // colour, both as the new layers fade in (egui's areas do).
        let ratio = f32::from(centre.a()) / f32::from(red.a());
        assert!((0.06..=0.10).contains(&ratio), "{centre:?} against {red:?}");
        // 0.7 of 120 points from the centre the red is gone.
        assert!(at(over, Pos2::new(350.0 + 90.0, 80.0)).a() < 10);
    }

    /// custom-fonts' Comic Sans tag: `transform: rotate(-3deg)` turns the
    /// whole box, its words and their two `text-shadow` copies included
    /// (gold under pink, the first layer on top), and its `2px dashed`
    /// border is drawn as dashes, over its gradient.
    #[test]
    fn a_turned_box_turns_its_words_and_their_shadows() {
        let tag = VNode {
            id: 1,
            tag: "span".into(),
            style: Style {
                display: Some("inline-block".into()),
                color: Some("#1E90FF".into()),
                text_shadow: Some("1px 1px 0 #FFD700,-1px -1px 0 #FF69B4".into()),
                transform: Some("rotate(-3deg) skew(-3deg)".into()),
                padding: Some("0.1em 0.3em".into()),
                border: Some("2px dashed #FF69B4".into()),
                border_radius: Some("8px".into()),
                background_image: Some("linear-gradient(45deg,#FFB6C1,#87CEFA)".into()),
                ..Style::default()
            },
            children: vec![text_node(2, "Edition")],
            ..VNode::default()
        };
        let holder = VNode {
            id: 3,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("10px".into()),
                left: Some("10px".into()),
                ..Style::default()
            },
            children: vec![tag],
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![holder]);
        let texts: Vec<&egui::epaint::TextShape> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text() == "Edition" => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(texts.len(), 3, "the words and two shadows");
        let turn = (-3.0f32).to_radians();
        for t in &texts {
            assert!((t.angle - turn).abs() < 1e-4, "{}", t.angle);
        }
        let pink = Color32::from_rgb(0xFF, 0x69, 0xB4);
        let gold = Color32::from_rgb(0xFF, 0xD7, 0x00);
        assert_eq!(texts[0].override_text_color, Some(pink));
        assert_eq!(texts[1].override_text_color, Some(gold));
        assert_eq!(texts[2].override_text_color, None);
        let dashes = shapes
            .iter()
            .filter(|s| match s {
                egui::Shape::LineSegment { stroke, .. } => same_hue(stroke.color, pink),
                _ => false,
            })
            .count();
        assert!(dashes > 12, "{dashes} dashes");
        // The dashes come after the gradient.
        let gradient = shapes
            .iter()
            .position(|s| matches!(s, egui::Shape::Mesh(_)))
            .expect("the gradient");
        let first_dash = shapes
            .iter()
            .position(
                |s| matches!(s, egui::Shape::LineSegment { stroke, .. } if same_hue(stroke.color, pink)),
            )
            .expect("a dash");
        assert!(first_dash > gradient);
    }

    /// `background-clip: text` with `color: transparent`: the background
    /// shows through the words only. A red layer half the word's width over
    /// a blue one paints the word's left half red and its right half blue,
    /// and no box is filled. It drew the gradients as a striped box over
    /// words of no colour (radiant-lyrics' sung word, wiped by a gradient,
    /// was a box of stripes).
    #[test]
    fn a_background_clipped_to_text_colours_the_words() {
        let red = Color32::from_rgb(0xff, 0, 0);
        let blue = Color32::from_rgb(0, 0, 0xff);
        let word = VNode {
            id: 1,
            tag: "span".into(),
            style: Style {
                display: Some("inline-block".into()),
                color: Some("transparent".into()),
                font_size: Some("40px".into()),
                background_clip: Some("text".into()),
                background_image: Some(
                    "linear-gradient(90deg,#ff0000 100%,transparent 100%),linear-gradient(90deg,#0000ff,#0000ff)"
                        .into(),
                ),
                background_size: Some("50% 100%,100% 100%".into()),
                background_position: Some("left,left".into()),
                ..Style::default()
            },
            children: vec![text_node(2, "Wiped")],
            ..VNode::default()
        };
        // The same background under words of their own colour: the colour.
        let mut sung = word.clone();
        sung.id = 4;
        sung.style.color = Some("#00ff00".into());
        sung.children = vec![text_node(5, "Sung")];
        let holder = VNode {
            id: 3,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("10px".into()),
                left: Some("10px".into()),
                ..Style::default()
            },
            children: vec![word, sung],
            ..VNode::default()
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![holder],
            }],
            ..DomOverlay::default()
        }));
        plugin_dom::test_viewport([800.0, 600.0]);
        let ctx = app_fonts_ctx();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                    ..egui::RawInput::default()
                },
                |ui| floating(ui),
            );
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let words: Vec<(Rect, Option<Color32>, Rect)> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == "Wiped" => Some((
                    c.clip_rect,
                    t.override_text_color,
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                _ => None,
            })
            .collect();
        assert!(words.len() > 4, "painted in strips: {}", words.len());
        let whole = words[0].2;
        let mid = whole.center().x;
        for (clip, colour, _) in &words {
            let colour = colour.expect("each strip has its colour");
            if clip.max.x < mid - 1.0 {
                assert!(same_hue(colour, red), "left of the middle: {colour:?}");
            } else if clip.min.x > mid + 1.0 {
                assert!(same_hue(colour, blue), "right of the middle: {colour:?}");
            }
        }
        let green = Color32::from_rgb(0, 0xff, 0);
        let sung: Vec<Option<Color32>> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == "Sung" => Some(t.override_text_color),
                _ => None,
            })
            .collect();
        assert!(!sung.is_empty());
        assert!(
            sung.iter().all(|c| c.is_some_and(|c| same_hue(c, green))),
            "its own colour over the background: {sung:?}"
        );
        let filled = shapes.iter().any(|c| match &c.shape {
            egui::Shape::Mesh(_) => true,
            egui::Shape::Rect(r) => r.fill.a() > 0,
            _ => false,
        });
        assert!(!filled, "no box is filled");
    }

    /// An animation moves each layer of a `background-size` or
    /// `background-position` list (a wipe grows its middle layer from `0`
    /// to `100%` and slides its soft edge from before the box to past it);
    /// only a single position moved, so the wipe stood still.
    #[test]
    fn an_animation_moves_each_background_layer() {
        use css_motion::Lerp as _;
        let size = BgList(".75em 100%,0 100%,100% 100%".into())
            .lerp(&BgList(".75em 100%,100% 100%,100% 100%".into()), 0.25);
        let layers = css::split_top(&size.0, ',');
        let width = |v: &str| {
            let w = css::words(v)[0];
            css::length(w, 20.0, 200.0, [800.0, 600.0]).expect(w)
        };
        assert!((width(layers[0]) - 15.0).abs() < 0.01, "{}", layers[0]);
        assert!((width(layers[1]) - 50.0).abs() < 0.01, "{}", layers[1]);
        assert!((width(layers[2]) - 200.0).abs() < 0.01, "{}", layers[2]);
        let at = BgList("calc(.75em / -2) 0,left,left".into())
            .lerp(&BgList("calc(100% + .75em / 2) 0,left,left".into()), 0.5);
        let first = css::words(css::split_top(&at.0, ',')[0])[0];
        // Half way from 7.5 points before the box to 7.5 past its 200.
        assert!(
            (css::length(first, 20.0, 200.0, [800.0, 600.0]).expect(first) - 100.0).abs() < 0.01,
            "{first}"
        );
    }

    /// An image keeps the corners its `border-radius` gives it: qr-share's
    /// card is rounded 12 points (only `50%` rounded an image), `50%` is
    /// still a circle, and a radius never passes half the shorter side.
    #[test]
    fn an_image_is_rounded_by_its_border_radius() {
        let (vars, keyframes) = (BTreeMap::new(), BTreeMap::new());
        let cx = Cx {
            vars: &vars,
            viewport: [1400.0, 900.0],
            keyframes: &keyframes,
        };
        let radius = |r: Option<&str>, size: Vec2| {
            let style = Style {
                border_radius: r.map(Into::into),
                ..Style::default()
            };
            image_radius(&style, size, 14.0, &cx)
        };
        assert_eq!(radius(Some("12px"), vec2(600.0, 600.0)), Some(12));
        assert_eq!(radius(Some("50%"), vec2(40.0, 40.0)), Some(20));
        assert_eq!(radius(Some("1em"), vec2(100.0, 100.0)), Some(14));
        assert_eq!(radius(Some("80px"), vec2(60.0, 100.0)), Some(30));
        assert_eq!(radius(Some("0"), vec2(60.0, 100.0)), None);
        assert_eq!(radius(None, vec2(60.0, 100.0)), None);
    }

    /// qr-share's dialog: a fixed backdrop over the window, a flex column
    /// that centres an `<img>` with no CSS size (its natural 600 x 600
    /// QR code) and a row of two buttons. The picture takes its natural
    /// size and everything is centred.
    #[test]
    fn a_modal_centres_a_naturally_sized_image_and_its_buttons() {
        let text = |id, t: &str| VNode {
            id,
            tag: "#text".into(),
            text: Some(t.into()),
            ..VNode::default()
        };
        let modal = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                width: Some("100vw".into()),
                height: Some("100vh".into()),
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                justify_content: Some("center".into()),
                align_items: Some("center".into()),
                z_index: Some("999999".into()),
                background_color: Some("rgba(0,0,0,0.7)".into()),
                ..Style::default()
            },
            children: vec![
                VNode {
                    id: 2,
                    tag: "img".into(),
                    natural: Some([600.0, 600.0]),
                    attrs: vec![("title".into(), "Right click to copy image".into())],
                    style: Style {
                        max_width: Some("90%".into()),
                        height: Some("auto".into()),
                        border_radius: Some("12px".into()),
                        // Not qr-share's: shows where the picture's box is.
                        background_color: Some("#ff0000".into()),
                        ..Style::default()
                    },
                    ..VNode::default()
                },
                VNode {
                    id: 3,
                    tag: "div".into(),
                    style: Style {
                        display: Some("flex".into()),
                        gap: Some("16px".into()),
                        ..Style::default()
                    },
                    children: vec![
                        VNode {
                            children: vec![text(11, "Download")],
                            ..button(4, "")
                        },
                        VNode {
                            children: vec![text(12, "Close")],
                            ..button(5, "")
                        },
                    ],
                    ..VNode::default()
                },
            ],
            ..VNode::default()
        };
        let overlay = |modal: VNode| DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![modal],
            }],
            ..DomOverlay::default()
        };
        plugin_dom::test_viewport([1400.0, 900.0]);
        let ctx = app_fonts_ctx();
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            ..egui::RawInput::default()
        };
        let mut shapes = Vec::new();
        // The picture's size arrives once it decoded, frames after the
        // dialog first showed.
        let mut loading = modal.clone();
        loading.children[0].natural = None;
        for (modal, frames) in [(loading, 3), (modal, 4)] {
            plugin_dom::test_overlay(Some(overlay(modal)));
            for _ in 0..frames {
                let mut out = ctx.run_ui(input(), |ui| floating(ui));
                out.textures_delta.clear();
                shapes = out.shapes;
            }
        }
        plugin_dom::test_overlay(None);
        let texts: Vec<(String, Rect)> = shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => {
                    Some((t.galley.text().to_string(), t.visual_bounding_rect()))
                }
                _ => None,
            })
            .collect();
        let find = |label: &str| {
            texts
                .iter()
                .find(|(t, _)| t == label)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("no {label} in {texts:?}"))
        };
        let picture = shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill.r() > 100 && r.fill.g() == 0 => Some(r.rect),
                _ => None,
            })
            .unwrap_or_else(|| {
                let rects: Vec<_> = shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Rect(r) => Some((r.fill, r.rect)),
                        _ => None,
                    })
                    .collect();
                panic!("no picture box in {rects:?}")
            });
        assert!(
            (picture.center().x - 700.0).abs() < 2.0 && (picture.width() - 600.0).abs() < 2.0,
            "the picture is centred at its natural width: {picture:?}"
        );
        let (download, close) = (find("Download"), find("Close"));
        // Below the 600-point picture, which is centred on the window.
        assert!(download.top() > 600.0, "{download:?}");
        let middle = (download.left() + close.right()) / 2.0;
        assert!(
            (middle - 700.0).abs() < 40.0,
            "centred: {download:?} {close:?}"
        );
    }
    /// star-ratings: five star SVGs in a span that listens for clicks (and
    /// each star for pointer moves, its hover preview). A click lands on the
    /// star under the pointer, as `event.target`, with that star's box, and
    /// moves over a star reach it; before, the span took every click (it is
    /// laid over its children) and no move was sent.
    #[test]
    fn clicks_and_moves_reach_the_deepest_element_with_its_box() {
        let star = |id: u64| {
            VNode {
            id,
            tag: "svg".into(),
            markup: Some(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><path d="M0 0h32v32H0z"/></svg>"#.into(),
            ),
            attrs: vec![("width".into(), "20px".into()), ("height".into(), "20px".into())],
            style: Style {
                cursor: Some("pointer".into()),
                ..Style::default()
            },
            listens: vec!["click".into(), "mousemove".into()],
            ..VNode::default()
        }
        };
        let stars = VNode {
            id: 1,
            tag: "span".into(),
            style: Style {
                display: Some("flex".into()),
                ..Style::default()
            },
            listens: vec!["click".into()],
            children: vec![star(2), star(3)],
            ..VNode::default()
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "x".into(),
                position: Position::Append,
                nodes: vec![stars],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                inline(ui, "x", Position::Append);
                plugin_dom::end_frame(ui.ctx());
            });
            out.textures_delta.clear();
        };
        frame(Vec::new());
        frame(Vec::new());
        let second = ctx
            .read_response(egui::Id::new(("luna-node", 3u64)))
            .expect("the second star")
            .rect;
        let at = second.center();
        plugin_dom::take_test_events();
        frame(vec![egui::Event::PointerMoved(at)]);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![button(true)]);
        frame(vec![button(false)]);
        let events = plugin_dom::take_test_events();
        plugin_dom::test_overlay(None);
        let click = events.iter().find(|e| e.kind == "click").expect("a click");
        assert_eq!(click.target, DomTarget::Node(3), "{events:?}");
        let rect = click.data["rect"]
            .as_array()
            .expect("the star's box")
            .clone();
        assert_eq!(rect[2], json!(second.width()), "{events:?}");
        let moved = events
            .iter()
            .find(|e| e.kind == "mousemove")
            .expect("a move");
        assert_eq!(moved.target, DomTarget::Node(3), "{events:?}");
    }
    #[test]
    fn svg_markup_gets_a_namespace_and_colour() {
        let node = VNode {
            tag: "svg".into(),
            markup: Some("<svg viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M0 0h24v24H0z\"/></svg>".into()),
            ..VNode::default()
        };
        let Some(ImageSource::Bytes(uri, bytes)) =
            image_source(&node, Color32::from_rgb(255, 0, 0), None, Color32::BLACK)
        else {
            panic!("svg source");
        };
        assert!(uri.ends_with(".svg"));
        let svg = String::from_utf8(bytes).unwrap();
        assert!(svg.contains("xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("#ff0000"));
        // AltPlay's icon in its 55 % white link: `currentColor` keeps the
        // alpha (it was drawn white).
        let Some(ImageSource::Bytes(_, bytes)) = image_source(
            &node,
            Color32::from_rgba_unmultiplied(255, 255, 255, 140),
            None,
            Color32::BLACK,
        ) else {
            panic!("svg source");
        };
        let svg = String::from_utf8(bytes).unwrap();
        assert!(svg.contains("rgba(255,255,255,0.549)"), "{svg}");
    }

    /// The audiovisualiser's bars: `flex: 1 1 0` items of a fixed-width row,
    /// each `height: 100%` and `scaleY(level)`, bottom-aligned.
    #[test]
    fn growing_bars_share_the_row_and_scale_their_height() {
        assert_eq!(axis_scale("scaleY(0.25)"), vec2(1.0, 0.25));
        assert_eq!(
            axis_scale("translateX(2px) scaleX(-2) scaleY(50%)"),
            vec2(2.0, 0.5)
        );
        assert_eq!(
            axis_scale("scale(2)"),
            Vec2::ONE,
            "uniform scale stays visual"
        );
        let grow = |f: &str| {
            flex_grow(&Style {
                flex: Some(f.into()),
                ..Style::default()
            })
        };
        assert_eq!(
            (grow("1 1 0"), grow("2"), grow("auto"), grow("none")),
            (1.0, 2.0, 1.0, 0.0)
        );
        let bar = |id: u64, level: f32| VNode {
            id,
            tag: "div".into(),
            style: Style {
                flex: Some("1 1 0".into()),
                height: Some("100%".into()),
                background_color: Some("#33ffee".into()),
                transform: Some(format!("scaleY({level})")),
                ..Style::default()
            },
            ..VNode::default()
        };
        let row = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                align_items: Some("flex-end".into()),
                gap: Some("2px".into()),
                width: Some("100px".into()),
                height: Some("40px".into()),
                ..Style::default()
            },
            children: vec![bar(2, 0.5), bar(3, 1.0), bar(4, 0.25)],
            ..VNode::default()
        };
        let overlay = DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "bars".into(),
                position: Position::Append,
                nodes: vec![row],
            }],
            ..DomOverlay::default()
        };
        let size = layout(overlay, "bars");
        assert!(
            (size.x - 100.0).abs() < 1.0 && (size.y - 40.0).abs() < 1.0,
            "{size:?}"
        );
    }

    fn text(id: u64, t: &str) -> VNode {
        VNode {
            id,
            tag: "span".into(),
            children: vec![VNode {
                id: id + 100,
                tag: "#text".into(),
                text: Some(t.into()),
                ..VNode::default()
            }],
            ..VNode::default()
        }
    }

    /// The size of `node` drawn in `main` in an 800 x 600 window, after a
    /// few passes.
    fn drawn_size(node: VNode) -> Vec2 {
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "main".into(),
                position: Position::Prepend,
                nodes: vec![node],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        let mut size = Vec2::ZERO;
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let before = ui.cursor().min;
                inline(ui, "main", Position::Prepend);
                size = ui.min_rect().max - before;
            });
            out.textures_delta.clear();
        }
        plugin_dom::test_overlay(None);
        size
    }

    /// DownloadManager's start menu: `width: 400px` with a larger
    /// `max-width: calc(100vw - 16px)`. The width holds; what fills the
    /// room inside (its search row, a grown flex item) fills 400 points,
    /// not the room up to the window's edge, which the larger `max-width`
    /// gave it after the width had set it.
    #[test]
    fn a_larger_max_width_does_not_undo_a_width() {
        let filler = VNode {
            id: 3,
            tag: "div".into(),
            style: Style {
                flex: Some("1".into()),
                height: Some("10px".into()),
                background_color: Some("#0078d7".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let row = VNode {
            id: 2,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                ..Style::default()
            },
            children: vec![filler],
            ..VNode::default()
        };
        let menu = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                width: Some("400px".into()),
                max_width: Some("calc(100vw - 16px)".into()),
                ..Style::default()
            },
            children: vec![row],
            ..VNode::default()
        };
        plugin_dom::test_viewport([800.0, 600.0]);
        let size = drawn_size(menu);
        assert!(size.x <= 400.5, "the menu kept its width: {size:?}");
        assert!(size.x >= 399.5, "{size:?}");
    }

    /// `node` drawn in `main` in an 800 x 600 window after a few passes:
    /// the rect of each box filled with a colour, by colour.
    fn filled_rects(node: VNode) -> Vec<(Color32, Rect)> {
        filled_rects_in(node, &egui::Context::default())
    }

    /// A context with the app's faces at every weight: what `body`
    /// elements draw in (TIDAL's weight 500 is the app's medium face).
    fn app_fonts_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::install_plugin_fonts(&ctx, &[], None);
        ctx
    }

    /// [`filled_rects`] with the app's 28-point interactive rows, which
    /// egui's own 18 points would hide.
    fn filled_rects_app(node: VNode) -> Vec<(Color32, Rect)> {
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|s| s.spacing.interact_size = vec2(40.0, 28.0));
        filled_rects_in(node, &ctx)
    }

    fn filled_rects_in(node: VNode, ctx: &egui::Context) -> Vec<(Color32, Rect)> {
        plugin_dom::test_viewport([800.0, 600.0]);
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "main".into(),
                position: Position::Prepend,
                nodes: vec![node],
            }],
            ..DomOverlay::default()
        }));
        let mut rects = Vec::new();
        for _ in 0..5 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                inline(ui, "main", Position::Prepend);
            });
            out.textures_delta.clear();
            fn walk(shape: &egui::Shape, out: &mut Vec<(Color32, Rect)>) {
                match shape {
                    egui::Shape::Rect(r) if r.fill.a() > 0 => out.push((r.fill, r.rect)),
                    egui::Shape::Path(p) if p.fill.a() > 0 => {
                        out.push((p.fill, Rect::from_points(&p.points)));
                    }
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            rects.clear();
            for s in &out.shapes {
                walk(&s.shape, &mut rects);
            }
        }
        plugin_dom::test_overlay(None);
        rects
    }

    fn filled_rect(rects: &[(Color32, Rect)], colour: Color32) -> Rect {
        rects
            .iter()
            .find(|(c, _)| *c == colour)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("no box filled {colour:?} in {rects:?}"))
    }

    fn filled(id: u64, colour: &str, style: Style, children: Vec<VNode>) -> VNode {
        VNode {
            id,
            tag: "div".into(),
            style: Style {
                background_color: Some(colour.into()),
                ..style
            },
            children,
            ..VNode::default()
        }
    }

    /// DownloadManager's start menu tiles: `flex-wrap: wrap` puts the
    /// second 104-point tile under the first in the 188 points they have
    /// (they ran side by side and widened the 400-point menu to 490). Each
    /// tile keeps its 104-point height.
    #[test]
    fn a_wrapping_flex_row_runs_its_items_onto_new_lines() {
        let tile = |id: u64, colour: &str| {
            let icon = VNode {
                id: id * 10,
                tag: "span".into(),
                style: Style {
                    display: Some("inline-flex".into()),
                    line_height: Some("0".into()),
                    ..Style::default()
                },
                children: vec![VNode {
                    id: id * 10 + 1,
                    tag: "svg".into(),
                    markup: Some(
                        r#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30"></svg>"#
                            .into(),
                    ),
                    style: Style {
                        width: Some("30px".into()),
                        height: Some("30px".into()),
                        ..Style::default()
                    },
                    ..VNode::default()
                }],
                ..VNode::default()
            };
            let label = VNode {
                id: id * 10 + 2,
                tag: "span".into(),
                style: Style {
                    position: Some("absolute".into()),
                    left: Some("8px".into()),
                    right: Some("8px".into()),
                    bottom: Some("6px".into()),
                    ..Style::default()
                },
                children: vec![text_node(id * 10 + 3, "Tile")],
                ..VNode::default()
            };
            VNode {
                tag: "button".into(),
                ..filled(
                    id,
                    colour,
                    Style {
                        width: Some("104px".into()),
                        height: Some("104px".into()),
                        display: Some("flex".into()),
                        flex_direction: Some("column".into()),
                        align_items: Some("center".into()),
                        justify_content: Some("center".into()),
                        gap: Some("8px".into()),
                        padding: Some("0 0 18px".into()),
                        position: Some("relative".into()),
                        ..Style::default()
                    },
                    vec![icon, label],
                )
            }
        };
        let tiles = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_wrap: Some("wrap".into()),
                gap: Some("8px".into()),
                width: Some("188px".into()),
                ..Style::default()
            },
            children: vec![tile(2, "#ff0000"), tile(3, "#0000ff")],
            ..VNode::default()
        };
        let rects = filled_rects(tiles);
        let first = filled_rect(&rects, Color32::from_rgb(255, 0, 0));
        let second = filled_rect(&rects, Color32::from_rgb(0, 0, 255));
        assert!((first.height() - 104.0).abs() < 0.5, "{first:?}");
        assert!((second.height() - 104.0).abs() < 0.5, "{second:?}");
        assert!(
            (second.left() - first.left()).abs() < 0.5,
            "{first:?} {second:?}"
        );
        assert!(
            (second.top() - (first.bottom() + 8.0)).abs() < 0.5,
            "the second tile starts a line 8 points under the first: {first:?} {second:?}"
        );
        assert_eq!(
            wrap_lines(&[104.0, 104.0, 50.0], 220.0, 8.0),
            vec![0..2, 2..3]
        );
        assert_eq!(wrap_lines(&[300.0, 10.0], 100.0, 8.0), vec![0..1, 1..2]);
    }

    /// DownloadManager's start menu is a 540-point column whose app list
    /// grows (`flex: 1`): its footer sits at the menu's foot, not right
    /// under the two apps.
    #[test]
    fn a_growing_item_takes_a_columns_free_height() {
        let column = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                width: Some("100px".into()),
                height: Some("300px".into()),
                ..Style::default()
            },
            children: vec![
                filled(
                    2,
                    "#00ff00",
                    Style {
                        height: Some("20px".into()),
                        ..Style::default()
                    },
                    vec![],
                ),
                filled(
                    3,
                    "#ff0000",
                    Style {
                        flex: Some("1".into()),
                        ..Style::default()
                    },
                    vec![text_node(30, "Apps")],
                ),
                filled(
                    4,
                    "#0000ff",
                    Style {
                        height: Some("30px".into()),
                        ..Style::default()
                    },
                    vec![],
                ),
            ],
            ..VNode::default()
        };
        let rects = filled_rects(column);
        let top = filled_rect(&rects, Color32::from_rgb(0, 255, 0));
        let grown = filled_rect(&rects, Color32::from_rgb(255, 0, 0));
        let foot = filled_rect(&rects, Color32::from_rgb(0, 0, 255));
        assert!((grown.height() - 250.0).abs() < 0.5, "{grown:?}");
        assert!(
            (foot.bottom() - (top.top() + 300.0)).abs() < 0.5,
            "{foot:?}"
        );
    }

    /// DownloadManager's window: its nav is an item of a flex row with no
    /// `align-items` (stretch) that is as tall as the window leaves, and
    /// runs that whole height; the caption buttons fill their title bar
    /// (`align-items: stretch`). A row that centres keeps its item's size.
    #[test]
    fn a_flex_row_of_a_set_height_stretches_its_items() {
        let row = |align: Option<&str>| VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                align_items: align.map(Into::into),
                width: Some("300px".into()),
                height: Some("200px".into()),
                ..Style::default()
            },
            children: vec![filled(
                2,
                "#ff0000",
                Style {
                    width: Some("50px".into()),
                    ..Style::default()
                },
                vec![text_node(20, "Nav")],
            )],
            ..VNode::default()
        };
        for align in [None, Some("stretch")] {
            let nav = filled_rect(&filled_rects(row(align)), Color32::from_rgb(255, 0, 0));
            assert!((nav.height() - 200.0).abs() < 0.5, "{align:?}: {nav:?}");
        }
        let nav = filled_rect(
            &filled_rects(row(Some("center"))),
            Color32::from_rgb(255, 0, 0),
        );
        assert!(nav.height() < 50.0, "{nav:?}");
    }

    /// DownloadManager's window nav: a flex column whose buttons stretch
    /// across it (the open item's grey runs the nav's width, not its
    /// words'), and the open item's `border: none; border-left: 3px solid`
    /// draws its 3-point blue bar down the item's left edge.
    #[test]
    fn a_flex_columns_buttons_fill_it_and_keep_a_side_border() {
        let item = VNode {
            id: 2,
            tag: "button".into(),
            style: Style {
                display: Some("flex".into()),
                background_color: Some("#e6e6e6".into()),
                border: Some("none".into()),
                border_width: Some("0px 0px 0px 3px".into()),
                border_color: Some("currentcolor currentcolor currentcolor #0078d7".into()),
                color: Some("#000".into()),
                ..Style::default()
            },
            children: vec![text_node(20, "Downloads")],
            ..VNode::default()
        };
        let nav = filled(
            1,
            "#f3f3f3",
            Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                width: Some("160px".into()),
                ..Style::default()
            },
            vec![item],
        );
        let rects = filled_rects(nav);
        let column = filled_rect(&rects, Color32::from_rgb(0xf3, 0xf3, 0xf3));
        let row = filled_rect(&rects, Color32::from_rgb(0xe6, 0xe6, 0xe6));
        assert!((column.width() - 160.0).abs() < 0.5, "{column:?}");
        assert!(
            (row.width() - column.width()).abs() < 0.5,
            "{row:?} in {column:?}"
        );
        let bar = filled_rect(&rects, Color32::from_rgb(0x00, 0x78, 0xd7));
        assert!((bar.width() - 3.0).abs() < 0.5, "{bar:?}");
        assert!((bar.left() - row.left()).abs() < 0.5, "{bar:?} {row:?}");
    }

    /// DownloadManager's window nav item: a flex row (`align-items:
    /// center`, `padding: 9px 12px 9px 9px`) of a glyph and a 12-point
    /// label is its padding and its line tall, 34 points as in TIDAL, not
    /// egui's 28-point row plus padding.
    #[test]
    fn a_centred_flex_row_is_as_tall_as_its_words() {
        let span = |id: u64, text: &str| VNode {
            id,
            tag: "span".into(),
            children: vec![text_node(id * 10, text)],
            ..VNode::default()
        };
        let item = VNode {
            id: 2,
            tag: "button".into(),
            style: Style {
                display: Some("flex".into()),
                align_items: Some("center".into()),
                gap: Some("10px".into()),
                padding: Some("9px 12px 9px 9px".into()),
                font_size: Some("12px".into()),
                background_color: Some("#e6e6e6".into()),
                ..Style::default()
            },
            children: vec![span(3, "v"), span(4, "Downloads")],
            ..VNode::default()
        };
        let nav = filled(
            1,
            "#f3f3f3",
            Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                width: Some("160px".into()),
                ..Style::default()
            },
            vec![item],
        );
        let content = VNode {
            id: 9,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                width: Some("500px".into()),
                height: Some("400px".into()),
                ..Style::default()
            },
            children: vec![nav],
            ..VNode::default()
        };
        let rects = filled_rects_app(content);
        let row = filled_rect(&rects, Color32::from_rgb(0xe6, 0xe6, 0xe6));
        // Its padding and one line of the test's 12-point face (about 14),
        // not a 28-point row: 46 before.
        assert!((30.0..36.0).contains(&row.height()), "{row:?}");
    }

    /// DownloadManager's empty queue: a plain block with `text-align:
    /// center` is as wide as its column, and its words stand in the middle.
    #[test]
    fn a_centred_block_takes_its_columns_width_and_centres_its_words() {
        let note = VNode {
            id: 2,
            tag: "div".into(),
            style: Style {
                text_align: Some("center".into()),
                ..Style::default()
            },
            children: vec![text_node(3, "Queue is empty.")],
            ..VNode::default()
        };
        let body = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                width: Some("400px".into()),
                ..Style::default()
            },
            children: vec![note],
            ..VNode::default()
        };
        plugin_dom::test_viewport([800.0, 600.0]);
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "main".into(),
                position: Position::Prepend,
                nodes: vec![body],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        let mut words = None;
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                inline(ui, "main", Position::Prepend);
            });
            out.textures_delta.clear();
            words = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.rect.translate(t.pos.to_vec2())),
                _ => None,
            });
        }
        plugin_dom::test_overlay(None);
        let words = words.expect("the words are drawn");
        assert!(
            (words.center().x - 200.0).abs() < 2.0,
            "centred in the 400-point column: {words:?}"
        );
    }

    /// Words drawn and where, and filled boxes and where.
    type Parts = (Vec<(String, Rect)>, Vec<(Color32, Rect)>);

    /// The words and the filled boxes `node` draws in the app's `main`
    /// (1200 x 800), after a few frames for sizes kept from the last one.
    fn drawn_parts(node: VNode) -> Parts {
        plugin_dom::test_viewport([1200.0, 800.0]);
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "main".into(),
                position: Position::Prepend,
                nodes: vec![node],
            }],
            ..DomOverlay::default()
        }));
        let ctx = app_fonts_ctx();
        let (mut words, mut boxes) = (Vec::new(), Vec::new());
        for _ in 0..6 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 800.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                inline(ui, "main", Position::Prepend);
            });
            out.textures_delta.clear();
            fn walk(shape: &egui::Shape, out: &mut Parts) {
                match shape {
                    egui::Shape::Text(t) => out.0.push((
                        t.galley.text().to_string(),
                        t.galley.rect.translate(t.pos.to_vec2()),
                    )),
                    egui::Shape::Rect(r) if r.fill.a() > 0 => out.1.push((r.fill, r.rect)),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            let mut parts = (Vec::new(), Vec::new());
            for s in &out.shapes {
                walk(&s.shape, &mut parts);
            }
            (words, boxes) = parts;
        }
        plugin_dom::test_overlay(None);
        (words, boxes)
    }

    fn words_at(words: &[(String, Rect)], text: &str) -> Rect {
        words
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{text:?} not drawn: {words:?}"))
    }

    fn block_of(id: u64, tag: &str, style: Style, text: &str) -> VNode {
        VNode {
            id,
            tag: tag.into(),
            style,
            children: vec![text_node(id * 10, text)],
            ..VNode::default()
        }
    }

    /// A flex row that centres its items (`align-items: center`) puts the
    /// shorter ones in its middle, beside a tall one, as Chromium does: a
    /// growing column of words beside a 200-point picture (betterFullscreen's
    /// header, whose song words hung from the picture's top), and a plain
    /// block of words that does not grow.
    #[test]
    fn a_centring_row_puts_its_shorter_items_in_its_middle() {
        let art = |id: u64, colour: &str| {
            filled(
                id,
                colour,
                Style {
                    width: Some("200px".into()),
                    height: Some("200px".into()),
                    ..Style::default()
                },
                vec![],
            )
        };
        let row = |id: u64, children: Vec<VNode>| VNode {
            id,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                align_items: Some("center".into()),
                gap: Some("24px".into()),
                width: Some("900px".into()),
                ..Style::default()
            },
            children,
            ..VNode::default()
        };
        let info = VNode {
            id: 3,
            tag: "div".into(),
            style: Style {
                flex: Some("1".into()),
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                min_width: Some("0".into()),
                ..Style::default()
            },
            children: vec![
                block_of(4, "h1", Style::default(), "Song title"),
                block_of(5, "div", Style::default(), "Artist name"),
            ],
            ..VNode::default()
        };
        let plain = block_of(7, "div", Style::default(), "Plain words");
        let page = VNode {
            id: 100,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                gap: Some("40px".into()),
                ..Style::default()
            },
            children: vec![
                row(1, vec![art(2, "#ff0000"), info]),
                row(6, vec![art(8, "#0000ff"), plain]),
            ],
            ..VNode::default()
        };
        let (words, boxes) = drawn_parts(page);
        let red = filled_rect(&boxes, Color32::from_rgb(255, 0, 0));
        let title = words_at(&words, "Song title");
        let artist = words_at(&words, "Artist name");
        let middle = (title.top() + artist.bottom()) / 2.0;
        assert!(
            (middle - red.center().y).abs() < 3.0,
            "the growing column's words in the row's middle ({}): {title:?} {artist:?}",
            red.center().y
        );
        let blue = filled_rect(&boxes, Color32::from_rgb(0, 0, 255));
        let plain = words_at(&words, "Plain words");
        assert!(
            (plain.center().y - blue.center().y).abs() < 3.0,
            "the plain block's words in the row's middle ({}): {plain:?}",
            blue.center().y
        );
    }

    /// A flex column that centres its items (`align-items: center`) puts
    /// each block, as wide as what it holds, in its middle, and
    /// `justify-content: center` puts them all in the middle of its height:
    /// betterFullscreen's "No lyrics available" under its note symbol, in
    /// the middle of the lyrics area (the words sat at the column's left
    /// edge).
    #[test]
    fn a_centring_column_puts_its_blocks_in_its_middle() {
        let note = VNode {
            id: 2,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                align_items: Some("center".into()),
                justify_content: Some("center".into()),
                flex: Some("1".into()),
                gap: Some("24px".into()),
                ..Style::default()
            },
            children: vec![
                block_of(
                    3,
                    "div",
                    Style {
                        font_size: Some("40px".into()),
                        ..Style::default()
                    },
                    "♪",
                ),
                block_of(4, "div", Style::default(), "No lyrics available"),
            ],
            ..VNode::default()
        };
        let area = filled(
            1,
            "#00ff00",
            Style {
                display: Some("flex".into()),
                align_items: Some("center".into()),
                justify_content: Some("center".into()),
                width: Some("600px".into()),
                height: Some("300px".into()),
                ..Style::default()
            },
            vec![note],
        );
        let (words, boxes) = drawn_parts(area);
        let area = filled_rect(&boxes, Color32::from_rgb(0, 255, 0));
        let symbol = words_at(&words, "♪");
        let line = words_at(&words, "No lyrics available");
        for w in [symbol, line] {
            assert!(
                (w.center().x - area.center().x).abs() < 3.0,
                "in the middle of the 600-point area ({}): {w:?}",
                area.center().x
            );
        }
        let middle = (symbol.top() + line.bottom()) / 2.0;
        assert!(
            (middle - area.center().y).abs() < 4.0,
            "the pair in the middle of its height ({}): {symbol:?} {line:?}",
            area.center().y
        );
    }

    /// A `height: 100%` is a share of its containing box's definite height,
    /// however tall (it was a share of at most 400 points): a fullscreen
    /// view's column fills its 600-point player, and the growing lyrics
    /// area under its header puts its note in its own middle
    /// (betterFullscreen's "No lyrics available" sat under the header).
    #[test]
    fn a_percent_height_is_a_share_of_its_containers_height() {
        let note = VNode {
            id: 5,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                align_items: Some("center".into()),
                justify_content: Some("center".into()),
                flex: Some("1".into()),
                ..Style::default()
            },
            children: vec![block_of(6, "div", Style::default(), "Nothing here")],
            ..VNode::default()
        };
        let lyrics = filled(
            4,
            "#00ff00",
            Style {
                flex: Some("1".into()),
                display: Some("flex".into()),
                align_items: Some("center".into()),
                justify_content: Some("center".into()),
                min_height: Some("0".into()),
                ..Style::default()
            },
            vec![note],
        );
        let header = filled(
            3,
            "#ff0000",
            Style {
                width: Some("100px".into()),
                height: Some("100px".into()),
                ..Style::default()
            },
            vec![],
        );
        let content = filled(
            2,
            "#0000ff",
            Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                height: Some("100%".into()),
                padding: Some("20px".into()),
                gap: Some("20px".into()),
                ..Style::default()
            },
            vec![header, lyrics],
        );
        let player = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                width: Some("800px".into()),
                height: Some("600px".into()),
                ..Style::default()
            },
            children: vec![content],
            ..VNode::default()
        };
        let (words, boxes) = drawn_parts(player);
        let content = filled_rect(&boxes, Color32::from_rgb(0, 0, 255));
        assert!(
            (content.height() - 600.0).abs() < 2.0,
            "the column fills the player: {content:?}"
        );
        let area = filled_rect(&boxes, Color32::from_rgb(0, 255, 0));
        assert!(
            (area.bottom() - 580.0).abs() < 2.0,
            "the lyrics area runs to the column's padding: {area:?}"
        );
        let line = words_at(&words, "Nothing here");
        assert!(
            (line.center().y - area.center().y).abs() < 3.0,
            "in the middle of the lyrics area ({}): {line:?}",
            area.center().y
        );
        assert!(
            (line.center().x - area.center().x).abs() < 3.0,
            "and of its width ({}): {line:?}",
            area.center().x
        );
    }

    /// `align-items: flex-end` puts a row's shorter items at its foot and a
    /// column's items at its right; `justify-content: space-between` puts a
    /// row's first item at its start, its last at its end and shares the
    /// rest between them, in the app as in a plugin window.
    #[test]
    fn flex_end_and_space_between_place_items_as_css_does() {
        let tall = filled(
            2,
            "#ff0000",
            Style {
                width: Some("100px".into()),
                height: Some("120px".into()),
                ..Style::default()
            },
            vec![],
        );
        let foot = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                align_items: Some("flex-end".into()),
                width: Some("500px".into()),
                ..Style::default()
            },
            children: vec![tall, block_of(3, "div", Style::default(), "At the foot")],
            ..VNode::default()
        };
        let right = filled(
            4,
            "#00ff00",
            Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                align_items: Some("flex-end".into()),
                width: Some("500px".into()),
                ..Style::default()
            },
            vec![block_of(5, "div", Style::default(), "At the right")],
        );
        let spread = filled(
            6,
            "#0000ff",
            Style {
                display: Some("flex".into()),
                justify_content: Some("space-between".into()),
                width: Some("500px".into()),
                ..Style::default()
            },
            vec![
                block_of(7, "span", Style::default(), "First"),
                block_of(8, "span", Style::default(), "Middle"),
                block_of(9, "span", Style::default(), "Last"),
            ],
        );
        let page = VNode {
            id: 100,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                gap: Some("30px".into()),
                ..Style::default()
            },
            children: vec![foot, right, spread],
            ..VNode::default()
        };
        let (words, boxes) = drawn_parts(page);
        let red = filled_rect(&boxes, Color32::from_rgb(255, 0, 0));
        let at_foot = words_at(&words, "At the foot");
        assert!(
            (at_foot.bottom() - red.bottom()).abs() < 3.0,
            "at the row's foot ({}): {at_foot:?}",
            red.bottom()
        );
        let green = filled_rect(&boxes, Color32::from_rgb(0, 255, 0));
        let at_right = words_at(&words, "At the right");
        assert!(
            (at_right.right() - green.right()).abs() < 3.0,
            "at the column's right ({}): {at_right:?}",
            green.right()
        );
        let blue = filled_rect(&boxes, Color32::from_rgb(0, 0, 255));
        let (first, middle, last) = (
            words_at(&words, "First"),
            words_at(&words, "Middle"),
            words_at(&words, "Last"),
        );
        assert!(
            (first.left() - blue.left()).abs() < 3.0,
            "{first:?} {blue:?}"
        );
        assert!(
            (last.right() - blue.right()).abs() < 3.0,
            "{last:?} {blue:?}"
        );
        let before = middle.left() - first.right();
        let after = last.left() - middle.right();
        assert!(
            (before - after).abs() < 3.0 && before > 50.0,
            "the room shared out between them: {first:?} {middle:?} {last:?}"
        );
    }

    /// `align-self` places one flex item across its line against its
    /// container's `align-items`, as CSS does: in a row that starts its
    /// items, one in the row's middle and one at its foot; in a row that
    /// centres them, one at its top; in a column that stretches its items,
    /// one in the middle of the column's width and one at its right. Every
    /// item followed its container (`align-self` never reached the layout).
    #[test]
    fn align_self_places_an_item_against_its_containers_alignment() {
        let own = |id: u64, align: &str, text: &str| {
            block_of(
                id,
                "div",
                Style {
                    align_self: Some(align.into()),
                    ..Style::default()
                },
                text,
            )
        };
        let tall = |id: u64, colour: &str| {
            filled(
                id,
                colour,
                Style {
                    width: Some("100px".into()),
                    height: Some("120px".into()),
                    ..Style::default()
                },
                vec![],
            )
        };
        let starts = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                align_items: Some("flex-start".into()),
                width: Some("500px".into()),
                ..Style::default()
            },
            children: vec![
                tall(2, "#ff0000"),
                own(3, "center", "In the middle"),
                own(4, "flex-end", "At the foot"),
            ],
            ..VNode::default()
        };
        let centres = VNode {
            id: 5,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                align_items: Some("center".into()),
                width: Some("500px".into()),
                ..Style::default()
            },
            children: vec![tall(6, "#0000ff"), own(7, "flex-start", "At the top")],
            ..VNode::default()
        };
        let column = filled(
            8,
            "#00ff00",
            Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                width: Some("500px".into()),
                ..Style::default()
            },
            vec![
                own(9, "center", "Centred"),
                own(10, "flex-end", "On the right"),
            ],
        );
        let page = VNode {
            id: 100,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                gap: Some("30px".into()),
                ..Style::default()
            },
            children: vec![starts, centres, column],
            ..VNode::default()
        };
        let (words, boxes) = drawn_parts(page);
        let red = filled_rect(&boxes, Color32::from_rgb(255, 0, 0));
        let middle = words_at(&words, "In the middle");
        assert!(
            (middle.center().y - red.center().y).abs() < 3.0,
            "in the row's middle ({}): {middle:?}",
            red.center().y
        );
        let foot = words_at(&words, "At the foot");
        assert!(
            (foot.bottom() - red.bottom()).abs() < 3.0,
            "at the row's foot ({}): {foot:?}",
            red.bottom()
        );
        let blue = filled_rect(&boxes, Color32::from_rgb(0, 0, 255));
        let top = words_at(&words, "At the top");
        assert!(
            (top.top() - blue.top()).abs() < 3.0,
            "at the centring row's top ({}): {top:?}",
            blue.top()
        );
        let green = filled_rect(&boxes, Color32::from_rgb(0, 255, 0));
        let centred = words_at(&words, "Centred");
        assert!(
            (centred.center().x - green.center().x).abs() < 3.0,
            "in the column's middle ({}): {centred:?}",
            green.center().x
        );
        let right = words_at(&words, "On the right");
        assert!(
            (right.right() - green.right()).abs() < 3.0,
            "at the column's right ({}): {right:?}",
            green.right()
        );
    }

    /// element-hider's own menu listens for clicks (its document listener
    /// reaches it) and its "Hide This Element" item for `mouseenter`, which
    /// rings the element it would hide. The item gets `mouseenter` when the
    /// pointer comes over it, though the menu round it is the widget egui
    /// counts as hovered.
    #[test]
    fn mouseenter_reaches_an_item_inside_a_box_that_listens_too() {
        let item = VNode {
            id: 2,
            tag: "button".into(),
            listens: vec!["click".into(), "mouseenter".into(), "mouseleave".into()],
            style: Style {
                display: Some("flex".into()),
                width: Some("100%".into()),
                padding: Some("8px 16px".into()),
                ..Style::default()
            },
            children: vec![text(102, "Hide This Element")],
            ..VNode::default()
        };
        let menu = VNode {
            id: 1,
            tag: "div".into(),
            listens: vec!["click".into()],
            style: Style {
                position: Some("fixed".into()),
                left: Some("300px".into()),
                top: Some("100px".into()),
                min_width: Some("180px".into()),
                padding: Some("8px 0".into()),
                background_color: Some("#2a2a2a".into()),
                z_index: Some("999999".into()),
                ..Style::default()
            },
            children: vec![item],
            ..VNode::default()
        };
        plugin_dom::test_viewport([1400.0, 900.0]);
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![menu],
            }],
            ..DomOverlay::default()
        }));
        let ctx = app_fonts_ctx();
        let _ = plugin_dom::take_test_events();
        for _ in 0..5 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
                events: vec![egui::Event::PointerMoved(egui::pos2(340.0, 120.0))],
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| floating(ui));
            out.textures_delta.clear();
            plugin_dom::end_frame(&ctx);
        }
        let entered: Vec<_> = plugin_dom::take_test_events()
            .into_iter()
            .filter(|e| e.kind == "mouseenter")
            .map(|e| e.target)
            .collect();
        plugin_dom::test_overlay(None);
        assert_eq!(entered, vec![DomTarget::Node(2)]);
    }

    /// element-hider appends a 1-point divider and two `display: flex`,
    /// `width: 100%` buttons to TIDAL's context menu: they stack under its
    /// rows, the divider across the menu (less its margins) and the buttons
    /// as wide as the menu, one under the other. They sat side by side in a
    /// row, the divider not drawn.
    #[test]
    fn blocks_appended_to_a_menu_stack_and_fill_its_width() {
        let st = |f: &dyn Fn(&mut Style)| {
            let mut s = Style::default();
            f(&mut s);
            s
        };
        let divider = VNode {
            id: 1,
            tag: "div".into(),
            style: st(&|s| {
                s.height = Some("1px".into());
                s.background_color = Some("#444444".into());
                s.margin = Some("4px 8px".into());
            }),
            ..VNode::default()
        };
        let item = |id, label: &str| VNode {
            id,
            tag: "button".into(),
            style: st(&|s| {
                s.display = Some("flex".into());
                s.width = Some("100%".into());
                s.padding = Some("8px 16px".into());
                s.background_color = Some("#00ff00".into());
            }),
            children: vec![text(id + 100, label)],
            ..VNode::default()
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "contextmenu".into(),
                position: Position::Append,
                nodes: vec![divider, item(2, "Hide This Element"), item(3, "Unhide All")],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                ui.allocate_ui_with_layout(
                    vec2(240.0, 400.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(240.0);
                        column(ui, "contextmenu", Position::Append);
                    },
                );
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let fills = |c: Color32| {
            shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Rect(r) if r.fill == c => Some(r.rect),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let line = fills(Color32::from_rgb(0x44, 0x44, 0x44));
        assert_eq!(line.len(), 1, "the divider is drawn: {line:?}");
        assert!((line[0].width() - 224.0).abs() < 2.0, "{line:?}");
        assert!(line[0].height() <= 1.5, "{line:?}");
        let items = fills(Color32::from_rgb(0, 255, 0));
        assert_eq!(items.len(), 2, "{items:?}");
        for i in &items {
            assert!((i.width() - 240.0).abs() < 2.0, "{i:?}");
        }
        assert!(
            items[0].top() > line[0].bottom(),
            "{items:?} under {line:?}"
        );
        assert!(
            items[1].top() >= items[0].bottom() - 0.5,
            "stacked: {items:?}"
        );
    }

    /// tidal-genres' pills are `font-family: monospace, system-ui,
    /// sans-serif` spans: their text is drawn in the system's monospace
    /// face (Consolas on Windows), as a browser draws it, not in the app's
    /// own. A list with no installed family keeps the app's face.
    #[test]
    fn a_generic_monospace_family_draws_text_in_the_system_face() {
        let pill = |family: &str| VNode {
            id: 1,
            tag: "span".into(),
            style: Style {
                font_family: Some(family.into()),
                ..Style::default()
            },
            children: vec![VNode {
                id: 2,
                tag: "#text".into(),
                text: Some("R&B/Soul".into()),
                ..VNode::default()
            }],
            ..VNode::default()
        };
        let families = |family: &str| {
            plugin_dom::test_overlay(Some(DomOverlay {
                version: 1,
                injected: vec![Injected {
                    anchor: "main".into(),
                    position: Position::Prepend,
                    nodes: vec![pill(family)],
                }],
                ..DomOverlay::default()
            }));
            plugin_dom::test_font_families(&["Consolas"]);
            let ctx = egui::Context::default();
            let mut fonts = egui::FontDefinitions::default();
            let mono = fonts.families[&egui::FontFamily::Monospace].clone();
            fonts
                .families
                .insert(egui::FontFamily::Name("Consolas".into()), mono);
            ctx.set_fonts(fonts);
            let mut found = Vec::new();
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                    ..egui::RawInput::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    inline(ui, "main", Position::Prepend);
                });
                out.textures_delta.clear();
                found = out
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.text() == "R&B/Soul" => {
                            Some(t.galley.job.sections[0].format.font_id.family.clone())
                        }
                        _ => None,
                    })
                    .collect();
            }
            plugin_dom::test_overlay(None);
            found
        };
        assert_eq!(
            families("monospace, system-ui, sans-serif"),
            vec![egui::FontFamily::Name("Consolas".into())]
        );
        assert!(
            !families("system-ui, sans-serif").contains(&egui::FontFamily::Name("Consolas".into()))
        );
    }

    /// up-next's title and artist: two lines in a `flex-direction: column`
    /// box with no gap and `line-height: 1.3`, so each line is 1.3 times its
    /// font size tall (TIDAL: 16 points between a 12 and an 11 point line)
    /// and the box is as tall as the two together.
    #[test]
    fn line_height_sets_each_lines_height_and_a_flex_column_has_no_gap() {
        let line = |id, size: &str, block: bool| {
            let mut s = text(id, "Tears in the Rain");
            s.style.font_size = Some(size.into());
            s.style.white_space = Some("nowrap".into());
            if block {
                s.style.display = Some("block".into());
            }
            s
        };
        let column = |line_height: Option<&str>| VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                flex_direction: Some("column".into()),
                line_height: line_height.map(Into::into),
                ..Style::default()
            },
            children: vec![line(10, "12px", false), line(20, "11px", true)],
            ..VNode::default()
        };
        // egui lays rows out on whole points.
        let tall = drawn_size(column(Some("1.3")));
        assert!(
            (tall.y - (12.0 + 11.0) * 1.3).abs() < 2.5,
            "two lines of 1.3 em: {tall:?}"
        );
        let wide = drawn_size(column(Some("30px")));
        assert!((wide.y - 60.0).abs() < 2.5, "two 30-point lines: {wide:?}");
        // `normal` is the face's own height.
        let normal = drawn_size(column(Some("normal")));
        assert!(normal.y < 40.0, "{normal:?}");
        let double = drawn_size(column(Some("2")));
        assert!(
            (double.y - 46.0).abs() < 2.5,
            "two lines of 2 em: {double:?}"
        );
        // Lines shorter than the face's own row take only their height.
        let tight = drawn_size(column(Some("1")));
        assert!((tight.y - 23.0).abs() < 1.5, "two lines of 1 em: {tight:?}");
    }

    /// radiant-lyrics' words are `display: inline-block` spans in a `<p>`:
    /// they flow along a line and wrap at its width, not one per row.
    #[test]
    fn inline_block_children_of_a_block_flow_in_lines() {
        let words = |display: Option<&str>| VNode {
            id: 1,
            tag: "p".into(),
            style: Style {
                width: Some("200px".into()),
                ..Style::default()
            },
            children: (0..8)
                .map(|i| {
                    let mut w = text(10 + i, "word");
                    w.style.display = display.map(Into::into);
                    w
                })
                .collect(),
            ..VNode::default()
        };
        let flowing = drawn_size(words(Some("inline-block")));
        let stacked = drawn_size(words(None));
        assert!(flowing.y > 5.0, "{flowing:?}");
        assert!(
            flowing.y * 3.0 < stacked.y,
            "a few lines, not eight rows: {flowing:?} vs {stacked:?}"
        );
        assert!(flowing.x <= 201.0, "{flowing:?}");
        // One narrow line is not taller than a single word's row.
        let one = drawn_size(VNode {
            children: vec![{
                let mut w = text(10, "word");
                w.style.display = Some("inline-block".into());
                w
            }],
            ..words(None)
        });
        assert!(one.y < flowing.y, "{one:?} vs {flowing:?}");
    }

    /// A `" "` text node between two inline-block words is a space in the
    /// line (radiant-lyrics: "I won't leave", not "Iwon'tleave"); in a
    /// flex row it is nothing, as in a browser.
    #[test]
    fn white_space_between_inline_words_is_a_gap_only_in_a_line() {
        let space = |id| VNode {
            id,
            tag: "#text".into(),
            text: Some(" ".into()),
            ..VNode::default()
        };
        let line = |display: Option<&str>, spaced: bool| {
            let mut children = Vec::new();
            for i in 0..3 {
                let mut w = text(10 + i, "word");
                w.style.display = Some("inline-block".into());
                if spaced && i > 0 {
                    children.push(space(50 + i));
                }
                children.push(w);
            }
            VNode {
                id: 1,
                tag: "div".into(),
                style: Style {
                    display: display.map(Into::into),
                    ..Style::default()
                },
                children,
                ..VNode::default()
            }
        };
        // In a flex row, which sizes the line by what it holds.
        let words = |display, spaced| {
            let mut wrap = VNode {
                id: 2,
                tag: "div".into(),
                style: Style {
                    display: Some("flex".into()),
                    ..Style::default()
                },
                children: vec![line(display, spaced)],
                ..VNode::default()
            };
            wrap.children[0].id = 3;
            drawn_size(wrap)
        };
        let joined = words(None, false);
        let spaced = words(None, true);
        assert!(
            spaced.x > joined.x + 4.0,
            "two spaces in the line: {spaced:?} vs {joined:?}"
        );
        assert!((spaced.y - joined.y).abs() < 1.0, "still one line");
        let row = words(Some("flex"), false);
        let row_spaced = words(Some("flex"), true);
        assert!(
            (row.x - row_spaced.x).abs() < 0.5,
            "nothing in a flex row: {row_spaced:?} vs {row:?}"
        );
    }

    #[test]
    fn inline_lines_wrap_at_the_width_and_keep_spaces() {
        let w = |n: usize| vec![Some(40.0); n];
        // Five 40px words with 4px gaps: four fit in 180px (4*40+3*4=172).
        let lines = inline_lines(&w(5), &[false, true, true, true, true], 4.0, 180.0);
        assert_eq!(lines, vec![vec![0, 1, 2, 3], vec![4]]);
        // Without gaps five 40px boxes need 200px.
        let lines = inline_lines(&w(5), &[false; 5], 4.0, 200.0);
        assert_eq!(lines, vec![vec![0, 1, 2, 3, 4]]);
        // A box wider than the line is alone on its own.
        let lines = inline_lines(
            &[Some(40.0), Some(300.0), Some(40.0)],
            &[false; 3],
            0.0,
            100.0,
        );
        assert_eq!(lines, vec![vec![0], vec![1], vec![2]]);
        // Unknown widths: one line until they are measured.
        let lines = inline_lines(&[None, None], &[false, true], 4.0, 10.0);
        assert_eq!(lines, vec![vec![0, 1]]);
    }

    /// AltPlay's library table: `display: grid` with
    /// `grid-template-columns` puts its cells in rows of that many columns.
    #[test]
    fn grid_items_go_in_rows_of_their_columns() {
        let table = |display: &str| VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some(display.into()),
                width: Some("600px".into()),
                grid_template_columns: Some("40px repeat(2, minmax(0, 1fr))".into()),
                ..Style::default()
            },
            children: (0..6).map(|i| text(10 + i, "cell")).collect(),
            ..VNode::default()
        };
        let grid = drawn_size(table("grid"));
        let block = drawn_size(table("block"));
        assert!(grid.y > 5.0, "{grid:?}");
        assert!(
            grid.y * 2.5 < block.y,
            "two rows, not six: {grid:?} vs {block:?}"
        );
        assert!(grid.x <= 601.0, "{grid:?}");
        let cols = grid_columns(
            "40px repeat(2, minmax(0, 1fr))",
            600.0,
            10.0,
            16.0,
            [800.0, 600.0],
            &[],
        );
        assert_eq!(cols, Some(vec![40.0, 270.0, 270.0]));
        // `auto` tracks are as wide as their items beside an `fr` track,
        // and share the room among them without one.
        let cols = grid_columns(
            "auto 1fr auto",
            250.0,
            6.0,
            16.0,
            [800.0; 2],
            &[20.0, 0.0, 30.0],
        );
        assert_eq!(cols, Some(vec![20.0, 188.0, 30.0]));
        let cols = grid_columns("auto auto", 100.0, 0.0, 16.0, [800.0; 2], &[20.0, 40.0]);
        assert_eq!(cols, Some(vec![40.0, 60.0]));
    }

    /// AltPlay's library header: status, a `flex: 1` spacer, then Sync and
    /// close buttons, in a row of a set width. The spacer takes what the
    /// others leave, so the buttons stay inside the row.
    #[test]
    fn a_growing_spacer_leaves_room_for_the_items_after_it() {
        let row = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                display: Some("flex".into()),
                width: Some("400px".into()),
                ..Style::default()
            },
            children: vec![
                text(2, "Not connected"),
                VNode {
                    id: 3,
                    tag: "div".into(),
                    style: Style {
                        flex: Some("1".into()),
                        ..Style::default()
                    },
                    ..VNode::default()
                },
                button(4, "Sync now"),
                button(5, "Close"),
            ],
            ..VNode::default()
        };
        let size = drawn_size(row);
        assert!(size.x > 100.0 && size.x <= 401.0, "{size:?}");
    }

    /// TIDAL's CSS sizes boxes `border-box`: a `width: 100%` field with
    /// padding stays inside its parent (AltPlay's library filter).
    #[test]
    fn a_share_width_includes_the_padding() {
        let page = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                width: Some("300px".into()),
                ..Style::default()
            },
            children: vec![VNode {
                id: 2,
                tag: "div".into(),
                style: Style {
                    width: Some("100%".into()),
                    padding: Some("0 30px".into()),
                    ..Style::default()
                },
                children: vec![text(3, "Filter")],
                ..VNode::default()
            }],
            ..VNode::default()
        };
        let size = drawn_size(page);
        assert!(size.x > 100.0 && size.x <= 301.0, "{size:?}");
    }

    /// multipleplaylists' dialog: a card of `width: 90%; max-width: 500px`
    /// in a fixed backdrop that centres it. The card is 500 points wide, in
    /// the middle (it took 90% of the window).
    #[test]
    fn a_width_gives_way_to_max_width() {
        let card = VNode {
            id: 2,
            tag: "div".into(),
            style: Style {
                width: Some("90%".into()),
                max_width: Some("500px".into()),
                background_color: Some("#ff0000".into()),
                ..Style::default()
            },
            children: vec![text(3, "Add to Multiple Playlists")],
            ..VNode::default()
        };
        let backdrop = VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                right: Some("0".into()),
                bottom: Some("0".into()),
                display: Some("flex".into()),
                align_items: Some("center".into()),
                justify_content: Some("center".into()),
                z_index: Some("10000".into()),
                ..Style::default()
            },
            children: vec![card],
            ..VNode::default()
        };
        plugin_dom::test_viewport([1400.0, 900.0]);
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![backdrop],
            }],
            ..DomOverlay::default()
        }));
        let ctx = app_fonts_ctx();
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| floating(ui));
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let card = shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill.r() > 100 && r.fill.g() == 0 => Some(r.rect),
                _ => None,
            })
            .expect("the card");
        assert!((card.width() - 500.0).abs() < 2.0, "{card:?}");
    }

    /// element-hider's right-click menu: an auto-width `position: fixed` box
    /// of `min-width: 180px` holding buttons of `width: 100%`. The box is as
    /// wide as its widest button wants, the buttons fill it; it took the
    /// whole window's width.
    #[test]
    fn a_percent_wide_child_of_an_auto_width_fixed_box_fills_the_box_not_the_window() {
        let st = |f: &dyn Fn(&mut Style)| {
            let mut s = Style::default();
            f(&mut s);
            s
        };
        let item = |id, label: &str| VNode {
            id,
            tag: "button".into(),
            style: st(&|s| {
                s.display = Some("flex".into());
                s.width = Some("100%".into());
                s.padding = Some("8px 16px".into());
                s.background_color = Some("#00ff00".into());
            }),
            children: vec![text(id + 100, label)],
            ..VNode::default()
        };
        let menu = VNode {
            id: 1,
            tag: "div".into(),
            style: st(&|s| {
                s.position = Some("fixed".into());
                s.left = Some("300px".into());
                s.top = Some("100px".into());
                s.min_width = Some("180px".into());
                s.padding = Some("8px 0".into());
                s.background_color = Some("#ff0000".into());
                s.z_index = Some("999999".into());
            }),
            children: vec![
                item(2, "Hide This Element"),
                item(3, "Unhide All Elements, then show every page again"),
            ],
            ..VNode::default()
        };
        plugin_dom::test_viewport([1400.0, 900.0]);
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![menu],
            }],
            ..DomOverlay::default()
        }));
        let ctx = app_fonts_ctx();
        let mut shapes = Vec::new();
        for _ in 0..5 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| floating(ui));
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let fills = |red: bool| {
            shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Rect(r)
                        if (red && r.fill.r() > 100 && r.fill.g() == 0)
                            || (!red && r.fill.g() > 100 && r.fill.r() == 0) =>
                    {
                        Some(r.rect)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let menu = *fills(true).first().expect("the menu box");
        let items = fills(false);
        // Wider than the minimum (the long label), far from the window's 1400.
        assert!(menu.width() > 200.0 && menu.width() < 600.0, "{menu:?}");
        assert!(items.len() >= 2, "{items:?}");
        for i in &items {
            assert!((i.width() - menu.width()).abs() < 2.0, "{i:?} in {menu:?}");
        }
    }

    /// multipleplaylists' dialog, as it builds it: a backdrop that centres
    /// a card both ways; in the card a song box with a background and rows
    /// of a checkbox and a title. As in a browser the card sits in the
    /// middle of the window, the song box (a block) is as wide as the card,
    /// and the checkboxes can be seen on the dark card.
    #[test]
    fn a_dialog_card_is_centred_and_its_boxes_and_checkboxes_show() {
        let st = |f: &dyn Fn(&mut Style)| {
            let mut s = Style::default();
            f(&mut s);
            s
        };
        let div = |id, style: Style, children: Vec<VNode>| VNode {
            id,
            tag: "div".into(),
            style,
            children,
            ..VNode::default()
        };
        let row = |id: u64, title: &str| VNode {
            id,
            tag: "label".into(),
            listens: vec!["click".into()],
            style: st(&|s| {
                s.display = Some("flex".into());
                s.align_items = Some("center".into());
                s.padding = Some("8px".into());
            }),
            children: vec![
                VNode {
                    id: id + 1,
                    tag: "input".into(),
                    attrs: vec![("type".into(), "checkbox".into())],
                    value: Some("false".into()),
                    listens: vec!["click".into()],
                    style: st(&|s| s.margin = Some("0 12px 0 0".into())),
                    ..VNode::default()
                },
                div(
                    id + 2,
                    Style::default(),
                    vec![div(id + 3, Style::default(), vec![text(id + 4, title)])],
                ),
            ],
            ..VNode::default()
        };
        let card = div(
            2,
            st(&|s| {
                s.background_color = Some("#1a1a1a".into());
                s.padding = Some("24px".into());
                s.max_width = Some("500px".into());
                s.width = Some("90%".into());
                s.max_height = Some("80vh".into());
                s.overflow = Some("auto".into());
            }),
            vec![
                div(
                    3,
                    Style::default(),
                    vec![text(4, "Add to Multiple Playlists")],
                ),
                div(
                    5,
                    st(&|s| {
                        s.padding = Some("12px".into());
                        s.background_color = Some("rgba(0,255,0,0.05)".into());
                    }),
                    vec![div(6, Style::default(), vec![text(7, "Homicide")])],
                ),
                div(10, Style::default(), vec![row(20, "Body"), row(30, "e2")]),
            ],
        );
        let backdrop = VNode {
            id: 1,
            tag: "div".into(),
            listens: vec!["click".into()],
            style: st(&|s| {
                s.position = Some("fixed".into());
                s.top = Some("0".into());
                s.left = Some("0".into());
                s.right = Some("0".into());
                s.bottom = Some("0".into());
                s.display = Some("flex".into());
                s.align_items = Some("center".into());
                s.justify_content = Some("center".into());
                s.z_index = Some("10000".into());
            }),
            children: vec![card],
            ..VNode::default()
        };
        plugin_dom::test_viewport([1400.0, 900.0]);
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![backdrop],
            }],
            ..DomOverlay::default()
        }));
        let ctx = app_fonts_ctx();
        let mut shapes = Vec::new();
        for _ in 0..5 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| floating(ui));
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let rects: Vec<&egui::epaint::RectShape> = shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) => Some(r),
                _ => None,
            })
            .collect();
        let card = rects
            .iter()
            // #1a1a1a, faded in (premultiplied) while the layer appears.
            .find(|r| {
                r.fill.a() > 0
                    && r.fill.r() == r.fill.g()
                    && r.fill.g() == r.fill.b()
                    && r.rect.width() > 400.0
                    && r.rect.width() < 600.0
            })
            .unwrap_or_else(|| panic!("no card in {rects:#?}"))
            .rect;
        assert!((card.width() - 500.0).abs() < 2.0, "{card:?}");
        assert!(
            (card.center().y - 450.0).abs() < 4.0 && card.height() < 400.0,
            "centred on the window: {card:?}"
        );
        let song = rects
            .iter()
            .find(|r| r.fill.g() > 0 && r.fill.r() == 0 && r.fill.a() < 40)
            .expect("the song box")
            .rect;
        assert!(
            (song.width() - (500.0 - 48.0)).abs() < 2.0,
            "as wide as the card's content: {song:?}"
        );
        // Each checkbox draws a box that stands out from the dark card.
        let boxes = rects
            .iter()
            .filter(|r| {
                r.rect.width() < 24.0
                    && r.rect.height() < 24.0
                    && card.contains_rect(r.rect)
                    // (premultiplied, while the layer fades in)
                    && (r.stroke.width > 0.0 && r.stroke.color.r() >= 60
                        || r.fill.r() >= 60)
            })
            .count();
        assert!(boxes >= 2, "{rects:#?}");
    }

    /// Plugin columns in a track table (Tidal Tags' clones of the time
    /// heading): each node gets its own cell of the given width, side by
    /// side, and `text-transform: uppercase` (CSS, inherited) or the
    /// host's heading look writes capitals; `none` turns them off again.
    /// Before, `text-transform` was ignored and the clones ran together.
    #[test]
    fn plugin_columns_sit_in_their_cells_and_honour_text_transform() {
        let cell = |id: u64, label: &str, transform: Option<&str>| VNode {
            id,
            tag: "span".into(),
            style: Style {
                text_transform: transform.map(str::to_string),
                ..Style::default()
            },
            children: vec![text(id + 100, label)],
            ..VNode::default()
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "cols".into(),
                position: Position::After,
                nodes: vec![
                    cell(1, "Bitrate", None),
                    cell(2, "Sample Rate", Some("uppercase")),
                    cell(3, "kept", Some("none")),
                ],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut shapes = Vec::new();
        for upper in [true, false] {
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 200.0))),
                    ..egui::RawInput::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    columns(
                        ui,
                        "cols",
                        Position::After,
                        egui::pos2(100.0, 50.0),
                        &[100.0, 110.0, 77.0],
                        Look {
                            color: Color32::WHITE,
                            size: 12.0,
                            weight: 600,
                            upper,
                        },
                    );
                });
                out.textures_delta.clear();
                shapes = out.shapes;
            }
            let mut texts: Vec<(String, f32)> = shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos.x)),
                    _ => None,
                })
                .collect();
            texts.sort_by(|a, b| a.1.total_cmp(&b.1));
            let first = if upper { "BITRATE" } else { "Bitrate" };
            assert_eq!(
                texts.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
                vec![first, "SAMPLE RATE", "kept"],
                "upper {upper}"
            );
            for ((_, x), left) in texts.iter().zip([100.0, 200.0, 310.0]) {
                assert!((x - left).abs() < 1.0, "{texts:?}");
            }
        }
        plugin_dom::test_overlay(None);
    }

    /// TIDAL's sprite icons take the CSS `fill` an ancestor sets (LuckyLP's
    /// 60 % white), with its alpha as an opacity, else the text colour.
    #[test]
    fn sprite_icons_take_the_inherited_fill_else_the_text_colour() {
        let node = VNode {
            tag: "svg".into(),
            markup: Some(
                r#"<svg viewBox="0 0 20 20"><svg viewBox="0 0 24 24" width="100%" height="100%"><path d="M0 0" fill="none" stroke="__luna-paint__"/></svg></svg>"#.into(),
            ),
            ..VNode::default()
        };
        let svg = |fill| match image_source(&node, Color32::from_rgb(1, 2, 3), fill, Color32::BLACK)
        {
            Some(ImageSource::Bytes(_, bytes)) => String::from_utf8(bytes).unwrap(),
            _ => panic!("no svg"),
        };
        let filled = svg(Some(Color32::from_rgba_unmultiplied(255, 255, 255, 153)));
        assert!(
            filled.contains(r##"stroke="#ffffff" stroke-opacity="0.600""##),
            "{filled}"
        );
        let plain = svg(None);
        assert!(
            plain.contains(r##"stroke="#010203" stroke-opacity="1.000""##),
            "{plain}"
        );
    }

    /// A bare `<button>` looks as TIDAL draws one: no background (TIDAL's
    /// stylesheet takes it away), not even on hover. Tidalfast gave it a
    /// grey rounded box, so LuckyLP's shuffle sat on a tile.
    #[test]
    fn a_bare_button_has_no_background() {
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "bare".into(),
                position: Position::After,
                nodes: vec![VNode {
                    id: 1,
                    tag: "button".into(),
                    children: vec![text(2, "Go")],
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut shapes = Vec::new();
        for pointer in [None, Some(egui::pos2(110.0, 50.0))] {
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 200.0))),
                    events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
                    ..egui::RawInput::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    at(
                        ui,
                        "bare",
                        Position::After,
                        egui::pos2(100.0, 50.0),
                        Align2::LEFT_CENTER,
                    );
                });
                out.textures_delta.clear();
                shapes = out.shapes;
            }
            let fills: Vec<Color32> = shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r.fill),
                    _ => None,
                })
                .collect();
            assert!(fills.is_empty(), "hover {pointer:?}: {fills:?}");
        }
        plugin_dom::test_overlay(None);
    }

    /// multipleplaylists' dialog: the song box's `margin-bottom: 16px`
    /// (a longhand, apart from `margin`) and the button row's
    /// `display: flex; justify-content: flex-end`, which puts the buttons
    /// at the card's right edge. Both were ignored: no gap under the box,
    /// buttons on the left.
    #[test]
    fn margin_longhands_and_an_end_justified_flex_row() {
        let boxed = |id, f: &dyn Fn(&mut Style)| {
            let mut style = Style::default();
            f(&mut style);
            VNode {
                id,
                tag: "div".into(),
                style,
                ..VNode::default()
            }
        };
        let card = VNode {
            children: vec![
                boxed(2, &|s| {
                    s.height = Some("20px".into());
                    s.margin_bottom = Some("16px".into());
                    s.background_color = Some("#ff0000".into());
                }),
                boxed(3, &|s| {
                    s.height = Some("20px".into());
                    s.background_color = Some("#00ff00".into());
                }),
                VNode {
                    children: vec![boxed(5, &|s| {
                        s.width = Some("50px".into());
                        s.height = Some("10px".into());
                        s.background_color = Some("#0000ff".into());
                    })],
                    ..boxed(4, &|s| {
                        s.display = Some("flex".into());
                        s.justify_content = Some("flex-end".into());
                    })
                },
            ],
            ..boxed(1, &|s| s.width = Some("400px".into()))
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "card".into(),
                position: Position::After,
                nodes: vec![card],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                at(
                    ui,
                    "card",
                    Position::After,
                    egui::pos2(100.0, 20.0),
                    Align2::LEFT_TOP,
                );
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let rect_of = |c: Color32| {
            shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Rect(r) if r.fill == c => Some(r.rect),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no {c:?} box"))
        };
        let red = rect_of(Color32::from_rgb(255, 0, 0));
        let green = rect_of(Color32::from_rgb(0, 255, 0));
        let blue = rect_of(Color32::from_rgb(0, 0, 255));
        // 16 under the box, besides the column's own few points of spacing.
        let gap = green.top() - red.bottom();
        assert!((15.5..20.0).contains(&gap), "{red:?} {green:?}");
        assert!((blue.right() - 500.0).abs() < 1.0, "{blue:?}");
    }

    /// star-ratings' five stars sit in `display: flex` with no `gap`:
    /// they touch, 16 + 16 + ..., as in TIDAL. Tidalfast put 4 points
    /// between them.
    #[test]
    fn a_flex_row_without_a_gap_puts_its_items_side_by_side() {
        let star = |id, c: &str| VNode {
            id,
            tag: "div".into(),
            style: Style {
                width: Some("16px".into()),
                height: Some("16px".into()),
                background_color: Some(c.into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "stars".into(),
                position: Position::After,
                nodes: vec![VNode {
                    id: 1,
                    tag: "span".into(),
                    style: Style {
                        display: Some("flex".into()),
                        ..Style::default()
                    },
                    children: vec![star(2, "#ff0000"), star(3, "#00ff00")],
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 200.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                at(
                    ui,
                    "stars",
                    Position::After,
                    egui::pos2(10.0, 50.0),
                    Align2::LEFT_CENTER,
                );
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        let rect_of = |c: Color32| {
            shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Rect(r) if r.fill == c => Some(r.rect),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no {c:?} box"))
        };
        let (a, b) = (
            rect_of(Color32::from_rgb(255, 0, 0)),
            rect_of(Color32::from_rgb(0, 255, 0)),
        );
        assert!((b.left() - a.right()).abs() < 0.5, "{a:?} {b:?}");
    }

    /// Draws the nodes at `anchor` After for a few frames and returns the
    /// last frame's shapes.
    fn shapes_of(nodes: Vec<VNode>) -> Vec<egui::epaint::ClippedShape> {
        shapes_in(nodes, false)
    }

    /// [`shapes_of`] on a light panel (`light`) or the dark one.
    fn shapes_in(nodes: Vec<VNode>, light: bool) -> Vec<egui::epaint::ClippedShape> {
        plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: "slot".into(),
                position: Position::After,
                nodes,
            }],
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 200.0))),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                if light {
                    ui.visuals_mut().panel_fill = Color32::from_gray(0xf5);
                }
                at(
                    ui,
                    "slot",
                    Position::After,
                    egui::pos2(10.0, 50.0),
                    Align2::LEFT_CENTER,
                );
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        plugin_dom::test_overlay(None);
        shapes
    }

    /// HorizontalVolumeSlider's bar: the track and thumb its CSS gives the
    /// range input (a 4-point bar, white up to the value and 30 % white
    /// after it, a 3 x 12 white thumb centred on it), not egui's slider.
    #[test]
    fn a_styled_range_draws_its_track_and_thumb() {
        let range = VNode {
            id: 1,
            tag: "input".into(),
            attrs: vec![
                ("type".into(), "range".into()),
                ("min".into(), "0".into()),
                ("max".into(), "100".into()),
            ],
            value: Some("50".into()),
            style: Style {
                width: Some("86px".into()),
                height: Some("24px".into()),
                ..Style::default()
            },
            track: Some(Style {
                height: Some("4px".into()),
                border_radius: Some("2px".into()),
                background_image: Some(
                    "linear-gradient(to right, white 50%, rgba(255,255,255,0.3) 50%)".into(),
                ),
                ..Style::default()
            }),
            thumb: Some(Style {
                width: Some("3px".into()),
                height: Some("12px".into()),
                background_color: Some("white".into()),
                border_radius: Some("1px".into()),
                margin_top: Some("-4px".into()),
                ..Style::default()
            }),
            ..VNode::default()
        };
        let shapes = shapes_of(vec![range]);
        let thumb = shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == Color32::WHITE => Some(r.rect),
                _ => None,
            })
            .expect("a white thumb");
        assert!((thumb.width() - 3.0).abs() < 0.01, "{thumb:?}");
        assert!((thumb.height() - 12.0).abs() < 0.01, "{thumb:?}");
        // The track: a mesh 4 high, white at its left end, 30 % white at its
        // right end.
        let mesh = shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Mesh(m) if !m.vertices.is_empty() => Some(m.clone()),
                _ => None,
            })
            .expect("a track");
        let (top, bottom) = mesh
            .vertices
            .iter()
            .fold((f32::MAX, f32::MIN), |(t, b), v| {
                (t.min(v.pos.y), b.max(v.pos.y))
            });
        assert!((bottom - top - 4.0).abs() < 0.01, "{top} {bottom}");
        let (left, right) = mesh
            .vertices
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, r), v| {
                (l.min(v.pos.x), r.max(v.pos.x))
            });
        assert!((right - left - 86.0).abs() < 0.01, "{left} {right}");
        let at = |x: f32| {
            mesh.vertices
                .iter()
                .find(|v| (v.pos.x - x).abs() < 0.01)
                .map(|v| v.color)
                .unwrap()
        };
        assert_eq!(at(left), Color32::WHITE);
        assert!(at(right).a() < 0x60, "{:?}", at(right));
        // Centred: the thumb's middle is the track's.
        let track_mid = (top + bottom) / 2.0;
        assert!((thumb.center().y - track_mid).abs() < 0.01);
        // The thumb is at the value, inside the track's ends.
        assert!((thumb.left() - (left + (86.0 - 3.0) * 0.5)).abs() < 0.01);
    }

    /// The same bar in Light: its white would vanish into the light bar,
    /// so it takes dark ink, as plugin text does (the fill, its 30 % rest
    /// and the thumb). egui's slider read there; the styled bar did not.
    #[test]
    fn a_styled_range_reads_on_a_light_panel() {
        let range = VNode {
            id: 1,
            tag: "input".into(),
            attrs: vec![("type".into(), "range".into())],
            value: Some("50".into()),
            style: Style {
                width: Some("86px".into()),
                height: Some("24px".into()),
                ..Style::default()
            },
            track: Some(Style {
                height: Some("4px".into()),
                background_image: Some(
                    "linear-gradient(to right, white 50%, rgba(255,255,255,0.3) 50%)".into(),
                ),
                ..Style::default()
            }),
            thumb: Some(Style {
                width: Some("3px".into()),
                height: Some("12px".into()),
                background_color: Some("white".into()),
                ..Style::default()
            }),
            ..VNode::default()
        };
        let shapes = shapes_in(vec![range], true);
        for s in &shapes {
            match &s.shape {
                egui::Shape::Rect(r) if r.rect.width() < 4.0 => {
                    assert!(r.fill.r() < 0x40, "thumb {:?}", r.fill);
                }
                egui::Shape::Mesh(m) => {
                    for v in &m.vertices {
                        assert!(
                            v.color.a() == 0 || v.color.r() < 0x40 || v.color.a() < 0x60,
                            "{:?}",
                            v.color
                        );
                    }
                }
                _ => {}
            }
        }
        assert!(
            shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Mesh(m) if !m.vertices.is_empty()))
        );
    }

    /// star-ratings' empty stars are 28 % white, made for TIDAL's dark
    /// window: in Light they vanished into the bar. On a light backdrop an
    /// SVG paint that would not read takes dark ink with its opacity; on a
    /// dark one the plugin's colours are kept as written.
    #[test]
    fn svg_paints_that_would_not_read_take_dark_ink_in_light() {
        let node = VNode {
            id: 1,
            tag: "svg".into(),
            markup: Some(
                "<svg viewBox=\"0 0 32 32\"><path fill=\"rgba(255,255,255,0.28)\" d=\"M0 0h32v32H0z\"/><path style=\"stroke:#fff;fill:#1565c0\" d=\"M0 0h1v1H0z\"/></svg>".into(),
            ),
            ..VNode::default()
        };
        let svg = |back| match image_source(&node, Color32::WHITE, None, back) {
            Some(ImageSource::Bytes(_, bytes)) => String::from_utf8(bytes).unwrap(),
            _ => panic!("no svg"),
        };
        let light = svg(Color32::from_gray(0xf5));
        assert!(light.contains("fill=\"rgba(25,25,25,0.278)\""), "{light}");
        assert!(light.contains("stroke:rgba(26,26,26,1.000)"), "{light}");
        assert!(
            light.contains("fill:#1565c0"),
            "a blue that reads is kept: {light}"
        );
        let dark = svg(Color32::from_gray(0x10));
        assert!(dark.contains("fill=\"rgba(255,255,255,0.28)\""), "{dark}");
        assert!(dark.contains("stroke:#fff"), "{dark}");
    }

    /// HorizontalVolumeSlider's volume number: `border: none`, transparent,
    /// white, centred. Tidalfast drew egui's text field box around it.
    #[test]
    fn a_text_field_without_a_border_has_no_box() {
        let field = VNode {
            id: 1,
            tag: "input".into(),
            attrs: vec![("type".into(), "text".into())],
            value: Some("0".into()),
            style: Style {
                border: Some("none".into()),
                background_color: Some("transparent".into()),
                color: Some("white".into()),
                text_align: Some("center".into()),
                min_width: Some("28px".into()),
                max_width: Some("28px".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        let shapes = shapes_of(vec![field]);
        let boxed = shapes.iter().any(|s| match &s.shape {
            egui::Shape::Rect(r) => {
                r.fill.a() > 0 || r.stroke.width > 0.0 && r.stroke.color.a() > 0
            }
            _ => false,
        });
        assert!(!boxed, "{shapes:?}");
    }

    /// A `position: fixed` box `width` points wide at the window's corner,
    /// holding `children`.
    fn fixed_box(width: &str, children: Vec<VNode>) -> VNode {
        VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                width: Some(width.into()),
                ..Style::default()
            },
            children,
            ..VNode::default()
        }
    }

    /// Each text shape's words, box on the screen and font.
    fn texts(shapes: &[egui::Shape]) -> Vec<(String, Rect, egui::FontId)> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_owned(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                    t.galley.job.sections[0].format.font_id.clone(),
                )),
                _ => None,
            })
            .collect()
    }

    /// DownloadManager's About text: words and a `<strong>` in a `<p>` flow
    /// on one line, the bold word between the others (the line broke before
    /// and after each element), and the `<strong>` is bolder.
    #[test]
    fn words_and_a_strong_word_flow_on_one_line() {
        let strong = VNode {
            id: 3,
            tag: "strong".into(),
            children: vec![text_node(30, "FLAC")],
            ..VNode::default()
        };
        let p = VNode {
            id: 2,
            tag: "p".into(),
            children: vec![
                text_node(20, "save as "),
                strong,
                text_node(21, ", with lyrics"),
            ],
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![fixed_box("400px", vec![p])]);
        let texts = texts(&shapes);
        let find = |w: &str| {
            texts
                .iter()
                .find(|(t, ..)| t == w)
                .unwrap_or_else(|| panic!("{w:?} in {texts:?}"))
                .clone()
        };
        let (_, save, plain) = find("save");
        let (_, flac, bold) = find("FLAC");
        let (_, with, _) = find(",");
        assert!(
            (save.center().y - flac.center().y).abs() < 2.0,
            "{save:?} {flac:?}"
        );
        assert!(
            (flac.center().y - with.center().y).abs() < 2.0,
            "{flac:?} {with:?}"
        );
        assert!(flac.left() > save.right() && with.left() >= flac.right() - 0.5);
        assert_eq!(plain, crate::theme::medium(plain.size));
        assert_eq!(bold, crate::theme::bold(bold.size));
    }

    /// The lines of a flowing paragraph follow one another at its
    /// `line-height` (DownloadManager's 18.6-point lines were egui's
    /// 28-point rows apart).
    #[test]
    fn flowing_lines_are_their_line_height_apart() {
        let strong = VNode {
            id: 3,
            tag: "strong".into(),
            children: vec![text_node(30, "bold")],
            ..VNode::default()
        };
        let p = VNode {
            id: 2,
            tag: "p".into(),
            style: Style {
                font_size: Some("12px".into()),
                line_height: Some("1.25".into()),
                ..Style::default()
            },
            children: vec![
                text_node(20, "one two three four five six seven eight nine ten "),
                strong,
                text_node(21, " eleven twelve thirteen fourteen fifteen sixteen"),
            ],
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![fixed_box("150px", vec![p])]);
        let mut tops: Vec<f32> = texts(&shapes).iter().map(|(_, r, _)| r.top()).collect();
        tops.sort_by(f32::total_cmp);
        tops.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        assert!(tops.len() >= 3, "{tops:?}");
        for pair in tops.windows(2) {
            assert!((pair[1] - pair[0] - 15.0).abs() < 1.0, "{tops:?}");
        }
    }

    /// A list's items have their marker: a disc a little before each item
    /// of a `<ul>` (DownloadManager's About list had none), and none for
    /// `list-style: none`, which the items inherit.
    #[test]
    fn list_items_have_discs_unless_their_list_says_none() {
        let list = |style: Style| VNode {
            id: 2,
            tag: "ul".into(),
            style: Style {
                padding_left: Some("20px".into()),
                ..style
            },
            children: vec![VNode {
                id: 3,
                tag: "li".into(),
                children: vec![text_node(30, "Item")],
                ..VNode::default()
            }],
            ..VNode::default()
        };
        let discs = |style: Style| -> Vec<(Pos2, f32)> {
            let shapes = floating_shapes(vec![fixed_box("200px", vec![list(style)])]);
            shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Circle(c) if c.fill.a() > 0 => Some((c.center, c.radius)),
                    _ => None,
                })
                .collect()
        };
        let shown = discs(Style::default());
        assert_eq!(shown.len(), 1, "{shown:?}");
        let (centre, radius) = shown[0];
        assert!((1.5..4.0).contains(&radius), "{radius}");
        // Before the item's words (20 points in), inside the list.
        assert!(centre.x > 3.0 && centre.x < 17.0, "{centre:?}");
        let none = discs(Style {
            list_style_type: Some("none".into()),
            ..Style::default()
        });
        assert!(none.is_empty(), "{none:?}");
    }

    /// `overflow-y: auto` on a box of a set height scrolls what it holds:
    /// the box keeps its height (DownloadManager's About window grew past
    /// its 540 points and the taskbar).
    #[test]
    fn an_overflowing_box_of_a_set_height_scrolls() {
        let tall = filled(
            3,
            "#00ff00",
            Style {
                height: Some("300px".into()),
                ..Style::default()
            },
            vec![],
        );
        let scroller = filled(
            2,
            "#ff0000",
            Style {
                height: Some("100px".into()),
                overflow_y: Some("auto".into()),
                ..Style::default()
            },
            vec![tall],
        );
        let rects = filled_rects(VNode {
            id: 1,
            tag: "div".into(),
            style: Style {
                width: Some("200px".into()),
                ..Style::default()
            },
            children: vec![scroller],
            ..VNode::default()
        });
        let outer = filled_rect(&rects, Color32::from_rgb(255, 0, 0));
        assert!((outer.height() - 100.0).abs() < 0.5, "{outer:?}");
    }

    /// Words in a block with `text-overflow: ellipsis` end in "…" at its
    /// edge, words in its spans too (DownloadManager's start menu tile
    /// label ran past its tile).
    #[test]
    fn words_in_an_ellipsised_block_end_in_an_ellipsis() {
        let label = VNode {
            id: 2,
            tag: "div".into(),
            style: Style {
                white_space: Some("nowrap".into()),
                overflow: Some("hidden".into()),
                text_overflow: Some("ellipsis".into()),
                ..Style::default()
            },
            children: vec![text_node(20, "Downloader Manager with a long name")],
            ..VNode::default()
        };
        let shapes = floating_shapes(vec![fixed_box("80px", vec![label])]);
        let galley = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) => Some(t.galley.clone()),
                _ => None,
            })
            .expect("the label");
        assert!(galley.elided, "cut with an ellipsis: {:?}", galley.rect);
        assert!(galley.rect.width() <= 80.5, "{:?}", galley.rect);
    }

    /// TIDAL's body text is weight 500: words a plugin appends to `body`
    /// with no weight of their own are drawn at it (DownloadManager's
    /// window title and its "Output:" row are semibold in Segoe UI there),
    /// and a button keeps the browser's normal weight.
    #[test]
    fn body_text_takes_tidals_weight_and_a_button_the_browsers() {
        let shapes = floating_shapes(vec![fixed_box(
            "300px",
            vec![
                VNode {
                    id: 2,
                    tag: "span".into(),
                    children: vec![text_node(20, "Output")],
                    ..VNode::default()
                },
                VNode {
                    children: vec![text_node(30, "Choose")],
                    style: Style {
                        font_family: Some("inherit".into()),
                        ..Style::default()
                    },
                    ..button(3, "")
                },
            ],
        )]);
        let texts = texts(&shapes);
        let font = |w: &str| {
            texts
                .iter()
                .find(|(t, ..)| t == w)
                .map(|(.., f)| f.clone())
                .unwrap_or_else(|| panic!("{w:?} in {texts:?}"))
        };
        assert_eq!(font("Output"), crate::theme::medium(13.0));
        assert_eq!(font("Choose").size, 40.0 / 3.0);
        assert_eq!(font("Choose"), crate::theme::regular(40.0 / 3.0));
    }

    /// A growing flex item with padding fills its share of the row with
    /// its border box, padding inside (DownloadManager's About body ran its
    /// words 48 points past the window's right edge).
    #[test]
    fn a_growing_item_keeps_its_padding_inside_the_row() {
        let body = filled(
            2,
            "#ff0000",
            Style {
                flex: Some("1".into()),
                padding: Some("10px 24px".into()),
                ..Style::default()
            },
            vec![text_node(20, "Words")],
        );
        let row = filled(
            1,
            "#0000ff",
            Style {
                display: Some("flex".into()),
                width: Some("300px".into()),
                ..Style::default()
            },
            vec![body],
        );
        let rects = filled_rects(row);
        let outer = filled_rect(&rects, Color32::from_rgb(0, 0, 255));
        let grown = filled_rect(&rects, Color32::from_rgb(255, 0, 0));
        assert!(
            (grown.width() - outer.width()).abs() < 0.5,
            "{grown:?} in {outer:?}"
        );
    }
}
