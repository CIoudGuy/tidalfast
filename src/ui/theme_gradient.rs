//! Linear gradients a theme writes that the plain top-to-bottom and
//! left-to-right ramps cannot say: an angle (`120deg`, `to top`), colour
//! stops at positions (`#fff 0%, #f0f 40%`), a gradient image longer than its
//! box (`background-size: 350%`), and an angle an animation turns
//! (`linear-gradient(var(--rotate), ...)` with `@keyframes` that move
//! `--rotate`: the cotton candy family's page and its pills).
//!
//! Parsed once per overlay change (the angle of a turning gradient is read
//! each frame by [`super::theme_motion::angle`]); painted as bands of the
//! box cut perpendicular to the gradient line, each coloured at its
//! vertices, so a rounded box keeps its corners.

use egui::{Color32, CornerRadius, Painter, Pos2, Rect, vec2};

use super::plugin_slots::css;

/// The text a theme's `var(--name)` angle is kept as (instead of the empty
/// string an unknown variable resolves to), so the name survives to the
/// painter: `__spin(--rotate)`.
const SPIN: &str = "__spin(";

/// A linear gradient with an angle, stop positions, a length of its own, or
/// a turning angle.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Angled {
    /// Colour stops at positions along the gradient line (0 start, 1 end).
    pub stops: Vec<(f32, Color32)>,
    /// The CSS angle in degrees: 0 points up, 90 right, 180 down.
    pub angle: f32,
    /// The custom property an animation turns the angle by (`--rotate`).
    pub spin: Option<String>,
    /// How many times the box the gradient image is (`background-size`).
    pub size: [f32; 2],
    /// `background-position` as fractions of the room the image leaves.
    pub position: [f32; 2],
}

/// Keeps the name of a `var(--x)` angle that nothing declares: the first
/// argument of a gradient, so it is not lost when unknown variables resolve
/// to nothing.
pub fn keep_spin(value: &str, vars: &std::collections::BTreeMap<String, String>) -> String {
    let mut out = value.to_string();
    let mut from = 0;
    while let Some(at) = out[from..].find("gradient(var(--") {
        let start = from + at + "gradient(".len();
        let name_start = start + "var(".len();
        let Some(end) = out[name_start..].find([')', ',']) else {
            break;
        };
        let name = out[name_start..name_start + end].trim().to_string();
        let close = name_start + end;
        // `var(--x, fallback)` has a fallback of its own: left alone.
        if out.as_bytes()[close] == b')' && !vars.contains_key(&name) {
            out.replace_range(start..=close, &format!("{SPIN}{name})"));
            from = start + SPIN.len();
        } else {
            from = close;
        }
    }
    out
}

/// The gradient in `layer` if it needs this module's painter; `None` for
/// the plain vertical and horizontal ramps (and anything else).
pub fn parse(layer: &str) -> Option<Angled> {
    let lower = layer.trim().to_ascii_lowercase();
    let inner = css::func(&lower, "linear-gradient")?;
    let parts = css::split_top(inner, ',');
    let (first, rest) = parts.split_first()?;
    let (angle, spin, stops) = match direction(first) {
        Some((angle, spin)) => (angle, spin, rest),
        None => (180.0, None, parts.as_slice()),
    };
    let mut picked: Vec<(Option<f32>, Color32)> = Vec::new();
    for stop in stops {
        let words = css::words(stop.trim());
        let Some(color) = words.first().and_then(|w| css::color(w)) else {
            continue;
        };
        let at = words
            .last()
            .filter(|_| words.len() > 1)
            .and_then(|w| w.strip_suffix('%'))
            .and_then(|p| p.trim().parse::<f32>().ok())
            .map(|p| p / 100.0);
        picked.push((at, color));
    }
    if picked.len() < 2 {
        return None;
    }
    let stops = place(&picked);
    // Positions that spread the stops evenly (`0%` ... `100%` or none) are
    // the plain ramp's own.
    let last = (stops.len() - 1) as f32;
    let even = stops
        .iter()
        .enumerate()
        .all(|(i, (at, _))| (at - i as f32 / last).abs() < 1e-3);
    let plain_angle = [90.0, 180.0].iter().any(|a| (angle - a).abs() < 0.01);
    if spin.is_none() && plain_angle && even {
        return None;
    }
    Some(Angled {
        stops,
        angle,
        spin,
        size: [1.0, 1.0],
        position: [0.0, 0.0],
    })
}

/// Any `linear-gradient(...)` in `value` as an angled gradient, the plain
/// ramps too (a border image's: the cotton candy family's row lines, `to
/// right` fading in and out).
pub fn parse_any(value: &str) -> Option<Angled> {
    let lower = value.trim().to_ascii_lowercase();
    let at = lower.find("linear-gradient(")?;
    let rest = &value.trim()[at..];
    let mut depth = 0;
    let end = rest.char_indices().find_map(|(i, c)| {
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
        None
    })?;
    let layer = &rest[..=end];
    if let Some(g) = parse(layer) {
        return Some(g);
    }
    let inner = css::func(&layer.to_ascii_lowercase(), "linear-gradient")?.to_string();
    let parts = css::split_top(&inner, ',');
    let (first, rest) = parts.split_first()?;
    let (angle, stops) = match direction(first) {
        Some((angle, _)) => (angle, rest),
        None => (180.0, parts.as_slice()),
    };
    let colors: Vec<Color32> = stops
        .iter()
        .filter_map(|s| css::words(s.trim()).first().and_then(|w| css::color(w)))
        .collect();
    if colors.len() < 2 {
        return None;
    }
    let last = (colors.len() - 1) as f32;
    Some(Angled {
        stops: colors
            .iter()
            .enumerate()
            .map(|(i, c)| (i as f32 / last, *c))
            .collect(),
        angle,
        spin: None,
        size: [1.0, 1.0],
        position: [0.0, 0.0],
    })
}

/// The colour `g` has at `t` along its line (0 start, 1 end).
pub fn color_at(g: &Angled, t: f32) -> Color32 {
    let Some(first) = g.stops.first() else {
        return Color32::TRANSPARENT;
    };
    if t <= first.0 {
        return first.1;
    }
    for pair in g.stops.windows(2) {
        let ((a, ca), (b, cb)) = (pair[0], pair[1]);
        if t <= b {
            let k = if b > a { (t - a) / (b - a) } else { 1.0 };
            return ca.lerp_to_gamma(cb, k);
        }
    }
    g.stops.last().map_or(Color32::TRANSPARENT, |s| s.1)
}

/// CSS's rules for stops without a position: the first is at 0, the last at
/// 1, none goes before the one ahead of it, and a run of them is spread
/// evenly between its neighbours.
fn place(picked: &[(Option<f32>, Color32)]) -> Vec<(f32, Color32)> {
    let last = picked.len() - 1;
    let mut at: Vec<Option<f32>> = picked.iter().map(|(at, _)| *at).collect();
    at[0] = Some(at[0].unwrap_or(0.0));
    at[last] = Some(at[last].unwrap_or(1.0));
    let mut high = f32::MIN;
    for slot in at.iter_mut().flatten() {
        *slot = slot.max(high);
        high = *slot;
    }
    let mut i = 0;
    while i < at.len() {
        if at[i].is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let mut end = i;
        while at[end].is_none() {
            end += 1;
        }
        let (a, b) = (at[start].unwrap_or(0.0), at[end].unwrap_or(1.0));
        for (k, slot) in at.iter_mut().enumerate().take(end).skip(start + 1) {
            *slot = Some(a + (b - a) * (k - start) as f32 / (end - start) as f32);
        }
        i = end;
    }
    picked
        .iter()
        .zip(at)
        .map(|((_, color), at)| (at.unwrap_or(0.0), *color))
        .collect()
}

/// The gradient's first argument when it is a direction: the angle in
/// degrees and, for a turning one, the custom property's name.
fn direction(head: &str) -> Option<(f32, Option<String>)> {
    let head = head.trim();
    if let Some(name) = head.strip_prefix(SPIN).and_then(|r| r.strip_suffix(')')) {
        return Some((0.0, Some(name.trim().to_string())));
    }
    if let Some(to) = head.strip_prefix("to ") {
        let (mut x, mut y) = (0, 0);
        for word in to.split_whitespace() {
            match word {
                "top" => y = -1,
                "bottom" => y = 1,
                "left" => x = -1,
                "right" => x = 1,
                _ => return None,
            }
        }
        // A corner's angle depends on the box's shape; the square's
        // diagonal stands in.
        let angle = match (x, y) {
            (0, -1) => 0.0,
            (1, -1) => 45.0,
            (1, 0) => 90.0,
            (1, 1) => 135.0,
            (0, 1) => 180.0,
            (-1, 1) => 225.0,
            (-1, 0) => 270.0,
            (-1, -1) => 315.0,
            _ => return None,
        };
        return Some((angle, None));
    }
    angle(head).map(|a| (a, None))
}

/// A CSS angle (`120deg`, `.5turn`, `1.5rad`, `100grad`) in degrees.
pub fn angle(value: &str) -> Option<f32> {
    let v = value.trim().to_ascii_lowercase();
    for (unit, scale) in [
        ("deg", 1.0),
        ("grad", 0.9),
        ("rad", 180.0 / std::f32::consts::PI),
        ("turn", 360.0),
    ] {
        if let Some(n) = v.strip_suffix(unit) {
            return n.trim().parse::<f32>().ok().map(|n| n * scale);
        }
    }
    None
}

/// `background-size` of `350% 355%` as factors of the box (one value is
/// both; anything else is the box's own size).
pub fn size_of(value: Option<&str>) -> [f32; 2] {
    let mut parts = value.unwrap_or("").split_whitespace().map(|p| {
        p.strip_suffix('%')
            .and_then(|v| v.parse::<f32>().ok())
            .map_or(1.0, |v| (v / 100.0).clamp(0.01, 50.0))
    });
    let first = parts.next().unwrap_or(1.0);
    [first, parts.next().unwrap_or(first)]
}

/// Paints `g` in `rect` (inside its rounded `corners` when it has them)
/// at `angle` degrees, its image placed by `position`.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    corners: Option<CornerRadius>,
    g: &Angled,
    angle: f32,
    position: [f32; 2],
) {
    if let Some(mesh) = mesh(rect, corners, g, angle, position) {
        painter.add(mesh);
    }
}

/// [`paint`]'s shape, for a caller that places it under what it draws
/// next (a plugin box's gradient under its words).
pub fn mesh(
    rect: Rect,
    corners: Option<CornerRadius>,
    g: &Angled,
    angle: f32,
    position: [f32; 2],
) -> Option<egui::Mesh> {
    if g.stops.len() < 2 || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    let (gw, gh) = (rect.width() * g.size[0], rect.height() * g.size[1]);
    let image = Rect::from_min_size(
        rect.min
            - vec2(
                (gw - rect.width()) * position[0],
                (gh - rect.height()) * position[1],
            ),
        vec2(gw, gh),
    );
    let radians = angle.to_radians();
    let dir = vec2(radians.sin(), -radians.cos());
    // The gradient line reaches the corners the angle points to.
    let line = (gw * dir.x).abs() + (gh * dir.y).abs();
    if line < 1e-3 {
        return None;
    }
    let along = |p: Pos2| (p - image.center()).dot(dir) / line + 0.5;
    let outline = match corners {
        Some(c) => super::plugin_dom::rounded_outline(rect, c),
        None => vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ],
    };
    let ts = outline.iter().map(|p| along(*p));
    let (lo, hi) = ts.fold((f32::MAX, f32::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
    // The colour at `t` inside the band whose middle is `mid`: the band
    // lies between two stops, and its edges take that stretch's colours, so
    // two stops at one place (a hard stop: HorizontalVolumeSlider's fill
    // ending at the volume) change colour at once instead of blending
    // across the next band.
    let colour = |t: f32, mid: f32| {
        let first = g.stops[0];
        let last = g.stops[g.stops.len() - 1];
        if mid <= first.0 {
            return first.1;
        }
        if mid >= last.0 {
            return last.1;
        }
        let i = g.stops.iter().rposition(|(at, _)| *at <= mid).unwrap_or(0);
        let (a, b) = (g.stops[i], g.stops[(i + 1).min(g.stops.len() - 1)]);
        let span = (b.0 - a.0).max(1e-5);
        a.1.lerp_to_gamma(b.1, ((t - a.0) / span).clamp(0.0, 1.0))
    };
    // Band edges: the box's own extent and every stop inside it.
    let mut edges = vec![lo];
    edges.extend(
        g.stops
            .iter()
            .map(|(at, _)| *at)
            .filter(|t| *t > lo && *t < hi),
    );
    edges.push(hi);
    let mut mesh = egui::Mesh::default();
    for pair in edges.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if b - a < 1e-6 {
            continue;
        }
        let poly = super::plugin_dom::clip_band(&outline, along, a, b);
        if poly.len() < 3 {
            continue;
        }
        let first = mesh.vertices.len() as u32;
        for p in &poly {
            mesh.colored_vertex(*p, colour(along(*p).clamp(a, b), (a + b) / 2.0));
        }
        for i in 1..poly.len() as u32 - 1 {
            mesh.add_triangle(first, first + i, first + i + 1);
        }
    }
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use egui::pos2;

    use super::*;

    const RED: Color32 = Color32::from_rgb(255, 0, 0);
    const BLUE: Color32 = Color32::from_rgb(0, 0, 255);

    #[test]
    fn a_gradients_angle_and_stop_positions_are_read() {
        let g = parse("linear-gradient(120deg, #ff0000 0%, #00ff00 40%, #0000ff)").unwrap();
        assert_eq!(g.angle, 120.0);
        let at: Vec<f32> = g.stops.iter().map(|s| s.0).collect();
        assert_eq!(at, vec![0.0, 0.4, 1.0]);
        assert_eq!(
            parse("linear-gradient(to left, red, blue)").unwrap().angle,
            270.0
        );
        assert_eq!(
            parse("linear-gradient(.25turn, red, blue)"),
            None,
            "a quarter turn is 90deg: plain"
        );
        assert_eq!(
            parse("linear-gradient(.125turn, red, blue)").unwrap().angle,
            45.0
        );
        // The plain ramps keep their own, cheaper path.
        assert_eq!(parse("linear-gradient(180deg, red, blue)"), None);
        assert_eq!(parse("linear-gradient(to right, red, blue)"), None);
        assert_eq!(parse("linear-gradient(red, blue)"), None);
        assert_eq!(
            parse("linear-gradient(to bottom, red 0%, blue 100%)"),
            None,
            "positions that are the even ones change nothing"
        );
        assert!(parse("linear-gradient(to bottom, red 0%, blue 60%)").is_some());
    }

    #[test]
    fn stops_without_a_position_are_spread_between_their_neighbours() {
        let g = parse("linear-gradient(45deg, red, #00ff00, #0000ff 80%, #ffffff)").unwrap();
        let at: Vec<f32> = g.stops.iter().map(|s| s.0).collect();
        assert!((at[1] - 0.4).abs() < 1e-5, "{at:?}");
        assert_eq!((at[0], at[2], at[3]), (0.0, 0.8, 1.0));
    }

    #[test]
    fn a_turning_angle_keeps_its_variable_and_a_declared_one_is_left_alone() {
        let none = std::collections::BTreeMap::new();
        let kept = keep_spin("linear-gradient(var(--rotate), red, blue)", &none);
        assert_eq!(kept, "linear-gradient(__spin(--rotate), red, blue)");
        let g = parse(&kept).unwrap();
        assert_eq!(g.spin.as_deref(), Some("--rotate"));
        let known =
            std::collections::BTreeMap::from([("--rotate".to_string(), "45deg".to_string())]);
        let value = "linear-gradient(var(--rotate), red, blue)";
        assert_eq!(keep_spin(value, &known), value);
        let fallback = "linear-gradient(var(--rotate, 10deg), red, blue)";
        assert_eq!(keep_spin(fallback, &none), fallback);
    }

    fn painted(g: &Angled, angle: f32, rect: Rect) -> Vec<(Pos2, Color32)> {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui.painter(), rect, None, g, angle, g.position);
        });
        out.textures_delta.clear();
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Mesh(m) => Some(m.vertices.iter().map(|v| (v.pos, v.color))),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn a_diagonal_gradient_runs_from_one_corner_to_the_other() {
        let g = Angled {
            stops: vec![(0.0, RED), (1.0, BLUE)],
            angle: 135.0,
            spin: None,
            size: [1.0, 1.0],
            position: [0.0, 0.0],
        };
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        let v = painted(&g, 135.0, rect);
        let near = |p: Pos2| v.iter().find(|(q, _)| (*q - p).length() < 0.5).unwrap().1;
        assert_eq!(near(pos2(0.0, 0.0)), RED, "starts red at the top left");
        assert_eq!(
            near(pos2(100.0, 100.0)),
            BLUE,
            "ends blue at the bottom right"
        );
        // The other two corners are the middle of the line.
        let mid = near(pos2(100.0, 0.0));
        assert!(
            (i32::from(mid.r()) - 128).abs() < 4 && (i32::from(mid.b()) - 128).abs() < 4,
            "{mid:?}"
        );
        // Turned half a turn, it runs the other way.
        let v = painted(&g, 315.0, rect);
        let near = |p: Pos2| v.iter().find(|(q, _)| (*q - p).length() < 0.5).unwrap().1;
        assert_eq!(near(pos2(0.0, 0.0)), BLUE);
    }

    /// Two stops at one place change colour there at once: red up to 60 %,
    /// blue after it (HorizontalVolumeSlider's fill). The band after the
    /// stop blended from red to blue across the rest of the box.
    #[test]
    fn a_hard_stop_changes_colour_at_once() {
        let g = parse("linear-gradient(to right, #ff0000 60%, #0000ff 60%)").unwrap();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 10.0));
        let v = painted(&g, g.angle, rect);
        for (p, c) in &v {
            if p.x < 59.5 {
                assert_eq!(*c, RED, "{p:?}");
            } else if p.x > 60.5 {
                assert_eq!(*c, BLUE, "{p:?}");
            }
        }
        // The vertices on the stop belong to both bands, one of each colour.
        let on: Vec<Color32> = v
            .iter()
            .filter(|(p, _)| (p.x - 60.0).abs() < 0.5)
            .map(|(_, c)| *c)
            .collect();
        assert!(on.contains(&RED) && on.contains(&BLUE), "{on:?}");
    }

    #[test]
    fn an_oversized_image_shows_the_part_its_position_names() {
        let g = Angled {
            stops: vec![(0.0, RED), (1.0, BLUE)],
            angle: 90.0,
            spin: None,
            size: [4.0, 4.0],
            position: [0.0, 0.0],
        };
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        let start = painted(&g, 90.0, rect);
        let end_of_box = start.iter().find(|(p, _)| p.x > 99.0).unwrap().1;
        assert!(
            end_of_box.b() < 80,
            "a quarter of the way: mostly red, {end_of_box:?}"
        );
        let far = painted(&g, 90.0, rect)
            .iter()
            .map(|(_, c)| c.b())
            .max()
            .unwrap();
        assert!(far < 80);
        let moved = {
            let ctx = egui::Context::default();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint(ui.painter(), rect, None, &g, 90.0, [1.0, 1.0]);
            });
            out.textures_delta.clear();
            out.shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Mesh(m) => Some(m.vertices.iter().map(|v| v.color.b())),
                    _ => None,
                })
                .flatten()
                .max()
                .unwrap()
        };
        assert!(
            moved > 200,
            "at the far position the last quarter shows: {moved}"
        );
    }
}
